// SPDX-License-Identifier: GPL-3.0-or-later
//! Modi-IPC.

use crate::core::{AppContext, Mode};
use std::sync::Arc;

type IpcResult<T> = std::result::Result<T, String>;

#[tauri::command]
pub async fn get_modes(state: tauri::State<'_, Arc<AppContext>>) -> IpcResult<Vec<Mode>> {
    Ok(state.modes.current())
}

#[tauri::command]
pub async fn reload_modes(state: tauri::State<'_, Arc<AppContext>>) -> IpcResult<Vec<Mode>> {
    // Hot-Reload geschieht automatisch via notify-Watcher; dieser Command
    // liefert einfach den aktuellen Snapshot zurueck. (Bei expliziten
    // Reload-Wunsch koennte hier eine forced-reload-Logik ergaenzt werden.)
    Ok(state.modes.current())
}
