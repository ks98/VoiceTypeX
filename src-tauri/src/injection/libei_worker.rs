// SPDX-License-Identifier: GPL-3.0-or-later
//! libei-Worker fuer Tastendruck-Simulation auf Wayland.
//!
//! Wird vom `WaylandLibeiInjector` als dedizierter `std::thread` gespawnt,
//! sobald die `RemoteDesktop`-Portal-Session aufgebaut und ein
//! EIS-File-Descriptor verfuegbar ist (siehe `linux_wayland.rs`).
//!
//! Architektur (siehe CLAUDE.md §11):
//!
//! ```text
//!   tokio (Hauptthread)              std::thread (Worker)
//!   ─────────────────                 ──────────────────────
//!   inject() / Strg+V Anfrage  ──>   poll-loop {
//!                                       1) ei::Context::read() (non-block)
//!                                          → server events
//!                                       2) cmd_rx.try_recv()
//!                                          → KeyCommand::CtrlV
//!                                       3) wenn keymap+keyboard ready:
//!                                          send_ctrl_v(&context)
//!                                       4) context.flush()
//!                                       5) sleep 20 ms
//!                                    }
//! ```
//!
//! Warum manuell poll statt calloop:
//! - calloop's Architektur trennt Sources hart, der Cmd-Handler haette
//!   keinen Zugriff auf den `ei::Context` ohne Rc/RefCell-Tricks.
//! - Eine simple Poll-Loop mit non-blocking FD ist robuster und einfacher
//!   zu reasoning.
//! - 20 ms Polling-Latenz ist gegenueber dem Compositor-Roundtrip
//!   vernachlaessigbar.
//!
//! Vorbild fuer den EI-Handshake-Ablauf: `examples/type-text.rs` aus
//! github.com/ids1024/reis (gleiche Sequence: Handshake → Connection →
//! Seat → Device → Keyboard → start_emulating → key events → frame).

use reis::{ei, PendingRequestResult};
use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::time::Duration;
use tokio::sync::oneshot;
use xkbcommon::xkb;

/// Befehle, die der tokio-Hauptthread an den libei-Worker schickt.
#[derive(Debug)]
pub enum KeyCommand {
    /// Sende Ctrl+V (Press Ctrl, Press V, Release V, Release Ctrl).
    CtrlV,
    /// Worker sauber herunterfahren.
    Shutdown,
}

/// Liste der Interfaces, die wir vom EI-Server anfordern. Versionen
/// muessen mit dem Server matchen — `1` ist der aktuelle gemeinsame
/// Nenner, mit dem libei 1.x interoperiert.
fn interfaces() -> HashMap<&'static str, u32> {
    let mut m = HashMap::new();
    m.insert("ei_callback", 1);
    m.insert("ei_connection", 1);
    m.insert("ei_seat", 1);
    m.insert("ei_device", 1);
    m.insert("ei_pingpong", 1);
    m.insert("ei_keyboard", 1);
    m
}

#[derive(Default)]
struct SeatData {
    capabilities: HashMap<String, u64>,
}

#[derive(Default)]
struct DeviceData {
    interfaces: HashMap<String, reis::Object>,
}

impl DeviceData {
    fn interface<T: reis::Interface>(&self) -> Option<T> {
        self.interfaces.get(T::NAME)?.clone().downcast()
    }
}

struct WorkerState {
    seats: HashMap<ei::Seat, SeatData>,
    devices: HashMap<ei::Device, DeviceData>,
    sequence: u32,
    last_serial: u32,
    keymap: Option<xkb::Keymap>,
    /// Sobald wir ein keyboard-Device haben, das `Done` gemeldet hat,
    /// landet es hier. Bis dahin koennen wir nicht tippen.
    active_keyboard: Option<(ei::Device, ei::Keyboard)>,
    ready_tx: Option<oneshot::Sender<bool>>,
    pending_ctrl_v: bool,
    running: bool,
}

impl WorkerState {
    fn is_ready(&self) -> bool {
        self.active_keyboard.is_some() && self.keymap.is_some()
    }

