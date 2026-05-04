// SPDX-License-Identifier: GPL-3.0-or-later
//! Linux-Wayland Text-Injection — Composite-Strategie aus Clipboard + libei.
//!
//! Architektur (siehe CLAUDE.md §11 Phase 5 Teil 2):
//!   1. Text wird auf das System-Clipboard gesetzt.
//!   2. `Ctrl+V` wird via `xdg-desktop-portal.RemoteDesktop` + libei
//!      simuliert. **Iteration 5.2.A** baut nur die Portal-Session auf
//!      und cached den `restore_token`; der eigentliche EIS-Handshake +
//!      Keystroke folgt in 5.2.B.
//!
//! Session-Lifecycle: lazy beim ersten Inject (`ensure_session`), danach
//! gehalten fuer die App-Lebensdauer in einem `Arc<tokio::Mutex<...>>`.
//! Bei Permission-Ablehnung oder Compositor-Unsupport faellt der Injector
//! transparent auf das alte Verhalten zurueck (Clipboard + Notification
//! "Druecke Strg+V") — kein harter Fehler.
//!
//! Verifizierte Quellen (siehe CLAUDE.md §11):
//!   - ashpd 0.11 — Portal-Wrapper
//!   - reis 0.6   — EI-Protokoll (kommt in 5.2.B)
//!   - lan-mouse  — Vorbild fuer den Stack

use crate::core::error::{Result, VoiceTypeError};
use crate::injection::{InjectOptions, InjectorCapabilities, TextInjector};
use async_trait::async_trait;
use std::sync::Arc;
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_notification::NotificationExt;
use tokio::sync::Mutex;

/// Status der RemoteDesktop-Portal-Session. Wird lazy initialisiert; nach
/// erfolgreichem Aufbau bleibt die Session fuer die App-Lebensdauer
/// erhalten. Bei harten Fehlern (User lehnt ab, Portal nicht verfuegbar)
/// gehen wir in `Failed` und loggen das einmal — weitere Inject-Versuche
/// fallen dann silent auf den Clipboard-Pfad zurueck.
enum SessionState {
    /// Noch nicht versucht, Session aufzubauen.
    Uninitialized,
    /// Session steht. In 5.2.B kommt hier der `reis::Backend` +
    /// EIS-File-Descriptor dazu, in 5.2.C der `restore_token` zur
    /// Persistierung.
    Active,
    /// Setup ist gescheitert. Failure-Reason wird einmal geloggt, der
    /// Inject-Pfad faellt auf reines Clipboard zurueck.
    Failed { reason: String },
}

pub struct WaylandLibeiInjector {
    app_handle: tauri::AppHandle,
    session: Arc<Mutex<SessionState>>,
}

impl WaylandLibeiInjector {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self {
            app_handle,
            session: Arc::new(Mutex::new(SessionState::Uninitialized)),
        }
    }

    /// Stellt sicher, dass eine Portal-Session existiert oder als
    /// `Failed` markiert ist. Idempotent — nach erstem Erfolg/Failure
    /// keine erneuten Portal-Aufrufe.
    async fn ensure_session(&self) -> SessionOutcome {
        let mut guard = self.session.lock().await;
        match &*guard {
            SessionState::Active => return SessionOutcome::AlreadyActive,
            SessionState::Failed { reason } => {
                return SessionOutcome::PreviousFailure(reason.clone());
            }
            SessionState::Uninitialized => {}
        }

        match build_remote_desktop_session().await {
            Ok((restore_token, _eis_fd)) => {
                // _eis_fd wird in 5.2.B.2 an den reis-Worker-Thread
                // uebergeben — aktuell wird er hier gedroppt, was die
                // Pipe wieder schliesst. Funktional egal in 5.2.B.1, weil
                // wir noch nicht tippen.
                tracing::info!(
                    has_token = restore_token.is_some(),
                    "RemoteDesktop-Session erfolgreich aufgebaut + EIS-FD bezogen"
                );
                *guard = SessionState::Active;
                SessionOutcome::JustActivated
            }
            Err(e) => {
                let reason = e.to_string();
                tracing::warn!(error = %reason, "RemoteDesktop-Session-Setup fehlgeschlagen — Fallback auf Clipboard+Notification");
                *guard = SessionState::Failed {
                    reason: reason.clone(),
                };
                SessionOutcome::JustFailed(reason)
            }
        }
    }
}

enum SessionOutcome {
    AlreadyActive,
    JustActivated,
    PreviousFailure(String),
    JustFailed(String),
}

#[async_trait]
impl TextInjector for WaylandLibeiInjector {
    fn name(&self) -> &str {
        "linux-wayland-libei"
    }

    fn capabilities(&self) -> InjectorCapabilities {
        InjectorCapabilities {
            supports_clipboard: true,
            // In 5.2.B kommt true, sobald reis-Keystrokes implementiert sind.
            supports_keystrokes: false,
        }
    }

