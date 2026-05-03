// SPDX-License-Identifier: GPL-3.0-or-later
//! Clipboard-Fallback mit Session-Awareness.
//!
//! Auf Plattformen, wo Auto-Paste funktioniert (X11, Windows):
//!   1. Aktuellen Clipboard-Inhalt sichern
//!   2. Neuen Text setzen
//!   3. Paste-Shortcut senden (Ctrl+V / Cmd+V via enigo)
//!   4. Nach 200 ms vorherigen Inhalt wiederherstellen
//!
//! Auf Wayland (oder anderen "auto_paste_supported = false"):
//!   - Setze nur den neuen Text
//!   - **Kein** Paste-Shortcut (enigo's XTest-Pfad scheitert silent)
//!   - **Kein** Restore (würde den Text nach 200 ms überschreiben, bevor
//!     der User ihn manuell pasten kann)
//!   - Stattdessen Notification "Drücke Ctrl+V in der Ziel-App"

use crate::core::error::{Result, VoiceTypeError};
use crate::core::session::detect_session;
use crate::injection::{InjectOptions, InjectionStrategy, InjectorCapabilities, TextInjector};
use async_trait::async_trait;
use std::time::Duration;
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_notification::NotificationExt;

const RESTORE_DELAY_MS: u64 = 200;

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
            // Phase 1 setzt strikt auf Clipboard; "Keystrokes"-Wunsch wird
            // toleriert, aber der gleiche Pfad genutzt.
            supports_keystrokes: false,
        }
    }

    async fn inject(&self, text: &str, opts: InjectOptions) -> Result<()> {
        if opts.strategy == InjectionStrategy::Keystrokes {
            tracing::info!(
                "injection_method=keystrokes angefragt, aber Phase 1 nutzt Clipboard-Fallback"
            );
        }

        let session = detect_session();
        let clipboard = self.app_handle.clipboard();

        // Reihenfolge wichtig: erst original-Inhalt sichern, dann neuen setzen.
        let saved = if session.auto_paste_supported {
            clipboard.read_text().ok()
        } else {
            // Auf Wayland kein Restore sinnvoll, daher kein Save noetig.
            None
        };

        clipboard
            .write_text(text.to_string())
            .map_err(|e| VoiceTypeError::Injection(format!("clipboard write: {e}")))?;

        if !session.auto_paste_supported {
            tracing::info!(
                display_server = %session.display_server,
                "Clipboard gesetzt — Auto-Paste nicht verfuegbar, User muss Ctrl+V druecken"
            );
            let _ = self
                .app_handle
                .notification()
                .builder()
                .title("VoiceTypeX")
                .body("Text in der Zwischenablage. Druecke Ctrl+V in der Ziel-App.")
                .show();
            return Ok(());
        }

        // X11 / Windows: vollstaendiger Save → Set → Paste → Restore-Pfad.
        send_paste_shortcut().await?;

        if let Some(prev) = saved {
            let app = self.app_handle.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(RESTORE_DELAY_MS)).await;
                if let Err(e) = app.clipboard().write_text(prev) {
                    tracing::warn!(error = %e, "Clipboard-Restore fehlgeschlagen");
                }
            });
        }

        Ok(())
    }
}

/// Sende Cmd+V (macOS) oder Ctrl+V (sonst) via enigo. enigo's Initialisierung
/// kann blockieren, deshalb spawn_blocking.
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
