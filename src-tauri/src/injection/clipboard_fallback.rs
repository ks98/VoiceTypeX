// SPDX-License-Identifier: GPL-3.0-or-later
//! Clipboard fallback with session awareness and a keystrokes-direct
//! mode.
//!
//! Default path — `InjectionStrategy::Clipboard`, on X11/Windows:
//!   1. Save the current clipboard contents.
//!   2. Set the new text.
//!   3. Send the paste shortcut (Ctrl+V / Cmd+V via enigo).
//!   4. After 200 ms, restore the previous contents.
//!
//! Direct path — `InjectionStrategy::Keystrokes`, X11/Windows:
//!   - The text is typed character-by-character via enigo (no
//!     clipboard).
//!   - Useful for terminals (`Ctrl+V` is often `Ctrl+Shift+V`),
//!     IME-sensitive apps and input fields with clipboard blockers.
//!   - Trade-off: layout-dependent, slower than paste, Unicode chars
//!     outside the layout can fail.
//!
//! Wayland or other "auto_paste_supported = false" + clipboard
//! strategy:
//!   - Only set the new text.
//!   - **No** paste shortcut (enigo's XTest path fails silently).
//!   - **No** restore (would overwrite the text after 200 ms, before
//!     the user can paste manually).
//!   - Instead show a notification "Press Ctrl+V in the target app".

use crate::core::error::{Result, VoiceTypeError};
use crate::core::session::detect_session;
use crate::injection::{InjectOptions, InjectionStrategy, InjectorCapabilities, TextInjector};
use async_trait::async_trait;
use std::time::Duration;
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_notification::NotificationExt;

const RESTORE_DELAY_MS: u64 = 200;

/// How long to wait after the simulated copy shortcut before reading
/// the clipboard. The target app needs a moment to service Ctrl+C and
/// update the clipboard. Coarse fixed delay (matches the project's
/// existing timing approach); a clipboard sequence-number poll would be
/// a later refinement.
const COPY_SETTLE_MS: u64 = 120;

pub struct ClipboardFallbackInjector {
    app_handle: tauri::AppHandle,
}

impl ClipboardFallbackInjector {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self { app_handle }
    }
}

#[async_trait]
impl TextInjector for ClipboardFallbackInjector {
    fn name(&self) -> &str {
        "clipboard-fallback"
    }

    fn capabilities(&self) -> InjectorCapabilities {
        InjectorCapabilities {
            supports_clipboard: true,
            supports_keystrokes: true,
        }
    }

    async fn inject(&self, text: &str, opts: InjectOptions) -> Result<()> {
        let session = detect_session();

        if opts.strategy == InjectionStrategy::Keystrokes {
            // On macOS / unknown sessions enigo has no reliable path
            // for direct text injection. We try anyway — on failure
            // the only option is to bubble the hard error up.
            return inject_keystrokes(text).await;
        }

        let clipboard = self.app_handle.clipboard();

        // Order matters: first save the original contents, then set
        // the new value.
        let saved = if session.auto_paste_supported {
            clipboard.read_text().ok()
        } else {
            // No restore makes sense on Wayland, so no save needed.
            None
        };

        clipboard
            .write_text(text.to_string())
            .map_err(|e| VoiceTypeError::Injection(format!("clipboard write: {e}")))?;

        if !session.auto_paste_supported {
            tracing::info!(
                display_server = %session.display_server,
                "Clipboard set — auto-paste unavailable, user must press Ctrl+V"
            );
            let _ = self
                .app_handle
                .notification()
                .builder()
                .title("VoiceTypeX")
                .body("Text copied to clipboard. Press Ctrl+V in the target app.")
                .show();
            return Ok(());
        }

        // X11 / Windows: full save → set → paste → restore path.
        send_paste_shortcut().await?;

        if let Some(prev) = saved {
            let app = self.app_handle.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(RESTORE_DELAY_MS)).await;
                if let Err(e) = app.clipboard().write_text(prev) {
                    tracing::warn!(error = %e, "Clipboard restore failed");
                }
            });
        }

        Ok(())
    }

    async fn read_selection(&self) -> Result<Option<String>> {
        let session = detect_session();

        // We can only simulate the copy shortcut reliably where enigo's
        // key path works (X11/Windows). On "auto_paste_supported =
        // false" sessions (Wayland is routed to WaylandLibeiInjector;
        // this covers unknown/headless) we cannot read the selection
        // here.
        if !session.auto_paste_supported {
            tracing::debug!(
                display_server = %session.display_server,
                "read_selection: session has no key-simulation path, returning None"
            );
            return Ok(None);
        }

        let clipboard = self.app_handle.clipboard();

        // Save the current clipboard so the user's content survives the
        // copy round-trip — and so we can detect "nothing was selected"
        // (Ctrl+C then leaves the clipboard unchanged).
        let saved = clipboard.read_text().ok();

        send_copy_shortcut().await?;
        tokio::time::sleep(Duration::from_millis(COPY_SETTLE_MS)).await;
        let after = clipboard.read_text().ok();

        // Restore the original clipboard. We already read `after`, so
        // restoring immediately is safe (unlike the paste path, which
        // must keep the text long enough for Ctrl+V).
        if let Some(prev) = &saved {
            if let Err(e) = clipboard.write_text(prev.clone()) {
                tracing::warn!(error = %e, "Clipboard restore after selection read failed");
            }
        }

        // "Nothing selected" heuristic: the copy left the clipboard
        // empty or unchanged. A false negative is possible if the
        // selection text equals the prior clipboard content — accepted
        // edge case (see docs/PLATFORMS.md).
        match after {
            Some(text) if !text.is_empty() && saved.as_ref() != Some(&text) => Ok(Some(text)),
            _ => Ok(None),
        }
    }
}

