// SPDX-License-Identifier: GPL-3.0-or-later
//! Settings IPC.

use crate::audio;
use crate::core::config::Settings;
use crate::core::AppContext;
use crate::transcription::model_downloader::{
    download_llm, download_model, download_vad, LlmModelSlot, ModelSlot, VadModel,
};
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
    validate_settings(&settings)?;
    *state.settings.write() = settings;
    persist_settings(&state)
}

/// Boundary-validation. Catches user-supplied values that could later
/// surprise the runtime (e.g. an `ollama_url` that exfiltrates transcripts
/// to a third party because the user pasted in a fake "faster Ollama"
/// endpoint from a forum post).
fn validate_settings(s: &Settings) -> IpcResult<()> {
    let url = reqwest::Url::parse(&s.ollama_url).map_err(|e| format!("Invalid ollama_url: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!(
            "Invalid ollama_url scheme: {} (only http/https allowed)",
            url.scheme()
        ));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err("Invalid ollama_url: host missing".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Invalid ollama_url: credentials in URL not allowed".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn list_audio_devices() -> IpcResult<Vec<String>> {
    audio::list_input_devices().map_err(|e| e.to_string())
}

/// Returns the **effective** menu hotkey as it is actually bound
/// right now.
///
/// - X11/Windows: returns the settings value — the app registers the
///   hotkey directly, so settings is the truth.
/// - Wayland: returns the trigger reported by the compositor (or
///   `None` if the portal session has not yet answered or
///   `list_shortcuts` failed — the frontend then falls back to the
///   settings value). KDE/GNOME may deviate from the settings value
///   because the user can adjust the hotkey in system settings.
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

/// Download the default Whisper model configured in the settings
/// slot to `app_config_dir/models/`. Emits
/// `model-download-progress` events to the frontend during the
/// download.
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

    let slot = ModelSlot::from_setting(&slot_name);

    // Pull the VAD model in parallel (~885 kB, sub-second).
    // Best-effort — if the download fails, the Whisper path
    // transparently falls back to "no VAD" and the user only gets a
    // WARN line in the Whisper log. We don't want to kill the
    // Whisper download because of that.
    if let Err(e) = download_vad(VadModel::SileroV6_2_0, &dest_dir, |_| {}).await {
        tracing::warn!(error = %e, "VAD model download failed, Whisper path will run without VAD");
    }

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
    let _ = persist_settings(&state); // Best-effort; still return the download result.
    Ok(path_str)
}

/// **Phase 3b** — download the GGUF LLM model configured in
/// `Settings.llm_default_slot` to `app_config_dir/models/`. Emits
/// `llm-model-download-progress` events to the frontend (a separate
/// channel from Whisper so both downloads can run in parallel
/// without progress mixing).
#[tauri::command]
pub async fn download_llm_default_model(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppContext>>,
) -> IpcResult<String> {
    let (slot_name, dest_dir) = {
        let s = state.settings.read();
        (s.llm_default_slot.clone(), state.model_dir.clone())
    };

    let slot = LlmModelSlot::from_setting(&slot_name);

    let app_for_progress = app.clone();
    let result = download_llm(slot, &dest_dir, move |progress| {
        let _ = app_for_progress.emit(
            "llm-model-download-progress",
            ModelDownloadProgress {
                downloaded: progress.bytes_downloaded,
                total: progress.bytes_total,
            },
        );
    })
    .await
    .map_err(|e| e.to_string())?;

    let path_str = result.to_string_lossy().into_owned();
    state.settings.write().llm_model_path = Some(path_str.clone());
    let _ = persist_settings(&state);
    Ok(path_str)
}

/// Writes the current settings snapshot to disk. Called after every
/// mutating IPC so user changes survive an app restart.
fn persist_settings(state: &tauri::State<'_, Arc<AppContext>>) -> IpcResult<()> {
    let snapshot = state.settings.read().clone();
    snapshot
        .save(&state.settings_path)
        .map_err(|e| format!("Settings-Persist: {e}"))
}
