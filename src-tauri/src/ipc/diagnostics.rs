// SPDX-License-Identifier: GPL-3.0-or-later
//! Diagnostics-IPC: Logs-Stream, App-Version, System-Info.

type IpcResult<T> = std::result::Result<T, String>;

#[tauri::command]
pub async fn get_app_version() -> IpcResult<String> {
    Ok(env!("CARGO_PKG_VERSION").to_string())
}

#[tauri::command]
pub async fn get_recent_logs(_limit: u32) -> IpcResult<Vec<String>> {
    // In Phase 1.6 angeschlossen an einen tracing-Layer mit Ring-Buffer.
    Ok(vec![])
}
