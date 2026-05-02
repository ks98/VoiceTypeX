// SPDX-License-Identifier: GPL-3.0-or-later
//! Recording-IPC. Wird normalerweise vom Hotkey-Pfad ausgeloest, kann aber
//! auch manuell aus dem Frontend angestossen werden (z.B. fuer einen
//! "Test-Transkriptions-Button").

use crate::core::AppContext;
use crate::pipeline::execute_mode;
use std::sync::Arc;
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
        .ok_or_else(|| format!("Modus '{mode_id}' nicht gefunden"))?;
    execute_mode(app, Arc::clone(&state), mode)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stop_recording(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppContext>>,
    mode_id: String,
) -> IpcResult<()> {
    // Toggle-Logik in execute_mode entscheidet anhand State
    let mode = state
        .modes
        .find_by_id(&mode_id)
        .ok_or_else(|| format!("Modus '{mode_id}' nicht gefunden"))?;
    execute_mode(app, Arc::clone(&state), mode)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn run_test_transcription(_seconds: u32) -> IpcResult<f32> {
    // Phase 1.5 implementiert das mit echter RTF-Messung
    Err("run_test_transcription noch nicht implementiert (Phase 1.5)".into())
}
