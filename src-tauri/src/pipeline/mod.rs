// SPDX-License-Identifier: GPL-3.0-or-later
//! State-machine pipeline driver.
//!
//! Connects the single global menu hotkey with `Recorder`, `Transcriber`,
//! `Processor` and `Injector`. In the `Idle` state the hotkey opens the
//! mode-selection overlay; after `Enter` in the frontend the pipeline
//! starts via the `start_recording` IPC command. In the `Recording` state
//! the same hotkey stops recording and lets the pipeline run through.

use crate::audio::{
    play_start_cue, play_stop_cue, recorder::encode_wav_16k_mono, recorder::RecorderHandle,
    RecorderConfig,
};
use crate::core::edit::{compose_edit_input, resolve_output_action};
use crate::core::error::{Result, VoiceTypeError};
use crate::core::modes::{InputSource, Mode, OutputAction, ProcessingTarget, TranscriptionTarget};
use crate::core::state::AppState;
use crate::core::AppContext;
use crate::injection::{InjectOptions, InjectionStrategy};
#[cfg(not(target_os = "windows"))]
use crate::processing::embedded::LlamaEmbeddedProcessor;
use crate::processing::{make_cloud_processor, make_local_processor, ProcessOpts, Processor};
use crate::transcription::local::LocalTranscriber;
use crate::transcription::local_agreement::stable_prefix;
#[cfg(not(target_os = "windows"))]
use crate::transcription::model_downloader::LlmModelSlot;
use crate::transcription::model_downloader::ModelSlot;
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

/// The `AppHandle`-free dependency bundle the pure stage core
/// (`run_stages`) needs: the `StateBus` for the transitions, the resolved
/// transcriber, and the per-stage opts the caller computed from the
/// mode + settings (issue #34).
///
/// The processor is **not** bundled here: it is resolved lazily by the
/// caller-provided `resolve_processor` closure passed to `run_stages`, so
/// a processor-resolution failure surfaces *after* the `Postprocessing`
/// transition — preserving the original parked-from state (a resolve
/// failure must park from `Postprocessing`, not earlier, and must not
/// skip the STT pass). The transcriber, by contrast, is resolved by the
/// caller while the bus is already `Transcribing`, so its resolve failure
/// parks from `Transcribing` exactly as before; no closure is needed
/// there.
///
/// Resolving these (model/provider caches, beam/thread settings, the WAV
/// branch) and all the UI/glue (overlay, cues, events, the 80 ms
/// focus-handoff sleep, and the inject itself) stays in the caller;
/// `run_stages` only borrows what it executes up to the inject boundary,
/// so it carries no `AppHandle` and no `AppContext`. The injector and the
/// inject options stay caller-side because the inject is inseparable from
/// the Wayland overlay choreography (issue #36).
struct PipelineDeps<'a> {
    state_bus: &'a crate::core::state::StateBus,
    transcriber: StageTranscriber<'a>,
    transcribe_opts: TranscribeOpts,
    /// The processor's `system_prompt` + `ProcessOpts`, resolved by the
    /// caller from the mode/settings. Only consulted when the caller
    /// supplies a `resolve_processor` closure (processing != none).
    process_opts: StageProcessOpts<'a>,
}

/// How `run_stages` performs the STT step. The local path feeds the f32
/// samples straight to whisper-rs; the cloud path wraps them in a WAV
/// once and calls the `Transcriber` trait. The caller picks the variant
/// (resolving the local/cloud instance via the caches); `run_stages`
/// only runs it, so the #46 f32/WAV split stays a caller decision.
enum StageTranscriber<'a> {
    Local(&'a LocalTranscriber),
    Cloud(&'a dyn Transcriber),
}

/// The processor's `system_prompt`, resolved by the caller from the mode,
/// plus the `ProcessOpts`. Held in `PipelineDeps` so the core stays free
/// of mode lookups.
struct StageProcessOpts<'a> {
    system_prompt: &'a str,
    opts: ProcessOpts,
}

