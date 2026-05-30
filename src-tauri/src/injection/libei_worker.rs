// SPDX-License-Identifier: GPL-3.0-or-later
//! libei worker for keypress simulation on Wayland.
//!
//! Spawned by the `WaylandLibeiInjector` as a dedicated `std::thread`
//! once the `RemoteDesktop` portal session is set up and an EIS file
//! descriptor is available (see `linux_wayland.rs`).
//!
//! Architecture (see CLAUDE.md §11):
//!
//! ```text
//!   tokio (main thread)              std::thread (worker)
//!   ─────────────────                 ──────────────────────
//!   inject() / Ctrl+V request  ──>   poll-loop {
//!                                       1) ei::Context::read() (non-block)
//!                                          → server events
//!                                       2) cmd_rx.try_recv()
//!                                          → KeyCommand::CtrlV
//!                                       3) if keymap+keyboard ready:
//!                                          send_ctrl_v(&context)
//!                                       4) context.flush()
//!                                       5) sleep 20 ms
//!                                    }
//! ```
//!
//! Why manual poll instead of calloop:
//! - calloop's architecture separates sources strictly; the cmd handler
//!   would have no access to the `ei::Context` without Rc/RefCell tricks.
//! - A simple poll loop with a non-blocking FD is more robust and easier
//!   to reason about.
//! - 20 ms polling latency is negligible against the compositor
//!   round-trip.
//!
//! Reference for the EI handshake flow: `examples/type-text.rs` from
//! github.com/ids1024/reis (same sequence: Handshake → Connection →
//! Seat → Device → Keyboard → start_emulating → key events → frame).

use reis::{ei, PendingRequestResult};
use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::time::Duration;
use tokio::sync::oneshot;
use xkbcommon::xkb;

/// Commands the tokio main thread sends to the libei worker.
#[derive(Debug)]
pub enum KeyCommand {
    /// Send Ctrl+V (press Ctrl, press V, release V, release Ctrl).
    CtrlV,
    /// Shut down the worker cleanly.
    Shutdown,
}

