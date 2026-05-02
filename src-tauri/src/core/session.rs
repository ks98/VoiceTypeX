// SPDX-License-Identifier: GPL-3.0-or-later
//! Display-Server-Detection.
//!
//! Wir entscheiden zur Laufzeit anhand der Standard-Env-Variablen, ob die
//! App auf Wayland, X11 oder einer anderen Plattform laeuft. Das hat
//! Auswirkungen auf Hotkey-Registrierung und Auto-Paste-Shortcut: beides
//! schlaegt unter Wayland in Phase 1–3 fehl, weil `XGrabKey` und
//! `XTest` nicht erlaubt sind. Phase 5 wird das via xdg-desktop-portal
//! und libei reparieren.

use crate::ipc::diagnostics::SessionInfo;

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
            return SessionInfo {
                display_server: "wayland".into(),
                global_hotkeys_supported: false,
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