/// The `final_text` plus the resolved injection action `run_stages`
/// produced for the caller to inject. `run_stages` runs the STT pass, the
/// optional `Postprocessing` transition + LLM pass, and the output-action
/// resolution, then returns at the inject boundary — so the caller keeps
/// the Wayland-critical overlay-hide + 80 ms focus-handoff sleep wrapped
/// around the inject (issue #36). The `transcribe_ms`/`process_ms` fields
/// are the #43 stage timings, returned so the caller logs byte-identical
/// numbers.
struct StageOutput {
    final_text: String,
    output_action: OutputAction,
    transcribe_ms: u64,
    process_ms: u64,
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
        // `Error` shares the `Idle` path: the menu hotkey doubles as the
        // recovery action. From `Error` we first clear it (`Error → Idle`,
        // which makes the overlay listener hide the error overlay), then
        // open the menu exactly like from `Idle`. This is why pipeline
        // error paths can park in `Error` without trapping the app.
        AppState::Idle | AppState::Error(_) => {
            if matches!(current, AppState::Error(_)) {
                let _ = ctx.state_bus.transition(AppState::Idle);
            }
            // Eager selection capture for the "Bearbeiten" feature: read
            // the focused app's selection NOW, while it still has focus
            // — showing the menu below steals it. Gated on edit-mode
            // presence so pure dictation setups pay nothing.
            capture_selection_if_edit_modes(&ctx).await;

            if let Some(menu) = app.get_webview_window("menu") {
                if let Err(e) = menu.show() {
                    tracing::warn!(error = %e, "menu.show() failed");
                } else {
                    // center() after show(): config-center is unreliable for a
                    // window created `visible:false` (computed pre-map → top-left
                    // on Windows). Per-show because the menu is re-shown on every
                    // hotkey press, and config-center only fires once. (#5)
                    let _ = menu.center();
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

    // Overlay engine status (issue #8): tell the overlay which engines +
    // models this mode uses, emitted after active_mode is set and before the
    // overlay is shown so the status line is populated when recording renders.
    let engine_status = {
        let settings = ctx.settings.read();
        crate::core::modes::resolve_engine_status(mode, &settings)
    };
    if let Err(e) = app.emit("app://active-engine", &engine_status) {
        tracing::warn!(error = %e, "emit app://active-engine failed");
    }

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
        } else {
            // center() after show(): config `center:true` is unreliable for a
            // window created `visible:false` (computed pre-map → top-left on
            // Windows). Done per-show because the overlay is re-shown every
            // recording, and config-center only fires once at creation. (#5)
            let _ = overlay.center();
        }
    }

    // Fire-and-forget: the start cue is UX feedback, not a dependency of
    // recording. Awaiting it (sink.sleep_until_end over the whole beep)
    // delayed RecorderHandle::start until the beep finished, so speech on
    // the cue clipped the first syllables (#44). Spawn it so the recorder
    // comes up immediately; a cue failure is still logged, just off the
    // critical path.
    tauri::async_runtime::spawn(async {
        if let Err(e) = play_start_cue().await {
            tracing::warn!(error = %e, "Start cue failed (non-fatal)");
        }
    });

    let device_name = ctx.settings.read().audio_input_device.clone();
    let mut recorder = RecorderHandle::start(RecorderConfig { device_name }).inspect_err(|e| {
        // Surface the failure: stay in `Error` (the overlay shows it, the
        // tray turns red) instead of silently snapping back to `Idle`. The
        // next menu hotkey clears `Error` → `Idle` and reopens the menu.
        // Clear active_mode so that recovery does not see a stale entry.
        *ctx.active_mode.lock() = None;
        let _ = ctx.state_bus.transition(AppState::Error(e.to_string()));
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
                tracing::warn!(iteration, "to_16k_mono returned an empty result");
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
                    "streaming pass done"
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
                    "LA-2 convergence (telemetry)"
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
                        tracing::info!(iteration, len = committed.len(), "partial emitted");
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

/// Spawns a pulsing animation: while the `StateBus` reports `Recording`,
/// the tray icon alternates between `recording` and `recording_pulse`
/// every 600 ms. Outside `Recording` the task sleeps on the state watch
/// channel and produces no periodic wakeups. The loop exits when the
/// `StateBus` sender is dropped (app shutdown).
pub fn spawn_tray_recording_pulse(app: AppHandle) {
    use std::time::Duration;

    let mut state_rx = {
        let state = app.state::<Arc<AppContext>>();
        state.state_bus.subscribe()
    };

    tauri::async_runtime::spawn(async move {
        let mut bright = false;
        loop {
            // Idle path: no timer — sleep until the state changes. When
            // the sender is dropped on shutdown, `changed()` errors and
            // we exit.
            if !matches!(*state_rx.borrow(), AppState::Recording) {
                if state_rx.changed().await.is_err() {
                    break;
                }
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

            // Recording path: tick the pulse every 600 ms, but wake
            // early if the state leaves Recording so we drop straight
            // back to the idle wait instead of blinking a stale frame.
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(600)) => {}
                changed = state_rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
            }
        }
    });
}

/// Pure, `AppHandle`-free stage core (issue #35): runs the STT →
/// (optional) LLM stage sequence on the captured `samples`, plus the
/// edit-mode composition and the output-action resolution, and drives the
/// `Postprocessing` AppState transition with the same per-stage
/// `Error(state)` recovery as before. The caller has already transitioned
/// the bus to `Transcribing` (that transition is coupled to the finalize
/// step, whose failure must park from `Transcribing`), so this core does
/// not touch it. It stops at the inject boundary and returns the
/// `final_text` plus the resolved action so the caller keeps the inject
/// and the surrounding Wayland overlay-hide + 80 ms focus-handoff sleep
/// (issue #36) — that choreography is timing-critical and must stay
/// around the inject, not inside this core.
///
/// `resolve_processor` is `Some(closure)` when `processing != none` and
/// `None` for pass-through. The closure is invoked *after* the
/// `Postprocessing` transition so a processor-resolution failure parks
/// from `Postprocessing` (not earlier) and never skips the STT pass —
/// matching the original inline behavior. The closure carries the only
/// `AppContext` dependency the processing step has; the core itself names
/// neither `AppHandle` nor `AppContext`.
///
/// `selection` is the eager-captured edit-mode selection (already taken
/// out of `ctx.selection_buffer` by the caller); for voice modes it is
/// `None`. The #43 transcribe/process timings are measured here and
/// returned so the caller logs byte-identical numbers.
async fn run_stages<F>(
    deps: &PipelineDeps<'_>,
    samples: &[f32],
    mode: &Mode,
    selection: Option<String>,
    resolve_processor: Option<F>,
) -> Result<StageOutput>
where
    F: FnOnce() -> Result<Arc<dyn Processor>>,
{
    // STT — local (f32 straight to whisper-rs) or cloud (WAV-wrapped by
    // the caller's `StageTranscriber::Cloud` resolution). The #43 timing
    // mark wraps the whole step including the cloud-only WAV encode, so
    // the cost lands in `transcribe_ms`.
    let t_transcribe_start = std::time::Instant::now();
    let transcript = match &deps.transcriber {
        StageTranscriber::Local(local) => {
            local
                .transcribe_samples(samples, deps.transcribe_opts.clone())
                .await
        }
        StageTranscriber::Cloud(transcriber) => match encode_wav_16k_mono(samples) {
            Ok(wav) => {
                transcriber
                    .transcribe_oneshot(&wav, deps.transcribe_opts.clone())
                    .await
            }
            Err(e) => Err(e),
        },
    }
    .inspect_err(|e| {
        let _ = deps.state_bus.transition(AppState::Error(e.to_string()));
    })?;
    let transcribe_ms = t_transcribe_start.elapsed().as_millis() as u64;

    // For edit modes ("Bearbeiten") the LLM input is the captured
    // selection plus the spoken instruction; for voice modes it is just
    // the transcript. The Processor trait is unchanged — it receives the
    // composed string as its "transcript".
    let processing_input = match mode.input {
        InputSource::Voice => transcript,
        InputSource::Selection => {
            let selection = selection.unwrap_or_default();
            if selection.is_empty() {
                tracing::warn!(
                    mode = %mode.id,
                    "Edit mode without a captured selection — applying the instruction to an empty selection"
                );
            }
            compose_edit_input(&selection, &transcript)
        }
    };

    // Postprocessing — pass-through (`None`) / processor. The pass-through
    // arm reads ~0 ms, which is the correct attribution. The
    // `Postprocessing` transition precedes the processor resolution, so a
    // resolve failure parks from `Postprocessing` exactly as the inline
    // `run_local_processing` / `run_cloud_processing` did.
    let t_process_start = std::time::Instant::now();
    let llm_output = match resolve_processor {
        None => processing_input,
        Some(resolve) => {
            deps.state_bus.transition(AppState::Postprocessing)?;
            let processor = resolve().inspect_err(|e| {
                let _ = deps.state_bus.transition(AppState::Error(e.to_string()));
            })?;
            processor
                .process(
                    &processing_input,
                    deps.process_opts.system_prompt,
                    deps.process_opts.opts.clone(),
                )
                .await
                .inspect_err(|e| {
                    let _ = deps.state_bus.transition(AppState::Error(e.to_string()));
                })?
        }
    };
    let process_ms = t_process_start.elapsed().as_millis() as u64;

    // For edit modes, resolve the effective injection action and strip
    // the auto-mode sentinel; voice modes inject at the cursor.
    let (output_action, final_text) = match mode.input {
        InputSource::Selection => {
            resolve_output_action(mode.output, mode.output_fallback, &llm_output)
        }
        InputSource::Voice => (OutputAction::Insert, llm_output),
    };
    tracing::debug!(input = ?mode.input, action = ?output_action, "Injection action resolved");

    Ok(StageOutput {
        final_text,
        output_action,
        transcribe_ms,
        process_ms,
    })
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
        "pipeline start (mode properties)"
    );

    // Clear active_mode already here — from this point the pipeline is
    // busy and the menu hotkey would do nothing in the Recording state
    // anyway (state is already Transcribing/Postprocessing/Injecting).
    // We avoid a pipeline exception leaving the entry behind.
    *ctx.active_mode.lock() = None;

    // Abort the phase-2 streaming worker before the final pass runs.
    // Two-part cancel (issue #47):
    // 1. `abort_streaming()` sets the cooperative cancel flag the
    //    streaming pass's whisper.cpp abort callback checks, so an
    //    in-flight `spawn_blocking` decode returns early instead of
    //    running to completion and starving the latency-critical final
    //    pass for CPU cores. Resolved on the same transcriber instance
    //    the streaming worker uses (same `resolve_local_transcriber`).
    //    The final pass uses `DecodeProfile::Final`, which never installs
    //    the callback, so it can never be aborted by this flag.
    // 2. `handle.abort()` stops the worker's async loop at the next
    //    await, so no further streaming pass is started.
    // Only local STT spawns a streaming worker; for cloud modes there is
    // nothing to cancel and resolving the transcriber would needlessly
    // touch the model cache.
    if mode.transcription == TranscriptionTarget::Local {
        resolve_local_transcriber(ctx, mode).abort_streaming();
    }
    if let Some(handle) = ctx.active_streaming_handle.lock().take() {
        handle.abort();
        tracing::debug!("Streaming worker aborted");
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

    // Fire-and-forget: the stop cue is UX feedback, not a dependency of
    // finalize/transcribe. Awaiting it (sink.sleep_until_end over the
    // whole beep) added the full cue length to the felt paste latency
    // (perf #3). Spawn it so the pipeline proceeds to stop_and_finalize
    // immediately; a cue failure is still logged, just off the critical
    // path.
    tauri::async_runtime::spawn(async {
        if let Err(e) = play_stop_cue().await {
            tracing::warn!(error = %e, "Stop cue failed (non-fatal)");
        }
    });

    // Per-stage latency instrumentation (issue #43): mark the boundary
    // before each stage so felt paste latency can be attributed to
    // finalize vs. STT vs. LLM vs. inject on real dictations. Logged once
    // at the end at info level (Logs tab). No transcript text is logged.
    // The transcribe/process marks live inside `run_stages` and come back
    // as `transcribe_ms`/`process_ms`, so the logged numbers are
    // unchanged by the extraction.
    let t_finalize_start = std::time::Instant::now();
    // 16 kHz mono f32 — the local path feeds these straight to whisper;
    // only the cloud path lazily wraps them in a WAV (issue #46).
    let samples = recorder.stop_and_finalize().await.inspect_err(|e| {
        let _ = ctx.state_bus.transition(AppState::Error(e.to_string()));
    })?;
    let finalize_ms = t_finalize_start.elapsed().as_millis() as u64;

    // --- Resolve the deps for the pure stage core (issue #34). ---
    // The model/provider caches, the beam/thread settings, the WAV branch
    // choice, the processor + its system prompt, and the inject options
    // are all resolved here so `run_stages` carries no AppHandle/AppContext
    // and no mode/settings lookups.

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
    let transcribe_opts = TranscribeOpts {
        language: mode.language.clone(),
        initial_prompt: mode.initial_prompt.clone(),
        n_threads,
        beam_size: Some(beam_size),
    };

    // STT instance: local (with optional mode-override slot) or cloud.
    // A cloud-resolve failure stays a stage error parked in `Error`, as
    // before; previously it surfaced inside the STT match arm, now it
    // surfaces here at resolve time — same `Error(state)` recovery, same
    // propagated error.
    let local_transcriber;
    let cloud_transcriber;
    let stage_transcriber = match mode.transcription {
        TranscriptionTarget::Local => {
            local_transcriber = resolve_local_transcriber(ctx, mode);
            StageTranscriber::Local(local_transcriber.as_ref())
        }
        TranscriptionTarget::Cloud => {
            let provider = mode.cloud_stt_provider.as_deref().unwrap_or("xai");
            cloud_transcriber = resolve_cloud_transcriber(ctx, provider).inspect_err(|e| {
                let _ = ctx.state_bus.transition(AppState::Error(e.to_string()));
            })?;
            StageTranscriber::Cloud(cloud_transcriber.as_ref())
        }
    };

    // Processor `system_prompt` + opts. The processor *instance* is not
    // resolved here: `run_stages` invokes the `resolve_processor` closure
    // below only after the `Postprocessing` transition, so a resolve
    // failure (cloud keychain, Windows-embedded steering error, missing
    // ollama tag) parks from `Postprocessing` and never skips STT — the
    // original inline ordering of `run_local_processing` /
    // `run_cloud_processing`.
    let system_prompt = mode.system_prompt.as_deref().unwrap_or("");
    let process_opts = StageProcessOpts {
        system_prompt,
        opts: ProcessOpts {
            // Cloud uses the mode's cloud model; embedded/Ollama ignore it.
            model: if matches!(mode.processing, ProcessingTarget::Cloud) {
                mode.cloud_llm_model.clone()
            } else {
                None
            },
            temperature: mode.temperature,
            top_p: mode.top_p,
            repeat_penalty: mode.repeat_penalty,
            max_tokens: mode.max_tokens,
            ..Default::default()
        },
    };

    // The selection is consumed (`take`) so a later invocation cannot
    // reuse a stale one. Only meaningful for edit modes; voice modes
    // ignore it inside `run_stages`.
    let selection = ctx.selection_buffer.lock().take();

    let deps = PipelineDeps {
        state_bus: &ctx.state_bus,
        transcriber: stage_transcriber,
        transcribe_opts,
        process_opts,
    };

    // `None` = pass-through (processing == none); otherwise the closure
    // resolves the cloud/local processor on demand inside `run_stages`,
    // after the `Postprocessing` transition. It captures `ctx`/`mode`, so
    // the only `AppContext` dependency of the processing step lives in the
    // closure, not in `run_stages`.
    let resolve_processor = match mode.processing {
        ProcessingTarget::None => None,
        ProcessingTarget::Local | ProcessingTarget::Cloud => {
            Some(|| resolve_processor_for_mode(ctx, mode))
        }
    };

    let StageOutput {
        final_text,
        output_action,
        transcribe_ms,
        process_ms,
    } = run_stages(&deps, &samples, mode, selection, resolve_processor).await?;

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

    // Terminals (Konsole, …) paste on Ctrl+Shift+V, not Ctrl+V. Explicit
    // per-mode `ctrl_shift_v`/`ctrl_v` win; `auto` consults the KDE focus
    // tracker (terminal -> shift) and stays Ctrl+V wherever it's unavailable.
    let paste_with_shift = match mode.paste_shortcut {
        crate::core::modes::PasteShortcut::CtrlShiftV => true,
        crate::core::modes::PasteShortcut::CtrlV => false,
        crate::core::modes::PasteShortcut::Auto => {
            #[cfg(target_os = "linux")]
            {
                ctx.kde_focus
                    .read()
                    .as_ref()
                    .is_some_and(|f| f.focused_is_terminal())
            }
            #[cfg(not(target_os = "linux"))]
            {
                false
            }
        }
    };

    let t_inject_start = std::time::Instant::now();
    ctx.injector
        .inject(
            &final_text,
            InjectOptions {
                strategy: injection_strategy,
                action: output_action,
                paste_with_shift,
            },
        )
        .await
        .inspect_err(|e| {
            let _ = ctx.state_bus.transition(AppState::Error(e.to_string()));
        })?;
    let inject_ms = t_inject_start.elapsed().as_millis() as u64;

    ctx.state_bus.transition(AppState::Idle)?;
    // Per-stage latency summary (issue #43). The stage durations sum to
    // less than felt latency because fixed glue (overlay hide + 80 ms
    // focus-handoff sleep, audio cues) sits between them; `total` is the
    // sum of the measured stages, not wall-clock from the stop hotkey.
    let total_ms = finalize_ms + transcribe_ms + process_ms + inject_ms;
    tracing::info!(
        mode = %mode.id,
        finalize_ms,
        transcribe_ms,
        process_ms,
        inject_ms,
        total_ms,
        "pipeline stage timings"
    );
    tracing::info!(mode = %mode.id, len = final_text.len(), "pipeline complete");

    Ok(())
}

/// Drive the captured diagnostic samples through the shared stage core
/// (issue #37): the test-transcription IPC command no longer hand-rolls a
/// second Transcribe choreography — it records, transitions the bus to
/// `Transcribing`, and then calls this, so STT flows through the same
/// `run_stages` path (and the same `Transcribing → Error` parking on an
/// STT failure) as a real dictation.
///
/// The diagnostic is mode-less, so it uses `Mode::diagnostic()`
/// (voice/local/pass-through). `run_stages` is invoked with no processor
/// (`resolve_processor = None`), so there is no `Postprocessing`
/// transition and no LLM pass; the returned `final_text` is exactly the
/// transcript. The diagnostic stays silent: `run_stages` stops at the
/// inject boundary, the caller never injects, and nothing here touches the
/// overlay, cues or tray.
///
/// Returns just the transcript — the caller measures RTF/`processing_ms`
/// itself (it wraps the call in its own `Instant`), so the #43 stage
/// timings inside `StageOutput` are unused here.
pub(crate) async fn run_test_transcription_stage(
    ctx: &Arc<AppContext>,
    samples: &[f32],
    transcribe_opts: TranscribeOpts,
) -> Result<String> {
    let mode = Mode::diagnostic();
    let transcriber = app_default_transcriber(ctx);
    let deps = PipelineDeps {
        state_bus: &ctx.state_bus,
        transcriber: StageTranscriber::Local(transcriber.as_ref()),
        transcribe_opts,
        // Pass-through diagnostic: `run_stages` skips the processor when
        // `resolve_processor` is `None`, so these opts are never read.
        process_opts: StageProcessOpts {
            system_prompt: "",
            opts: ProcessOpts::default(),
        },
    };

    // `None` = pass-through. The turbofish names a concrete `F` for the
    // never-taken `Some` arm so the generic `run_stages` resolves; a
    // fn-pointer satisfies `FnOnce() -> Result<Arc<dyn Processor>>`.
    let out = run_stages(
        &deps,
        samples,
        &mode,
        None,
        None::<fn() -> Result<Arc<dyn Processor>>>,
    )
    .await?;
    Ok(out.final_text)
}

/// Resolve the `Processor` instance a mode should use, dispatching on
/// `mode.processing` (issue #34/#35). Pure resolution: it builds (or
/// fetches from cache) the cloud / embedded / Ollama processor and
/// returns the `Arc<dyn Processor>`; the `.process()` call and the
/// `ProcessOpts`/`system_prompt` construction now live in `run_stages` /
/// the caller. Synchronous — every backend's resolution (keychain read,
/// cache lookup, model-path build) is sync; only `.process()` was async.
///
/// `run_stages` invokes this only after the `Postprocessing` transition,
/// so a resolution error (cloud keychain, Windows-embedded steering,
/// missing ollama tag) parks from `Postprocessing` exactly as the inline
/// `run_local_processing`/`run_cloud_processing` did.
fn resolve_processor_for_mode(ctx: &Arc<AppContext>, mode: &Mode) -> Result<Arc<dyn Processor>> {
    match mode.processing {
        ProcessingTarget::None => {
            unreachable!("pass-through is handled by run_stages, not resolved")
        }
        ProcessingTarget::Cloud => resolve_cloud_processor_for_mode(ctx, mode),
        ProcessingTarget::Local => resolve_local_processor_for_mode(ctx, mode),
    }
}

fn resolve_local_processor_for_mode(
    ctx: &Arc<AppContext>,
    mode: &Mode,
) -> Result<Arc<dyn Processor>> {
    // Engine choice per mode. `None` now falls back to `"embedded"` —
    // the built-in llama-cpp-2 path needs no external daemon and has
    // been the production default variant since phase 3b. Existing
    // TOMLs with Ollama config are explicitly set to `"ollama"` by
    // `Mode::migrate_deprecated_fields`, so this default switch does
    // not affect them. `"ollama"` remains available as opt-in for users
    // with their own daemon installation.
    // On Windows the embedded llama.cpp engine is not compiled in (issue
    // #1 ggml link collision), so an unset engine defaults to ollama there
    // instead of embedded.
    let engine = mode
        .local_engine
        .as_deref()
        .unwrap_or(if cfg!(target_os = "windows") {
            "ollama"
        } else {
            "embedded"
        });
    match engine {
        #[cfg(not(target_os = "windows"))]
        "embedded" => Ok(resolve_embedded_llm(ctx, mode)),
        // The embedded engine is gated off on Windows; steer the user to
        // an alternative instead of failing with an opaque error.
        #[cfg(target_os = "windows")]
        "embedded" => Err(VoiceTypeError::Mode(format!(
            "Mode '{}': the embedded local LLM is not available on Windows. \
             Set local_engine = \"ollama\" (with your own Ollama install) or \
             switch this mode's processing to a cloud provider.",
            mode.id
        ))),
        "ollama" => resolve_ollama_processor(ctx, mode),
        other => Err(VoiceTypeError::Mode(format!(
            "Mode '{}': unknown local_engine '{other}' (allowed: \"embedded\" | \"ollama\")",
            mode.id
        ))),
    }
}

fn resolve_ollama_processor(ctx: &Arc<AppContext>, mode: &Mode) -> Result<Arc<dyn Processor>> {
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
                "Mode '{}': processing=local engine=ollama, but no ollama_model_tag set",
                mode.id
            ))
        })?;
    let (ollama_url, keep_alive) = {
        let s = ctx.settings.read();
        (s.ollama_url.clone(), s.ollama_keep_alive.clone())
    };
    Ok(make_local_processor(ollama_url, model, keep_alive))
}

