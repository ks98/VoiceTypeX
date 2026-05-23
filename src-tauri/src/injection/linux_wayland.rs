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
use crate::injection::{InjectOptions, InjectionStrategy, InjectorCapabilities, TextInjector};
use async_trait::async_trait;
use std::path::PathBuf;
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
    /// Pfad fuer den `restore_token` (~/.config/.../wayland_session.json).
    /// Wird beim ersten erfolgreichen Setup geschrieben und bei naechsten
    /// App-Starts gelesen — dann fragt das Portal nicht mehr nach
    /// Permission, weil der Token gueltig ist.
    token_path: PathBuf,
}

impl WaylandLibeiInjector {
    pub fn new(app_handle: tauri::AppHandle, token_path: PathBuf) -> Self {
        Self {
            app_handle,
            session: Arc::new(Mutex::new(SessionState::Uninitialized)),
            token_path,
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

        // Vorhandenen Token laden (kein Permission-Dialog beim Start,
        // wenn KWin/Mutter den Token akzeptiert).
        let prior_token = load_restore_token(&self.token_path);
        if prior_token.is_some() {
            tracing::info!("RemoteDesktop: nutze gespeicherten restore_token");
        }

        let (restore_token, eis_fd) = match build_remote_desktop_session(prior_token.as_deref())
            .await
        {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "RemoteDesktop-Session-Setup fehlgeschlagen — Fallback auf Clipboard+Notification");
                *guard = SessionState::Failed { reason: e.clone() };
                return None;
            }
        };
        tracing::info!(
            has_token = restore_token.is_some(),
            "RemoteDesktop-Session aufgebaut + EIS-FD bezogen"
        );

        // Neuen Token persistieren, falls der Compositor einen geliefert
        // hat. Best-effort — wenn der Disk-Write fehlschlaegt, laeuft die
        // App weiter, der User sieht dann beim naechsten Start halt nochmal
        // den Permission-Dialog.
        if let Some(token) = &restore_token {
            if let Err(e) = save_restore_token(&self.token_path, token) {
                tracing::warn!(error = %e, "restore_token konnte nicht persistiert werden");
            }
        }

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

    async fn inject(&self, text: &str, opts: InjectOptions) -> Result<()> {
        // `Keystrokes`-Strategy ist auf Wayland nicht implementiert. libei
        // koennte das prinzipiell (KeyCommand::Type{keysyms} im Worker),
        // braucht aber Char-zu-Keysym-Mapping via xkbcommon. Solange das
        // nicht da ist, fallen wir auf den Clipboard+Auto-Paste-Pfad zurueck
        // und loggen den Mismatch — sonst bleibt es fuer den User schwer
        // nachvollziehbar, warum sein `injection_method = "keystrokes"`
        // wie Clipboard aussieht.
        if opts.strategy == InjectionStrategy::Keystrokes {
            tracing::info!(
                "Wayland: injection_method=keystrokes nicht unterstuetzt, nutze libei+Clipboard"
            );
        }

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

/// Baut eine `xdg-desktop-portal.RemoteDesktop`-Session auf. Falls
/// `prior_token` existiert, wird er in `select_devices` durchgereicht —
/// dann fragt der Compositor nicht erneut nach Permission, vorausgesetzt
/// der Token ist noch gueltig (User hat die Erlaubnis nicht widerrufen,
/// Compositor wurde nicht zwischenzeitlich neu gestartet).
async fn build_remote_desktop_session(
    prior_token: Option<&str>,
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
            prior_token,
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

#[derive(serde::Serialize, serde::Deserialize)]
struct StoredToken {
    restore_token: String,
}

/// Liest den persistierten `restore_token` aus `path`. Liefert `None`
/// bei fehlender Datei oder Parse-Fehler — beides ist nicht-fatal,
/// es kommt halt der Permission-Dialog wieder.
fn load_restore_token(path: &std::path::Path) -> Option<String> {
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    let stored: StoredToken = serde_json::from_str(&content).ok()?;
    Some(stored.restore_token)
}

/// Schreibt den `restore_token` als JSON nach `path`. Erstellt das
/// Parent-Verzeichnis bei Bedarf. Auf Linux mit chmod 0600 — der Token
/// ist zwar kein Secret im Sinne von API-Key, aber das File-Permission-
/// Setup ist konsistent mit dem Secrets-Storage, weil das Token einer
/// erteilten Tastatur-Inject-Erlaubnis entspricht.
fn save_restore_token(path: &std::path::Path, token: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| VoiceTypeError::Injection(format!("create_dir: {e}")))?;
    }
    let stored = StoredToken {
        restore_token: token.to_string(),
    };
    let json = serde_json::to_string_pretty(&stored)
        .map_err(|e| VoiceTypeError::Injection(format!("json: {e}")))?;
    std::fs::write(path, json).map_err(|e| VoiceTypeError::Injection(format!("write: {e}")))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}
