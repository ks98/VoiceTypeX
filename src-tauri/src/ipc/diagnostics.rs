// SPDX-License-Identifier: GPL-3.0-or-later
//! Diagnostics-IPC: Logs-Stream, App-Version, System-Info.

use crate::core::AppContext;
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