/// The Whisper model path the app-default transcriber should currently
/// use, derived from the settings: an explicit `whisper_model_path`
/// override wins, otherwise the slot-based default file in `model_dir`.
///
/// Single source of truth shared by startup (`lib.rs`) and the runtime
/// rebuild in `app_default_transcriber`, so both agree on what "the
/// current default model" means (issue #30).
pub fn resolve_default_model_path(
    settings: &crate::core::config::Settings,
    model_dir: &std::path::Path,
) -> std::path::PathBuf {
    settings
        .whisper_model_path
        .as_ref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            let slot = ModelSlot::from_setting(&settings.whisper_default_slot);
            model_dir.join(slot.filename())
        })
}

/// Returns the app-default `LocalTranscriber`, rebuilding it first if the
/// settings Whisper slot/path no longer matches the model the stored
/// transcriber was built for.
///
/// This is what makes a slot change (or `download_default_model`, which
/// writes a new `whisper_model_path`) take effect on the next dictation
/// without an app restart (issue #30). The startup instance in
/// `ctx.local_transcriber` is no longer authoritative on its own; the
/// settings path is, and a mismatch swaps in a fresh transcriber (whose
/// Whisper context loads lazily on the first `transcribe` call).
pub(crate) fn app_default_transcriber(ctx: &Arc<AppContext>) -> Arc<LocalTranscriber> {
    let wanted_path = {
        let settings = ctx.settings.read();
        resolve_default_model_path(&settings, &ctx.model_dir)
    };

    {
        let current = ctx.local_transcriber.read();
        if current.model_path() == wanted_path {
            return current.clone();
        }
    }

    // Path changed since the stored transcriber was built: rebuild and
    // swap so all subsequent passes use the new model. The double-check
    // under the write lock guards against a racing concurrent rebuild.
    let vad_model_path = Some(ctx.model_dir.join("ggml-silero-v6.2.0.bin"));
    let mut current = ctx.local_transcriber.write();
    if current.model_path() != wanted_path {
        tracing::info!(
            model_path = %wanted_path.display(),
            "App-default LocalTranscriber rebuilt for changed Whisper slot/path"
        );
        *current = Arc::new(LocalTranscriber::new(wanted_path, vad_model_path));
    }
    current.clone()
}

