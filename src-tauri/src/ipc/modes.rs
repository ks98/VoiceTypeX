// SPDX-License-Identifier: GPL-3.0-or-later
//! Modi-IPC.

use crate::core::Mode;

type IpcResult<T> = std::result::Result<T, String>;

#[tauri::command]
pub async fn get_modes() -> IpcResult<Vec<Mode>> {
    Ok(vec![])
}

#[tauri::command]
pub async fn reload_modes() -> IpcResult<()> {
    Err("reload_modes noch nicht implementiert (Phase 1.4)".into())
}
