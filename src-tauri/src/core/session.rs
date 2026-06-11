// SPDX-License-Identifier: GPL-3.0-or-later
//! Display-server detection.
//!
//! We decide at runtime, based on the standard env vars, whether the
//! app is running on Wayland, X11 or another platform. This drives:
//!
//! - Hotkey registration: Wayland → xdg-desktop-portal.GlobalShortcuts
//!   (`hotkey::linux_wayland`), X11/Windows →
//!   `tauri-plugin-global-shortcut` (XGrabKey/RegisterHotKey).
//! - Auto-paste: Wayland → libei via xdg-desktop-portal.RemoteDesktop
//!   (`injection::linux_wayland`), X11/Windows → enigo Ctrl+V
//!   (`injection::clipboard_fallback`).

use serde::Serialize;

#[derive(Serialize)]
pub struct SessionInfo {
    /// `wayland`, `x11`, `windows`, `macos`, `unknown`
    pub display_server: String,
    /// True if global hotkeys are expected to work on this session
    /// (Wayland: no, until phase 5-full).
    pub global_hotkeys_supported: bool,
    /// True if the auto-paste shortcut after clipboard-set works
    /// (Wayland: no without libei).
    pub auto_paste_supported: bool,
}

// cfg-dispatch: exactly one arm survives per target, each `return`s its
// SessionInfo — needless only because the other arms are stripped.
#[allow(clippy::needless_return)]
pub fn detect_session() -> SessionInfo {
    #[cfg(target_os = "windows")]
    {
        return SessionInfo {
            display_server: "windows".into(),
            global_hotkeys_supported: true,
            auto_paste_supported: true,
        };
    }
    #[cfg(target_os = "macos")]
    {
        return SessionInfo {
            display_server: "macos".into(),
            global_hotkeys_supported: false, // Phase 6
            auto_paste_supported: false,     // Phase 6
        };
    }
    #[cfg(target_os = "linux")]
    {
        let wayland = std::env::var("WAYLAND_DISPLAY")
            .ok()
            .filter(|v| !v.is_empty())
            .is_some();
        if wayland {
            // Phase 5-full part 1: GlobalShortcuts via xdg-portal.
            // Auto-paste (RemoteDesktop portal) follows in part 2.
            return SessionInfo {
                display_server: "wayland".into(),
                global_hotkeys_supported: true,
                auto_paste_supported: false,
            };
        }
        let x11 = std::env::var("DISPLAY")
            .ok()
            .filter(|v| !v.is_empty())
            .is_some();
        if x11 {
            return SessionInfo {
                display_server: "x11".into(),
                global_hotkeys_supported: true,
                auto_paste_supported: true,
            };
        }
        SessionInfo {
            display_server: "unknown".into(),
            global_hotkeys_supported: false,
            auto_paste_supported: false,
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        SessionInfo {
            display_server: "unknown".into(),
            global_hotkeys_supported: false,
            auto_paste_supported: false,
        }
    }
}

pub fn is_wayland() -> bool {
    detect_session().display_server == "wayland"
}