/// Resolver for the `LocalTranscriber` that a mode should use.
///
/// - No `mode.whisper_model_slot` set → app-default transcriber via
///   `app_default_transcriber` (Whisper model from the settings; rebuilt
///   on a slot/path change without a restart, issue #30).
/// - Slot identical to the global default slot → also the app-default
///   transcriber, so we don't hold a second model in RAM in parallel.
/// - Otherwise: cache lookup in `ctx.extra_transcribers`; on a cache
///   miss a new `LocalTranscriber` is constructed for the slot (the
///   model file is only loaded into the Whisper context on the first
///   `transcribe` call — the resolver only allocates metadata).
fn resolve_local_transcriber(ctx: &Arc<AppContext>, mode: &Mode) -> Arc<LocalTranscriber> {
    let Some(slot_slug) = mode.whisper_model_slot.as_ref() else {
        return app_default_transcriber(ctx);
    };
    let default_slot = ctx.settings.read().whisper_default_slot.clone();
    if slot_slug == &default_slot {
        return app_default_transcriber(ctx);
    }

    {
        let mut cache = ctx.extra_transcribers.lock();
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
        "LocalTranscriber override created for mode"
    );
    let mut cache = ctx.extra_transcribers.lock();
    // Re-check under the lock: a concurrent resolver may have inserted this
    // slot between the read above and now. Reuse theirs so we don't hold two
    // copies of the same model.
    if let Some(found) = cache.get(slot_slug) {
        return found.clone();
    }
    cache.insert(slot_slug.clone(), new_transcriber.clone());
    new_transcriber
}

