// SPDX-License-Identifier: GPL-3.0-or-later
//! Recording-IPC. Wird hauptsaechlich vom Hotkey-Pfad ausgeloest, kann aber
//! auch manuell aus dem Frontend angestossen werden (z.B. fuer einen
//! "Test-Transkriptions-Button").

type IpcResult<T> = std::result::Result<T, String>;

#[tauri::command]
pub async fn start_recording(_mode_id: String) -> IpcResult<()> {
    Err("start_recording noch nicht implementiert (Phase 1.4)".into())
}

#[tauri::command]
pub async fn stop_recording() -> IpcResult<()> {
    Err("stop_recording noch nicht implementiert (Phase 1.4)".into())
}

#[tauri::command]
pub async fn run_test_transcription(_seconds: u32) -> IpcResult<f32> {
    // Returnt Real-Time-Factor (RTF). Phase 1.5.
    Err("run_test_transcription noch nicht implementiert (Phase 1.5)".into())
}
