// SPDX-License-Identifier: GPL-3.0-or-later
//! Geteilte Datentypen für Hotkey-Events.
//!
//! Es gibt **einen** globalen Shortcut für die ganze App
//! (`Settings.menu_hotkey`). Die Registrierung ist plattform-direkt,
//! weil X11/Windows-Callback-APIs und die Wayland-Portal-Session
//! strukturell zu unterschiedlich sind, um sinnvoll hinter ein Trait zu
//! passen:
//!
//! - X11/Windows: `pipeline::register_menu_hotkey()` ruft
//!   `app.global_shortcut().on_shortcut()` aus `tauri-plugin-global-shortcut`
//!   direkt; nur `ShortcutState::Pressed` wird verarbeitet.
//! - Wayland: `pipeline::spawn_wayland_hotkey_session()` startet
//!   `linux_wayland::run_global_shortcuts_session()` als langlebige Task;
//!   die Session bindet einen einzigen `WaylandShortcutSpec`-Eintrag an
//!   `xdg-desktop-portal.GlobalShortcuts` und dispatched `HotkeyEvent`s
//!   über einen broadcast-Channel.
//! - macOS: out-of-scope (siehe CLAUDE.md §11).

#[cfg(target_os = "linux")]
pub mod linux_wayland;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEventKind {
    /// Hotkey wurde gedrueckt (KeyDown).
    Pressed,
    /// Hotkey wurde losgelassen (KeyUp). Auf Wayland nicht von allen
    /// Compositors zuverlaessig geliefert.
    Released,
}

#[derive(Debug, Clone)]
pub struct HotkeyEvent {
    pub id: String,
    pub kind: HotkeyEventKind,
}
