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
use crate::transcription::TranscribeOpts;
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

    // Phase 1: nur der Modus `exakt` ist end-to-end. Andere bleiben Stub.
    if mode.transcription != TranscriptionTarget::Local {
        notify_unimplemented(app, mode);
        ctx.state_bus.transition(AppState::Idle)?;
        return Ok(());
    }

    let transcript = ctx
        .transcriber
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

    if mode.processing != ProcessingTarget::None {
        // LLM-Nachbearbeitung steht in Phase 1.4 nur als Stub bereit.
        notify_unimplemented(app, mode);
        ctx.state_bus.transition(AppState::Idle)?;
        return Ok(());
    }

    ctx.state_bus.transition(AppState::Injecting)?;

    if transcript.trim().is_empty() {
        tracing::warn!(mode = %mode.id, "Transkript leer — kein Inject");
        ctx.state_bus.transition(AppState::Idle)?;
        return Ok(());
    }

    ctx.injector
        .inject(
            &transcript,
            InjectOptions {
                strategy: InjectionStrategy::Clipboard,
            },
        )
        .await
        .inspect_err(|e| {
            let _ = ctx.state_bus.transition(AppState::Error(e.to_string()));
            let _ = ctx.state_bus.transition(AppState::Idle);
        })?;

    ctx.state_bus.transition(AppState::Idle)?;
    tracing::info!(mode = %mode.id, len = transcript.len(), "Pipeline abgeschlossen");
    Ok(())
}

fn notify_unimplemented(app: &AppHandle, mode: &Mode) {
    tracing::info!(mode = %mode.id, "Modus noch nicht implementiert (Phase 2+)");
    let _ = app
        .notification()
        .builder()
        .title("VoiceTypeX")
        .body(format!(
            "Modus '{}' wird in einer spaeteren Phase implementiert.",
            mode.name
        ))
        .show();
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
