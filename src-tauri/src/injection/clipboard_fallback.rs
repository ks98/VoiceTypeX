// SPDX-License-Identifier: GPL-3.0-or-later
//! Clipboard-Fallback mit Session-Awareness und Keystrokes-Direct-Modus.
//!
//! Default-Pfad — `InjectionStrategy::Clipboard`, auf X11/Windows:
//!   1. Aktuellen Clipboard-Inhalt sichern
//!   2. Neuen Text setzen
//!   3. Paste-Shortcut senden (Ctrl+V / Cmd+V via enigo)
//!   4. Nach 200 ms vorherigen Inhalt wiederherstellen
//!
//! Direct-Pfad — `InjectionStrategy::Keystrokes`, X11/Windows:
//!   - Text wird Zeichen fuer Zeichen via enigo getippt (kein Clipboard).
//!   - Sinnvoll fuer Terminals (`Ctrl+V` ist dort oft `Ctrl+Shift+V`),
//!     IME-empfindliche Apps und Eingabefelder mit Clipboard-Blockern.
//!   - Trade-off: Layout-abhaengig, langsamer als Paste, Unicode-Chars
//!     ausserhalb des Layouts koennen scheitern.
//!
//! Wayland oder andere "auto_paste_supported = false" + Clipboard-Strategy:
//!   - Setze nur den neuen Text
//!   - **Kein** Paste-Shortcut (enigo's XTest-Pfad scheitert silent)
//!   - **Kein** Restore (wuerde den Text nach 200 ms ueberschreiben, bevor
//!     der User ihn manuell pasten kann)
//!   - Stattdessen Notification "Druecke Ctrl+V in der Ziel-App"

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
            supports_keystrokes: true,
        }
    }

    async fn inject(&self, text: &str, opts: InjectOptions) -> Result<()> {
        let session = detect_session();

        if opts.strategy == InjectionStrategy::Keystrokes {
            // Auf macOS / unbekannten Sessions hat enigo keinen verlaesslichen
            // Pfad fuer direkten Text-Inject. Wir versuchen es trotzdem —
            // bei Fehler bleibt nichts uebrig als der harte Error nach oben.
            return inject_keystrokes(text).await;
        }

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

/// Tippt den Text direkt via enigo's `Keyboard::text` — kein Clipboard-Umweg,
/// kein Paste-Shortcut. enigo waehlt selbst den Plattform-Pfad (Windows
/// SendInput, X11 XTest, macOS CGEvent). Auf Wayland wuerde der XTest-Pfad
/// silent scheitern; dieser Code-Pfad wird dort aber nicht erreicht, weil
/// `make_default_injector` Wayland-Sessions auf `WaylandLibeiInjector`
/// routet (siehe injection/mod.rs).
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