    async fn inject(&self, text: &str, _opts: InjectOptions) -> Result<()> {
        // Schritt 1: Clipboard setzen — passiert immer, unabhaengig vom
        // Session-Status. Auch der Permission-abgelehnt-Fall fuehrt
        // dann wenigstens dazu, dass der Text in der Zwischenablage ist.
        self.app_handle
            .clipboard()
            .write_text(text.to_string())
            .map_err(|e| VoiceTypeError::Injection(format!("clipboard write: {e}")))?;

        // Schritt 2: Portal-Session sicherstellen. In 5.2.A loggen wir
        // nur den Status — der eigentliche Strg+V-Keystroke kommt in
        // 5.2.B. In jedem Fall (Active oder Failed) zeigen wir bis dahin
        // die Strg+V-Notification, damit das aktuelle UX-Verhalten
        // erhalten bleibt.
        let outcome = self.ensure_session().await;
        match &outcome {
            SessionOutcome::JustActivated => {
                tracing::info!("RemoteDesktop-Permission erteilt — Auto-Paste folgt in 5.2.B");
            }
            SessionOutcome::AlreadyActive => {
                tracing::debug!("RemoteDesktop-Session aktiv — Auto-Paste folgt in 5.2.B");
            }
            SessionOutcome::JustFailed(reason) | SessionOutcome::PreviousFailure(reason) => {
                tracing::debug!(reason = %reason, "RemoteDesktop nicht verfuegbar — Clipboard-only");
            }
        }

        // 5.2.A: noch immer manuelles Strg+V — die Notification ist die
        // gleiche wie im ClipboardFallback-Wayland-Pfad. In 5.2.B faellt
        // dieser Block bei aktiver Session weg.
        let _ = self
            .app_handle
            .notification()
            .builder()
            .title("VoiceTypeX")
            .body("Text in der Zwischenablage. Druecke Ctrl+V in der Ziel-App.")
            .show();

        Ok(())
    }
}

/// Baut eine `xdg-desktop-portal.RemoteDesktop`-Session auf:
///   1. `RemoteDesktop::new()` → Proxy fuer das Portal.
///   2. `create_session()` → Session-Handle.
///   3. `select_devices(KEYBOARD, persist_mode=ExplicitlyRevoked)` → fragt
///      Tastatur-Permission an, mit Wunsch auf permanenten Token (= bis
///      User die Erlaubnis aktiv widerruft).
///   4. `start(...)` → Compositor zeigt Permission-Dialog (wenn nicht
///      schon via restore_token genehmigt). Returnet `restore_token`.
///   5. `connect_to_eis(&session)` → liefert den EIS-File-Descriptor, mit
///      dem in 5.2.B.2 das `reis::ei::Context` aufgebaut wird.
///
/// Returnet die ueberfluessigen Bits, die in spaeteren Sub-Iterationen
/// weiterverarbeitet werden:
///   - `restore_token` (5.2.C: persistieren)
///   - `eis_fd` (5.2.B.2: an reis-Worker-Thread uebergeben)
async fn build_remote_desktop_session(
) -> std::result::Result<(Option<String>, std::os::fd::OwnedFd), String> {
    use ashpd::desktop::remote_desktop::{DeviceType, RemoteDesktop};
    use ashpd::desktop::PersistMode;

    let proxy = RemoteDesktop::new()
        .await
        .map_err(|e| format!("RemoteDesktop::new: {e}"))?;
    let session = proxy
        .create_session()
        .await
        .map_err(|e| format!("create_session: {e}"))?;
    proxy
        .select_devices(
            &session,
            DeviceType::Keyboard.into(),
            None, // restore_token kommt in 5.2.C
            PersistMode::ExplicitlyRevoked,
        )
        .await
        .map_err(|e| format!("select_devices: {e}"))?;

    let response = proxy
        .start(&session, None)
        .await
        .map_err(|e| format!("start: {e}"))?
        .response()
        .map_err(|e| format!("start response: {e}"))?;
    let restore_token = response.restore_token().map(|s| s.to_string());

    // EIS-File-Descriptor anfordern. Nach diesem Aufruf hat der Compositor
    // dem Client einen Pipe geoeffnet, ueber den das EI-Protokoll laeuft.
    // In 5.2.B.2 verbinden wir reis damit und fuehren den Handshake durch.
    let eis_fd = proxy
        .connect_to_eis(&session)
        .await
        .map_err(|e| format!("connect_to_eis: {e}"))?;
    tracing::info!(
        eis_fd_raw = ?std::os::fd::AsRawFd::as_raw_fd(&eis_fd),
        "RemoteDesktop EIS-File-Descriptor erhalten"
    );

    Ok((restore_token, eis_fd))
}
