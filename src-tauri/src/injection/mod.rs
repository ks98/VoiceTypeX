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
use crate::core::modes::OutputAction;
use async_trait::async_trait;

pub mod clipboard_fallback;

#[cfg(target_os = "linux")]
pub mod linux_wayland;

#[cfg(target_os = "linux")]
pub mod libei_worker;

#[cfg(target_os = "linux")]
pub mod focus_tracker;

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
    /// Where the text lands relative to a selection (edit modes):
    /// `Replace`/`Insert` paste at the caret (overwriting any active
    /// selection), `Append`/`Prepend` collapse the selection first.
    /// `Auto` is resolved before reaching the injector and is treated
    /// as `Replace` defensively.
    pub action: OutputAction,
    /// Paste with `Ctrl+Shift+V` instead of `Ctrl+V` (terminals like KDE
    /// Konsole ignore plain `Ctrl+V`). Resolved from the mode's
    /// `paste_shortcut` before injection. Only affects the clipboard path.
    pub paste_with_shift: bool,
}

#[async_trait]
pub trait TextInjector: Send + Sync {
    fn name(&self) -> &str;
    fn capabilities(&self) -> InjectorCapabilities;
    async fn inject(&self, text: &str, opts: InjectOptions) -> Result<()>;

    /// Read the text currently selected in the focused target app
    /// (input side of the "Bearbeiten" feature).
    ///
    /// Returns `Ok(None)` when nothing is selected, or when the
    /// selection cannot be read on this platform/session. The
    /// implementation simulates the copy shortcut and reads the
    /// clipboard, so it must run **while the target app still has
    /// focus** — i.e. before the menu/overlay windows steal it (see
    /// `pipeline::handle_menu_hotkey`).
    async fn read_selection(&self) -> Result<Option<String>>;
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

/// Read the current PRIMARY selection (the highlighted text) on Linux.
///
/// Used by the edit-mode input side on both X11 and Wayland: the
/// highlighted text lands in the PRIMARY selection automatically, so we
/// read it directly — no Ctrl+C simulation, no clipboard save/restore,
/// no keyboard focus needed. arboard reads PRIMARY natively on X11 and
/// via wlr/ext-data-control on Wayland (`wayland-data-control` feature).
/// Returns `None` when nothing is selected or PRIMARY is unavailable
/// (some apps — certain Electron builds, terminals — do not populate it).
///
/// arboard's calls block (X11/Wayland round-trip), hence `spawn_blocking`.
#[cfg(target_os = "linux")]
pub(crate) async fn read_primary_selection_linux() -> Option<String> {
    let joined = tokio::task::spawn_blocking(|| -> Option<String> {
        use arboard::{Clipboard, GetExtLinux, LinuxClipboardKind};
        let mut clipboard = Clipboard::new().ok()?;
        let text = clipboard
            .get()
            .clipboard(LinuxClipboardKind::Primary)
            .text()
            .ok()?;
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    })
    .await;
    match joined {
        Ok(opt) => opt,
        Err(e) => {
            tracing::warn!(error = %e, "read_primary_selection spawn_blocking failed");
            None
        }
    }
}
