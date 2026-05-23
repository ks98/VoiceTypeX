// SPDX-License-Identifier: GPL-3.0-or-later
//! State-Machine-Pipeline-Driver.
//!
//! Verbindet den einen globalen Menue-Hotkey mit Recorder, Transcriber,
//! Processor und Injector. Der Hotkey oeffnet im Idle-State das Modus-
//! Auswahl-Overlay; nach Enter im Frontend startet die Pipeline ueber
//! den `start_recording`-IPC-Command. Im Recording-State stoppt
//! derselbe Hotkey die Aufnahme und laesst die Pipeline durchlaufen.

use crate::audio::{play_start_cue, play_stop_cue, recorder::RecorderHandle, RecorderConfig};
use crate::core::error::{Result, VoiceTypeError};
use crate::core::modes::{Mode, ProcessingTarget, TranscriptionTarget};
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

/// Payload fuer das `app://partial-transcript`-Event. Frontend zeigt den
/// Text im Overlay an, jedes Event ersetzt den bisherigen Stand (kein
/// Append). Leerer String = "Partial loeschen" (vor/nach Streaming).
#[derive(Clone, Serialize)]
struct PartialTranscriptPayload {
    text: String,
}

/// Konfiguration des Streaming-Decode-Loops. Steht hier zentral, damit
/// die Latenz/CPU-Trade-offs an einer Stelle sichtbar sind.
///
/// Werte sind defensiv fuer CPU-only-Builds (`fast-cpu` ohne GPU-Backend)
/// gewaehlt. Auf Vulkan/CUDA-Builds koennte man INITIAL_WAIT halbieren
/// und INTERVAL auf 500 ms reduzieren — Phase-3-Thema.
const STREAMING_INITIAL_WAIT_MS: u64 = 2_000;
const STREAMING_INTERVAL_MS: u64 = 800;
/// Erste Pass startet erst, wenn 1 s Audio im Buffer ist — kuerzere Audios
/// landen oft im "single timestamp ending - skip entire chunk"-Fall von
/// whisper.cpp, was leere Outputs erzeugt.
const STREAMING_MIN_AUDIO_SAMPLES: usize = 16_000; // 1 s bei 16 kHz

/// Toggle-Logik fuer den IPC-Pfad (UI-Trigger-Button in der Modi-Liste,
/// `stop_recording`-Command).
///
/// Beim Toggle-Stop nutzen wir den im AppContext gespeicherten
/// `active_mode` statt des Parameters: sonst koennte ein UI-Trigger fuer
/// Modus B die Pipeline finalisieren, die mit Modus A vom Menue-Hotkey
/// gestartet wurde. Der Parameter-Modus ist nur fuer den Start-Pfad
/// relevant.
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

/// Handler fuer den einen globalen Menue-Hotkey.
///
/// - `Idle` → `menu`-Window zeigen, Fokus dorthin geben.
/// - `Recording` → laufende Aufnahme mit dem `active_mode` finalisieren
///   (Toggle-Stop).
/// - sonst → ignorieren (Pipeline arbeitet gerade, kein erneuter Trigger).
pub async fn handle_menu_hotkey(app: AppHandle, ctx: Arc<AppContext>) -> Result<()> {
    let current = ctx.state_bus.current();
    match current {
        AppState::Idle => {
            if let Some(menu) = app.get_webview_window("menu") {
                if let Err(e) = menu.show() {
                    tracing::warn!(error = %e, "menu.show() fehlgeschlagen");
                }
                // set_focus auf Wayland Compositor-abhaengig. Das menu-
                // Window startet mit `focus: true` in der Config, das gibt
                // KDE einen staerkeren Hint als ein nachtraegliches
                // set_focus auf das overlay-Window.
                if let Err(e) = menu.set_focus() {
                    tracing::warn!(error = %e, "menu.set_focus() fehlgeschlagen (Compositor-Quirk)");
                }
            }
            tracing::info!("Menue-Hotkey: Idle → Menue geoeffnet");
        }
        AppState::Recording => {
            let active = ctx.active_mode.lock().clone();
            let Some(mode) = active else {
                tracing::warn!("Recording-State ohne active_mode — Menue-Hotkey-Stop ignoriert");
                return Ok(());
            };
            tracing::info!(mode = %mode.id, "Menue-Hotkey: Recording → finish");
            finish_recording_and_inject(&app, &ctx, &mode).await?;
        }
        other => {
            tracing::warn!(state = %other.label(), "Menue-Hotkey ignoriert (busy)");
        }
    }
    Ok(())
}

