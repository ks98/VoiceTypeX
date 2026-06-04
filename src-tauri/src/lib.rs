// SPDX-License-Identifier: GPL-3.0-or-later
//! VoiceTypeX — library entry point.
//!
//! Sets up app state, plugins, tray and hotkey registration. Pipeline
//! details see [`pipeline`].

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
#[cfg(not(target_os = "windows"))]
use crate::processing::embedded::LlamaEmbeddedProcessor;
use crate::transcription::local::LocalTranscriber;
#[cfg(not(target_os = "windows"))]
use crate::transcription::model_downloader::LlmModelSlot;
use crate::transcription::model_downloader::ModelSlot;
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

    // Report the REAL runtime backend (not just the build flag) once at
    // startup. On a Vulkan build with no usable Vulkan device this prints
    // "running on CPU", which is the only way to spot a silent fallback.
    crate::transcription::backend::log_active_backend();

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
        .plugin(tauri_plugin_process::init())
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
            ipc::modes::reseed_default_modes,
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
            ipc::secrets::is_secrets_encrypted_at_rest,
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

            // Secrets backend detection: keyring vs. file. Recognises
            // broken or conflicting Linux keyring setups automatically
            // (e.g. gnome-keyring + kwallet running simultaneously).
            crate::secrets::init_backend(config_dir.clone());

            // Load settings first — the bootstrap_defaults_if_empty call
            // below needs the locale to pick the right default-mode set,
            // and downstream pipeline construction needs custom model
            // paths. On corrupt JSON, Settings::default falls in (with a
            // warn log).
            let mut initial_settings = Settings::load_or_default(&settings_path);

            // First-run locale detection: if Settings.locale is unset,
            // adopt the OS locale and persist. This runs centrally in
            // the backend (not the frontend) so the three webviews
            // (main, overlay, menu) don't race on the settings file —
            // single writer is this setup hook.
            // On Windows `tauri_plugin_os::locale()` reads the display/UI
            // language (GetUserPreferredUILanguages) and can return None (empty
            // MUI list). Fall back to the regional/format locale so the
            // first-run guess isn't a blind `en`. Additive: only used when the
            // plugin returns None, so the English-display cohort is untouched. (#6)
            #[cfg(windows)]
            fn locale_fallback() -> Option<String> {
                use windows::Win32::Globalization::GetUserDefaultLocaleName;
                let mut buf = [0u16; 85]; // LOCALE_NAME_MAX_LENGTH
                let len = unsafe { GetUserDefaultLocaleName(&mut buf) };
                if len <= 0 {
                    return None;
                }
                // `len` includes the trailing NUL.
                let s = String::from_utf16_lossy(&buf[..len as usize - 1]);
                (!s.is_empty()).then_some(s)
            }
            #[cfg(not(windows))]
            fn locale_fallback() -> Option<String> {
                None
            }

            if initial_settings.locale.is_none() {
                let detected = tauri_plugin_os::locale().or_else(locale_fallback);
                tracing::info!(detected = ?detected, "First-run locale detection");
                initial_settings.locale = detected;
                if let Err(e) = initial_settings.save(&settings_path) {
                    // Persist failed → reset in-memory value to None so
                    // the persisted state and runtime state stay
                    // consistent. Consequence: detection re-runs next
                    // start. Heavy enough for ERROR — if the settings
                    // file isn't writable, other settings actions will
                    // fail too.
                    tracing::error!(
                        error = %e,
                        "First-run locale persist failed — detection will re-run on every start",
                    );
                    initial_settings.locale = None;
                }
            }

            // 9 default modes for the active locale, bootstrap only if
            // the modes dir is still empty (user edits are preserved).
            bootstrap_defaults_if_empty(&modes_dir, initial_settings.locale.as_deref())
                .map_err(|e| format!("bootstrap defaults: {e}"))?;

            // Load modes + activate hot-reload watcher.
            let modes_registry = Arc::new(
                ModesRegistry::load(modes_dir.clone())
                    .map_err(|e| format!("ModesRegistry::load: {e}"))?,
            );
            modes_registry
                .start_watching(modes_dir.clone())
                .map_err(|e| format!("start_watching: {e}"))?;

            // Pipeline components
            let state_bus = StateBus::new();

            // Model path: an explicitly set custom path takes
            // precedence (`settings.whisper_model_path`). Otherwise
            // the slot-based default name inside `model_dir`.
            let model_path: PathBuf = initial_settings
                .whisper_model_path
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    let slot = ModelSlot::from_setting(&initial_settings.whisper_default_slot);
                    model_dir.join(slot.filename())
                });

            // The VAD path points to the standard Silero file.
            // `LocalTranscriber` itself checks whether it exists — if
            // not (e.g. because the VAD download has not run yet),
            // Whisper transparently runs without VAD, with one WARN
            // log line per call.
            let vad_model_path = Some(model_dir.join("ggml-silero-v6.2.0.bin"));
            // Phase 2: two Arcs onto the same `LocalTranscriber`
            // instance. The dyn-trait Arc goes to the trait-based
            // pipeline sites, the concrete Arc to the streaming
            // worker (which needs the non-trait method
            // `transcribe_streaming_pass`).
            let local_transcriber = Arc::new(LocalTranscriber::new(
                model_path.clone(),
                vad_model_path,
            ));
            let transcriber: Arc<dyn Transcriber> = local_transcriber.clone();

            // Phase 3b: embedded LLM processor. Path: user override
            // takes precedence, otherwise the slot-based default. The
            // model is loaded LAZILY on the first `process()` call —
            // if the user doesn't use embedded, the file stays
            // optional. Linux/macOS-only — llama-cpp-2 is not compiled
            // on Windows (issue #1 ggml link collision).
            #[cfg(not(target_os = "windows"))]
            let llm_model_path: PathBuf = initial_settings
                .llm_model_path
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    let slot = LlmModelSlot::from_setting(&initial_settings.llm_default_slot);
                    model_dir.join(slot.filename())
                });
            #[cfg(not(target_os = "windows"))]
            let local_llm_processor = Arc::new(LlamaEmbeddedProcessor::new(llm_model_path));
            let wayland_token_path = config_dir.join("wayland_session.json");
            let injector_box = make_default_injector(app_handle.clone(), wayland_token_path);
            let injector: Arc<dyn TextInjector> = Arc::from(injector_box);

            // KDE/Wayland terminal auto-detection (drives `paste_shortcut =
            // auto`). Filled asynchronously so a slow or absent KWin/D-Bus
            // setup never blocks or panics startup; until it lands the paste
            // path uses Ctrl+V.
            #[cfg(target_os = "linux")]
            let kde_focus: Arc<
                RwLock<Option<Arc<crate::injection::focus_tracker::KdeFocusTracker>>>,
            > = Arc::new(RwLock::new(None));
            #[cfg(target_os = "linux")]
            {
                let slot = kde_focus.clone();
                tauri::async_runtime::spawn(async move {
                    if let Some(tracker) = crate::injection::focus_tracker::start().await {
                        *slot.write() = Some(tracker);
                    }
                });
            }

            let ctx = Arc::new(AppContext {
                state_bus,
                modes: Arc::clone(&modes_registry),
                recorder_slot: Arc::new(Mutex::new(None)),
                active_mode: Arc::new(Mutex::new(None)),
                effective_menu_hotkey: Arc::new(RwLock::new(None)),
                transcriber,
                local_transcriber,
                #[cfg(not(target_os = "windows"))]
                local_llm_processor,
                extra_transcribers: Arc::new(Mutex::new(std::collections::HashMap::new())),
                #[cfg(not(target_os = "windows"))]
                extra_llm_processors: Arc::new(Mutex::new(std::collections::HashMap::new())),
                active_streaming_handle: Arc::new(Mutex::new(None)),
                injector,
                selection_buffer: Arc::new(Mutex::new(None)),
                settings: Arc::new(RwLock::new(initial_settings)),
                settings_path,
                log_buffer: log_buffer.clone(),
                model_dir,
                modes_dir,
                #[cfg(target_os = "linux")]
                kde_focus,
            });

            app.manage(Arc::clone(&ctx));

            // Tray, hotkeys, state listeners
            let tray_locale = ctx.settings.read().locale.clone();
            tray::setup_tray(&app_handle, tray_locale.as_deref())
                .map_err(|e| format!("setup_tray: {e}"))?;

            // The main window X button should hide instead of quit —
            // otherwise the user is left with only the tray icon
            // after close. The app keeps running and is reachable
            // again via tray left-click or "Open settings".
            if let Some(main_window) = app.get_webview_window("main") {
                let main_clone = main_window.clone();
                main_window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = main_clone.hide();
                    }
                });
            }

            // Overlay window: no `set_ignore_cursor_events` — that
            // causes a tao panic on initially-hidden windows
            // (`visible: false`) on Linux/GTK. Instead, backend
            // show/hide pattern: the overlay is initially invisible,
            // shown on menu-open (focus desired, for arrow/enter
            // navigation) and recording start, and explicitly hidden
            // again before the libei inject — see
            // `pipeline/mod.rs::handle_menu_hotkey` and
            // `finish_recording_and_inject`. In recording render mode
            // the `StatusView` additionally has `pointer-events-none`
            // as a CSS safety net; the menu render mode allows input.

            // Hotkey registration dispatch per display server:
            //   - X11/Windows: tauri-plugin-global-shortcut
            //     (XGrabKey/RegisterHotKey)
            //   - Wayland: xdg-desktop-portal.GlobalShortcuts via
            //     ashpd
            //   - other: skip with WARN log + UI triggers as fallback
            let session = crate::core::session::detect_session();
            match session.display_server.as_str() {
                #[cfg(target_os = "linux")]
                "wayland" => {
                    spawn_wayland_hotkey_session(app_handle.clone(), Arc::clone(&ctx));
                    tracing::info!(
                        "Wayland hotkeys registered via xdg-portal — trigger buttons remain available as fallback"
                    );
                }
                "x11" | "windows" => {
                    register_menu_hotkey(&app_handle, Arc::clone(&ctx))
                        .map_err(|e| format!("register_menu_hotkey: {e}"))?;
                }
                other => {
                    tracing::warn!(
                        display_server = %other,
                        "Global hotkeys not supported — UI triggers as workaround"
                    );
                }
            }

            spawn_tray_state_listener(app_handle.clone());
            spawn_tray_recording_pulse(app_handle.clone());
            spawn_state_event_emitter(app_handle.clone());
            spawn_overlay_state_listener(app_handle.clone());

            tracing::info!(
                display_server = %session.display_server,
                "VoiceTypeX started (Phase 3 — Editor + Onboarding)"
            );
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Tauri app failed to start");
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
