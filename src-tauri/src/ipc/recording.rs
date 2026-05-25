// SPDX-License-Identifier: GPL-3.0-or-later
//! Recording IPC. Normally triggered by the hotkey path, but can also
//! be invoked manually from the frontend (e.g. for a "test
//! transcription button").

use crate::audio::recorder::{RecorderConfig, RecorderHandle};
use crate::core::state::AppState;
use crate::core::AppContext;
use crate::pipeline::execute_mode;
use crate::transcription::TranscribeOpts;
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::AppHandle;

type IpcResult<T> = std::result::Result<T, String>;

#[tauri::command]
pub async fn start_recording(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppContext>>,
    mode_id: String,
) -> IpcResult<()> {
    let mode = state
        .modes
        .find_by_id(&mode_id)
        .ok_or_else(|| format!("Mode '{mode_id}' not found"))?;

    // Remember the selection as "last selected" so the cursor lands
    // on this mode on the next menu open.
    {
        let mut settings = state.settings.write();
        if settings.last_selected_mode_id.as_deref() != Some(&mode_id) {
            settings.last_selected_mode_id = Some(mode_id.clone());
            if let Err(e) = settings.save(&state.settings_path) {
                tracing::warn!(error = %e, "Failed to persist last_selected_mode_id");
            }
        }
    }

    execute_mode(app, Arc::clone(&state), mode)
        .await
        .map_err(|e| e.to_string())
}

/// Closes the menu window without starting recording. Called from
/// Esc in the frontend menu.
#[tauri::command]
pub async fn cancel_menu(app: AppHandle) -> IpcResult<()> {
    use tauri::Manager;
    if let Some(menu) = app.get_webview_window("menu") {
        menu.hide().map_err(|e| format!("menu.hide(): {e}"))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn stop_recording(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppContext>>,
    mode_id: String,
) -> IpcResult<()> {
    // The toggle logic in `execute_mode` decides based on state.
    let mode = state
        .modes
        .find_by_id(&mode_id)
        .ok_or_else(|| format!("Mode '{mode_id}' not found"))?;
    execute_mode(app, Arc::clone(&state), mode)
        .await
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct TestTranscriptionResult {
    /// Real-time factor: < 1.0 = faster than real time.
    pub rtf: f32,
    /// Text transcribed by Whisper.
    pub text: String,
    /// Recording duration in seconds (input value).
    pub audio_seconds: f32,
    /// Pure transcription time in milliseconds.
    pub processing_ms: u64,
}

/// Diagnostic end-to-end test of the local STT path. Records
/// `seconds` seconds of audio from the default microphone,
/// transcribes it with the configured local Whisper model, and
/// reports the real RTF on the current system.
#[tauri::command]
pub async fn run_test_transcription(
    state: tauri::State<'_, Arc<AppContext>>,
    seconds: u32,
) -> IpcResult<TestTranscriptionResult> {
    if seconds == 0 || seconds > 30 {
        return Err("seconds must be between 1 and 30".into());
    }
    if !matches!(state.state_bus.current(), AppState::Idle) {
        return Err("Test only possible in idle state".into());
    }

    state
        .state_bus
        .transition(AppState::Recording)
        .map_err(|e| e.to_string())?;

    let device_name = state.settings.read().audio_input_device.clone();
    let recorder = match RecorderHandle::start(RecorderConfig { device_name }) {
        Ok(r) => r,
        Err(e) => {
            let _ = state.state_bus.transition(AppState::Idle);
            return Err(e.to_string());
        }
    };

    tokio::time::sleep(Duration::from_secs(seconds as u64)).await;

    let wav = match recorder.stop_and_finalize().await {
        Ok(w) => w,
        Err(e) => {
            let _ = state.state_bus.transition(AppState::Idle);
            return Err(e.to_string());
        }
    };

    state
        .state_bus
        .transition(AppState::Transcribing)
        .map_err(|e| e.to_string())?;

    let start = Instant::now();
    let n_threads = state.settings.read().whisper_n_threads;
    // The test endpoint leaves the language open — Whisper
    // auto-detect. Previously hardcoded "de", which broke English
    // testers. Anyone who wants to test a specific language uses a
    // mode with the `language` field set.
    let result = state
        .transcriber
        .transcribe_oneshot(
            &wav,
            TranscribeOpts {
                language: None,
                initial_prompt: None,
                n_threads,
            },
        )
        .await;
    let elapsed = start.elapsed();

    let _ = state.state_bus.transition(AppState::Idle);

    let text = result.map_err(|e| e.to_string())?;
    let rtf = elapsed.as_secs_f32() / (seconds as f32);
    Ok(TestTranscriptionResult {
        rtf,
        text,
        audio_seconds: seconds as f32,
        processing_ms: elapsed.as_millis() as u64,
    })
}
