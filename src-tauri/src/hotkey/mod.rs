// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared data types for hotkey events.
//!
//! There is **one** global shortcut for the whole app
//! (`Settings.menu_hotkey`). Registration is platform-direct because
//! the X11/Windows callback APIs and the Wayland portal session are
//! structurally too different to fit behind a shared trait:
//!
//! - X11/Windows: `pipeline::register_menu_hotkey()` calls
//!   `app.global_shortcut().on_shortcut()` from
//!   `tauri-plugin-global-shortcut` directly; only
//!   `ShortcutState::Pressed` is handled.
//! - Wayland: `pipeline::spawn_wayland_hotkey_session()` starts
//!   `linux_wayland::run_global_shortcuts_session()` as a long-lived
//!   task; the session binds a single `WaylandShortcutSpec` entry to
//!   `xdg-desktop-portal.GlobalShortcuts` and dispatches `HotkeyEvent`s
//!   through a broadcast channel.
//! - macOS: out of scope (see CLAUDE.md §11).

#[cfg(target_os = "linux")]
pub mod linux_wayland;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEventKind {
    /// Hotkey was pressed (KeyDown).
    Pressed,
    /// Hotkey was released (KeyUp). On Wayland not reliably delivered
    /// by all compositors.
    Released,
}

#[derive(Debug, Clone)]
pub struct HotkeyEvent {
    pub id: String,
    pub kind: HotkeyEventKind,
}