async fn start_recording(app: &AppHandle, ctx: &Arc<AppContext>, mode: &Mode) -> Result<()> {
    ctx.state_bus.transition(AppState::Recording)?;

    // Aktiven Modus merken: der Menue-Hotkey-Stop liest hier den Modus,
    // mit dem die Pipeline finalisiert werden muss. Wird in
    // `finish_recording_and_inject` wieder geleert.
    *ctx.active_mode.lock() = Some(mode.clone());

    // Menue-Window verstecken, falls der Start aus dem Menue kam — sonst
    // bleibt es sichtbar hinter dem Overlay. Idempotent: war es schon
    // versteckt (UI-Trigger-Pfad), passiert nichts.
    if let Some(menu) = app.get_webview_window("menu") {
        if let Err(e) = menu.hide() {
            tracing::warn!(error = %e, "menu.hide() vor Recording fehlgeschlagen");
        }
    }

    // Status-Overlay sichtbar machen. Das Window hat `focus: false`,
    // klaut also keinen Tastatur-Fokus von der Ziel-App. Vor dem
    // libei-Inject (`finish_recording_and_inject`) versteckt sich das
    // Overlay wieder und der Fokus bleibt bei der Ziel-App.
    if let Some(overlay) = app.get_webview_window("overlay") {
        if let Err(e) = overlay.show() {
            tracing::warn!(error = %e, "Overlay show() fehlgeschlagen (nicht fatal)");
        }
    }

    if let Err(e) = play_start_cue().await {
        tracing::warn!(error = %e, "Start-Cue fehlgeschlagen (nicht fatal)");
    }

    let mut recorder = RecorderHandle::start(RecorderConfig::default()).inspect_err(|e| {
        // Bei Fehler State zurueck auf Idle, damit kein Deadlock entsteht.
        // active_mode auch raeumen, sonst sieht der Menue-Hotkey einen
        // veralteten Eintrag.
        *ctx.active_mode.lock() = None;
        let _ = ctx.state_bus.transition(AppState::Error(e.to_string()));
        let _ = ctx.state_bus.transition(AppState::Idle);
    })?;

    // Streaming-Worker nur bei lokalem STT spawnen. Cloud-Modi (xAI,
    // OpenAI, Groq, Deepgram) haben keine Streaming-Schnittstelle, dort
    // bleibt der One-Shot-Pfad nach Stop-Hotkey aktiv. Wir holen jetzt
    // samples_handle + meta vom Recorder, bevor er in den Slot gelegt
    // wird — danach ist er hinter einem Mutex, den wir ueber `.await`
    // hinweg nicht halten duerfen.
    if mode.transcription == TranscriptionTarget::Local {
        let samples_arc = recorder.samples_handle();
        match recorder.await_meta().await {
            Ok(meta) => {
                let app_clone = app.clone();
                let language = mode.language.clone();
                let initial_prompt = mode.initial_prompt.clone();
                let n_threads = ctx.settings.read().whisper_n_threads;
                // Mode-Override fuer Whisper-Slot bereits hier aufloesen,
                // damit der Streaming-Worker die gleiche Transcriber-
                // Instanz nutzt wie der Final-Pass nach Stop. Sonst
                // wuerde der User waehrend der Aufnahme die Default-
                // Modell-Partials sehen und nach Stop einen abweichenden
                // Final-Decode bekommen.
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
                tracing::info!("Streaming-Worker gespawnt");
            }
            Err(e) => {
                // Streaming-Worker-Start fehlgeschlagen ist nicht fatal —
                // der Final-Pass nach Stop-Hotkey laeuft weiter, der User
                // bekommt nur kein Live-Partial. WARN, kein Abort.
                tracing::warn!(error = %e, "await_meta vor Streaming-Worker fehlgeschlagen — laufe ohne Live-Partial");
            }
        }
    }

    *ctx.recorder_slot.lock() = Some(recorder);
    tracing::info!(mode = %mode.id, "Aufnahme gestartet");
    Ok(())
}

