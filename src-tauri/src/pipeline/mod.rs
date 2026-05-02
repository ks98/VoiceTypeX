// SPDX-License-Identifier: GPL-3.0-or-later
//! State-Machine-Pipeline-Driver.
//!
//! Verbindet Hotkey-Events mit Recorder, Transcriber, Processor, Injector.
//! Phase 1.4: nur der Modus `exakt` ist end-to-end verdrahtet (lokales
//! Whisper, keine LLM-Nachbearbeitung, Clipboard-Inject). Die anderen 5
//! Modi loggen `noch nicht implementiert` und zeigen eine Notification —
//! ihre Hotkeys sind aber registriert (DoD §6.1).

use crate::audio::{play_start_cue, play_stop_cue, recorder::RecorderHandle, RecorderConfig};
use crate::core::error::{Result, VoiceTypeError};
use crate::core::modes::{Mode, ProcessingTarget, TranscriptionTarget};
use crate::core::state::AppState;
use crate::core::AppContext;
use crate::injection::{InjectOptions, InjectionStrategy};
use crate::processing::{make_cloud_processor, make_local_processor, ProcessOpts, Processor};
use crate::transcription::{make_cloud_transcriber, TranscribeOpts, Transcriber};
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_notification::NotificationExt;

/// Toggle-Logik: wenn das Backend Idle ist → Recording starten; wenn es
/// Recording ist → stoppen und durch die Pipeline schicken.
///
/// Diese Funktion ist `async`, weil sie den Transcriber/Injector-Trait nutzt,
/// die beide async-Methoden haben.
pub async fn execute_mode(app: AppHandle, ctx: Arc<AppContext>, mode: Mode) -> Result<()> {
    let current = ctx.state_bus.current();

    if matches!(current, AppState::Recording) {
        finish_recording_and_inject(&app, &ctx, &mode).await
    } else if matches!(current, AppState::Idle) {
        start_recording(&ctx, &mode).await
    } else {
        tracing::warn!(state = %current.label(), "Mode-Trigger ignoriert (busy)");
        Ok(())
    }
}

async fn start_recording(ctx: &Arc<AppContext>, mode: &Mode) -> Result<()> {
    ctx.state_bus.transition(AppState::Recording)?;

    if let Err(e) = play_start_cue().await {
        tracing::warn!(error = %e, "Start-Cue fehlgeschlagen (nicht fatal)");
    }

    let recorder = RecorderHandle::start(RecorderConfig::default()).inspect_err(|e| {
        // Bei Fehler State zurueck auf Idle, damit kein Deadlock entsteht
        let _ = ctx.state_bus.transition(AppState::Error(e.to_string()));
        let _ = ctx.state_bus.transition(AppState::Idle);
    })?;

    *ctx.recorder_slot.lock() = Some(recorder);
    tracing::info!(mode = %mode.id, "Aufnahme gestartet");
    Ok(())
}

/// Spawnt eine Pulsing-Animation: alle 600 ms wechselt das Tray-Icon
/// zwischen `recording` und `recording_pulse`, solange der StateBus
/// `Recording` meldet. Der Loop terminiert sich selbst — kein Stop-Signal
/// noetig.
pub fn spawn_tray_recording_pulse(app: AppHandle) {
    use std::time::Duration;

    let state_rx = {
        let state = app.state::<Arc<AppContext>>();
        state.state_bus.subscribe()
    };

    tauri::async_runtime::spawn(async move {
        let mut bright = false;
        loop {
            tokio::time::sleep(Duration::from_millis(600)).await;

            let current = state_rx.borrow().clone();
            if !matches!(current, AppState::Recording) {
                // Pulse pausiert ausserhalb des Recording-States;
                // wir bleiben im Loop, weil ein neuer Recording-Zyklus
                // dieselbe Task wiederbeleben soll.
                continue;
            }

            let bytes = if bright {
                crate::tray::icon_bytes_recording_pulse()
            } else {
                crate::tray::icon_bytes_for_state(&AppState::Recording)
            };
            if let Some(tray) = app.tray_by_id("main") {
                if let Ok(image) = tauri::image::Image::from_bytes(bytes) {
                    let _ = tray.set_icon(Some(image));
                }
            }
            bright = !bright;

            // Wenn der Receiver geschlossen wurde (App-Shutdown), beende.
            if state_rx.has_changed().is_err() {
                break;
            }
        }
    });
}

