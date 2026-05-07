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
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_notification::NotificationExt;

/// Toggle-Logik (legacy): wird vom IPC-Pfad genutzt (UI-Trigger-Button).
/// Hotkey-Pfad nutzt jetzt `handle_hotkey_pressed` / `handle_hotkey_released`,
/// die das Push-to-Talk-Verhalten machen.
pub async fn execute_mode(app: AppHandle, ctx: Arc<AppContext>, mode: Mode) -> Result<()> {
    let current = ctx.state_bus.current();

    if matches!(current, AppState::Recording) {
        finish_recording_and_inject(&app, &ctx, &mode).await
    } else if matches!(current, AppState::Idle) {
        start_recording(&app, &ctx, &mode).await
    } else {
        tracing::warn!(state = %current.label(), "Mode-Trigger ignoriert (busy)");
        Ok(())
    }
}

/// Hotkey-Press-Handler: in PTT-Mode "Recording starten", in
/// Toggle-Mode "execute_mode" (Toggle-Verhalten).
pub async fn handle_hotkey_pressed(app: AppHandle, ctx: Arc<AppContext>, mode: Mode) -> Result<()> {
    let ptt = ctx.settings.read().ptt_mode;
    if !ptt {
        return execute_mode(app, ctx, mode).await;
    }
    let current = ctx.state_bus.current();
    if !matches!(current, AppState::Idle) {
        tracing::warn!(
            state = %current.label(),
            "PTT-Press ignoriert (nicht im Idle-State)"
        );
        return Ok(());
    }
    start_recording(&app, &ctx, &mode).await
}

/// Hotkey-Release-Handler: stoppt Recording, durchlaeuft die Pipeline.
pub async fn handle_hotkey_released(
    app: AppHandle,
    ctx: Arc<AppContext>,
    mode: Mode,
) -> Result<()> {
    let ptt = ctx.settings.read().ptt_mode;
    if !ptt {
        return Ok(());
    }
    let current = ctx.state_bus.current();
    if !matches!(current, AppState::Recording) {
        tracing::debug!(
            state = %current.label(),
            "PTT-Release ignoriert (nicht Recording)"
        );
        return Ok(());
    }
    finish_recording_and_inject(&app, &ctx, &mode).await
}

