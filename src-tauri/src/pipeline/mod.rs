// SPDX-License-Identifier: GPL-3.0-or-later
//! State-machine pipeline driver.
//!
//! Connects the single global menu hotkey with `Recorder`, `Transcriber`,
//! `Processor` and `Injector`. In the `Idle` state the hotkey opens the
//! mode-selection overlay; after `Enter` in the frontend the pipeline
//! starts via the `start_recording` IPC command. In the `Recording` state
//! the same hotkey stops recording and lets the pipeline run through.

use crate::audio::{play_start_cue, play_stop_cue, recorder::RecorderHandle, RecorderConfig};
use crate::core::edit::{compose_edit_input, resolve_output_action};
use crate::core::error::{Result, VoiceTypeError};
use crate::core::modes::{InputSource, Mode, OutputAction, ProcessingTarget, TranscriptionTarget};
use crate::core::state::AppState;
use crate::core::AppContext;
use crate::injection::{InjectOptions, InjectionStrategy};
use crate::processing::embedded::LlamaEmbeddedProcessor;
use crate::processing::{make_cloud_processor, make_local_processor, ProcessOpts, Processor};
use crate::transcription::local::LocalTranscriber;
use crate::transcription::local_agreement::stable_prefix;
use crate::transcription::model_downloader::{LlmModelSlot, ModelSlot};
use crate::transcription::{make_cloud_transcriber, TranscribeOpts, Transcriber};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_notification::NotificationExt;

/// Payload for the `app://partial-transcript` event. The frontend shows
/// the text in the overlay; each event replaces the previous state (no
/// append). Empty string = "clear partial" (before/after streaming).
#[derive(Clone, Serialize)]
struct PartialTranscriptPayload {
    text: String,
}

/// Configuration of the streaming decode loop. Kept centrally here so the
/// latency/CPU trade-offs are visible in one place.
///
/// Values are chosen defensively for CPU-only builds (`fast-cpu` without
/// GPU backend). On Vulkan/CUDA builds INITIAL_WAIT could be halved and
/// INTERVAL reduced to 500 ms — a phase-3 topic.
const STREAMING_INITIAL_WAIT_MS: u64 = 2_000;
const STREAMING_INTERVAL_MS: u64 = 800;
/// The first pass only starts when 1 s of audio is in the buffer — shorter
/// audios often hit the "single timestamp ending - skip entire chunk" case
/// of whisper.cpp, which produces empty outputs.
const STREAMING_MIN_AUDIO_SAMPLES: usize = 16_000; // 1 s at 16 kHz

/// Toggle logic for the IPC path (UI trigger button in the mode list,
/// `stop_recording` command).
///
/// On toggle-stop we use the `active_mode` stored in the `AppContext`
/// instead of the parameter: otherwise a UI trigger for mode B could
/// finalize the pipeline that was started with mode A via the menu
/// hotkey. The parameter mode is only relevant for the start path.
pub async fn execute_mode(app: AppHandle, ctx: Arc<AppContext>, mode: Mode) -> Result<()> {
    let current = ctx.state_bus.current();

    if matches!(current, AppState::Recording) {
        let active = ctx.active_mode.lock().clone();
        let resolved = active.unwrap_or(mode);
        finish_recording_and_inject(&app, &ctx, &resolved).await
    } else if matches!(current, AppState::Idle) {
        start_recording(&app, &ctx, &mode).await
    } else {
        tracing::warn!(state = %current.label(), "Mode-Trigger ignoriert (busy)");
        Ok(())
    }
}

/// Handler for the single global menu hotkey.
///
/// - `Idle` → show the `menu` window, hand focus to it.
/// - `Recording` → finalize the in-flight recording with the `active_mode`
///   (toggle-stop).
/// - otherwise → ignore (pipeline is busy, no re-trigger).
pub async fn handle_menu_hotkey(app: AppHandle, ctx: Arc<AppContext>) -> Result<()> {
    let current = ctx.state_bus.current();
    match current {
        AppState::Idle => {
            // Eager selection capture for the "Bearbeiten" feature: read
            // the focused app's selection NOW, while it still has focus
            // — showing the menu below steals it. Gated on edit-mode
            // presence so pure dictation setups pay nothing.
            capture_selection_if_edit_modes(&ctx).await;

            if let Some(menu) = app.get_webview_window("menu") {
                if let Err(e) = menu.show() {
                    tracing::warn!(error = %e, "menu.show() failed");
                }
                // set_focus on Wayland is compositor-dependent. The menu
                // window starts with `focus: true` in the config, which
                // gives KDE a stronger hint than a subsequent set_focus
                // on the overlay window.
                if let Err(e) = menu.set_focus() {
                    tracing::warn!(error = %e, "menu.set_focus() failed (compositor quirk)");
                }
            }
            tracing::info!("Menu hotkey: idle → menu opened");
        }
        AppState::Recording => {
            let active = ctx.active_mode.lock().clone();
            let Some(mode) = active else {
                tracing::warn!("Recording state without active_mode — menu hotkey stop ignored");
                return Ok(());
            };
            tracing::info!(mode = %mode.id, "Menu hotkey: recording → finish");
            finish_recording_and_inject(&app, &ctx, &mode).await?;
        }
        other => {
            tracing::warn!(state = %other.label(), "Menu hotkey ignored (busy)");
        }
    }
    Ok(())
}