async fn finish_recording_and_inject(
    app: &AppHandle,
    ctx: &Arc<AppContext>,
    mode: &Mode,
) -> Result<()> {
    let recorder = ctx
        .recorder_slot
        .lock()
        .take()
        .ok_or_else(|| VoiceTypeError::Audio("Stop ohne aktiven Recorder".into()))?;

    ctx.state_bus.transition(AppState::Transcribing)?;

    if let Err(e) = play_stop_cue().await {
        tracing::warn!(error = %e, "Stop-Cue fehlgeschlagen (nicht fatal)");
    }

    let wav = recorder.stop_and_finalize().inspect_err(|e| {
        let _ = ctx.state_bus.transition(AppState::Error(e.to_string()));
        let _ = ctx.state_bus.transition(AppState::Idle);
    })?;

    // STT — lokal oder Cloud, je nach Modus.
    let transcriber: Arc<dyn Transcriber> = match mode.transcription {
        TranscriptionTarget::Local => Arc::clone(&ctx.transcriber),
        TranscriptionTarget::Cloud => {
            let provider = mode.cloud_stt_provider.as_deref().unwrap_or("xai");
            make_cloud_transcriber(provider).inspect_err(|e| {
                let _ = ctx.state_bus.transition(AppState::Error(e.to_string()));
                let _ = ctx.state_bus.transition(AppState::Idle);
            })?
        }
    };

    let transcript = transcriber
        .transcribe_oneshot(
            &wav,
            TranscribeOpts {
                language: mode.language.clone(),
                initial_prompt: None,
            },
        )
        .await
        .inspect_err(|e| {
            let _ = ctx.state_bus.transition(AppState::Error(e.to_string()));
            let _ = ctx.state_bus.transition(AppState::Idle);
        })?;

    // Postprocessing — none / lokal (Ollama) / Cloud-LLM.
    let final_text = match mode.processing {
        ProcessingTarget::None => transcript,
        ProcessingTarget::Local => {
            ctx.state_bus.transition(AppState::Postprocessing)?;
            run_local_processing(ctx, mode, &transcript)
                .await
                .inspect_err(|e| {
                    let _ = ctx.state_bus.transition(AppState::Error(e.to_string()));
                    let _ = ctx.state_bus.transition(AppState::Idle);
                })?
        }
        ProcessingTarget::Cloud => {
            ctx.state_bus.transition(AppState::Postprocessing)?;
            run_cloud_processing(mode, &transcript)
                .await
                .inspect_err(|e| {
                    let _ = ctx.state_bus.transition(AppState::Error(e.to_string()));
                    let _ = ctx.state_bus.transition(AppState::Idle);
                })?
        }
    };

    ctx.state_bus.transition(AppState::Injecting)?;

    if final_text.trim().is_empty() {
        tracing::warn!(mode = %mode.id, "Pipeline-Output leer — kein Inject");
        ctx.state_bus.transition(AppState::Idle)?;
        return Ok(());
    }

    let injection_strategy = match mode.injection_method {
        crate::core::modes::InjectionMethod::Clipboard => InjectionStrategy::Clipboard,
        crate::core::modes::InjectionMethod::Keystrokes => InjectionStrategy::Keystrokes,
    };

    ctx.injector
        .inject(
            &final_text,
            InjectOptions {
                strategy: injection_strategy,
            },
        )
        .await
        .inspect_err(|e| {
            let _ = ctx.state_bus.transition(AppState::Error(e.to_string()));
            let _ = ctx.state_bus.transition(AppState::Idle);
        })?;

    ctx.state_bus.transition(AppState::Idle)?;
    tracing::info!(mode = %mode.id, len = final_text.len(), "Pipeline abgeschlossen");

    let _ = app; // app wird perspektivisch fuer Erfolgs-Notifications genutzt
    Ok(())
}