/// Types the text directly via enigo's `Keyboard::text` — no
/// clipboard detour, no paste shortcut. enigo picks the platform
/// path itself (Windows SendInput, X11 XTest, macOS CGEvent). On
/// Wayland the XTest path would fail silently; this code path is
/// not reached there because `make_default_injector` routes Wayland
/// sessions to `WaylandLibeiInjector` (see injection/mod.rs).
async fn inject_keystrokes(text: &str) -> Result<()> {
    let owned = text.to_string();
    tokio::task::spawn_blocking(move || -> Result<()> {
        use enigo::{Enigo, Keyboard, Settings};

        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| VoiceTypeError::Injection(format!("enigo::new: {e}")))?;
        enigo
            .text(&owned)
            .map_err(|e| VoiceTypeError::Injection(format!("enigo.text: {e}")))?;
        Ok(())
    })
    .await
    .map_err(|e| VoiceTypeError::Injection(format!("spawn_blocking: {e}")))?
}

/// Send Cmd+C (macOS) or Ctrl+C (otherwise) via enigo to copy the
/// current selection of the focused app. Mirror of
/// `send_paste_shortcut`; enigo init can block, hence `spawn_blocking`.
async fn send_copy_shortcut() -> Result<()> {
    tokio::task::spawn_blocking(move || -> Result<()> {
        use enigo::{Direction, Enigo, Key, Keyboard, Settings};

        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| VoiceTypeError::Injection(format!("enigo::new: {e}")))?;

        #[cfg(target_os = "macos")]
        let modifier = Key::Meta;
        #[cfg(not(target_os = "macos"))]
        let modifier = Key::Control;

        enigo
            .key(modifier, Direction::Press)
            .map_err(|e| VoiceTypeError::Injection(format!("modifier press: {e}")))?;
        enigo
            .key(Key::Unicode('c'), Direction::Click)
            .map_err(|e| VoiceTypeError::Injection(format!("C click: {e}")))?;
        enigo
            .key(modifier, Direction::Release)
            .map_err(|e| VoiceTypeError::Injection(format!("modifier release: {e}")))?;

        Ok(())
    })
    .await
    .map_err(|e| VoiceTypeError::Injection(format!("spawn_blocking: {e}")))?
}

/// Send Cmd+V (macOS) or Ctrl+V (otherwise) via enigo. enigo's
/// initialization can block, hence `spawn_blocking`.
async fn send_paste_shortcut() -> Result<()> {
    tokio::task::spawn_blocking(move || -> Result<()> {
        use enigo::{Direction, Enigo, Key, Keyboard, Settings};

        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| VoiceTypeError::Injection(format!("enigo::new: {e}")))?;

        #[cfg(target_os = "macos")]
        let modifier = Key::Meta;
        #[cfg(not(target_os = "macos"))]
        let modifier = Key::Control;

        enigo
            .key(modifier, Direction::Press)
            .map_err(|e| VoiceTypeError::Injection(format!("modifier press: {e}")))?;
        enigo
            .key(Key::Unicode('v'), Direction::Click)
            .map_err(|e| VoiceTypeError::Injection(format!("V click: {e}")))?;
        enigo
            .key(modifier, Direction::Release)
            .map_err(|e| VoiceTypeError::Injection(format!("modifier release: {e}")))?;

        Ok(())
    })
    .await
    .map_err(|e| VoiceTypeError::Injection(format!("spawn_blocking: {e}")))?
}
