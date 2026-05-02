// SPDX-License-Identifier: GPL-3.0-or-later
//! Settings-IPC.
//!
//! Phase 1.2: nur Stub-Signaturen. Phase 1.5 verdrahtet sie mit
//! `tauri-plugin-store` und Frontend-Aufrufen.

use crate::core::config::Settings;

/// IPC-Fehler werden als String an das Frontend serialisiert (Tauri-Konvention).
type IpcResult<T> = std::result::Result<T, String>;

#[tauri::command]
pub async fn get_settings() -> IpcResult<Settings> {
    Ok(Settings::default())
}

#[tauri::command]
pub async fn set_settings(_settings: Settings) -> IpcResult<()> {
    Err("set_settings noch nicht implementiert (Phase 1.5)".into())
}

#[tauri::command]
pub async fn list_audio_devices() -> IpcResult<Vec<String>> {
    Err("list_audio_devices noch nicht implementiert (Phase 1.3)".into())
}

#[tauri::command]
pub async fn set_whisper_model_path(_path: String) -> IpcResult<()> {
    Err("set_whisper_model_path noch nicht implementiert (Phase 1.5)".into())
}
