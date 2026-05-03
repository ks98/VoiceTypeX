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
use crate::transcription::{TranscribeOpts, Transcriber, TranscriptionMode};
use async_trait::async_trait;
use parking_lot::RwLock;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

const SUPPORTED: &[TranscriptionMode] = &[TranscriptionMode::OneShot];

pub struct LocalTranscriber {
    model_path: PathBuf,
    context: Arc<RwLock<Option<WhisperContext>>>,
}

impl LocalTranscriber {
    pub fn new(model_path: PathBuf) -> Self {
        Self {
            model_path,
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

    fn supports(&self) -> &'static [TranscriptionMode] {
        SUPPORTED
    }

    async fn transcribe_oneshot(&self, audio: &[u8], opts: TranscribeOpts) -> Result<String> {
        let samples = decode_wav_to_f32_mono_16k(audio)?;
        let ctx = Arc::clone(&self.context);
        let model_path = self.model_path.clone();

        // ensure_loaded auf dem aktuellen Thread vor dem spawn_blocking.
        self.ensure_loaded()?;

        let language = opts.language.clone();
        let initial_prompt = opts.initial_prompt.clone();

        tokio::task::spawn_blocking(move || -> Result<String> {
            run_whisper_blocking(&ctx, &model_path, &samples, language, initial_prompt)
        })
        .await
        .map_err(|e| VoiceTypeError::Transcription(format!("spawn_blocking: {e}")))?
    }
}

fn run_whisper_blocking(
    ctx: &Arc<RwLock<Option<WhisperContext>>>,
    model_path: &Path,
    samples: &[f32],
    language: Option<String>,
    initial_prompt: Option<String>,
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

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    // n_threads explizit setzen — whisper-rs Default ist 4, zu wenig auf
    // modernen 8+ Core-CPUs. available_parallelism() liefert logical cores;
    // ueber 8 lohnt sich kaum (Memory-Bandwidth-Limit). Konservativ cappen.
    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(8);
    params.set_n_threads(n_threads as i32);
    tracing::info!(n_threads, "Whisper n_threads gesetzt");

    if let Some(lang) = language.as_deref() {
        params.set_language(Some(lang));
    }
    if let Some(prompt) = initial_prompt.as_deref() {
        params.set_initial_prompt(prompt);
    }

    state
        .full(params, samples)
        .map_err(|e| VoiceTypeError::Transcription(format!("whisper full: {e}")))?;

    let n_segments = state
        .full_n_segments()
        .map_err(|e| VoiceTypeError::Transcription(format!("n_segments: {e}")))?;
    let mut text = String::new();
    for i in 0..n_segments {
        let segment = state
            .full_get_segment_text(i)
            .map_err(|e| VoiceTypeError::Transcription(format!("segment {i}: {e}")))?;
        text.push_str(&segment);
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
