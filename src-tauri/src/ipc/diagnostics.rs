// SPDX-License-Identifier: GPL-3.0-or-later
//! Diagnostics-IPC: Logs-Stream, App-Version, System-Info.

use crate::core::AppContext;
use crate::injection::{InjectOptions, InjectionStrategy};
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;

type IpcResult<T> = std::result::Result<T, String>;

#[tauri::command]
pub async fn get_app_version() -> IpcResult<String> {
    Ok(env!("CARGO_PKG_VERSION").to_string())
}

#[tauri::command]
pub async fn get_recent_logs(
    state: tauri::State<'_, Arc<AppContext>>,
    limit: u32,
) -> IpcResult<Vec<String>> {
    Ok(state.log_buffer.lines(limit as usize))
}

#[derive(Serialize)]
pub struct SessionInfo {
    /// `wayland`, `x11`, `windows`, `macos`, `unknown`
    pub display_server: String,
    /// True wenn globale Hotkeys auf dieser Session voraussichtlich
    /// funktionieren (Wayland: nein, bis Phase 5-full).
    pub global_hotkeys_supported: bool,
    /// True wenn Auto-Paste-Shortcut nach Clipboard-Set funktioniert
    /// (Wayland: nein ohne libei).
    pub auto_paste_supported: bool,
}

#[tauri::command]
pub async fn get_session_info() -> IpcResult<SessionInfo> {
    Ok(crate::core::session::detect_session())
}

#[tauri::command]
pub async fn get_whisper_backend() -> IpcResult<crate::transcription::backend::WhisperBackendInfo> {
    Ok(crate::transcription::backend::active_backend())
}

#[tauri::command]
pub async fn get_hardware_report() -> IpcResult<crate::core::hardware::HardwareReport> {
    Ok(crate::core::hardware::detect())
}

/// Diagnose-Test fuer Auto-Paste. Schlaeft `delay_secs` Sekunden, sodass
/// der User Zeit hat, das Ziel-Fenster zu fokussieren, dann triggert
/// einen kompletten Inject (Clipboard + libei-Strg+V) mit `text`.
/// Damit ist der Fokus-Race der normalen Pipeline ausgeschlossen — wenn
/// das funktioniert aber der echte Diktat-Pfad nicht, ist nicht libei
/// das Problem, sondern was zwischen Hotkey-Press und Inject den Fokus
/// verschiebt.
#[tauri::command]
pub async fn test_auto_paste(
    state: tauri::State<'_, Arc<AppContext>>,
    text: String,
    delay_secs: u64,
) -> IpcResult<()> {
    tracing::info!(
        delay_secs,
        text_len = text.len(),
        "test_auto_paste: Countdown gestartet — bitte Ziel-Fenster fokussieren"
    );
    tokio::time::sleep(Duration::from_secs(delay_secs)).await;
    tracing::info!("test_auto_paste: triggere Inject");
    state
        .injector
        .inject(
            &text,
            InjectOptions {
                strategy: InjectionStrategy::Clipboard,
            },
        )
        .await
        .map_err(|e| e.to_string())
}
