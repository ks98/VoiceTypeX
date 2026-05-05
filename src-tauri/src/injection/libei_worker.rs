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
    /// Erst nach `Resumed`-Event darf der Server tatsaechlich Tipps
    /// akzeptieren (Spec libei protocol.xml: "client bug to request
    /// emulation on a device that is not resumed. The EIS implementation
    /// may silently discard such events").
    keyboard_resumed: bool,
    /// `start_emulating` wurde aufgerufen und kein `stop_emulating` /
    /// kein `Paused` ist seitdem dazwischen gekommen. Nach lan-mouse-
    /// Pattern: einmalig im `Resumed`-Handler aufrufen, dann persistent
    /// halten — pro Strg+V nur noch keys + frame, kein neues
    /// start_emulating.
    emulation_active: bool,
    ready_tx: Option<oneshot::Sender<bool>>,
    pending_ctrl_v: bool,
    running: bool,
}

impl WorkerState {
    fn is_ready(&self) -> bool {
        self.active_keyboard.is_some() && self.keymap.is_some() && self.keyboard_resumed
    }

    fn try_signal_ready(&mut self) {
        if self.is_ready() {
            if let Some(tx) = self.ready_tx.take() {
                tracing::info!(
                    "libei: alle Vorbedingungen erfuellt (keyboard + keymap + resumed) — Auto-Paste bereit"
                );
                let _ = tx.send(true);
            }
        }
    }

