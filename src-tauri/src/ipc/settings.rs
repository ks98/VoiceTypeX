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
    persist_settings(&state)
}

#[tauri::command]
pub async fn list_audio_devices() -> IpcResult<Vec<String>> {
    audio::list_input_devices().map_err(|e| e.to_string())
}

/// Liefert den **effektiven** Menue-Hotkey, wie er gerade tatsaechlich
/// gebunden ist.
///
/// - X11/Windows: gibt den Settings-Wert zurueck — die App registriert
///   den Hotkey direkt, also ist Settings die Wahrheit.
/// - Wayland: gibt den vom Compositor zurueckgegebenen Trigger zurueck
///   (oder `None`, falls die Portal-Session noch nicht geantwortet hat
///   bzw. `list_shortcuts` fehlschlug — Frontend faellt dann auf den
///   Settings-Wert zurueck). KDE/GNOME duerfen vom Settings-Wert
///   abweichen, weil der User den Hotkey in den System-Einstellungen
///   nachjustieren kann.
#[tauri::command]
pub async fn get_effective_menu_hotkey(
    state: tauri::State<'_, Arc<AppContext>>,
) -> IpcResult<Option<String>> {
    Ok(state.effective_menu_hotkey.read().clone())
}

#[tauri::command]
pub async fn set_whisper_model_path(
    state: tauri::State<'_, Arc<AppContext>>,
    path: String,
) -> IpcResult<()> {
    state.settings.write().whisper_model_path = Some(path);
    persist_settings(&state)
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
    let _ = persist_settings(&state); // Best-effort, Download-Result trotzdem zurueckgeben
    Ok(path_str)
}

/// Schreibt den aktuellen Settings-Snapshot auf Disk. Wird nach jedem
/// Mutations-IPC aufgerufen, damit User-Aenderungen App-Restart-fest sind.
fn persist_settings(state: &tauri::State<'_, Arc<AppContext>>) -> IpcResult<()> {
    let snapshot = state.settings.read().clone();
    snapshot
        .save(&state.settings_path)
        .map_err(|e| format!("Settings-Persist: {e}"))
}