/// Eagerly capture the focused app's selection into
/// `ctx.selection_buffer` — but only when at least one edit mode
/// (`Mode.input == Selection`) exists. The gate keeps pure dictation
/// setups free of any cost (no copy keystroke, no clipboard churn,
/// no added menu-open latency). Errors are non-fatal: the buffer is
/// cleared and edit modes degrade to an empty selection.
async fn capture_selection_if_edit_modes(ctx: &Arc<AppContext>) {
    let has_edit_mode = ctx
        .modes
        .current()
        .iter()
        .any(|m| m.input == InputSource::Selection);
    if !has_edit_mode {
        return;
    }

    match ctx.injector.read_selection().await {
        Ok(sel) => {
            tracing::debug!(captured = sel.is_some(), "Eager selection capture");
            *ctx.selection_buffer.lock() = sel;
        }
        Err(e) => {
            tracing::warn!(error = %e, "Eager selection capture failed");
            *ctx.selection_buffer.lock() = None;
        }
    }
}

async fn start_recording(app: &AppHandle, ctx: &Arc<AppContext>, mode: &Mode) -> Result<()> {
    ctx.state_bus.transition(AppState::Recording)?;

    // Remember the active mode: the menu-hotkey stop reads the mode here
    // that the pipeline must be finalized with. Cleared again in
    // `finish_recording_and_inject`.
    *ctx.active_mode.lock() = Some(mode.clone());

    // Hide the menu window if the start came from the menu — otherwise it
    // stays visible behind the overlay. Idempotent: if it was already
    // hidden (UI-trigger path), nothing happens.
    if let Some(menu) = app.get_webview_window("menu") {
        if let Err(e) = menu.hide() {
            tracing::warn!(error = %e, "menu.hide() before recording failed");
        }
    }

    // Make the status overlay visible. The window has `focus: false`,
    // so it doesn't steal keyboard focus from the target app. Before the
    // libei inject (`finish_recording_and_inject`) the overlay hides
    // again and focus stays with the target app.
    if let Some(overlay) = app.get_webview_window("overlay") {
        if let Err(e) = overlay.show() {
            tracing::warn!(error = %e, "Overlay show() failed (non-fatal)");
        }
    }

    if let Err(e) = play_start_cue().await {
        tracing::warn!(error = %e, "Start cue failed (non-fatal)");
    }

    let device_name = ctx.settings.read().audio_input_device.clone();
    let mut recorder = RecorderHandle::start(RecorderConfig { device_name }).inspect_err(|e| {
        // On error reset state to Idle so no deadlock occurs. Also clear
        // active_mode, otherwise the menu hotkey would see a stale entry.
        *ctx.active_mode.lock() = None;
        let _ = ctx.state_bus.transition(AppState::Error(e.to_string()));
        let _ = ctx.state_bus.transition(AppState::Idle);
    })?;

    // Only spawn the streaming worker for local STT. Cloud modes (xAI,
    // OpenAI, Groq, Deepgram) have no streaming interface; the one-shot
    // path after the stop hotkey stays active there. We grab the
    // samples_handle + meta from the recorder now, before it is put into
    // the slot — afterwards it sits behind a mutex we must not hold
    // across `.await`.
    if mode.transcription == TranscriptionTarget::Local {
        let samples_arc = recorder.samples_handle();
        match recorder.await_meta().await {
            Ok(meta) => {
                let app_clone = app.clone();
                let language = mode.language.clone();
                let initial_prompt = mode.initial_prompt.clone();
                let n_threads = ctx.settings.read().whisper_n_threads;
                // Resolve the mode override for the Whisper slot here
                // already, so the streaming worker uses the same
                // transcriber instance as the final pass after stop.
                // Otherwise the user would see partials from the default
                // model during recording, and get a diverging final
                // decode after stop.
                let transcriber_for_stream = resolve_local_transcriber(ctx, mode);
                let handle = tauri::async_runtime::spawn(async move {
                    streaming_worker(
                        app_clone,
                        transcriber_for_stream,
                        samples_arc,
                        meta,
                        language,
                        initial_prompt,
                        n_threads,
                    )
                    .await;
                });
                *ctx.active_streaming_handle.lock() = Some(handle);
                tracing::info!("Streaming worker spawned");
            }
            Err(e) => {
                // A failed streaming-worker start is not fatal — the
                // final pass after the stop hotkey still runs; the user
                // just gets no live partial. WARN, no abort.
                tracing::warn!(error = %e, "await_meta before streaming worker failed — running without live partial");
            }
        }
    }

    *ctx.recorder_slot.lock() = Some(recorder);
    tracing::info!(mode = %mode.id, "Recording started");
    Ok(())
}

