// SPDX-License-Identifier: GPL-3.0-or-later
//! Settings-IPC.

use crate::audio;
use crate::core::config::Settings;
use crate::core::AppContext;
use std::sync::Arc;

type IpcResult<T> = std::result::Result<T, String>;

#[tauri::command]
pub async fn get_settings(state: tauri::State<'_, Arc<AppContext>>) -> IpcResult<Settings> {
    Ok(state.settings.read().clone())
}

#[tauri::command]
pub async fn set_settings(
    state: tauri::State<'_, Arc<AppContext>>,
    settings: Settings,
) -> IpcResult<()> {
    *state.settings.write() = settings;
    Ok(())
}

#[tauri::command]
pub async fn list_audio_devices() -> IpcResult<Vec<String>> {
    audio::list_input_devices().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_whisper_model_path(
    state: tauri::State<'_, Arc<AppContext>>,
    path: String,
) -> IpcResult<()> {
    state.settings.write().whisper_model_path = Some(path);
    Ok(())
}
