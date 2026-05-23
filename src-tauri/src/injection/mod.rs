// SPDX-License-Identifier: GPL-3.0-or-later
//! Text-Injection an die aktive Cursor-Position.
//!
//! Zwei Strategien, vom Modus per `injection_method` waehlbar:
//!
//! - `Clipboard` (Default): Clipboard sichern → neuen Text setzen → Paste-
//!   Shortcut senden (Ctrl+V / Cmd+V) → nach 200 ms Original wiederherstellen.
//!   Auf Wayland faellt der Auto-Paste-Schritt auf libei (siehe
//!   `linux_wayland`) zurueck; auf Compositors ohne RemoteDesktop-Portal
//!   bleibt der Text im Clipboard mit Notification.
//! - `Keystrokes` (Opt-in): Text direkt als Folge von Tastendruecken senden
//!   (enigo). Sinnvoll fuer Terminals und Apps mit nicht-standard Paste-
//!   Shortcut (Ctrl+Shift+V) oder restriktivem Clipboard-Zugriff.
//!
//! Plattform-Routing erfolgt in `make_default_injector`: Wayland nutzt den
//! dedizierten `WaylandLibeiInjector` (Clipboard + libei via Portal), alle
//! anderen Plattformen den `ClipboardFallbackInjector` (X11/Windows mit
//! enigo; macOS-Fallback ohne Auto-Paste).

use crate::core::error::Result;
use async_trait::async_trait;

pub mod clipboard_fallback;

#[cfg(target_os = "linux")]
pub mod linux_wayland;

#[cfg(target_os = "linux")]
pub mod libei_worker;

#[derive(Debug, Clone, Copy, Default)]
pub struct InjectorCapabilities {
    pub supports_clipboard: bool,
    pub supports_keystrokes: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectionStrategy {
    Clipboard,
    Keystrokes,
}

#[derive(Debug, Clone)]
pub struct InjectOptions {
    pub strategy: InjectionStrategy,
}

#[async_trait]
pub trait TextInjector: Send + Sync {
    fn name(&self) -> &str;
    fn capabilities(&self) -> InjectorCapabilities;
    async fn inject(&self, text: &str, opts: InjectOptions) -> Result<()>;
}

/// Liefert den Default-Injector. Plattform-Routing:
///   - Linux + Wayland → `WaylandLibeiInjector` (Clipboard + libei via
///     xdg-desktop-portal.RemoteDesktop, siehe §4.9).
///   - sonst → `ClipboardFallbackInjector` (X11/Windows mit Auto-Paste,
///     macOS Stub).
///
/// `wayland_token_path` ist der Pfad zum persistierten `restore_token`
/// (auf Nicht-Wayland-Plattformen ungenutzt).
pub fn make_default_injector(
    app_handle: tauri::AppHandle,
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
    wayland_token_path: std::path::PathBuf,
) -> Box<dyn TextInjector> {
    #[cfg(target_os = "linux")]
    {
        if crate::core::session::is_wayland() {
            return Box::new(linux_wayland::WaylandLibeiInjector::new(
                app_handle,
                wayland_token_path,
            ));
        }
    }
    Box::new(clipboard_fallback::ClipboardFallbackInjector::new(
        app_handle,
    ))
}