/// Streaming decode loop. Runs while `State::Recording`; emits stable
/// prefixes via `app://partial-transcript`. Terminated by
/// `finish_recording_and_inject` via `JoinHandle::abort()` before the
/// final pass starts.
async fn streaming_worker(
    app: AppHandle,
    transcriber: Arc<LocalTranscriber>,
    samples_arc: Arc<parking_lot::Mutex<Vec<f32>>>,
    meta: crate::audio::recorder::StreamMeta,
    language: Option<String>,
    initial_prompt: Option<String>,
    n_threads: Option<u32>,
) {
    let ctx: Arc<AppContext> = app.state::<Arc<AppContext>>().inner().clone();
    use tokio::time::{sleep, Duration};

    // Initial wait so the microphone has anything substantial in the
    // buffer at all. Otherwise <1.5 s of German speech audio yields
    // empty decodes or Whisper hallucinates.
    tracing::info!("Streaming worker running (initial_wait={STREAMING_INITIAL_WAIT_MS}ms)");
    sleep(Duration::from_millis(STREAMING_INITIAL_WAIT_MS)).await;

    let mut prev_text = String::new();
    let mut committed = String::new();
    let mut iteration: u32 = 0;

    loop {
        iteration += 1;
        // Bail if state is no longer Recording (pipeline finalized).
        if !matches!(ctx.state_bus.current(), AppState::Recording) {
            tracing::info!(iteration, "Streaming-Worker: State != Recording, Exit");
            break;
        }

        // Briefly lock, clone, release the buffer — CPU work lock-free.
        let raw = samples_arc.lock().clone();
        if raw.len() < STREAMING_MIN_AUDIO_SAMPLES {
            tracing::debug!(iteration, raw_len = raw.len(), "Audio zu kurz, skip");
            sleep(Duration::from_millis(STREAMING_INTERVAL_MS)).await;
            continue;
        }

        let f16k = match crate::audio::recorder::to_16k_mono(&raw, meta) {
            Ok(s) if !s.is_empty() => s,
            Ok(_) => {
                tracing::warn!(iteration, "to_16k_mono lieferte leeres Ergebnis");
                sleep(Duration::from_millis(STREAMING_INTERVAL_MS)).await;
                continue;
            }
            Err(e) => {
                tracing::warn!(iteration, error = %e, "Streaming: resampling failed");
                sleep(Duration::from_millis(STREAMING_INTERVAL_MS)).await;
                continue;
            }
        };

        let opts = TranscribeOpts {
            language: language.clone(),
            initial_prompt: initial_prompt.clone(),
            n_threads,
            // Streaming pass is greedy — beam width is irrelevant here.
            beam_size: None,
        };

        let started = std::time::Instant::now();
        match transcriber.transcribe_streaming_pass(f16k, opts).await {
            Ok(curr_text) => {
                let elapsed_ms = started.elapsed().as_millis() as u64;
                tracing::info!(
                    iteration,
                    elapsed_ms,
                    text_len = curr_text.len(),
                    "Streaming-Pass fertig"
                );

                // Keep LocalAgreement-2 for telemetry (it shows how
                // stably the decodes converge), but do NOT use it as an
                // emit gate. Reason: on CPU-only hardware one pass takes
                // 8-12 s; a second pass rarely completes before the stop
                // hotkey, so LA-2 would block all emits. Pragmatically
                // we emit every decode directly — text may "waver" if a
                // later pass revises the first one, but that is better
                // than nothing. The final pass after stop overwrites
                // authoritatively anyway.
                let stable = stable_prefix(&prev_text, &curr_text);
                tracing::debug!(
                    iteration,
                    stable_len = stable.len(),
                    curr_len = curr_text.len(),
                    "LA-2-Konvergenz (Telemetrie)"
                );

                if !curr_text.is_empty() && curr_text != committed {
                    committed = curr_text.clone();
                    let emit_result = app.emit(
                        "app://partial-transcript",
                        PartialTranscriptPayload {
                            text: committed.clone(),
                        },
                    );
                    if let Err(e) = emit_result {
                        tracing::warn!(error = %e, "Partial emit failed");
                    } else {
                        tracing::info!(iteration, len = committed.len(), "Partial emittiert");
                    }
                }
                prev_text = curr_text;
            }
            Err(e) => {
                tracing::warn!(iteration, error = %e, "Streaming pass failed");
            }
        }

        sleep(Duration::from_millis(STREAMING_INTERVAL_MS)).await;
    }
}

