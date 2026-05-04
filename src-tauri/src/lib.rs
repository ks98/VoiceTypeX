// SPDX-License-Identifier: GPL-3.0-or-later
//! VoiceTypeX — Bibliotheks-Einstiegspunkt.
//!
//! Phase 1.4: Pipeline ist verdrahtet. `exakt` (lokales Diktat) ist
//! end-to-end funktional; die anderen 5 Modi haben registrierte Hotkeys,
//! antworten aber mit "noch nicht implementiert" + Notification.

pub mod audio;
pub mod core;
pub mod hotkey;
pub mod injection;
pub mod ipc;
pub mod pipeline;
pub mod processing;
pub mod secrets;
pub mod transcription;
pub mod tray;

use crate::core::config::Settings;
use crate::core::default_modes::bootstrap_defaults_if_empty;
use crate::core::log_buffer::LogRingBuffer;
use crate::core::modes::ModesRegistry;
use crate::core::state::StateBus;
use crate::core::AppContext;
use crate::injection::{make_default_injector, TextInjector};
#[cfg(target_os = "linux")]
use crate::pipeline::spawn_wayland_hotkey_session;
use crate::pipeline::{
    register_mode_hotkeys, spawn_state_event_emitter, spawn_tray_recording_pulse,
    spawn_tray_state_listener,
};
use crate::transcription::local::LocalTranscriber;
use crate::transcription::Transcriber;
use parking_lot::{Mutex, RwLock};
use std::sync::Arc;
use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

pub fn run() {
    let log_buffer = LogRingBuffer::default();
    init_tracing(&log_buffer);

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
        .invoke_handler(tauri::generate_handler![
            ipc::settings::get_settings,
            ipc::settings::set_settings,
            ipc::settings::list_audio_devices,
            ipc::settings::set_whisper_model_path,
            ipc::settings::download_default_model,
            ipc::modes::get_modes,
            ipc::modes::reload_modes,
            ipc::modes::create_mode,
            ipc::modes::update_mode,
            ipc::modes::delete_mode,
            ipc::recording::start_recording,
            ipc::recording::stop_recording,
            ipc::recording::run_test_transcription,
            ipc::diagnostics::get_app_version,
            ipc::diagnostics::get_recent_logs,
            ipc::diagnostics::get_session_info,
            ipc::diagnostics::get_whisper_backend,
            ipc::diagnostics::get_hardware_report,
            ipc::secrets::get_provider_status,
            ipc::secrets::set_provider_key,
            ipc::secrets::delete_provider_key,
            ipc::secrets::test_provider_connection,
        ])
        .setup(move |app| {
            let app_handle = app.handle().clone();

            let config_dir = app
                .path()
                .app_config_dir()
                .map_err(|e| format!("app_config_dir: {e}"))?;
            let modes_dir = config_dir.join("modes");
            let model_dir = config_dir.join("models");
            std::fs::create_dir_all(&model_dir)?;
            std::fs::create_dir_all(&config_dir)?;

            // Backend-Detection fuer Secrets: keyring vs. file. Erkennt
            // automatisch defekte oder konfliktreiche Linux-Keyring-Setups
            // (z.B. gnome-keyring + kwallet gleichzeitig).
            crate::secrets::init_backend(config_dir.clone());

            // 6 Default-Modi anlegen, falls modes/ leer
            bootstrap_defaults_if_empty(&modes_dir)
                .map_err(|e| format!("bootstrap defaults: {e}"))?;

            // Modi laden + Hot-Reload aktivieren
            let modes_registry = Arc::new(
                ModesRegistry::load(modes_dir.clone())
                    .map_err(|e| format!("ModesRegistry::load: {e}"))?,
            );
            modes_registry
                .start_watching(modes_dir.clone())
                .map_err(|e| format!("start_watching: {e}"))?;

            // Pipeline-Komponenten
            let state_bus = StateBus::new();
            let model_path = model_dir.join("ggml-large-v3-turbo-q5_0.bin");
            let transcriber: Arc<dyn Transcriber> =
                Arc::new(LocalTranscriber::new(model_path.clone()));
            let injector_box = make_default_injector(app_handle.clone());
            let injector: Arc<dyn TextInjector> = Arc::from(injector_box);

            let ctx = Arc::new(AppContext {
                state_bus,
                modes: Arc::clone(&modes_registry),
                recorder_slot: Arc::new(Mutex::new(None)),
                transcriber,
                injector,
                settings: Arc::new(RwLock::new(Settings::default())),
                log_buffer: log_buffer.clone(),
                model_dir,
                modes_dir,
            });

            app.manage(Arc::clone(&ctx));

            // Tray, Hotkeys, State-Listener
            tray::setup_tray(&app_handle).map_err(|e| format!("setup_tray: {e}"))?;

            // Hotkey-Registrierung dispatch je nach Display-Server:
            //   - X11/Windows: tauri-plugin-global-shortcut (XGrabKey/RegisterHotKey)
            //   - Wayland: xdg-desktop-portal.GlobalShortcuts via ashpd
            //   - andere: Skip mit Warn-Log + UI-Trigger als Fallback
            let session = crate::core::session::detect_session();
            match session.display_server.as_str() {
                #[cfg(target_os = "linux")]
                "wayland" => {
                    spawn_wayland_hotkey_session(app_handle.clone(), Arc::clone(&ctx));
                    tracing::info!(
                        "Wayland-Hotkeys via xdg-portal angemeldet — Trigger-Buttons als Fallback bleiben verfuegbar"
                    );
                }
                "x11" | "windows" => {
                    register_mode_hotkeys(&app_handle, Arc::clone(&ctx))
                        .map_err(|e| format!("register_mode_hotkeys: {e}"))?;
                }
                other => {
                    tracing::warn!(
                        display_server = %other,
                        "Globale Hotkeys nicht unterstuetzt — UI-Trigger als Workaround"
                    );
                }
            }

            spawn_tray_state_listener(app_handle.clone());
            spawn_tray_recording_pulse(app_handle.clone());
            spawn_state_event_emitter(app_handle.clone());

            tracing::info!(
                display_server = %session.display_server,
                "VoiceTypeX gestartet (Phase 3 — Editor + Onboarding)"
            );
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Tauri-App konnte nicht gestartet werden");
}

fn init_tracing(buffer: &LogRingBuffer) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("voicetypex=info,tauri=info,warn"));
    let fmt_layer = tracing_subscriber::fmt::layer().with_target(true);
    let buffer_layer = buffer.layer();
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .with(buffer_layer)
        .init();
}
