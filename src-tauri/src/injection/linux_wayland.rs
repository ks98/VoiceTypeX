// SPDX-License-Identifier: GPL-3.0-or-later
//! Linux Wayland text injection — composite strategy of clipboard +
//! libei.
//!
//! Architecture (see CLAUDE.md §11 phase 5 part 2):
//!   1. Text is placed on the system clipboard.
//!   2. `Ctrl+V` is simulated via
//!      `xdg-desktop-portal.RemoteDesktop` + libei. Implemented in
//!      sub-iterations 5.2.A through 5.2.B.3:
//!        - 5.2.A: portal session, lazy on first hotkey
//!        - 5.2.B.1: connect_to_eis() + EIS FD
//!        - 5.2.B.2: reis worker thread + EI handshake
//!        - 5.2.B.3: the actual Ctrl+V keystroke
//!
//! Session lifecycle: lazy on the first inject (`ensure_session`),
//! then held for the app lifetime in an `Arc<tokio::Mutex<...>>`. The
//! libei worker runs as a dedicated `std::thread` and communicates
//! with the tokio main thread via an `mpsc::Sender<KeyCommand>`.
//!
//! Failure UX: on permission denial, compositor unsupport or worker
//! setup timeout, the injector silently falls back to clipboard +
//! notification ("Press Ctrl+V") — no hard error.

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

/// State of the RemoteDesktop portal session + libei worker.
/// Initialized lazily; after a successful setup it stays alive for the
/// app lifetime. On hard errors we go into `Failed` and log it once —
/// further inject attempts then silently fall back to the clipboard
/// path.
enum SessionState {
    /// Session setup not attempted yet.
    Uninitialized,
    /// Worker is running, the keyboard device is ready, commands can
    /// be sent.
    Active {
        cmd_tx: std::sync::mpsc::Sender<KeyCommand>,
    },
    /// Setup failed. The failure reason is logged once; the inject
    /// path falls back to clipboard only.
    Failed { reason: String },
}

pub struct WaylandLibeiInjector {
    app_handle: tauri::AppHandle,
    session: Arc<Mutex<SessionState>>,
    /// Path for the `restore_token`
    /// (`~/.config/.../wayland_session.json`). Written on first
    /// successful setup and read on subsequent app starts — then the
    /// portal stops asking for permission because the token is valid.
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

    /// Ensures the session is either Active or Failed. Idempotent —
    /// no further portal calls after the first success/failure.
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

        // Load an existing token (no permission dialog on start if
        // KWin/Mutter accepts the token).
        let prior_token = load_restore_token(&self.token_path);
        if prior_token.is_some() {
            tracing::info!("RemoteDesktop: using stored restore_token");
        }

        let (restore_token, eis_fd) = match build_remote_desktop_session(prior_token.as_deref())
            .await
        {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "RemoteDesktop session setup failed — falling back to clipboard+notification");
                *guard = SessionState::Failed { reason: e.clone() };
                return None;
            }
        };
        tracing::info!(
            has_token = restore_token.is_some(),
            "RemoteDesktop-Session aufgebaut + EIS-FD bezogen"
        );

        // Persist the new token if the compositor delivered one.
        // Best-effort — if the disk write fails, the app keeps
        // running; the user just sees the permission dialog again on
        // the next start.
        if let Some(token) = &restore_token {
            if let Err(e) = save_restore_token(&self.token_path, token) {
                tracing::warn!(error = %e, "restore_token could not be persisted");
            }
        }

        // Spawn the worker and wait until keyboard is ready (or
        // timeout/fail).
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

        // Wait for the ready signal — the worker reports `true` as
        // soon as the keyboard device is available, or `false` on
        // setup error / 5 s timeout.
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
        // The `Keystrokes` strategy is not implemented on Wayland.
        // libei could do it in principle (`KeyCommand::Type{keysyms}`
        // in the worker), but needs char-to-keysym mapping via
        // xkbcommon. Until that exists we fall back to the
        // clipboard+auto-paste path and log the mismatch — otherwise
        // it would be hard for the user to figure out why their
        // `injection_method = "keystrokes"` looks like clipboard.
        if opts.strategy == InjectionStrategy::Keystrokes {
            tracing::info!(
                "Wayland: injection_method=keystrokes nicht unterstuetzt, nutze libei+Clipboard"
            );
        }

        // Step 1: set the clipboard — happens always, regardless of
        // the session state.
        self.app_handle
            .clipboard()
            .write_text(text.to_string())
            .map_err(|e| VoiceTypeError::Injection(format!("clipboard write: {e}")))?;

        // Step 2: ensure the libei session and send Ctrl+V.
        match self.ensure_session().await {
            Some(cmd_tx) => {
                // Wayland quirk: `wl_data_device.set_selection` only
                // takes effect on the next compositor round-trip
                // (~10–30 ms). Without a pause here Ctrl+V would paste
                // the old clipboard content, because the compositor
                // doesn't "have" our new selection yet. 60 ms is
                // conservative enough for all tested compositors,
                // without a perceptible UX delay.
                tokio::time::sleep(Duration::from_millis(60)).await;

                if let Err(e) = cmd_tx.send(KeyCommand::CtrlV) {
                    tracing::warn!(error = %e, "libei-Cmd-Channel zu — Fallback auf Notification");
                    self.notify_manual_paste();
                } else {
                    // Short pause so the compositor can process the
                    // keypress before any subsequent code (e.g. the
                    // state bus switching to Idle) runs.
                    tokio::time::sleep(Duration::from_millis(80)).await;
                    tracing::debug!("libei: Ctrl+V sent");
                }
            }
            None => {
                self.notify_manual_paste();
            }
        }

        Ok(())
    }

    async fn read_selection(&self) -> Result<Option<String>> {
        // Selection reading on Wayland needs a simulated Ctrl+C via the
        // libei worker (a `KeyCommand::CtrlC`) plus a clipboard read.
        // Implemented in a dedicated step after verifying the
        // ext-data-control / libei protocol docs. Until then edit modes
        // degrade gracefully to an empty selection on Wayland.
        tracing::debug!("read_selection: not yet implemented on Wayland (libei Ctrl+C pending)");
        Ok(None)
    }
}

impl WaylandLibeiInjector {
    fn notify_manual_paste(&self) {
        let _ = self
            .app_handle
            .notification()
            .builder()
            .title("VoiceTypeX")
            .body("Text copied to clipboard. Press Ctrl+V in the target app.")
            .show();
    }
}

/// Sets up an `xdg-desktop-portal.RemoteDesktop` session. If
/// `prior_token` exists, it is forwarded in `select_devices` — then
/// the compositor doesn't ask for permission again, provided the
/// token is still valid (the user has not revoked permission and the
/// compositor was not restarted in the meantime).
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

/// Reads the persisted `restore_token` from `path`. Returns `None`
/// on a missing file or parse error — both are non-fatal; the
/// permission dialog just shows up again.
fn load_restore_token(path: &std::path::Path) -> Option<String> {
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    let stored: StoredToken = serde_json::from_str(&content).ok()?;
    Some(stored.restore_token)
}

/// Writes the `restore_token` as JSON to `path`. Creates the parent
/// directory if needed. chmod 0600, because the token is effectively
/// a persistent capability token for keyboard injection: anyone who
/// reads it can replay it against the same compositor and thereby
/// send keystrokes indefinitely without a further user dialog. It
/// should therefore be treated as an authentication secret.
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