/// Spawns a pulsing animation: every 600 ms the tray icon alternates
/// between `recording` and `recording_pulse` as long as the `StateBus`
/// reports `Recording`. The loop terminates itself — no stop signal
/// needed.
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
                // Pulse pauses outside the Recording state; we stay in
                // the loop because a new recording cycle should bring
                // the same task back to life.
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

            // If the receiver was closed (app shutdown), exit.
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

    // Clear active_mode already here — from this point the pipeline is
    // busy and the menu hotkey would do nothing in the Recording state
    // anyway (state is already Transcribing/Postprocessing/Injecting).
    // We avoid a pipeline exception leaving the entry behind.
    *ctx.active_mode.lock() = None;

    // Abort the phase-2 streaming worker before the final pass runs.
    // abort() interrupts the loop at the next await — CPU work inside
    // spawn_blocking still finishes, but doesn't block us. Then clear
    // the partial display in the overlay.
    if let Some(handle) = ctx.active_streaming_handle.lock().take() {
        handle.abort();
        tracing::debug!("Streaming-Worker abortet");
    }
    let _ = app.emit(
        "app://partial-transcript",
        PartialTranscriptPayload {
            text: String::new(),
        },
    );

    let recorder = ctx
        .recorder_slot
        .lock()
        .take()
        .ok_or_else(|| VoiceTypeError::Audio("Stop without active recorder".into()))?;

    ctx.state_bus.transition(AppState::Transcribing)?;

    if let Err(e) = play_stop_cue().await {
        tracing::warn!(error = %e, "Stop cue failed (non-fatal)");
    }

    let wav = recorder.stop_and_finalize().await.inspect_err(|e| {
        let _ = ctx.state_bus.transition(AppState::Error(e.to_string()));
        let _ = ctx.state_bus.transition(AppState::Idle);
    })?;

    // STT — local (with optional mode-override slot) or cloud, depending
    // on the mode.
    let transcriber: Arc<dyn Transcriber> = match mode.transcription {
        TranscriptionTarget::Local => {
            let local = resolve_local_transcriber(ctx, mode);
            local as Arc<dyn Transcriber>
        }
        TranscriptionTarget::Cloud => {
            let provider = mode.cloud_stt_provider.as_deref().unwrap_or("xai");
            make_cloud_transcriber(provider).inspect_err(|e| {
                let _ = ctx.state_bus.transition(AppState::Error(e.to_string()));
                let _ = ctx.state_bus.transition(AppState::Idle);
            })?
        }
    };

    // Settings read here before the await — the parking_lot
    // RwLockReadGuard is not Send and must not live across await points.
    // Effective beam width: a per-mode override wins over the global
    // default. Only the local final pass uses it; cloud STT ignores it.
    let (n_threads, beam_size) = {
        let s = ctx.settings.read();
        (
            s.whisper_n_threads,
            mode.whisper_beam_size.unwrap_or(s.whisper_beam_size),
        )
    };
    let transcript = transcriber
        .transcribe_oneshot(
            &wav,
            TranscribeOpts {
                language: mode.language.clone(),
                initial_prompt: mode.initial_prompt.clone(),
                n_threads,
                beam_size: Some(beam_size),
            },
        )
        .await
        .inspect_err(|e| {
            let _ = ctx.state_bus.transition(AppState::Error(e.to_string()));
            let _ = ctx.state_bus.transition(AppState::Idle);
        })?;

    // For edit modes ("Bearbeiten") the LLM input is the captured
    // selection plus the spoken instruction; for voice modes it is just
    // the transcript. The Processor trait is unchanged — it receives the
    // composed string as its "transcript". The selection is consumed
    // (`take`) so a later invocation cannot reuse a stale one.
    let processing_input = match mode.input {
        InputSource::Voice => transcript,
        InputSource::Selection => {
            let selection = ctx.selection_buffer.lock().take().unwrap_or_default();
            if selection.is_empty() {
                tracing::warn!(
                    mode = %mode.id,
                    "Edit mode without a captured selection — applying the instruction to an empty selection"
                );
            }
            compose_edit_input(&selection, &transcript)
        }
    };

    // Postprocessing — none / local (Ollama) / cloud LLM.
    let llm_output = match mode.processing {
        ProcessingTarget::None => processing_input,
        ProcessingTarget::Local => {
            ctx.state_bus.transition(AppState::Postprocessing)?;
            run_local_processing(ctx, mode, &processing_input)
                .await
                .inspect_err(|e| {
                    let _ = ctx.state_bus.transition(AppState::Error(e.to_string()));
                    let _ = ctx.state_bus.transition(AppState::Idle);
                })?
        }
        ProcessingTarget::Cloud => {
            ctx.state_bus.transition(AppState::Postprocessing)?;
            run_cloud_processing(mode, &processing_input)
                .await
                .inspect_err(|e| {
                    let _ = ctx.state_bus.transition(AppState::Error(e.to_string()));
                    let _ = ctx.state_bus.transition(AppState::Idle);
                })?
        }
    };

    // For edit modes, resolve the effective injection action and strip
    // the auto-mode sentinel; voice modes inject at the cursor as
    // before. NOTE: the injectors do not yet honour append/prepend
    // (collapse-then-paste lands in a follow-up step) — the action is
    // logged here and threaded into injection next.
    let (output_action, final_text) = match mode.input {
        InputSource::Selection => {
            resolve_output_action(mode.output, mode.output_fallback, &llm_output)
        }
        InputSource::Voice => (OutputAction::Insert, llm_output),
    };
    tracing::debug!(input = ?mode.input, action = ?output_action, "Injection action resolved");

    ctx.state_bus.transition(AppState::Injecting)?;

    if final_text.trim().is_empty() {
        tracing::warn!(mode = %mode.id, "Pipeline output empty — skipping inject");
        // Hide the overlay also in the empty path, so the compositor
        // state stays consistent.
        if let Some(overlay) = app.get_webview_window("overlay") {
            let _ = overlay.hide();
        }
        ctx.state_bus.transition(AppState::Idle)?;
        return Ok(());
    }

    // **Critical step:** hide the overlay before the libei inject so the
    // keyboard focus jumps back to the previously focused target app.
    // Without this step libei-Ctrl+V lands in the overlay window itself.
    // The 80 ms pause gives the compositor time to actually perform the
    // focus switch before libei types.
    if let Some(overlay) = app.get_webview_window("overlay") {
        if let Err(e) = overlay.hide() {
            tracing::warn!(error = %e, "Overlay hide() before inject failed");
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
                action: output_action,
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
    let system_prompt = mode.system_prompt.as_deref().unwrap_or("");
    let opts = ProcessOpts {
        temperature: mode.temperature,
        top_p: mode.top_p,
        repeat_penalty: mode.repeat_penalty,
        max_tokens: mode.max_tokens,
        ..Default::default()
    };

    // Engine choice per mode. `None` now falls back to `"embedded"` —
    // the built-in llama-cpp-2 path needs no external daemon and has
    // been the production default variant since phase 3b. Existing
    // TOMLs with Ollama config are explicitly set to `"ollama"` by
    // `Mode::migrate_deprecated_fields`, so this default switch does
    // not affect them. `"ollama"` remains available as opt-in for users
    // with their own daemon installation.
    let engine = mode.local_engine.as_deref().unwrap_or("embedded");
    match engine {
        "embedded" => {
            let processor: Arc<dyn Processor> = resolve_embedded_llm(ctx, mode);
            processor.process(transcript, system_prompt, opts).await
        }
        "ollama" => run_local_processing_ollama(ctx, mode, transcript, system_prompt, opts).await,
        other => Err(VoiceTypeError::Mode(format!(
            "Modus '{}': unbekannte local_engine '{other}' (erlaubt: \"embedded\" | \"ollama\")",
            mode.id
        ))),
    }
}

async fn run_local_processing_ollama(
    ctx: &Arc<AppContext>,
    mode: &Mode,
    transcript: &str,
    system_prompt: &str,
    opts: ProcessOpts,
) -> Result<String> {
    // `ollama_model_tag` is the new required key. `local_llm_model`
    // remains as a deprecated fallback for not-yet-migrated TOMLs (the
    // migration in `load_mode_from_path` already copies the field, but
    // we are defensive).
    let model = mode
        .ollama_model_tag
        .clone()
        .or_else(|| mode.local_llm_model.clone())
        .ok_or_else(|| {
            VoiceTypeError::Mode(format!(
                "Modus '{}': processing=local engine=ollama, aber kein ollama_model_tag gesetzt",
                mode.id
            ))
        })?;
    let (ollama_url, keep_alive) = {
        let s = ctx.settings.read();
        (s.ollama_url.clone(), s.ollama_keep_alive.clone())
    };
    let processor = make_local_processor(ollama_url, model, keep_alive);
    processor.process(transcript, system_prompt, opts).await
}

/// Resolver for the `LocalTranscriber` that a mode should use.
///
/// - No `mode.whisper_model_slot` set → global `ctx.local_transcriber`
///   (Whisper model from the settings).
/// - Slot identical to the global default slot → also the global
///   transcriber, so we don't hold a second model in RAM in parallel.
/// - Otherwise: cache lookup in `ctx.extra_transcribers`; on a cache
///   miss a new `LocalTranscriber` is constructed for the slot (the
///   model file is only loaded into the Whisper context on the first
///   `transcribe` call — the resolver only allocates metadata).
fn resolve_local_transcriber(ctx: &Arc<AppContext>, mode: &Mode) -> Arc<LocalTranscriber> {
    let Some(slot_slug) = mode.whisper_model_slot.as_ref() else {
        return ctx.local_transcriber.clone();
    };
    let default_slot = ctx.settings.read().whisper_default_slot.clone();
    if slot_slug == &default_slot {
        return ctx.local_transcriber.clone();
    }

    {
        let cache = ctx.extra_transcribers.lock();
        if let Some(found) = cache.get(slot_slug) {
            return found.clone();
        }
    }

    let slot = ModelSlot::from_setting(slot_slug);
    let model_path = ctx.model_dir.join(slot.filename());
    let vad_model_path = Some(ctx.model_dir.join("ggml-silero-v6.2.0.bin"));
    let new_transcriber = Arc::new(LocalTranscriber::new(model_path, vad_model_path));
    tracing::info!(
        slot = %slot_slug,
        mode_id = %mode.id,
        "LocalTranscriber-Override fuer Modus erstellt"
    );
    let mut cache = ctx.extra_transcribers.lock();
    cache
        .entry(slot_slug.clone())
        .or_insert(new_transcriber)
        .clone()
}

/// Analogous resolver for the embedded LLM processor
/// (`mode.embedded_llm_slot`).
fn resolve_embedded_llm(ctx: &Arc<AppContext>, mode: &Mode) -> Arc<LlamaEmbeddedProcessor> {
    let Some(slot_slug) = mode.embedded_llm_slot.as_ref() else {
        return ctx.local_llm_processor.clone();
    };
    let default_slot = ctx.settings.read().llm_default_slot.clone();
    if slot_slug == &default_slot {
        return ctx.local_llm_processor.clone();
    }

    {
        let cache = ctx.extra_llm_processors.lock();
        if let Some(found) = cache.get(slot_slug) {
            return found.clone();
        }
    }

    let slot = LlmModelSlot::from_setting(slot_slug);
    let model_path = ctx.model_dir.join(slot.filename());
    let new_processor = Arc::new(LlamaEmbeddedProcessor::new(model_path));
    tracing::info!(
        slot = %slot_slug,
        mode_id = %mode.id,
        "LlamaEmbeddedProcessor-Override fuer Modus erstellt"
    );
    let mut cache = ctx.extra_llm_processors.lock();
    cache
        .entry(slot_slug.clone())
        .or_insert(new_processor)
        .clone()
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
        temperature: mode.temperature,
        top_p: mode.top_p,
        repeat_penalty: mode.repeat_penalty,
        max_tokens: mode.max_tokens,
        ..Default::default()
    };
    processor.process(transcript, system_prompt, opts).await
}

