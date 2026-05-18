// SPDX-License-Identifier: GPL-3.0-or-later
//! Lokales STT via whisper.cpp (whisper-rs Bindings).
//!
//! Architektur:
//! - `WhisperContext` ist teuer zu erstellen (Modell-Datei laden, Quantisierung
//!   in RAM dekomprimieren). Wir laden einmal pro Modellpfad und cachen ihn
//!   hinter `Arc<RwLock<Option<WhisperContext>>>`.
//! - whisper-rs ist nicht async; wir umhuellen den Aufruf mit
//!   `tokio::task::spawn_blocking`, weil Transkription mehrere Sekunden CPU
//!   braucht und wir die tokio-Runtime nicht blockieren wollen.
//! - Eingabe ist 16 kHz Mono f32 (Whisper-Konvention). Wir nehmen WAV rein,
//!   dekodieren mit hound, konvertieren zu f32 [-1, 1].

use crate::core::error::{Result, VoiceTypeError};
use crate::transcription::{TranscribeOpts, Transcriber};
use async_trait::async_trait;
use parking_lot::RwLock;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperVadParams,
};

pub struct LocalTranscriber {
    model_path: PathBuf,
    /// Optionaler Pfad auf das Silero-VAD-Modell. Wenn gesetzt UND die Datei
    /// existiert, aktiviert `run_whisper_blocking` den whisper.cpp-Built-in-
    /// VAD-Pfad. Fehlt die Datei (z.B. weil der Download noch nicht lief),
    /// laeuft Whisper wie bisher ohne VAD und loggt einmalig eine Warnung.
    vad_model_path: Option<PathBuf>,
    context: Arc<RwLock<Option<WhisperContext>>>,
}

impl LocalTranscriber {
    pub fn new(model_path: PathBuf, vad_model_path: Option<PathBuf>) -> Self {
        Self {
            model_path,
            vad_model_path,
            context: Arc::new(RwLock::new(None)),
        }
    }

    /// Lade das Modell, falls noch nicht geschehen. Idempotent.
    fn ensure_loaded(&self) -> Result<()> {
        if self.context.read().is_some() {
            return Ok(());
        }
        let mut guard = self.context.write();
        if guard.is_some() {
            return Ok(());
        }
        let path_str = self
            .model_path
            .to_str()
            .ok_or_else(|| VoiceTypeError::Transcription("Modellpfad nicht UTF-8".into()))?;
        if !self.model_path.exists() {
            return Err(VoiceTypeError::Transcription(format!(
                "Modell-Datei fehlt: {path_str} (siehe Modell-Downloader)"
            )));
        }
        let ctx = WhisperContext::new_with_params(path_str, WhisperContextParameters::default())
            .map_err(|e| VoiceTypeError::Transcription(format!("WhisperContext: {e}")))?;
        *guard = Some(ctx);
        tracing::info!(model = %path_str, "Whisper-Modell geladen");
        Ok(())
    }
}

#[async_trait]
impl Transcriber for LocalTranscriber {
    fn name(&self) -> &str {
        "local-whisper"
    }

    async fn transcribe_oneshot(&self, audio: &[u8], opts: TranscribeOpts) -> Result<String> {
        let samples = decode_wav_to_f32_mono_16k(audio)?;
        let ctx = Arc::clone(&self.context);
        let model_path = self.model_path.clone();
        let vad_model_path = self.vad_model_path.clone();

        // ensure_loaded auf dem aktuellen Thread vor dem spawn_blocking.
        self.ensure_loaded()?;

        let language = opts.language.clone();
        let initial_prompt = opts.initial_prompt.clone();
        let n_threads_override = opts.n_threads;

        tokio::task::spawn_blocking(move || -> Result<String> {
            run_whisper_blocking(
                &ctx,
                &model_path,
                vad_model_path.as_deref(),
                &samples,
                language,
                initial_prompt,
                n_threads_override,
            )
        })
        .await
        .map_err(|e| VoiceTypeError::Transcription(format!("spawn_blocking: {e}")))?
    }
}

