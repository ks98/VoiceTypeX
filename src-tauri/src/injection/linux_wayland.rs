// SPDX-License-Identifier: GPL-3.0-or-later
//! Linux-Wayland Text-Injection — Composite-Strategie aus Clipboard + libei.
//!
//! Architektur (siehe CLAUDE.md §11 Phase 5 Teil 2):
//!   1. Text wird auf das System-Clipboard gesetzt.
//!   2. `Ctrl+V` wird via `xdg-desktop-portal.RemoteDesktop` + libei
//!      simuliert. Implementiert in den Sub-Iterationen 5.2.A bis 5.2.B.3:
//!        - 5.2.A: Portal-Session lazy beim ersten Hotkey
//!        - 5.2.B.1: connect_to_eis() + EIS-FD
//!        - 5.2.B.2: reis-Worker-Thread + EI-Handshake
//!        - 5.2.B.3: tatsaechlicher Strg+V-Keystroke
//!
//! Session-Lifecycle: lazy beim ersten Inject (`ensure_session`), danach
//! gehalten fuer die App-Lebensdauer in einem `Arc<tokio::Mutex<...>>`.
//! Der libei-Worker laeuft als dedizierter `std::thread` und kommuniziert
//! ueber einen `mpsc::Sender<KeyCommand>` mit dem tokio-Hauptthread.
//!
//! Failure-UX: bei Permission-Ablehnung, Compositor-Unsupport oder
//! Worker-Setup-Timeout faellt der Injector silent auf Clipboard +
//! Notification ("Druecke Strg+V") zurueck — kein harter Fehler.

use crate::core::error::{Result, VoiceTypeError};
use crate::injection::libei_worker::{run_libei_worker, KeyCommand};
use crate::injection::{InjectOptions, InjectorCapabilities, TextInjector};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_notification::NotificationExt;
use tokio::sync::Mutex;

/// Status der RemoteDesktop-Portal-Session + libei-Worker. Wird lazy
/// initialisiert; nach erfolgreichem Aufbau bleibt sie fuer die
/// App-Lebensdauer erhalten. Bei harten Fehlern gehen wir in `Failed`
/// und loggen das einmal — weitere Inject-Versuche fallen dann silent
/// auf den Clipboard-Pfad zurueck.
enum SessionState {
    /// Noch nicht versucht, Session aufzubauen.
    Uninitialized,
    /// Worker laeuft, Keyboard-Device ist ready, Cmds koennen geschickt werden.
    Active {
        cmd_tx: std::sync::mpsc::Sender<KeyCommand>,
    },
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

    /// Stellt sicher, dass die Session entweder Active oder Failed ist.
    /// Idempotent — nach erstem Erfolg/Failure keine weiteren
    /// Portal-Aufrufe.
    async fn ensure_session(&self) -> Option<std::sync::mpsc::Sender<KeyCommand>> {
        let mut guard = self.session.lock().await;
        match &*guard {
            SessionState::Active { cmd_tx } => return Some(cmd_tx.clone()),
            SessionState::Failed { reason } => {
                tracing::trace!(reason = %reason, "libei-Fallback weiterhin aktiv");
                return None;
            }
            SessionState::Uninitialized => {}
        }

        let (restore_token, eis_fd) = match build_remote_desktop_session().await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "RemoteDesktop-Session-Setup fehlgeschlagen — Fallback auf Clipboard+Notification");
                *guard = SessionState::Failed {
                    reason: e.clone(),
                };
                return None;
            }
        };
        tracing::info!(
            has_token = restore_token.is_some(),
            "RemoteDesktop-Session aufgebaut + EIS-FD bezogen"
        );

        // Worker spawnen + warten bis Keyboard ready (oder Timeout/Fail).
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<KeyCommand>();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<bool>();

        let spawn_result = std::thread::Builder::new()
            .name("voicetypex-libei".into())
            .spawn(move || {
                run_libei_worker(eis_fd, cmd_rx, ready_tx);
            });
        if let Err(e) = spawn_result {
            *guard = SessionState::Failed {
                reason: format!("Worker-Thread-Spawn: {e}"),
            };
            return None;
        }

        // Auf Ready-Signal warten — der Worker meldet `true`, sobald das
        // Keyboard-Device verfuegbar ist, oder `false` bei Setup-Fehler /
        // 5s-Timeout.
        match tokio::time::timeout(Duration::from_secs(6), ready_rx).await {
            Ok(Ok(true)) => {
                *guard = SessionState::Active {
                    cmd_tx: cmd_tx.clone(),
                };
                Some(cmd_tx)
            }
            Ok(Ok(false)) => {
                *guard = SessionState::Failed {
                    reason: "Worker meldet Setup-Failure".into(),
                };
                None
            }
            Ok(Err(_)) => {
                *guard = SessionState::Failed {
                    reason: "Worker-ready-Channel geschlossen".into(),
                };
                None
            }
            Err(_) => {
                *guard = SessionState::Failed {
                    reason: "Worker-Setup-Timeout (6s)".into(),
                };
                None
            }
        }
    }
}

#[async_trait]
impl TextInjector for WaylandLibeiInjector {
    fn name(&self) -> &str {
        "linux-wayland-libei"
    }

    fn capabilities(&self) -> InjectorCapabilities {
        InjectorCapabilities {
            supports_clipboard: true,
            supports_keystrokes: true,
        }
    }

    async fn inject(&self, text: &str, _opts: InjectOptions) -> Result<()> {
        // Schritt 1: Clipboard setzen — passiert immer, unabhaengig vom
        // Session-Status.
        self.app_handle
            .clipboard()
            .write_text(text.to_string())
            .map_err(|e| VoiceTypeError::Injection(format!("clipboard write: {e}")))?;

        // Schritt 2: libei-Session sicherstellen + Strg+V senden.
        match self.ensure_session().await {
            Some(cmd_tx) => {
                // Wayland-Eigenheit: `wl_data_device.set_selection` wird
                // erst beim naechsten Compositor-Roundtrip wirksam
                // (~10–30 ms). Ohne Pause hier wuerde Strg+V den alten
                // Clipboard-Inhalt einfuegen, weil der Compositor unsere
                // neue Selection noch nicht "hat". 60 ms ist konservativ
                // genug fuer alle getesteten Compositors, ohne spuerbare
                // UX-Verzoegerung.
                tokio::time::sleep(Duration::from_millis(60)).await;

                if let Err(e) = cmd_tx.send(KeyCommand::CtrlV) {
                    tracing::warn!(error = %e, "libei-Cmd-Channel zu — Fallback auf Notification");
                    self.notify_manual_paste();
                } else {
                    // Kurze Pause, damit der Compositor den Tastendruck
                    // verarbeiten kann, bevor irgend ein nachfolgender
                    // Code (z.B. State-Bus auf Idle) wechselt.
                    tokio::time::sleep(Duration::from_millis(80)).await;
                    tracing::debug!("libei: Ctrl+V gesendet");
                }
            }
            None => {
                self.notify_manual_paste();
            }
        }

        Ok(())
    }
}

impl WaylandLibeiInjector {
    fn notify_manual_paste(&self) {
        let _ = self
            .app_handle
            .notification()
            .builder()
            .title("VoiceTypeX")
            .body("Text in der Zwischenablage. Druecke Ctrl+V in der Ziel-App.")
            .show();
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
///   5. `connect_to_eis(&session)` → liefert den EIS-File-Descriptor.
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

    let eis_fd = proxy
        .connect_to_eis(&session)
        .await
        .map_err(|e| format!("connect_to_eis: {e}"))?;

    Ok((restore_token, eis_fd))
}