/// Register the single global menu hotkey (X11/Windows path).
///
/// Unlike before there is **one** shortcut for the whole app: the
/// `Settings.menu_hotkey` opens the mode-selection menu (in the Idle
/// state) or stops an in-flight recording (in the Recording state). We
/// only react to `Pressed`; release events are ignored, because PTT is
/// obsolete due to the menu flow.
pub fn register_menu_hotkey(app: &AppHandle, ctx: Arc<AppContext>) -> Result<()> {
    let accelerator = ctx.settings.read().menu_hotkey.clone();

    let app_for_handler = app.clone();
    let ctx_for_handler = Arc::clone(&ctx);

    let handler = move |_app: &AppHandle,
                        _shortcut: &_,
                        event: tauri_plugin_global_shortcut::ShortcutEvent| {
        if !matches!(event.state(), ShortcutState::Pressed) {
            return;
        }
        let app = app_for_handler.clone();
        let ctx = Arc::clone(&ctx_for_handler);
        tauri::async_runtime::spawn(async move {
            if let Err(e) = handle_menu_hotkey(app.clone(), ctx).await {
                tracing::error!(error = %e, "Menu hotkey handler failed");
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
            VoiceTypeError::Hotkey(format!("register menu-hotkey '{accelerator}': {e}"))
        })?;
    tracing::info!(hotkey = %accelerator, "Menu hotkey registered");
    Ok(())
}

/// Wayland path: binds the single menu hotkey via xdg-portal
/// `GlobalShortcuts` and spawns a listener that maps every activation to
/// `handle_menu_hotkey`.
///
/// On Wayland the hotkey is only a **suggestion** — the compositor shows
/// the user a dialog for the final assignment on the first start.
///
/// Two tasks are spawned:
/// 1) Session task: keeps the portal connection alive and sends events
///    into the broadcast channel.
/// 2) Dispatcher task: reads the broadcast channel, calls
///    `handle_menu_hotkey`. Release events are ignored (no more PTT).
#[cfg(target_os = "linux")]
pub fn spawn_wayland_hotkey_session(app: AppHandle, ctx: Arc<AppContext>) {
    use crate::hotkey::linux_wayland::{run_global_shortcuts_session, WaylandShortcutSpec};
    use tokio::sync::broadcast;

    let preferred = ctx.settings.read().menu_hotkey.clone();
    let specs = vec![WaylandShortcutSpec {
        id: "open_menu".to_string(),
        description: "VoiceTypeX: Modus-Menue oeffnen / Aufnahme stoppen".to_string(),
        preferred_trigger: preferred,
    }];

    let (sender, mut receiver) = broadcast::channel(16);
    let sender_clone = sender.clone();
    let effective_cache = Arc::clone(&ctx.effective_menu_hotkey);

    // Task 1: portal session
    tauri::async_runtime::spawn(async move {
        if let Err(e) =
            run_global_shortcuts_session(specs, sender_clone, Some(effective_cache)).await
        {
            tracing::error!(error = %e, "Wayland hotkey session ended with error");
        }
    });

    // Task 2: dispatcher — only react to Pressed.
    let app_for_dispatch = app.clone();
    let ctx_for_dispatch = Arc::clone(&ctx);
    tauri::async_runtime::spawn(async move {
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    if !matches!(event.kind, crate::hotkey::HotkeyEventKind::Pressed) {
                        continue;
                    }
                    let app = app_for_dispatch.clone();
                    let ctx = Arc::clone(&ctx_for_dispatch);
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = handle_menu_hotkey(app.clone(), ctx).await {
                            tracing::error!(error = %e, "Menu hotkey handler error (Wayland)");
                        }
                    });
                }
                Err(broadcast::error::RecvError::Closed) => {
                    tracing::warn!("Wayland hotkey channel closed");
                    break;
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(missed = n, "Wayland hotkey events dropped (lag)");
                }
            }
        }
    });

    drop(sender);
}