/// List of interfaces we request from the EI server. Versions must
/// match the server — `1` is the current common denominator that libei
/// 1.x interoperates with.
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
    /// As soon as we have a keyboard device that reported `Done`, it
    /// ends up here. Until then we cannot type.
    active_keyboard: Option<(ei::Device, ei::Keyboard)>,
    /// Only after a `Resumed` event may the server actually accept
    /// typing (libei protocol.xml spec: "client bug to request emulation
    /// on a device that is not resumed. The EIS implementation may
    /// silently discard such events").
    keyboard_resumed: bool,
    /// `start_emulating` was called and no `stop_emulating` / `Paused`
    /// has come in since then. Following the lan-mouse pattern: call
    /// once in the `Resumed` handler, then keep persistent — per
    /// Ctrl+V only keys + frame, no new `start_emulating`.
    emulation_active: bool,
    ready_tx: Option<oneshot::Sender<bool>>,
    /// A Ctrl+<key> combo requested from the tokio side, pending until
    /// the keyboard is ready. `Some(v)` = paste, `Some(c)` = copy.
    pending_key: Option<xkb::Keysym>,
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
                    "libei: all preconditions met (keyboard + keymap + resumed) — auto-paste ready"
                );
                let _ = tx.send(true);
            }
        }
    }

    /// Response to the server's initial handshake and discovery event
    /// stream. See `type-text.rs` for the reference implementation.
    /// Diagnostic logging at every step, because the setup is multi-
    /// stage and not traceable without logs.
    fn handle_request(&mut self, request: ei::Event) {
        match request {
            ei::Event::Handshake(handshake, request) => match request {
                ei::handshake::Event::HandshakeVersion { version } => {
                    tracing::info!(server_version = version, "libei: handshake started");
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
                    tracing::info!(serial, "libei: connection established");
                    self.last_serial = serial;
                }
                _ => {}
            },
            ei::Event::Connection(_connection, request) => match request {
                ei::connection::Event::Seat { seat } => {
                    tracing::debug!("libei: Seat object received");
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
                                "libei: Seat done WITHOUT ei_keyboard capability — auto-paste not possible"
                            );
                        }
                    }
                    ei::seat::Event::Device { device } => {
                        tracing::debug!("libei: Device object received");
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
                        tracing::info!(?interfaces, "libei: Device done");
                        if let Some(keyboard) = data.interface::<ei::Keyboard>() {
                            tracing::info!(
                                "libei: keyboard interface found — waiting for Resumed event"
                            );
                            self.active_keyboard = Some((device, keyboard));
                            self.try_signal_ready();
                        } else {
                            tracing::warn!(
                                "libei: Device done WITHOUT ei_keyboard interface — auto-paste not possible"
                            );
                        }
                    }
                    ei::device::Event::Resumed { serial } => {
                        tracing::info!(serial, "libei: Device resumed — sender may type");
                        self.last_serial = serial;
                        self.keyboard_resumed = true;
                        // `start_emulating` once per the lan-mouse
                        // pattern: exactly once per Resumed, then
                        // persistent. If a Paused event arrives, we
                        // reset and the next Resumed starts a new
                        // emulation sequence with an incremented
                        // sequence counter (spec: monotonic).
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
                        tracing::warn!(serial, "libei: Device paused — sender must stop typing");
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
                // SAFETY: `keymap` is an `OwnedFd` from the server,
                // `size` is the correct map size. reis delivers both
                // from the protocol in a suitable form by guarantee.
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
                        tracing::warn!("libei: keymap parsing returned None");
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "libei: keymap parsing failed");
                    }
                }
            }
            _ => {}
        }
    }

    /// Send a `Ctrl+<key>` sequence (e.g. Ctrl+V to paste, Ctrl+C to
    /// copy a selection). Only call when `is_ready()` is true, i.e.
    /// keyboard, keymap and resumed are all set and emulation is active
    /// (established in the `Resumed` handler). NO
    /// `start_emulating`/`stop_emulating` here — the emulation session
    /// is persistent from the first Resumed.
    ///
    /// **Important:** we pass `context` so we can flush after EVERY
    /// frame. Without flush reis collects all four `device.frame`
    /// records in the TX buffer and ships them as a bundle on the next
    /// loop tick — KWin then sees them nearly simultaneously on the
    /// wire stream, no matter how nicely the `time_us` stamps are
    /// spaced. With per-frame flush + 1 ms sleep we simulate the
    /// keyboard scan rhythm (around 1 kHz).
    ///
    /// Four frames, one per key transition. Each `device.frame` is one
    /// "logical hardware event" (libei spec); the pattern matches what
    /// lan-mouse uses in production.
    fn send_ctrl_combo(&mut self, context: &ei::Context, key_sym: xkb::Keysym) {
        let Some((device, keyboard)) = &self.active_keyboard else {
            tracing::warn!("libei: send_ctrl_combo without active keyboard");
            return;
        };
        let Some(keymap) = &self.keymap else {
            tracing::warn!("libei: send_ctrl_combo without keymap");
            return;
        };
        if !self.emulation_active {
            tracing::warn!("libei: send_ctrl_combo without active emulation (no Resumed?)");
            return;
        }

        let Some(ctrl_keycode) = find_keycode(keymap, xkb::Keysym::Control_L) else {
            tracing::warn!("libei: Control_L not in keymap");
            return;
        };
        let Some(key_keycode) = find_keycode(keymap, key_sym) else {
            tracing::warn!(keysym = ?key_sym, "libei: requested keysym not in keymap");
            return;
        };

        let serial = self.last_serial;
        // EI convention: keycode minus 8 (XKB keycodes are +8 relative
        // to Linux keycodes). Spec requires evdev keycodes
        // (KEY_LEFTCTRL=29, KEY_V=47, KEY_C=46).
        let ctrl_evdev = ctrl_keycode - 8;
        let key_evdev = key_keycode - 8;

        // Helper: write the frame + flush immediately + briefly sleep
        // (simulate the keyboard scan rhythm).
        let mut last_t = 0u64;
        let send_frame = |key: u32, state: ei::keyboard::KeyState, prev_t: u64| -> u64 {
            let t = monotonic_time_us().max(prev_t + 1);
            keyboard.key(key, state);
            device.frame(serial, t);
            if let Err(e) = context.flush() {
                tracing::warn!(error = %e, "libei: flush failed during Ctrl+combo");
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
            t
        };

        last_t = send_frame(ctrl_evdev, ei::keyboard::KeyState::Press, last_t);
        last_t = send_frame(key_evdev, ei::keyboard::KeyState::Press, last_t);
        last_t = send_frame(key_evdev, ei::keyboard::KeyState::Released, last_t);
        last_t = send_frame(ctrl_evdev, ei::keyboard::KeyState::Released, last_t);

        tracing::info!(
            serial,
            ctrl_evdev,
            key_evdev,
            last_time_us = last_t,
            "libei: Ctrl+combo sent (4 frames with per-frame flush + 1 ms pause)"
        );
    }
}