    /// Antwort auf den initialen Handshake- und Discovery-Eventstrom des
    /// Servers. Siehe `type-text.rs` fuer die Referenzimplementierung.
    fn handle_request(&mut self, request: ei::Event) {
        match request {
            ei::Event::Handshake(handshake, request) => match request {
                ei::handshake::Event::HandshakeVersion { version: _ } => {
                    handshake.handshake_version(1);
                    handshake.name("voicetypex");
                    handshake.context_type(ei::handshake::ContextType::Sender);
                    for (interface, version) in interfaces().iter() {
                        handshake.interface_version(interface, *version);
                    }
                    handshake.finish();
                }
                ei::handshake::Event::Connection {
                    connection: _,
                    serial,
                } => {
                    self.last_serial = serial;
                }
                _ => {}
            },
            ei::Event::Connection(_connection, request) => match request {
                ei::connection::Event::Seat { seat } => {
                    self.seats.insert(seat, SeatData::default());
                }
                ei::connection::Event::Ping { ping } => {
                    ping.done(0);
                }
                _ => {}
            },
            ei::Event::Seat(seat, request) => {
                let Some(data) = self.seats.get_mut(&seat) else {
                    return;
                };
                match request {
                    ei::seat::Event::Capability { mask, interface } => {
                        data.capabilities.insert(interface, mask);
                    }
                    ei::seat::Event::Done => {
                        if let Some(mask) = data.capabilities.get("ei_keyboard") {
                            seat.bind(*mask);
                        }
                    }
                    ei::seat::Event::Device { device } => {
                        self.devices.insert(device, DeviceData::default());
                    }
                    _ => {}
                }
            }
            ei::Event::Device(device, request) => {
                let Some(data) = self.devices.get_mut(&device) else {
                    return;
                };
                match request {
                    ei::device::Event::Interface { object } => {
                        data.interfaces
                            .insert(object.interface().to_owned(), object);
                    }
                    ei::device::Event::Done => {
                        if let Some(keyboard) = data.interface::<ei::Keyboard>() {
                            tracing::info!(
                                "libei-Keyboard-Device ready — Auto-Paste verfuegbar"
                            );
                            self.active_keyboard = Some((device, keyboard));
                            // Ready-Signal an tokio-Seite (nur einmal).
                            if let Some(tx) = self.ready_tx.take() {
                                let _ = tx.send(true);
                            }
                        }
                    }
                    ei::device::Event::Resumed { serial } => {
                        self.last_serial = serial;
                    }
                    _ => {}
                }
            }
            ei::Event::Keyboard(
                _keyboard,
                ei::keyboard::Event::Keymap {
                    keymap_type: _,
                    size,
                    keymap,
                },
            ) => {
                let xkb_ctx = xkb::Context::new(0);
                // SAFETY: `keymap` ist ein OwnedFd vom Server, `size` ist die
                // korrekte Map-Groesse. Beides liefert reis aus dem Protokoll
                // garantiert in passender Form.
                let parsed = unsafe {
                    xkb::Keymap::new_from_fd(
                        &xkb_ctx,
                        keymap,
                        size as _,
                        xkb::KEYMAP_FORMAT_TEXT_V1,
                        0,
                    )
                };
                match parsed {
                    Ok(Some(km)) => self.keymap = Some(km),
                    Ok(None) => {
                        tracing::warn!("libei: Keymap-Parsing lieferte None");
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "libei: Keymap-Parsing fehlgeschlagen");
                    }
                }
            }
            _ => {}
        }
    }

    /// Sendet die Ctrl+V-Sequenz. Nur aufrufen, wenn `is_ready()` true ist.
    fn send_ctrl_v(&mut self) {
        let Some((device, keyboard)) = &self.active_keyboard else {
            return;
        };
        let Some(keymap) = &self.keymap else {
            return;
        };

        let Some(ctrl_keycode) = find_keycode(keymap, xkb::Keysym::Control_L) else {
            tracing::warn!("libei: Control_L nicht im Keymap gefunden");
            return;
        };
        let Some(v_keycode) = find_keycode(keymap, xkb::Keysym::v) else {
            tracing::warn!("libei: keysym 'v' nicht im Keymap gefunden");
            return;
        };

        device.start_emulating(self.sequence, self.last_serial);
        self.sequence = self.sequence.wrapping_add(1);

        // EI-Konvention: keycode minus 8 (XKB-Keycodes sind +8 zu Linux-Keycodes).
        keyboard.key(ctrl_keycode - 8, ei::keyboard::KeyState::Press);
        keyboard.key(v_keycode - 8, ei::keyboard::KeyState::Press);
        keyboard.key(v_keycode - 8, ei::keyboard::KeyState::Released);
        keyboard.key(ctrl_keycode - 8, ei::keyboard::KeyState::Released);
        device.frame(self.last_serial, current_time_ns());
        // KEIN stop_emulating — wir wollen die Session fuer weitere Strg+Vs
        // offen halten.
    }
}