async fn start_recording(
    app: &AppHandle,
    ctx: &Arc<AppContext>,
    mode: &Mode,
) -> Result<()> {
    ctx.state_bus.transition(AppState::Recording)?;

    // Overlay sichtbar machen. Das `show()` klaut zwar kurz den
    // Tastatur-Fokus auf KDE Plasma 6, aber das ist OK: der User spricht
    // jetzt sowieso, kein Tastatur-Input nötig. Vor dem libei-Inject
    // (`finish_recording_and_inject`) versteckt sich das Overlay wieder
    // und der Fokus springt zurück zur Ziel-App, sodass libei dort tippt.
    if let Some(overlay) = app.get_webview_window("overlay") {
        if let Err(e) = overlay.show() {
            tracing::warn!(error = %e, "Overlay show() fehlgeschlagen (nicht fatal)");
        }
    }

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
    tracing::info!(
        mode_id = %mode.id,
        transcription = ?mode.transcription,
        processing = ?mode.processing,
        cloud_stt = ?mode.cloud_stt_provider,
        cloud_llm = ?mode.cloud_llm_provider,
        "Pipeline-Start (Modus-Eigenschaften)"
    );

    let recorder = ctx
        .recorder_slot
        .lock()
        .take()
        .ok_or_else(|| VoiceTypeError::Audio("Stop ohne aktiven Recorder".into()))?;

    ctx.state_bus.transition(AppState::Transcribing)?;

    if let Err(e) = play_stop_cue().await {
        tracing::warn!(error = %e, "Stop-Cue fehlgeschlagen (nicht fatal)");
    }

    let wav = recorder.stop_and_finalize().await.inspect_err(|e| {
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

    // Settings-Read hier vor dem await — der RwLockReadGuard von parking_lot
    // ist nicht Send und darf nicht ueber await-Punkte hinweg leben.
    let n_threads = ctx.settings.read().whisper_n_threads;
    let transcript = transcriber
        .transcribe_oneshot(
            &wav,
            TranscribeOpts {
                language: mode.language.clone(),
                initial_prompt: None,
                n_threads,
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
        // Overlay verstecken auch im Empty-Pfad, damit der Compositor-State
        // konsistent bleibt.
        if let Some(overlay) = app.get_webview_window("overlay") {
            let _ = overlay.hide();
        }
        ctx.state_bus.transition(AppState::Idle)?;
        return Ok(());
    }

    // **Kritischer Schritt:** Overlay vor libei-Inject verstecken, damit
    // der Tastatur-Fokus zur vorher fokussierten Ziel-App zurueckspringt.
    // Ohne diesen Schritt landet libei-Strg+V im Overlay-Window selbst.
    // Die 80 ms Pause gibt dem Compositor Zeit, den Fokus-Wechsel
    // tatsaechlich zu vollziehen, bevor libei tippt.
    if let Some(overlay) = app.get_webview_window("overlay") {
        if let Err(e) = overlay.hide() {
            tracing::warn!(error = %e, "Overlay hide() vor Inject fehlgeschlagen");
        }
    }
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;

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


/// Registriere die Hotkeys aller geladenen Modi (X11/Windows-Pfad).
/// Press → handle_hotkey_pressed, Release → handle_hotkey_released.
/// PTT-vs-Toggle entscheidet sich erst in den Handlern via Settings.
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
                let app = app_for_handler.clone();
                let ctx = Arc::clone(&ctx_for_handler);
                let mode = mode_clone.clone();
                let kind = event.state();
                tauri::async_runtime::spawn(async move {
                    let result = match kind {
                        ShortcutState::Pressed => {
                            handle_hotkey_pressed(app.clone(), ctx, mode.clone()).await
                        }
                        ShortcutState::Released => {
                            handle_hotkey_released(app.clone(), ctx, mode.clone()).await
                        }
                    };
                    if let Err(e) = result {
                        tracing::error!(
                            mode = %mode.id,
                            kind = ?kind,
                            error = %e,
                            "Pipeline fehlgeschlagen"
                        );
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

/// Wayland-Pfad: bind alle Modus-Hotkeys ueber das xdg-portal.GlobalShortcuts
/// und spawnt einen Listener, der Activations auf `execute_mode` mappt.
///
/// Zwei Tasks werden gespawnt:
/// 1) Session-Task: hält die Portal-Verbindung am Leben + sendet Events
///    in den broadcast-Channel.
/// 2) Dispatcher-Task: liest broadcast-Channel, ruft `execute_mode` mit
///    dem entsprechenden Modus.
#[cfg(target_os = "linux")]
pub fn spawn_wayland_hotkey_session(app: AppHandle, ctx: Arc<AppContext>) {
    use crate::hotkey::linux_wayland::{run_global_shortcuts_session, WaylandShortcutSpec};
    use tokio::sync::broadcast;

    let modes = ctx.modes.current();
    let specs: Vec<WaylandShortcutSpec> = modes
        .iter()
        .map(|m| WaylandShortcutSpec {
            id: m.id.clone(),
            description: m.name.clone(),
            preferred_trigger: m.hotkey.clone(),
        })
        .collect();

    if specs.is_empty() {
        tracing::warn!("Keine Modi geladen — kein Wayland-Hotkey-Bind");
        return;
    }

    let (sender, mut receiver) = broadcast::channel(16);
    let sender_clone = sender.clone();

    // Task 1: Portal-Session
    tauri::async_runtime::spawn(async move {
        if let Err(e) = run_global_shortcuts_session(specs, sender_clone).await {
            tracing::error!(error = %e, "Wayland-Hotkey-Session beendet mit Fehler");
        }
    });

    // Task 2: Dispatcher — leitet Pressed/Released an PTT-aware Handler weiter.
    let app_for_dispatch = app.clone();
    let ctx_for_dispatch = Arc::clone(&ctx);
    tauri::async_runtime::spawn(async move {
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    let mode = ctx_for_dispatch.modes.find_by_id(&event.id);
                    let Some(mode) = mode else {
                        tracing::warn!(id = %event.id, "Wayland-Hotkey-Event ohne passenden Modus");
                        continue;
                    };
                    let app = app_for_dispatch.clone();
                    let ctx = Arc::clone(&ctx_for_dispatch);
                    let kind = event.kind;
                    tauri::async_runtime::spawn(async move {
                        let result = match kind {
                            crate::hotkey::HotkeyEventKind::Pressed => {
                                handle_hotkey_pressed(app.clone(), ctx, mode.clone()).await
                            }
                            crate::hotkey::HotkeyEventKind::Released => {
                                handle_hotkey_released(app.clone(), ctx, mode.clone()).await
                            }
                        };
                        if let Err(e) = result {
                            tracing::error!(
                                mode = %mode.id,
                                kind = ?kind,
                                error = %e,
                                "Pipeline-Fehler (Wayland-Hotkey)"
                            );
                        }
                    });
                }
                Err(broadcast::error::RecvError::Closed) => {
                    tracing::warn!("Wayland-Hotkey-Channel geschlossen");
                    break;
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(missed = n, "Wayland-Hotkey-Events verworfen (Lag)");
                }
            }
        }
    });

    // Sender lebt im Session-Task; wir lassen die Variable hier im Scope
    // sterben, weil receiver bereits den Kanal-Lifetime sichert.
    drop(sender);
}

/// Spawnt einen Listener, der jede State-Aenderung als Tauri-Event
/// `app://state` ans Frontend emittiert. Payload: { state: "recording"
/// | "transcribing" | ..., error?: string }. Das Overlay-Window
/// abonniert das Event und zeigt sich entsprechend.
pub fn spawn_state_event_emitter(app: AppHandle) {
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
            let payload = match &state {
                AppState::Error(msg) => serde_json::json!({
                    "state": "error",
                    "error": msg,
                }),
                other => serde_json::json!({ "state": other.label() }),
            };
            let _ = app.emit("app://state", payload);
        }
    });
}

/// Spawnt einen Listener, der das Overlay-Window automatisch versteckt,
/// sobald der State auf `Idle` (oder kurzzeitig `Error`) wechselt. Damit
/// ist sichergestellt, dass das Overlay auch bei Pipeline-Fehlern
/// (Transkriptions-Error, LLM-Failure) wieder verschwindet — und nicht
/// nur im Happy-Path-Inject-Pfad. Das Sichtbar-Machen ist explizit in
/// `start_recording` (siehe oben), nicht hier — sonst koennte das Window
/// kurzzeitig wieder aufpoppen, wenn schon hidden, weil ein State-Event
/// nochmal Recording meldet.
pub fn spawn_overlay_state_listener(app: AppHandle) {
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
            if matches!(state, AppState::Idle | AppState::Error(_)) {
                if let Some(overlay) = app.get_webview_window("overlay") {
                    let _ = overlay.hide();
                }
            }
        }
    });
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