async fn run_local_processing(
    ctx: &Arc<AppContext>,
    mode: &Mode,
    transcript: &str,
) -> Result<String> {
    let model = mode.local_llm_model.clone().ok_or_else(|| {
        VoiceTypeError::Mode(format!(
            "Modus '{}': processing=local, aber kein local_llm_model gesetzt",
            mode.id
        ))
    })?;
    let ollama_url = ctx.settings.read().ollama_url.clone();
    let processor = make_local_processor(ollama_url, model);
    let system_prompt = mode.system_prompt.as_deref().unwrap_or("");
    processor
        .process(transcript, system_prompt, ProcessOpts::default())
        .await
}

async fn run_cloud_processing(mode: &Mode, transcript: &str) -> Result<String> {
    let provider = mode.cloud_llm_provider.as_deref().ok_or_else(|| {
        VoiceTypeError::Mode(format!(
            "Modus '{}': processing=cloud, aber kein cloud_llm_provider gesetzt",
            mode.id
        ))
    })?;
    let processor: Arc<dyn Processor> = make_cloud_processor(provider)?;
    let system_prompt = mode.system_prompt.as_deref().unwrap_or("");
    let opts = ProcessOpts {
        model: mode.cloud_llm_model.clone(),
        ..Default::default()
    };
    processor.process(transcript, system_prompt, opts).await
}

/// Registriere die Hotkeys aller geladenen Modi und verbinde sie mit
/// `execute_mode`. Bei Hotkey-Release passiert nichts; Trigger ist Press.
pub fn register_mode_hotkeys(app: &AppHandle, ctx: Arc<AppContext>) -> Result<()> {
    let modes = ctx.modes.current();

    for mode in modes {
        let accelerator = mode.hotkey.clone();
        let app_for_handler = app.clone();
        let ctx_for_handler = Arc::clone(&ctx);
        let mode_clone = mode.clone();

        let handler =
            move |_app: &AppHandle,
                  _shortcut: &_,
                  event: tauri_plugin_global_shortcut::ShortcutEvent| {
                if event.state() != ShortcutState::Pressed {
                    return;
                }
                let app = app_for_handler.clone();
                let ctx = Arc::clone(&ctx_for_handler);
                let mode = mode_clone.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = execute_mode(app.clone(), ctx, mode.clone()).await {
                        tracing::error!(mode = %mode.id, error = %e, "Pipeline fehlgeschlagen");
                        let _ = app
                            .notification()
                            .builder()
                            .title("VoiceTypeX — Fehler")
                            .body(e.to_string())
                            .show();
                    }
                });
            };

        app.global_shortcut()
            .on_shortcut(accelerator.as_str(), handler)
            .map_err(|e| {
                VoiceTypeError::Hotkey(format!(
                    "register '{}' fuer Modus '{}': {e}",
                    mode.hotkey, mode.id
                ))
            })?;
        tracing::info!(mode = %mode.id, hotkey = %mode.hotkey, "Hotkey registriert");
    }

    Ok(())
}

/// Spawnt einen Listener, der StateBus-Aenderungen in Tray-Icon-Updates
/// uebersetzt.
pub fn spawn_tray_state_listener(app: AppHandle) {
    // `tauri::State` ist nur eine Reference auf das gemanagte Singleton; wir
    // ziehen uns einen Receiver aus dem StateBus und lassen die State-Reference
    // sofort wieder fallen (passiert implizit am Block-Ende).
    let mut rx = {
        let state = app.state::<Arc<AppContext>>();
        state.state_bus.subscribe()
    };

    tauri::async_runtime::spawn(async move {
        loop {
            if rx.changed().await.is_err() {
                break;
            }
            let state = rx.borrow().clone();
            let icon_bytes = crate::tray::icon_bytes_for_state(&state);
            if let Some(tray) = app.tray_by_id("main") {
                match tauri::image::Image::from_bytes(icon_bytes) {
                    Ok(image) => {
                        if let Err(e) = tray.set_icon(Some(image)) {
                            tracing::warn!(error = %e, "Tray-Icon-Update fehlgeschlagen");
                        }
                    }
                    Err(e) => tracing::warn!(error = %e, "Tray-Icon-Decode fehlgeschlagen"),
                }
            }
        }
    });
}