fn run_whisper_blocking(
    ctx: &Arc<RwLock<Option<WhisperContext>>>,
    model_path: &Path,
    vad_model_path: Option<&Path>,
    samples: &[f32],
    language: Option<String>,
    initial_prompt: Option<String>,
    n_threads_override: Option<u32>,
) -> Result<String> {
    let guard = ctx.read();
    let context = guard.as_ref().ok_or_else(|| {
        VoiceTypeError::Transcription(format!(
            "Whisper-Context nicht geladen ({})",
            model_path.display()
        ))
    })?;

    let mut state = context
        .create_state()
        .map_err(|e| VoiceTypeError::Transcription(format!("create_state: {e}")))?;

    // BeamSearch statt Greedy: ~2-3 % WER-Verbesserung auf deutschem
    // Mehr-Satz-Diktat, ~3x langsamer pro Decode-Step. Phase 2 (Streaming)
    // kompensiert das durch Overlap; im Oneshot-Pfad ist der Latenz-Hit
    // kleiner als der Quality-Gewinn rechtfertigt. patience=1.0 ist der
    // OpenAI-Original-Default.
    let mut params = FullParams::new(SamplingStrategy::BeamSearch {
        beam_size: 5,
        patience: 1.0,
    });
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    // Quality-Hardening:
    // - suppress_blank: keine "leeren" Tokens am Segment-Anfang. Wichtig
    //   vor allem fuer Streaming (Phase 2), schadet im Oneshot nicht.
    // - no_speech_thold 0.6: Segmente mit no_speech-Prob > 0.6 fallen
    //   weg (verhindert Stille-Halluzinationen, additiv zu VAD).
    // - temperature 0.0 fix + temperature_inc 0.2 als Fallback: wenn
    //   logprob_thold (Default -1.0) reisst, dreht Whisper temperature
    //   in 0.2er Schritten hoch und versucht erneut — gibt
    //   deterministische Outputs auf einfachem Audio, retten auf
    //   schwierigem.
    params.set_suppress_blank(true);
    params.set_no_speech_thold(0.6);
    params.set_temperature(0.0);
    params.set_temperature_inc(0.2);

    // n_threads: User-Override aus Settings hat Vorrang. Sonst Auto-Detect
    // via available_parallelism (logical cores), gedeckelt bei 8 wegen
    // Memory-Bandwidth diminishing returns.
    let n_threads = n_threads_override.map(|n| n as usize).unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(8)
    });
    params.set_n_threads(n_threads as i32);
    tracing::info!(
        n_threads,
        from_setting = n_threads_override.is_some(),
        "Whisper n_threads gesetzt"
    );

    if let Some(lang) = language.as_deref() {
        params.set_language(Some(lang));
    }
    if let Some(prompt) = initial_prompt.as_deref() {
        params.set_initial_prompt(prompt);
    }

    // VAD-Aktivierung (whisper.cpp Silero-Pfad). Defaults bewusst
    // konservativer als upstream:
    // - min_silence_duration 500 ms (vs. 100 ms): keine Mid-Sentence-Cuts
    //   bei Diktaten mit kurzen Atempausen.
    // - speech_pad 200 ms (vs. 30 ms): Puffer reicht fuer harte
    //   Konsonanten-Onsets ("k", "t", "p"), die sonst geklippt wuerden.
    // Wenn das VAD-Modell-File fehlt (z.B. weil der User noch keinen
    // Download getriggert hat), faellt der Pfad lautlos auf "ohne VAD"
    // zurueck — nur ein WARN-Log, kein Fehler.
    let vad_path_str: Option<&str> = vad_model_path.and_then(|p| {
        if p.exists() {
            p.to_str()
        } else {
            tracing::warn!(
                vad_model = %p.display(),
                "VAD-Modell-Datei fehlt — laufe ohne VAD"
            );
            None
        }
    });
    if let Some(vad_path_str) = vad_path_str {
        let mut vad_params = WhisperVadParams::default();
        vad_params.set_min_silence_duration(500);
        vad_params.set_speech_pad(200);
        params.set_vad_params(vad_params);
        params.set_vad_model_path(Some(vad_path_str));
        params.enable_vad(true);
        tracing::debug!(vad_model = vad_path_str, "Silero-VAD aktiv");
    }

    state
        .full(params, samples)
        .map_err(|e| VoiceTypeError::Transcription(format!("whisper full: {e}")))?;

    // whisper-rs 0.16: full_n_segments gibt direkt i32 zurueck (kein Result);
    // get_segment(i) gibt Option<WhisperSegment> mit eigenen Text-Accessoren.
    // to_str_lossy ersetzt ungueltige UTF-8-Bytes durch U+FFFD statt zu
    // crashen — relevant, weil Whisper-Output gelegentlich Multi-Byte-
    // Sequenzen an Segment-Grenzen zerschneidet.
    let n_segments = state.full_n_segments();
    let mut text = String::new();
    for i in 0..n_segments {
        let segment = state.get_segment(i).ok_or_else(|| {
            VoiceTypeError::Transcription(format!("get_segment({i}) lieferte None"))
        })?;
        let segment_text = segment
            .to_str_lossy()
            .map_err(|e| VoiceTypeError::Transcription(format!("segment {i} to_str: {e}")))?;
        text.push_str(&segment_text);
    }
    Ok(text.trim().to_string())
}

/// Lese WAV-Bytes → 16 kHz Mono f32-Samples. Akzeptiert auch andere
/// Sample-Formate (i16, i32, f32) und konvertiert; lehnt aber falsche
/// Sample-Rate oder Channel-Anzahl ab (das soll der Recorder bereits
/// vorgegeben haben).
fn decode_wav_to_f32_mono_16k(wav_bytes: &[u8]) -> Result<Vec<f32>> {
    let cursor = std::io::Cursor::new(wav_bytes);
    let reader = hound::WavReader::new(cursor)
        .map_err(|e| VoiceTypeError::Transcription(format!("WavReader: {e}")))?;
    let spec = reader.spec();
    if spec.sample_rate != 16_000 {
        return Err(VoiceTypeError::Transcription(format!(
            "Erwarte 16 kHz, bekam {} Hz",
            spec.sample_rate
        )));
    }
    if spec.channels != 1 {
        return Err(VoiceTypeError::Transcription(format!(
            "Erwarte Mono, bekam {} Channels",
            spec.channels
        )));
    }

    match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Int, 16) => {
            let mut reader = hound::WavReader::new(std::io::Cursor::new(wav_bytes))
                .map_err(|e| VoiceTypeError::Transcription(format!("WavReader: {e}")))?;
            let scale = i16::MAX as f32;
            reader
                .samples::<i16>()
                .map(|r| r.map(|s| s as f32 / scale))
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| VoiceTypeError::Transcription(format!("samples<i16>: {e}")))
        }
        (hound::SampleFormat::Float, 32) => {
            let mut reader = hound::WavReader::new(std::io::Cursor::new(wav_bytes))
                .map_err(|e| VoiceTypeError::Transcription(format!("WavReader: {e}")))?;
            reader
                .samples::<f32>()
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| VoiceTypeError::Transcription(format!("samples<f32>: {e}")))
        }
        other => Err(VoiceTypeError::Transcription(format!(
            "Unsupported WAV-Format: {other:?}"
        ))),
    }
}
