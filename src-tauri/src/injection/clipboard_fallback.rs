// SPDX-License-Identifier: GPL-3.0-or-later
//! Clipboard-Fallback: aktuellen Inhalt sichern → neuen Text setzen → Paste-
//! Shortcut senden → 200 ms warten → vorherigen Inhalt wiederherstellen.
//!
//! Paste-Shortcut wird via `enigo` simuliert: Cmd+V auf macOS, Ctrl+V sonst.
//! Restore laeuft als detached `tokio::spawn` — wir warten nicht darauf,
//! damit der Hauptpfad nicht blockiert.

use crate::core::error::{Result, VoiceTypeError};
use crate::injection::{InjectOptions, InjectionStrategy, InjectorCapabilities, TextInjector};
use async_trait::async_trait;
use std::time::Duration;
use tauri_plugin_clipboard_manager::ClipboardExt;

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

        let clipboard = self.app_handle.clipboard();
        let saved = clipboard.read_text().ok();

        clipboard
            .write_text(text.to_string())
            .map_err(|e| VoiceTypeError::Injection(format!("clipboard write: {e}")))?;

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