    /// Antwort auf den initialen Handshake- und Discovery-Eventstrom des
    /// Servers. Siehe `type-text.rs` fuer die Referenzimplementierung.
    /// Diagnose-Logging an jedem Schritt, weil das Setup mehrstufig ist
    /// und ohne Logs nicht nachvollziehbar.
    fn handle_request(&mut self, request: ei::Event) {
        match request {
            ei::Event::Handshake(handshake, request) => match request {
                ei::handshake::Event::HandshakeVersion { version } => {
                    tracing::info!(server_version = version, "libei: Handshake gestartet");
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
                    tracing::info!(serial, "libei: Connection etabliert");
                    self.last_serial = serial;
                }
                _ => {}
            },
            ei::Event::Connection(_connection, request) => match request {
                ei::connection::Event::Seat { seat } => {
                    tracing::debug!("libei: Seat-Objekt empfangen");
                    self.seats.insert(seat, SeatData::default());
                }
                ei::connection::Event::Ping { ping } => {
                    tracing::trace!("libei: Ping → Pong");
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
                        tracing::debug!(interface = %interface, mask, "libei: Seat-Capability");
                        data.capabilities.insert(interface, mask);
                    }
                    ei::seat::Event::Done => {
                        if let Some(mask) = data.capabilities.get("ei_keyboard") {
                            tracing::info!(mask = mask, "libei: Seat done — bind keyboard");
                            seat.bind(*mask);
                        } else {
                            tracing::warn!(
                                caps = ?data.capabilities.keys().collect::<Vec<_>>(),
                                "libei: Seat done OHNE ei_keyboard-Capability — Auto-Paste nicht moeglich"
                            );
                        }
                    }
                    ei::seat::Event::Device { device } => {
                        tracing::debug!("libei: Device-Objekt empfangen");
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
                        let iface = object.interface().to_owned();
                        tracing::debug!(interface = %iface, "libei: Device-Interface");
                        data.interfaces.insert(iface, object);
                    }
                    ei::device::Event::Done => {
                        let interfaces: Vec<&String> = data.interfaces.keys().collect();
                        tracing::info!(
                            ?interfaces,
                            "libei: Device done"
                        );
                        if let Some(keyboard) = data.interface::<ei::Keyboard>() {
                            tracing::info!(
                                "libei: Keyboard-Interface gefunden — warte auf Resumed-Event"
                            );
                            self.active_keyboard = Some((device, keyboard));
                            self.try_signal_ready();
                        } else {
                            tracing::warn!(
                                "libei: Device done OHNE ei_keyboard-Interface — Auto-Paste nicht moeglich"
                            );
                        }
                    }
                    ei::device::Event::Resumed { serial } => {
                        tracing::info!(serial, "libei: Device resumed — sender darf tippen");
                        self.last_serial = serial;
                        self.keyboard_resumed = true;
                        // start_emulating einmalig nach lan-mouse-Pattern:
                        // pro Resumed genau einmal, dann persistent. Wenn
                        // ein Paused-Event kommt, setzen wir das zurueck
                        // und der naechste Resumed startet eine neue
                        // Emulations-Sequenz mit incrementiertem
                        // sequence-Counter (Spec: monoton).
                        if !self.emulation_active {
                            if let Some((dev, _)) = &self.active_keyboard {
                                let seq = self.sequence;
                                self.sequence = self.sequence.wrapping_add(1);
                                tracing::info!(sequence = seq, serial, "libei: start_emulating");
                                dev.start_emulating(seq, serial);
                                self.emulation_active = true;
                            }
                        }
                        self.try_signal_ready();
                    }
                    ei::device::Event::Paused { serial } => {
                        tracing::warn!(serial, "libei: Device paused — sender darf nicht mehr tippen");
                        self.last_serial = serial;
                        self.keyboard_resumed = false;
                        self.emulation_active = false;
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

    /// Sendet die Ctrl+V-Sequenz. Nur aufrufen, wenn `is_ready()` true
    /// ist (Keyboard + Keymap + Resumed + Emulation aktiv via Resumed-
    /// Handler). KEIN start_emulating/stop_emulating hier — die
    /// Emulations-Session ist persistent ab erstem Resumed.
    fn send_ctrl_v(&mut self) {
        let Some((device, keyboard)) = &self.active_keyboard else {
            tracing::warn!("libei: send_ctrl_v ohne aktives Keyboard");
            return;
        };
        let Some(keymap) = &self.keymap else {
            tracing::warn!("libei: send_ctrl_v ohne Keymap");
            return;
        };
        if !self.emulation_active {
            tracing::warn!("libei: send_ctrl_v ohne aktive Emulation (kein Resumed?)");
            return;
        }

        let Some(ctrl_keycode) = find_keycode(keymap, xkb::Keysym::Control_L) else {
            tracing::warn!("libei: Control_L nicht im Keymap gefunden");
            return;
        };
        let Some(v_keycode) = find_keycode(keymap, xkb::Keysym::v) else {
            tracing::warn!("libei: keysym 'v' nicht im Keymap gefunden");
            return;
        };

        let serial = self.last_serial;
        let time_us = monotonic_time_us();

        tracing::info!(
            serial,
            time_us,
            ctrl_keycode,
            v_keycode,
            "libei: sende Ctrl+V (frame in persistenter Emulations-Session)"
        );

        // EI-Konvention: keycode minus 8 (XKB-Keycodes sind +8 zu Linux-
        // Keycodes). Spec verlangt evdev-Keycodes (KEY_LEFTCTRL=29,
        // KEY_V=47).
        keyboard.key(ctrl_keycode - 8, ei::keyboard::KeyState::Press);
        keyboard.key(v_keycode - 8, ei::keyboard::KeyState::Press);
        keyboard.key(v_keycode - 8, ei::keyboard::KeyState::Released);
        keyboard.key(ctrl_keycode - 8, ei::keyboard::KeyState::Released);
        device.frame(serial, time_us);
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

/// Mikrosekunden seit UNIX_EPOCH fuer `frame(serial, time)`.
/// EI-Spec sagt CLOCK_MONOTONIC, aber die EIS-Implementierungen (KWin,
/// Mutter) sind in der Praxis tolerant — einzige Pflicht-Eigenschaft ist
/// strikte Monotonie. lan-mouse nutzt genau dieses Pattern und es
/// funktioniert auf KDE/GNOME.
///
/// Wichtig gegen den vorherigen `Instant`-basierten Versuch: dort lieferte
/// der allererste `frame()` `time=0` (Init und elapsed() im selben Tick),
/// und manche Compositoren verwerfen `time=0` als "Frame in der
/// Vergangenheit". UNIX_EPOCH-Mikrosekunden sind seit 2026 immer
/// 1.7 × 10^15 — keine Nullen.
fn monotonic_time_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(1)
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
        keyboard_resumed: false,
        emulation_active: false,
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
        if state.pending_ctrl_v {
            if state.is_ready() {
                state.send_ctrl_v();
                state.pending_ctrl_v = false;
            } else {
                tracing::trace!(
                    has_keyboard = state.active_keyboard.is_some(),
                    has_keymap = state.keymap.is_some(),
                    resumed = state.keyboard_resumed,
                    "libei: Ctrl+V pending, aber Vorbedingungen noch nicht erfuellt"
                );
            }
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