/// Sucht im Keymap den Keycode, der ein gegebenes Keysym **ohne Modifier**
/// erzeugt (Shift-Level 0). Fuer Ctrl- und v-Keys ist das die richtige
/// Wahl.
fn find_keycode(keymap: &xkb::Keymap, target: xkb::Keysym) -> Option<u32> {
    let min = keymap.min_keycode().raw();
    let max = keymap.max_keycode().raw();
    for kc in min..=max {
        let syms = keymap.key_get_syms_by_level(xkb::Keycode::new(kc), 0, 0);
        if syms.contains(&target) {
            return Some(kc);
        }
    }
    None
}

/// Zeitstempel in Nanosekunden seit Boot. EI's `frame()` braucht eine
/// Zeitangabe; der genaue Wert ist fuer Wayland-Compositors nicht
/// inhaltlich relevant, aber er muss monoton steigen.
fn current_time_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// Worker-Hauptschleife. Laeuft, bis `KeyCommand::Shutdown` empfangen wird
/// oder der Sender geschlossen wird (z.B. App-Shutdown).
pub fn run_libei_worker(
    fd: OwnedFd,
    cmd_rx: mpsc::Receiver<KeyCommand>,
    ready_tx: oneshot::Sender<bool>,
) {
    let stream = UnixStream::from(fd);
    if let Err(e) = stream.set_nonblocking(true) {
        tracing::error!(error = %e, "libei: set_nonblocking fehlgeschlagen");
        let _ = ready_tx.send(false);
        return;
    }

    let context = match ei::Context::new(stream) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "libei: ei::Context::new fehlgeschlagen");
            let _ = ready_tx.send(false);
            return;
        }
    };
    let _handshake = context.handshake();
    if let Err(e) = context.flush() {
        tracing::error!(error = %e, "libei: erstes flush fehlgeschlagen");
        let _ = ready_tx.send(false);
        return;
    }

    let mut state = WorkerState {
        seats: HashMap::new(),
        devices: HashMap::new(),
        sequence: 0,
        last_serial: u32::MAX,
        keymap: None,
        active_keyboard: None,
        ready_tx: Some(ready_tx),
        pending_ctrl_v: false,
        running: true,
    };

    let poll_interval = Duration::from_millis(20);
    let setup_deadline = std::time::Instant::now() + Duration::from_secs(5);

    while state.running {
        // 1. Server-Events lesen (non-blocking dank set_nonblocking)
        match context.read() {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => {
                tracing::warn!(error = %e, "libei: context.read() fehlgeschlagen — Worker beendet");
                break;
            }
        }
        while let Some(result) = context.pending_event() {
            match result {
                PendingRequestResult::Request(request) => {
                    state.handle_request(request);
                }
                PendingRequestResult::ParseError(msg) => {
                    tracing::warn!(message = %msg, "libei: ParseError im Event-Stream");
                }
                PendingRequestResult::InvalidObject(_) => {}
            }
        }

        // 2. Cmds aus tokio-Seite pollen
        loop {
            match cmd_rx.try_recv() {
                Ok(KeyCommand::CtrlV) => state.pending_ctrl_v = true,
                Ok(KeyCommand::Shutdown) => state.running = false,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    state.running = false;
                    break;
                }
            }
        }

        // 3. Pending Ctrl+V ausfuehren, wenn alles bereit ist
        if state.pending_ctrl_v && state.is_ready() {
            state.send_ctrl_v();
            state.pending_ctrl_v = false;
        }

        // 4. Setup-Timeout: wenn nach 5 s noch kein Keyboard ready ist,
        //    Failure signalisieren und beenden.
        if state.ready_tx.is_some() && std::time::Instant::now() > setup_deadline {
            tracing::warn!("libei: Setup-Timeout — kein Keyboard-Device innerhalb 5s ready");
            if let Some(tx) = state.ready_tx.take() {
                let _ = tx.send(false);
            }
            state.running = false;
        }

        // 5. Flush + sleep
        let _ = context.flush();
        std::thread::sleep(poll_interval);
    }

    // Abschluss: ungesendetes Ready-Signal mit `false` quittieren, damit
    // die tokio-Seite nicht haengt.
    if let Some(tx) = state.ready_tx.take() {
        let _ = tx.send(false);
    }
    tracing::info!("libei-Worker beendet");
}
