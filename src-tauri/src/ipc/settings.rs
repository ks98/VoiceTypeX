// SPDX-License-Identifier: GPL-3.0-or-later
//! Settings IPC.

use crate::audio;
use crate::core::config::Settings;
use crate::core::AppContext;
use crate::transcription::model_downloader::{
    download_llm, download_model, download_vad, LlmModelSlot, ModelSlot, VadModel,
};
use serde::Serialize;
use std::path::Path;
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

/// Boundary-validation for a user-supplied model path. The value is
/// later mmap/parsed as a GGML/GGUF model by native code, so reject
/// anything that is not an existing file with a model extension before
/// it ever reaches the loader. The dialog picker already restricts to
/// `bin`/`gguf`, but the IPC is callable with any string.
fn validate_model_path(path: &Path) -> IpcResult<()> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if !matches!(ext.as_deref(), Some("bin" | "gguf")) {
        return Err("Invalid model path: must end in .bin or .gguf".into());
    }
    if !path.is_file() {
        return Err(format!("Invalid model path: no file at {}", path.display()));
    }
    Ok(())
}

#[tauri::command]
pub async fn set_whisper_model_path(
    state: tauri::State<'_, Arc<AppContext>>,
    path: String,
) -> IpcResult<()> {
    validate_model_path(Path::new(&path))?;
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
            crate::core::events::MODEL_DOWNLOAD_PROGRESS,
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
            crate::core::events::LLM_MODEL_DOWNLOAD_PROGRESS,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // Contract test — PAYLOAD-SHAPE parity for `ModelDownloadProgress` (#49).
    //
    // Pins the exact serialized JSON key set the backend emits on the
    // `model-download-progress` / `llm-model-download-progress` events.
    // The identical key list is hard-coded on the TS side in
    // `src/lib/payload-contract.test.ts` (matching the `ModelDownloadProgress`
    // interface in `src/lib/tauri.ts`), anchoring both sides to the same
    // canonical fields. A Rust field add/rename/remove fails THIS test; a
    // TS-side drift fails the TS test.
    //
    // `total: Some(..)` ensures the `Option` field serializes (no skip
    // attribute, but `None` -> `null` still emits the key — either way the
    // key is present).
    //
    // Honest limit (contract-tests-over-codegen, no specta/ts-rs): the two
    // sides do NOT auto-derive — a coordinated change to the struct AND
    // both key lists would pass. Accepted trade-off.
    #[test]
    fn model_download_progress_serialized_key_set_is_pinned() {
        let value = serde_json::to_value(ModelDownloadProgress {
            downloaded: 1,
            total: Some(2),
        })
        .expect("ModelDownloadProgress serializes");
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("ModelDownloadProgress serializes to a JSON object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();

        let mut expected = ["downloaded", "total"];
        expected.sort_unstable();

        assert_eq!(keys, expected);
    }

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "voicetypex-model-path-test-{}-{name}",
            std::process::id()
        ))
    }

    #[test]
    fn accepts_existing_bin_and_gguf() {
        for name in ["model.bin", "model.gguf", "MODEL.GGUF"] {
            let p = temp_path(name);
            std::fs::write(&p, b"x").expect("write temp file");
            let result = validate_model_path(&p);
            let _ = std::fs::remove_file(&p);
            assert!(result.is_ok(), "{name} should be accepted: {result:?}");
        }
    }

    #[test]
    fn rejects_wrong_extension() {
        let p = temp_path("model.txt");
        std::fs::write(&p, b"x").expect("write temp file");
        let result = validate_model_path(&p);
        let _ = std::fs::remove_file(&p);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_missing_extension() {
        let p = temp_path("model");
        std::fs::write(&p, b"x").expect("write temp file");
        let result = validate_model_path(&p);
        let _ = std::fs::remove_file(&p);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_nonexistent_path() {
        let p = temp_path("does-not-exist.bin");
        assert!(validate_model_path(&p).is_err());
    }

    #[test]
    fn rejects_directory_with_model_extension() {
        let p = temp_path("dir.gguf");
        std::fs::create_dir_all(&p).expect("create temp dir");
        let result = validate_model_path(&p);
        let _ = std::fs::remove_dir_all(&p);
        assert!(result.is_err());
    }

    fn settings_with_ollama_url(url: &str) -> Settings {
        Settings {
            ollama_url: url.to_string(),
            ..Settings::default()
        }
    }

    #[test]
    fn accepts_valid_http_and_https_ollama_url() {
        for url in ["http://localhost:11434", "https://x.example"] {
            let result = validate_settings(&settings_with_ollama_url(url));
            assert!(result.is_ok(), "{url} should be accepted: {result:?}");
        }
    }

    #[test]
    fn rejects_non_http_ollama_url_scheme() {
        for url in ["ftp://host", "file:///etc/passwd"] {
            let result = validate_settings(&settings_with_ollama_url(url));
            assert!(result.is_err(), "{url} should be rejected");
        }
    }

    #[test]
    fn rejects_empty_ollama_url() {
        assert!(validate_settings(&settings_with_ollama_url("")).is_err());
    }

    #[test]
    fn rejects_host_less_ollama_url() {
        // `http://` (empty authority) has no host to send the transcript
        // to. For a special scheme the URL parser rejects this outright,
        // so the failure surfaces from the `parse` step.
        for url in ["http://", "https://"] {
            assert!(
                validate_settings(&settings_with_ollama_url(url)).is_err(),
                "{url} should be rejected (host missing)"
            );
        }
    }

    #[test]
    fn rejects_credentials_in_ollama_url() {
        for url in [
            "http://user:pw@host:11434",
            "https://user@host",
            "http://:pw@host",
        ] {
            let result = validate_settings(&settings_with_ollama_url(url));
            assert!(result.is_err(), "{url} should be rejected (creds in URL)");
        }
    }
}
