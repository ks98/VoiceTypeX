// SPDX-License-Identifier: GPL-3.0-or-later
//! VoiceTypeX — Bibliotheks-Einstiegspunkt.
//!
//! Setzt App-State, Plugins, Tray und Hotkey-Registrierung auf.
//! Pipeline-Details siehe [`pipeline`].

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
    register_menu_hotkey, spawn_overlay_state_listener, spawn_state_event_emitter,
    spawn_tray_recording_pulse, spawn_tray_state_listener,
};
use crate::processing::embedded::LlamaEmbeddedProcessor;
use crate::transcription::local::LocalTranscriber;
use crate::transcription::model_downloader::{LlmModelSlot, ModelSlot};
use crate::transcription::Transcriber;
use parking_lot::{Mutex, RwLock};
use std::path::PathBuf;
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
            ipc::settings::get_effective_menu_hotkey,
            ipc::settings::set_whisper_model_path,
            ipc::settings::download_default_model,
            ipc::settings::download_llm_default_model,
            ipc::cache::list_cached_files,
            ipc::cache::delete_cached_file,
            ipc::cache::delete_all_models,
            ipc::cache::clean_partial_downloads,
            ipc::reset::reset_api_keys,
            ipc::reset::reset_wayland_token,
            ipc::reset::reset_app_factory,
            ipc::modes::get_modes,
            ipc::modes::reload_modes,
            ipc::modes::create_mode,
            ipc::modes::update_mode,
            ipc::modes::delete_mode,
            ipc::recording::start_recording,
            ipc::recording::stop_recording,
            ipc::recording::cancel_menu,
            ipc::recording::run_test_transcription,
            ipc::diagnostics::get_app_version,
            ipc::diagnostics::get_recent_logs,
            ipc::diagnostics::get_session_info,
            ipc::diagnostics::get_whisper_backend,
            ipc::diagnostics::get_hardware_report,
            ipc::diagnostics::test_auto_paste,
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
            let settings_path = config_dir.join("settings.json");
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

            // Settings VOR der Pipeline-Konstruktion laden — sonst wuerde
            // der Modell-Pfad hartkodiert sein und User-Settings (Custom-
            // Pfad oder anderer Default-Slot) wuerden beim Bootstrap
            // ignoriert. Bei korruptem JSON fallen Defaults rein
            // (load_or_default loggt eine Warnung).
            let mut initial_settings = Settings::load_or_default(&settings_path);

            // First-Run-Locale-Detection: wenn settings.locale noch nicht
            // gesetzt ist, OS-Locale uebernehmen und persistieren. Das
            // passiert hier zentral im Backend (nicht im Frontend), damit
            // die drei Webview-Fenster (main, overlay, menu) nicht
            // unabhaengig schreiben — der Single-Writer ist der Setup-Hook.
            if initial_settings.locale.is_none() {
                let detected = tauri_plugin_os::locale();
                tracing::info!(detected = ?detected, "First-run locale detection");
                initial_settings.locale = detected;
                if let Err(e) = initial_settings.save(&settings_path) {
                    // Persistenz fehlgeschlagen → in-Memory-Wert wieder
                    // auf None setzen, damit Persistenz- und Laufzeit-
                    // Zustand konsistent bleiben. Konsequenz: beim
                    // naechsten Start laeuft die Detection erneut.
                    // Schwerwiegend genug fuer ERROR-Level — wenn die
                    // Settings-Datei nicht schreibbar ist, werden auch
                    // andere Settings-Aktionen scheitern.
                    tracing::error!(
                        error = %e,
                        "First-run locale persist failed — Detection laeuft bei jedem Start erneut",
                    );
                    initial_settings.locale = None;
                }
            }

            // Pipeline-Komponenten
            let state_bus = StateBus::new();

            // Modell-Pfad: Vorrang hat ein explizit gesetzter Custom-Pfad
            // (settings.whisper_model_path). Sonst Slot-basierter Default-
            // Name im model_dir.
            let model_path: PathBuf = initial_settings
                .whisper_model_path
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    let slot = ModelSlot::from_setting(&initial_settings.whisper_default_slot);
                    model_dir.join(slot.filename())
                });

            // VAD-Pfad zeigt auf die Standard-Silero-Datei. LocalTranscriber
            // prueft selbst, ob sie existiert — wenn nicht (z.B. weil der
            // VAD-Download noch nicht lief), laeuft Whisper transparent
            // ohne VAD, mit einer WARN-Log-Zeile pro Aufruf.
            let vad_model_path = Some(model_dir.join("ggml-silero-v6.2.0.bin"));
            // Phase 2: zwei Arcs auf dieselbe LocalTranscriber-Instanz. Der
            // dyn-Trait-Arc geht an die Trait-basierten Pipeline-Stellen,
            // der konkrete Arc an den Streaming-Worker (braucht die nicht-
            // trait-Methode transcribe_streaming_pass).
            let local_transcriber = Arc::new(LocalTranscriber::new(
                model_path.clone(),
                vad_model_path,
            ));
            let transcriber: Arc<dyn Transcriber> = local_transcriber.clone();

            // Phase 3b: Embedded-LLM-Processor. Pfad: User-Override hat
            // Vorrang, sonst Slot-basierter Default. Modell wird LAZY beim
            // ersten `process()`-Aufruf geladen — wenn der User Embedded
            // nicht benutzt, bleibt die Datei optional.
            let llm_model_path: PathBuf = initial_settings
                .llm_model_path
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    let slot = LlmModelSlot::from_setting(&initial_settings.llm_default_slot);
                    model_dir.join(slot.filename())
                });
            let local_llm_processor = Arc::new(LlamaEmbeddedProcessor::new(llm_model_path));
            let wayland_token_path = config_dir.join("wayland_session.json");
            let injector_box = make_default_injector(app_handle.clone(), wayland_token_path);
            let injector: Arc<dyn TextInjector> = Arc::from(injector_box);

            let ctx = Arc::new(AppContext {
                state_bus,
                modes: Arc::clone(&modes_registry),
                recorder_slot: Arc::new(Mutex::new(None)),
                active_mode: Arc::new(Mutex::new(None)),
                effective_menu_hotkey: Arc::new(RwLock::new(None)),
                transcriber,
                local_transcriber,
                local_llm_processor,
                extra_transcribers: Arc::new(Mutex::new(std::collections::HashMap::new())),
                extra_llm_processors: Arc::new(Mutex::new(std::collections::HashMap::new())),
                active_streaming_handle: Arc::new(Mutex::new(None)),
                injector,
                settings: Arc::new(RwLock::new(initial_settings)),
                settings_path,
                log_buffer: log_buffer.clone(),
                model_dir,
                modes_dir,
            });

            app.manage(Arc::clone(&ctx));

            // Tray, Hotkeys, State-Listener
            tray::setup_tray(&app_handle).map_err(|e| format!("setup_tray: {e}"))?;

            // Hauptfenster X-Knopf soll verstecken statt beenden — sonst
            // hat der User nach Close nur noch das Tray-Icon. App laeuft
            // weiter, ueber Tray-Linksklick oder "Einstellungen oeffnen"
            // wieder erreichbar.
            if let Some(main_window) = app.get_webview_window("main") {
                let main_clone = main_window.clone();
                main_window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = main_clone.hide();
                    }
                });
            }

            // Overlay-Window: kein set_ignore_cursor_events — das
            // verursacht einen tao-Panic bei initial-hidden Windows
            // (`visible: false`) auf Linux/GTK. Stattdessen Backend-
            // show/hide-Pattern: Overlay ist initial unsichtbar, wird
            // bei Menue-Open (Fokus erwuenscht, fuer Pfeil-/Enter-
            // Navigation) und Recording-Start sichtbar gemacht und vor
            // dem libei-Inject explizit wieder versteckt — siehe
            // pipeline/mod.rs::handle_menu_hotkey und
            // finish_recording_and_inject. Im Recording-Render-Modus
            // hat der StatusView zusaetzlich `pointer-events-none` als
            // CSS-Sicherheit; der Menue-Render-Modus laesst Eingaben zu.

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
                    register_menu_hotkey(&app_handle, Arc::clone(&ctx))
                        .map_err(|e| format!("register_menu_hotkey: {e}"))?;
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
            spawn_overlay_state_listener(app_handle.clone());

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