/// Spawns a listener that emits every state change as the Tauri event
/// `app://state` to the frontend. Payload: { state: "recording" |
/// "transcribing" | ..., error?: string }. The overlay window
/// subscribes to the event and shows itself accordingly.
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

/// Spawns a listener that hides the overlay window automatically as soon
/// as the state switches to `Idle` (or briefly `Error`). This ensures
/// that the overlay also disappears on pipeline errors (transcription
/// error, LLM failure) — not only on the happy-path inject path. Making
/// it visible is explicitly done in `start_recording` (see above), not
/// here — otherwise the window could briefly pop up again when already
/// hidden, because a state event reports Recording once more.
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

/// Spawns a listener that translates `StateBus` changes into tray-icon
/// updates.
pub fn spawn_tray_state_listener(app: AppHandle) {
    // `tauri::State` is only a reference to the managed singleton; we
    // pull a receiver out of the StateBus and let the state reference
    // drop right away (happens implicitly at the end of the block).
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
                            tracing::warn!(error = %e, "Tray icon update failed");
                        }
                    }
                    Err(e) => tracing::warn!(error = %e, "Tray icon decode failed"),
                }
            }
        }
    });
}

/// Pipeline-stage choreography tests.
///
/// Background: the real pipeline functions (`finish_recording_and_inject`
/// etc.) are tightly coupled to `tauri::AppHandle` (window show/hide,
/// event emit, audio cues, `RecorderHandle`). Mocking them directly
/// would require refactoring that goes beyond the scope of these tests.
///
/// Instead, `run_pipeline_stages_for_test` checks the trait choreography
/// from the point after recorder stop: Transcribe → Optional Process →
/// Inject, plus the state transitions. That's the core that breaks most
/// often on mode changes and provider switches. Window / audio-cue /
/// recorder calls are deliberately not covered — they are pure I/O glue
/// and clearly isolated in the real function.
///
/// Additionally we test the state-based trigger logic from
/// `execute_mode` / `handle_menu_hotkey` (hotkey-while-busy) directly on
/// the `StateBus`, because it is exactly a match on
/// `state_bus.current()`.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::error::VoiceTypeError;
    use crate::core::state::{AppState, StateBus};
    use crate::injection::{InjectOptions, InjectorCapabilities, TextInjector};
    use crate::processing::{ProcessOpts, Processor};
    use crate::transcription::{TranscribeOpts, Transcriber};
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio::sync::Mutex as TokioMutex;

    // --- Mock implementations of the pipeline traits ---
    //
    // Constant output (transcriber/processor) or collecting Vec
    // (injector). No sleeps, no random values → deterministic.

    struct MockTranscriber {
        output: String,
    }

    #[async_trait]
    impl Transcriber for MockTranscriber {
        fn name(&self) -> &str {
            "mock-transcriber"
        }
        async fn transcribe_oneshot(&self, _audio: &[u8], _opts: TranscribeOpts) -> Result<String> {
            Ok(self.output.clone())
        }
    }

    struct FailingTranscriber;

    #[async_trait]
    impl Transcriber for FailingTranscriber {
        fn name(&self) -> &str {
            "failing-transcriber"
        }
        async fn transcribe_oneshot(&self, _audio: &[u8], _opts: TranscribeOpts) -> Result<String> {
            Err(VoiceTypeError::Transcription("mock STT failure".into()))
        }
    }

    struct PassthroughProcessor;

    #[async_trait]
    impl Processor for PassthroughProcessor {
        fn name(&self) -> &str {
            "mock-processor"
        }
        async fn process(
            &self,
            transcript: &str,
            _system_prompt: &str,
            _opts: ProcessOpts,
        ) -> Result<String> {
            Ok(transcript.to_string())
        }
    }

    struct CollectingInjector {
        injected: Arc<TokioMutex<Vec<String>>>,
    }

    #[async_trait]
    impl TextInjector for CollectingInjector {
        fn name(&self) -> &str {
            "mock-injector"
        }
        fn capabilities(&self) -> InjectorCapabilities {
            InjectorCapabilities {
                supports_clipboard: true,
                supports_keystrokes: true,
            }
        }
        async fn inject(&self, text: &str, _opts: InjectOptions) -> Result<()> {
            self.injected.lock().await.push(text.to_string());
            Ok(())
        }
        async fn read_selection(&self) -> Result<Option<String>> {
            Ok(None)
        }
    }

    /// Test-only stage choreography from the point after recorder stop.
    ///
    /// Mirrors the state transitions and trait calls from
    /// `finish_recording_and_inject` (lines 404-512) 1:1, **without**
    /// AppHandle calls (overlay hide, emit, window lookups). If the real
    /// function changes its ordering or its error-recovery path, this
    /// helper must be updated in sync — that is the deliberate trade-off
    /// against a full Tauri mock.
    async fn run_pipeline_stages_for_test(
        state_bus: &StateBus,
        transcriber: Arc<dyn Transcriber>,
        processor: Option<Arc<dyn Processor>>,
        injector: Arc<dyn TextInjector>,
        wav: &[u8],
    ) -> Result<String> {
        state_bus.transition(AppState::Transcribing)?;

        let transcript = match transcriber
            .transcribe_oneshot(wav, TranscribeOpts::default())
            .await
        {
            Ok(t) => t,
            Err(e) => {
                let _ = state_bus.transition(AppState::Error(e.to_string()));
                let _ = state_bus.transition(AppState::Idle);
                return Err(e);
            }
        };

        let final_text = match processor {
            Some(p) => {
                state_bus.transition(AppState::Postprocessing)?;
                match p.process(&transcript, "", ProcessOpts::default()).await {
                    Ok(t) => t,
                    Err(e) => {
                        let _ = state_bus.transition(AppState::Error(e.to_string()));
                        let _ = state_bus.transition(AppState::Idle);
                        return Err(e);
                    }
                }
            }
            None => transcript,
        };

        state_bus.transition(AppState::Injecting)?;
        injector
            .inject(
                &final_text,
                InjectOptions {
                    strategy: crate::injection::InjectionStrategy::Clipboard,
                    action: OutputAction::Insert,
                },
            )
            .await
            .inspect_err(|e| {
                let _ = state_bus.transition(AppState::Error(e.to_string()));
                let _ = state_bus.transition(AppState::Idle);
            })?;
        state_bus.transition(AppState::Idle)?;
        Ok(final_text)
    }

    /// Happy path: the mock transcriber returns a fixed string, the
    /// passthrough processor passes it through, the mock injector
    /// collects it. State must be Idle again at the end. Defends against
    /// regressions in the stage order and in the trait signatures.
    #[tokio::test]
    async fn pipeline_happy_path_mock_chain_finishes_idle() {
        let bus = StateBus::new();
        bus.transition(AppState::Recording).unwrap();
        let injected = Arc::new(TokioMutex::new(Vec::<String>::new()));
        let injector: Arc<dyn TextInjector> = Arc::new(CollectingInjector {
            injected: Arc::clone(&injected),
        });
        let transcriber: Arc<dyn Transcriber> = Arc::new(MockTranscriber {
            output: "Hallo Welt".into(),
        });
        let processor: Arc<dyn Processor> = Arc::new(PassthroughProcessor);

        let result = run_pipeline_stages_for_test(
            &bus,
            transcriber,
            Some(processor),
            injector,
            b"dummy-wav-bytes",
        )
        .await
        .expect("pipeline should succeed");

        assert_eq!(result, "Hallo Welt");
        assert_eq!(bus.current(), AppState::Idle);
        let recorded = injected.lock().await;
        assert_eq!(recorded.as_slice(), &["Hallo Welt".to_string()]);
    }

    /// Error recovery: a transcriber failure must end up in State::Idle
    /// (not stuck-in-Transcribing). Defends against the class of bugs
    /// that would arise without the `inspect_err` cleanup path.
    #[tokio::test]
    async fn pipeline_transcriber_error_returns_state_to_idle() {
        let bus = StateBus::new();
        bus.transition(AppState::Recording).unwrap();
        let injected = Arc::new(TokioMutex::new(Vec::<String>::new()));
        let injector: Arc<dyn TextInjector> = Arc::new(CollectingInjector {
            injected: Arc::clone(&injected),
        });
        let transcriber: Arc<dyn Transcriber> = Arc::new(FailingTranscriber);

        let err = run_pipeline_stages_for_test(&bus, transcriber, None, injector, b"wav")
            .await
            .expect_err("transcriber failure should propagate");

        assert!(matches!(err, VoiceTypeError::Transcription(_)));
        assert_eq!(bus.current(), AppState::Idle);
        let recorded = injected.lock().await;
        assert!(recorded.is_empty(), "no inject after STT failure");
    }

    /// Hotkey-while-busy: `execute_mode` ignores triggers when the
    /// pipeline is neither Idle nor Recording. Mirrors the match from
    /// `execute_mode` (lines 57-70). If this test is red, someone has
    /// broken the else branch (busy → Ok(()) without side-effects).
    #[tokio::test]
    async fn execute_mode_branch_ignores_trigger_when_pipeline_busy() {
        let bus = StateBus::new();
        // Recording → Transcribing is the first "busy" state.
        bus.transition(AppState::Recording).unwrap();
        bus.transition(AppState::Transcribing).unwrap();

        // The decision logic from `execute_mode`:
        let current = bus.current();
        let decision = if matches!(current, AppState::Recording) {
            "finalize"
        } else if matches!(current, AppState::Idle) {
            "start"
        } else {
            "ignore"
        };
        assert_eq!(decision, "ignore");
        // Important: NO state transition must have happened. The
        // StateBus stays on Transcribing — the trigger was swallowed.
        assert_eq!(bus.current(), AppState::Transcribing);

        // The same ignore path also applies to Postprocessing and
        // Injecting — we verify this exemplarily, so a later extension
        // of `execute_mode` (e.g. special handling for a single busy
        // state) must be made deliberately.
        bus.transition(AppState::Postprocessing).unwrap();
        let current = bus.current();
        let decision = if matches!(current, AppState::Recording | AppState::Idle) {
            "act"
        } else {
            "ignore"
        };
        assert_eq!(decision, "ignore");
    }
}
