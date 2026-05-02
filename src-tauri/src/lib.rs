// SPDX-License-Identifier: GPL-3.0-or-later
//! VoiceTypeX — Bibliotheks-Einstiegspunkt.
//!
//! Phase 1.1: minimale Tauri-App mit allen geplanten Plugins. Funktionale
//! Module (Audio, STT, Modes, Tray, Hotkey, Injection) folgen in 1.2 ff.

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
        .setup(|_app| {
            tracing::info!("VoiceTypeX gestartet (Phase 1.1 — Scaffolding)");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Tauri-App konnte nicht gestartet werden");
}

/// Initialisiere `tracing` mit `RUST_LOG`-Filter (Default: `info`).
///
/// In Phase 1.6 wird zusaetzlich ein In-Memory-Ring-Buffer-Layer fuer die
/// Logs-View im Frontend ergaenzt; Audio-/Transkript-Daten bleiben dabei
/// strikt aussen vor (siehe CLAUDE.md §8 — keine sensiblen Daten ins Log).
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("voicetypex=info,tauri=info,warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();
}
