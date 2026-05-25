// SPDX-License-Identifier: GPL-3.0-or-later
//! Text injection at the active cursor position.
//!
//! Two strategies, chosen by mode via `injection_method`:
//!
//! - `Clipboard` (default): save the clipboard → set the new text →
//!   send the paste shortcut (Ctrl+V / Cmd+V) → restore the original
//!   after 200 ms. On Wayland the auto-paste step falls back to libei
//!   (see `linux_wayland`); on compositors without the RemoteDesktop
//!   portal the text stays in the clipboard with a notification.
//! - `Keystrokes` (opt-in): send the text directly as a sequence of
//!   keypresses (enigo). Useful for terminals and apps with a
//!   non-standard paste shortcut (Ctrl+Shift+V) or restrictive
//!   clipboard access.
//!
//! Platform routing happens in `make_default_injector`: Wayland uses
//! the dedicated `WaylandLibeiInjector` (clipboard + libei via the
//! portal); all other platforms use the `ClipboardFallbackInjector`
//! (X11/Windows with enigo; macOS fallback without auto-paste).

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

/// Returns the default injector. Platform routing:
///   - Linux + Wayland → `WaylandLibeiInjector` (clipboard + libei
///     via xdg-desktop-portal.RemoteDesktop, see §4.9).
///   - otherwise → `ClipboardFallbackInjector` (X11/Windows with
///     auto-paste, macOS stub).
///
/// `wayland_token_path` is the path to the persisted `restore_token`
/// (unused on non-Wayland platforms).
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