/// Resolver for the cloud STT transcriber of a provider, with a
/// per-provider cache in `ctx.cloud_transcribers` (issue #42).
///
/// On a cache miss the wrapper is built once via `make_cloud_transcriber`
/// (which reads the provider's keychain entry) and cached; every further
/// dictation with the same provider reuses it instead of re-reading the
/// keychain and re-constructing the `Arc`. The entry is dropped when the
/// provider's key changes (`AppContext::invalidate_cloud_provider` /
/// `clear_cloud_caches`), so a stale key is never served.
fn resolve_cloud_transcriber(
    ctx: &Arc<AppContext>,
    provider: &str,
) -> Result<Arc<dyn Transcriber>> {
    {
        let cache = ctx.cloud_transcribers.lock();
        if let Some(found) = cache.get(provider) {
            return Ok(found.clone());
        }
    }

    // Build outside the lock — `make_cloud_transcriber` does a keychain
    // read, which we don't want to hold the cache mutex across.
    let new_transcriber = make_cloud_transcriber(provider, ctx.http_client.clone())?;
    let mut cache = ctx.cloud_transcribers.lock();
    // Re-check under the lock: a concurrent resolver may have inserted
    // this provider between the read above and now. Reuse theirs.
    if let Some(found) = cache.get(provider) {
        return Ok(found.clone());
    }
    cache.insert(provider.to_string(), new_transcriber.clone());
    Ok(new_transcriber)
}