/// Searches the keymap for the keycode that produces a given keysym
/// **without modifiers** (shift level 0). For the Ctrl and v keys this
/// is the right choice.
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

/// Anchor for CLOCK_MONOTONIC microseconds. Initialized at worker
/// start (see `run_libei_worker`) so the first `elapsed()` in
/// `monotonic_time_us` is never 0 (handshake + discovery run before
/// that, which is many milliseconds).
static MONOTONIC_ANCHOR: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// CLOCK_MONOTONIC microseconds for `frame(serial, time)`. The EI spec
/// requires monotonic — Rust's `Instant` is `CLOCK_MONOTONIC` on Linux
/// by platform guarantee. Compositors (especially KWin) compare `time`
/// against their own MONOTONIC clock; if the value is strongly in the
/// past or future (e.g. UNIX_EPOCH), this can cause silently dropped
/// frames. Reference: lan-mouse.
fn monotonic_time_us() -> u64 {
    let anchor = MONOTONIC_ANCHOR.get_or_init(std::time::Instant::now);
    anchor.elapsed().as_micros() as u64
}

/// Worker main loop. Runs until `KeyCommand::Shutdown` is received or
/// the sender is closed (e.g. app shutdown).
pub fn run_libei_worker(
    fd: OwnedFd,
    cmd_rx: mpsc::Receiver<KeyCommand>,
    ready_tx: oneshot::Sender<bool>,
) {
    // Initialize the CLOCK_MONOTONIC anchor early so on the first
    // `frame()` call `elapsed()` already reports a few hundred
    // milliseconds (handshake + discovery) — not 0, which some
    // compositors would reject as invalid.
    let _ = monotonic_time_us();

    let stream = UnixStream::from(fd);
    if let Err(e) = stream.set_nonblocking(true) {
        tracing::error!(error = %e, "libei: set_nonblocking failed");
        let _ = ready_tx.send(false);
        return;
    }

    let context = match ei::Context::new(stream) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "libei: ei::Context::new failed");
            let _ = ready_tx.send(false);
            return;
        }
    };
    let _handshake = context.handshake();
    if let Err(e) = context.flush() {
        tracing::error!(error = %e, "libei: first flush failed");
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
        pending_key: None,
        running: true,
    };

    let poll_interval = Duration::from_millis(20);
    let setup_deadline = std::time::Instant::now() + Duration::from_secs(5);

    while state.running {
        // 1. Read server events (non-blocking thanks to set_nonblocking)
        match context.read() {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => {
                tracing::warn!(error = %e, "libei: context.read() failed — worker exiting");
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

        // 2. Poll commands from the tokio side
        loop {
            match cmd_rx.try_recv() {
                Ok(KeyCommand::CtrlV) => state.pending_key = Some(xkb::Keysym::v),
                Ok(KeyCommand::Shutdown) => state.running = false,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    state.running = false;
                    break;
                }
            }
        }

        // 3. Execute a pending Ctrl+<key> combo once everything is ready
        if let Some(key_sym) = state.pending_key {
            if state.is_ready() {
                state.send_ctrl_combo(&context, key_sym);
                state.pending_key = None;
            } else {
                tracing::trace!(
                    has_keyboard = state.active_keyboard.is_some(),
                    has_keymap = state.keymap.is_some(),
                    resumed = state.keyboard_resumed,
                    "libei: Ctrl+combo pending, but preconditions not yet met"
                );
            }
        }

        // 4. Setup timeout: if no keyboard is ready after 5 s, signal
        //    failure and exit.
        if state.ready_tx.is_some() && std::time::Instant::now() > setup_deadline {
            tracing::warn!("libei: setup timeout — no keyboard device ready within 5s");
            if let Some(tx) = state.ready_tx.take() {
                let _ = tx.send(false);
            }
            state.running = false;
        }

        // 5. Flush + sleep
        let _ = context.flush();
        std::thread::sleep(poll_interval);
    }

    // Cleanup: acknowledge an unsent ready signal with `false` so the
    // tokio side does not hang.
    if let Some(tx) = state.ready_tx.take() {
        let _ = tx.send(false);
    }
    tracing::info!("libei worker exited");
}