/// Streaming-Decode-Loop. Laeuft solange `State::Recording`; emittiert
/// stabile Prefixes ueber `app://partial-transcript`. Wird in
/// `finish_recording_and_inject` per `JoinHandle::abort()` beendet, bevor
/// der Final-Pass startet.
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

    // Erste Wartezeit, damit das Mikrofon ueberhaupt etwas Substanzielles
    // im Buffer hat. <1.5 s deutsches Sprach-Audio liefert sonst leere
    // Decodes oder Whisper halluziniert.
    tracing::info!("Streaming-Worker laeuft (initial_wait={STREAMING_INITIAL_WAIT_MS}ms)");
    sleep(Duration::from_millis(STREAMING_INITIAL_WAIT_MS)).await;

    let mut prev_text = String::new();
    let mut committed = String::new();
    let mut iteration: u32 = 0;

    loop {
        iteration += 1;
        // Bail wenn State nicht mehr Recording (Pipeline finalisiert).
        if !matches!(ctx.state_bus.current(), AppState::Recording) {
            tracing::info!(iteration, "Streaming-Worker: State != Recording, Exit");
            break;
        }

        // Buffer kurz locken, klonen, freigeben — CPU-Arbeit lockfrei.
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
                tracing::warn!(iteration, error = %e, "Streaming: Resampling fehlgeschlagen");
                sleep(Duration::from_millis(STREAMING_INTERVAL_MS)).await;
                continue;
            }
        };

        let opts = TranscribeOpts {
            language: language.clone(),
            initial_prompt: initial_prompt.clone(),
            n_threads,
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

                // LocalAgreement-2 zur Telemetrie behalten (zeigt wie stabil
                // die Decodes konvergieren), aber NICHT als Emit-Gate benutzen.
                // Begruendung: auf CPU-only-Hardware dauert ein Pass 8-12 s;
                // ein zweiter Pass kommt vor dem Stop-Hotkey selten durch,
                // wodurch LA-2 alle Emits blockiert. Pragmatisch emittieren
                // wir jeden Decode direkt — Text kann "wabern" wenn ein
                // spaeterer Pass den ersten revidiert, aber das ist besser
                // als gar nichts. Der Final-Pass nach Stop ueberschreibt
                // sowieso autoritativ.
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
                        tracing::warn!(error = %e, "Partial-Emit fehlgeschlagen");
                    } else {
                        tracing::info!(
                            iteration,
                            len = committed.len(),
                            preview = %committed.chars().take(40).collect::<String>(),
                            "Partial emittiert"
                        );
                    }
                }
                prev_text = curr_text;
            }
            Err(e) => {
                tracing::warn!(iteration, error = %e, "Streaming-Pass fehlgeschlagen");
            }
        }

        sleep(Duration::from_millis(STREAMING_INTERVAL_MS)).await;
    }
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

    // active_mode jetzt schon raeumen — ab hier ist die Pipeline busy und
    // der Menue-Hotkey wuerde im Recording-State sowieso nichts mehr tun
    // (State ist schon Transcribing/Postprocessing/Injecting). Wir
    // vermeiden, dass eine Pipeline-Exception den Eintrag stehen laesst.
    *ctx.active_mode.lock() = None;

    // Phase-2-Streaming-Worker abbrechen, bevor der Final-Pass laeuft.
    // abort() unterbricht den Loop am naechsten await — CPU-Arbeit
    // innerhalb spawn_blocking laeuft noch zu Ende, blockiert uns aber
    // nicht. Anschliessend Partial-Anzeige im Overlay loeschen.
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
        .ok_or_else(|| VoiceTypeError::Audio("Stop ohne aktiven Recorder".into()))?;

    ctx.state_bus.transition(AppState::Transcribing)?;

    if let Err(e) = play_stop_cue().await {
        tracing::warn!(error = %e, "Stop-Cue fehlgeschlagen (nicht fatal)");
    }

    let wav = recorder.stop_and_finalize().await.inspect_err(|e| {
        let _ = ctx.state_bus.transition(AppState::Error(e.to_string()));
        let _ = ctx.state_bus.transition(AppState::Idle);
    })?;

    // STT — lokal (ggf. Mode-Override-Slot) oder Cloud, je nach Modus.
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

    // Settings-Read hier vor dem await — der RwLockReadGuard von parking_lot
    // ist nicht Send und darf nicht ueber await-Punkte hinweg leben.
    let n_threads = ctx.settings.read().whisper_n_threads;
    let transcript = transcriber
        .transcribe_oneshot(
            &wav,
            TranscribeOpts {
                language: mode.language.clone(),
                initial_prompt: mode.initial_prompt.clone(),
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
    let system_prompt = mode.system_prompt.as_deref().unwrap_or("");
    let opts = ProcessOpts {
        temperature: mode.temperature,
        top_p: mode.top_p,
        repeat_penalty: mode.repeat_penalty,
        max_tokens: mode.max_tokens,
        ..Default::default()
    };

    // Phase 3b: Engine-Wahl pro Modus. `None` faellt auf Ollama zurueck
    // (Backward-Compat fuer Modi aus Phase 1/2). `"embedded"` aktiviert
    // den `LlamaEmbeddedProcessor` — wenn der Modus einen anderen Slot
    // verlangt als der globale Default, gibt der Resolver einen lazy
    // geladenen Override-Processor zurueck.
    let engine = mode.local_engine.as_deref().unwrap_or("ollama");
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
    // `ollama_model_tag` ist der neue Pflicht-Schluessel. `local_llm_model`
    // bleibt als deprecated Fallback fuer noch nicht migrierte TOMLs (die
    // Migration in `load_mode_from_path` kopiert das Feld bereits, aber wir
    // sind defensiv).
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

/// Resolver fuer den `LocalTranscriber`, den ein Modus benutzen soll.
///
/// - Kein `mode.whisper_model_slot` gesetzt → globaler
///   `ctx.local_transcriber` (Whisper-Modell aus den Settings).
/// - Slot identisch mit dem globalen Default-Slot → ebenfalls globaler
///   Transcriber, damit wir kein zweites Modell parallel im RAM halten.
/// - Sonst: Cache-Lookup in `ctx.extra_transcribers`; bei Cache-Miss
///   wird ein neuer `LocalTranscriber` fuer den Slot konstruiert
///   (Modell-Datei wird erst beim ersten `transcribe`-Aufruf in den
///   Whisper-Context geladen — der Resolver allokiert nur Metadaten).
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

/// Analog fuer den Embedded-LLM-Processor (`mode.embedded_llm_slot`).
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

/// Registriere den einen globalen Menue-Hotkey (X11/Windows-Pfad).
///
/// Anders als frueher gibt es **einen** Shortcut fuer die ganze App: der
/// `Settings.menu_hotkey` oeffnet das Modus-Auswahl-Menue (im Idle-State)
/// bzw. stoppt eine laufende Aufnahme (im Recording-State). Wir reagieren
/// nur auf `Pressed`; Release-Events werden ignoriert, weil PTT durch
/// den Menue-Flow obsolet ist.
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
                tracing::error!(error = %e, "Menue-Hotkey-Handler fehlgeschlagen");
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
    tracing::info!(hotkey = %accelerator, "Menue-Hotkey registriert");
    Ok(())
}

/// Wayland-Pfad: bindet den einen Menue-Hotkey ueber das
/// xdg-portal.GlobalShortcuts und spawnt einen Listener, der jede
/// Activation auf `handle_menu_hotkey` mappt.
///
/// Auf Wayland ist der Hotkey nur ein **Vorschlag** — der Compositor zeigt
/// dem User beim ersten Start einen Dialog zur finalen Zuweisung.
///
/// Zwei Tasks werden gespawnt:
/// 1) Session-Task: haelt die Portal-Verbindung am Leben + sendet Events
///    in den broadcast-Channel.
/// 2) Dispatcher-Task: liest broadcast-Channel, ruft `handle_menu_hotkey`.
///    Release-Events werden ignoriert (kein PTT mehr).
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

    // Task 1: Portal-Session
    tauri::async_runtime::spawn(async move {
        if let Err(e) =
            run_global_shortcuts_session(specs, sender_clone, Some(effective_cache)).await
        {
            tracing::error!(error = %e, "Wayland-Hotkey-Session beendet mit Fehler");
        }
    });

    // Task 2: Dispatcher — nur auf Pressed reagieren.
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
                            tracing::error!(error = %e, "Menue-Hotkey-Handler-Fehler (Wayland)");
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
