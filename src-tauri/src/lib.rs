// SPDX-License-Identifier: GPL-3.0-or-later
//! VoiceTypeX — Bibliotheks-Einstiegspunkt.
//!
//! Phase 1.2: Backend-Skelett mit allen Modulen, Traits und Stubs. Funktionale
//! Implementierungen folgen in 1.3 ff.

pub mod audio;
pub mod core;
pub mod hotkey;
pub mod injection;
pub mod ipc;
pub mod processing;
pub mod secrets;
pub mod transcription;
pub mod tray;

use tauri_plugin_autostart::MacosLauncher;
use tracing_subscriber::EnvFilter;

pub fn run() {
    init_tracing();

    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            ipc::settings::get_settings,
            ipc::settings::set_settings,
            ipc::settings::list_audio_devices,
            ipc::settings::set_whisper_model_path,
            ipc::modes::get_modes,
            ipc::modes::reload_modes,
            ipc::recording::start_recording,
            ipc::recording::stop_recording,
            ipc::recording::run_test_transcription,
            ipc::diagnostics::get_app_version,
            ipc::diagnostics::get_recent_logs,
        ])
        .setup(|_app| {
            tracing::info!("VoiceTypeX gestartet (Phase 1.2 — Backend-Skelett)");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Tauri-App konnte nicht gestartet werden");
}

/// Initialisiere `tracing` mit `RUST_LOG`-Filter (Default: `info`).
///
/// CLAUDE.md §8: Audio-/Transkript-Daten gehen NIE ins Default-Logging. Phase
/// 1.6 ergaenzt einen In-Memory-Ring-Buffer-Layer fuer die Logs-View, der
/// nach denselben Regeln arbeitet.
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("voicetypex=info,tauri=info,warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();
}
