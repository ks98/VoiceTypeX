// SPDX-License-Identifier: GPL-3.0-or-later
//! Settings-IPC.

use crate::audio;
use crate::core::config::Settings;
use crate::core::AppContext;
use crate::transcription::model_downloader::{download_model, ModelSlot};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

type IpcResult<T> = std::result::Result<T, String>;

#[derive(Serialize, Clone)]
struct ModelDownloadProgress {
    downloaded: u64,
    total: Option<u64>,
}

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

/// Lade das im Settings-Slot konfigurierte Default-Whisper-Modell nach
/// `app_data_dir/models/`. Sendet waehrend des Downloads
/// `model-download-progress`-Events ans Frontend.
#[tauri::command]
pub async fn download_default_model(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppContext>>,
) -> IpcResult<String> {
    let (slot_name, dest_dir) = {
        let settings = state.settings.read();
        (
            settings.whisper_default_slot.clone(),
            state.model_dir.clone(),
        )
    };

    let slot = match slot_name.as_str() {
        "small-q5_1" => ModelSlot::SmallQ51,
        "large-v3-turbo" => ModelSlot::LargeV3Turbo,
        _ => ModelSlot::LargeV3TurboQ5,
    };

    let app_for_progress = app.clone();
    let result = download_model(slot, &dest_dir, move |progress| {
        let _ = app_for_progress.emit(
            "model-download-progress",
            ModelDownloadProgress {
                downloaded: progress.bytes_downloaded,
                total: progress.bytes_total,
            },
        );
    })
    .await
    .map_err(|e| e.to_string())?;

    let path_str = result.to_string_lossy().into_owned();
    state.settings.write().whisper_model_path = Some(path_str.clone());
    Ok(path_str)
}