/// Resolver for the cloud LLM processor of a provider, with a
/// per-provider cache in `ctx.cloud_processors` (issue #42). Mirrors
/// `resolve_cloud_transcriber`; see there for the cache/invalidation
/// contract.
fn resolve_cloud_processor(ctx: &Arc<AppContext>, provider: &str) -> Result<Arc<dyn Processor>> {
    {
        let cache = ctx.cloud_processors.lock();
        if let Some(found) = cache.get(provider) {
            return Ok(found.clone());
        }
    }

    let new_processor = make_cloud_processor(provider, ctx.http_client.clone())?;
    let mut cache = ctx.cloud_processors.lock();
    if let Some(found) = cache.get(provider) {
        return Ok(found.clone());
    }
    cache.insert(provider.to_string(), new_processor.clone());
    Ok(new_processor)
}

/// Analogous resolver for the embedded LLM processor
/// (`mode.embedded_llm_slot`). Linux/macOS-only — the embedded engine is
/// not compiled on Windows (issue #1), where `resolve_local_processor_for_mode`
/// returns a steering error instead of calling this.
#[cfg(not(target_os = "windows"))]
fn resolve_embedded_llm(ctx: &Arc<AppContext>, mode: &Mode) -> Arc<LlamaEmbeddedProcessor> {
    let Some(slot_slug) = mode.embedded_llm_slot.as_ref() else {
        return ctx.local_llm_processor.clone();
    };
    let default_slot = ctx.settings.read().llm_default_slot.clone();
    if slot_slug == &default_slot {
        return ctx.local_llm_processor.clone();
    }

    {
        let mut cache = ctx.extra_llm_processors.lock();
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
        "LlamaEmbeddedProcessor override created for mode"
    );
    let mut cache = ctx.extra_llm_processors.lock();
    // Re-check under the lock: a concurrent resolver may have inserted this
    // slot between the read above and now. Reuse theirs so we don't hold two
    // copies of the same model.
    if let Some(found) = cache.get(slot_slug) {
        return found.clone();
    }
    cache.insert(slot_slug.clone(), new_processor.clone());
    new_processor
}

