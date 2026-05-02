// SPDX-License-Identifier: GPL-3.0-or-later
//! Diagnostics-IPC: Logs-Stream, App-Version, System-Info.

use crate::core::AppContext;
use serde::Serialize;
use std::sync::Arc;

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