fn resolve_cloud_processor_for_mode(
    ctx: &Arc<AppContext>,
    mode: &Mode,
) -> Result<Arc<dyn Processor>> {
    let provider = mode.cloud_llm_provider.as_deref().ok_or_else(|| {
        VoiceTypeError::Mode(format!(
            "Mode '{}': processing=cloud, but no cloud_llm_provider set",
            mode.id
        ))
    })?;
    resolve_cloud_processor(ctx, provider)
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
                    .title("VoiceTypeX — Error")
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
        description: "VoiceTypeX: open mode menu / stop recording".to_string(),
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

/// Spawns a listener that drives the overlay window's visibility from the
/// pipeline state: it **shows** the overlay on `Error` (so the failure is
/// visible — this also covers the inject-failure path, where the overlay
/// was already hidden right before the libei inject) and **hides** it on
/// `Idle`. Making it visible during recording is done explicitly in
/// `start_recording` (see above), not here — otherwise the window could
/// briefly pop up again when already hidden, because a state event reports
/// Recording once more. The happy/empty paths hide the overlay themselves
/// before reaching `Idle`; for the error path this listener is the sole
/// show/hide driver.
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
            if let Some(overlay) = app.get_webview_window("overlay") {
                match state {
                    AppState::Error(_) => {
                        let _ = overlay.show();
                    }
                    AppState::Idle => {
                        let _ = overlay.hide();
                    }
                    _ => {}
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
            Err(VoiceTypeError::transcription("mock STT failure"))
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
                    paste_with_shift: false,
                },
            )
            .await
            .inspect_err(|e| {
                let _ = state_bus.transition(AppState::Error(e.to_string()));
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

    /// Error surfacing: a transcriber failure must leave the bus parked in
    /// `Error(_)` — NOT snap back to `Idle`. The old immediate `→ Idle`
    /// coalesced in the `watch` channel so the `app://state` emitter never
    /// saw the error frame and the overlay showed nothing. Parking in
    /// `Error` is what makes the error visible; recovery happens later via
    /// the menu hotkey (see
    /// `menu_hotkey_from_error_clears_to_idle_and_opens_menu`).
    #[tokio::test]
    async fn pipeline_transcriber_error_parks_in_error_state() {
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

        assert!(matches!(err, VoiceTypeError::Transcription { .. }));
        assert!(
            matches!(bus.current(), AppState::Error(_)),
            "STT failure must park in Error, not snap back to Idle"
        );
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

    /// Recovery semantics: `handle_menu_hotkey` treats `Error` like `Idle`
    /// — from `Error` it clears the error (`Error → Idle`) and opens the
    /// menu, so a parked pipeline error never traps the app. Mirrors the
    /// branch selection from `handle_menu_hotkey` on the `StateBus`.
    #[tokio::test]
    async fn menu_hotkey_from_error_clears_to_idle_and_opens_menu() {
        let bus = StateBus::new();
        bus.transition(AppState::Error("boom".into())).unwrap();
        assert!(matches!(bus.current(), AppState::Error(_)));

        // Decision from `handle_menu_hotkey`: Idle | Error → open menu.
        let current = bus.current();
        let opens_menu = matches!(current, AppState::Idle | AppState::Error(_));
        assert!(
            opens_menu,
            "Error must be a recoverable, menu-openable state"
        );

        // From Error the handler first clears to Idle (legal transition).
        if matches!(current, AppState::Error(_)) {
            bus.transition(AppState::Idle).unwrap();
        }
        assert_eq!(bus.current(), AppState::Idle);

        // And a fresh recording can start again afterwards.
        bus.transition(AppState::Recording).unwrap();
        assert_eq!(bus.current(), AppState::Recording);
    }

    // --- Issue #30: app-default transcriber staleness trigger ---
    //
    // `app_default_transcriber` rebuilds the global transcriber when the
    // settings-resolved model path no longer matches the stored one.
    // `resolve_default_model_path` is the function that decides that path,
    // so it is the load-bearing comparison: if a slot change here did not
    // change the path, no rebuild would ever fire (the original bug).

    use crate::core::config::Settings;

    #[test]
    fn slot_change_changes_resolved_default_model_path() {
        let model_dir = std::path::Path::new("/models");
        let before = Settings {
            whisper_model_path: None,
            whisper_default_slot: "large-v3-turbo-q5_0".into(),
            ..Settings::default()
        };
        let after = Settings {
            whisper_default_slot: "small-q5_1".into(),
            ..before.clone()
        };

        let path_before = resolve_default_model_path(&before, model_dir);
        let path_after = resolve_default_model_path(&after, model_dir);

        assert_ne!(
            path_before, path_after,
            "a whisper_default_slot change must change the resolved path so the rebuild fires"
        );
        assert_eq!(
            path_after,
            model_dir.join(ModelSlot::from_setting("small-q5_1").filename())
        );
    }

    #[test]
    fn explicit_model_path_override_wins_and_is_stable_across_slots() {
        let model_dir = std::path::Path::new("/models");
        let s = Settings {
            whisper_model_path: Some("/custom/my-model.bin".into()),
            whisper_default_slot: "large-v3-turbo-q5_0".into(),
            ..Settings::default()
        };
        assert_eq!(
            resolve_default_model_path(&s, model_dir),
            std::path::PathBuf::from("/custom/my-model.bin"),
            "an explicit whisper_model_path must win over the slot default"
        );

        // Same override, different slot → still the override (no spurious
        // rebuild just because the slot string moved).
        let s2 = Settings {
            whisper_default_slot: "small-q5_1".into(),
            ..s.clone()
        };
        assert_eq!(
            resolve_default_model_path(&s, model_dir),
            resolve_default_model_path(&s2, model_dir)
        );
    }
}
