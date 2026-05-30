// SPDX-License-Identifier: GPL-3.0-or-later
//! Local STT via whisper.cpp (whisper-rs bindings).
//!
//! Architecture:
//! - `WhisperContext` is expensive to create (load model file,
//!   decompress quantization into RAM). We load once per model path and
//!   cache behind `Arc<RwLock<Option<WhisperContext>>>`.
//! - whisper-rs is not async; we wrap the call in
//!   `tokio::task::spawn_blocking`, because transcription needs several
//!   seconds of CPU and we don't want to block the tokio runtime.
//! - Input is 16 kHz mono f32 (Whisper convention). We take WAV in,
//!   decode with hound, convert to f32 [-1, 1].

use crate::core::error::{Result, VoiceTypeError};
use crate::transcription::{TranscribeOpts, Transcriber};
use async_trait::async_trait;
use parking_lot::RwLock;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperVadParams,
};

/// Which sampling and latency characteristic the current pass should
/// have. Controls: sampling strategy (beam vs. greedy), `audio_ctx`
/// trick, log verbosity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeProfile {
    /// Final pass after the stop hotkey — quality first. BeamSearch
    /// (size=5), no `audio_ctx` trick. ~3x slower than streaming, but
    /// the definitive output.
    Final,
    /// Live pass during recording — latency first. Greedy sampling,
    /// dynamic `audio_ctx` (shortens the mel encoder on short audio).
    /// Called by the streaming worker every ~800 ms.
    Streaming,
}

/// The Whisper encoder always processes a 30 s mel spec with 1500
/// frames. For shorter audio we can reduce `audio_ctx` — Whisper then
/// effectively processes only the relevant frames.
///
/// Aggressive defaults for CPU-only hardware: lower bound 256 frames
/// (5 s context). On CPU+BLAS the encoder dominates latency; a smaller
/// `audio_ctx` halves or thirds the decode time. From 25 s audio we
/// return `None` so nothing is truncated.
///
/// 50 frames/sec + 64 frames padding for Whisper's internal token-buffer
/// reserve.
fn dynamic_audio_ctx_frames(samples_len_16k: usize) -> Option<i32> {
    let frames_estimate = ((samples_len_16k as f64 / 16_000.0) * 50.0).ceil() as i32 + 64;
    if frames_estimate >= 1250 {
        None
    } else {
        Some(frames_estimate.max(256))
    }
}

pub struct LocalTranscriber {
    model_path: PathBuf,
    /// Optional path to the Silero VAD model. If set AND the file
    /// exists, `run_whisper_blocking` activates the whisper.cpp
    /// built-in VAD path. If the file is missing (e.g. because the
    /// download has not run yet), Whisper runs without VAD as before
    /// and logs a one-time warning.
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

    /// Load the model if not already done. Idempotent.
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
            .ok_or_else(|| VoiceTypeError::Transcription("Model path not UTF-8".into()))?;
        if !self.model_path.exists() {
            return Err(VoiceTypeError::Transcription(format!(
                "Model file missing: {path_str} (see the model downloader)"
            )));
        }
        let ctx = WhisperContext::new_with_params(path_str, WhisperContextParameters::default())
            .map_err(|e| VoiceTypeError::Transcription(format!("WhisperContext: {e}")))?;
        *guard = Some(ctx);
        tracing::info!(model = %path_str, "Whisper model loaded");
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

        // ensure_loaded on the current thread before spawn_blocking.
        self.ensure_loaded()?;

        let language = opts.language.clone();
        let initial_prompt = opts.initial_prompt.clone();
        let n_threads_override = opts.n_threads;
        let beam_size_override = opts.beam_size;

        tokio::task::spawn_blocking(move || -> Result<String> {
            run_whisper_blocking(
                &ctx,
                &model_path,
                vad_model_path.as_deref(),
                &samples,
                language,
                initial_prompt,
                n_threads_override,
                beam_size_override,
                DecodeProfile::Final,
            )
        })
        .await
        .map_err(|e| VoiceTypeError::Transcription(format!("spawn_blocking: {e}")))?
    }
}

impl LocalTranscriber {
    /// Streaming pass: takes already converted 16 kHz mono f32 samples,
    /// runs with greedy + dynamic `audio_ctx`, returns the current
    /// decode as a string. Idempotent — the `WhisperContext` stays warm
    /// between calls; only the `WhisperState` (decoder buffer) is
    /// recreated per pass. VAD stays active.
    ///
    /// Called by the streaming worker (`pipeline/`) every ~800 ms
    /// during recording. The result goes through LocalAgreement-2;
    /// only the stable prefix is emitted to the overlay.
    pub async fn transcribe_streaming_pass(
        &self,
        samples: Vec<f32>,
        opts: TranscribeOpts,
    ) -> Result<String> {
        let ctx = Arc::clone(&self.context);
        let model_path = self.model_path.clone();
        let vad_model_path = self.vad_model_path.clone();

        self.ensure_loaded()?;

        let language = opts.language.clone();
        let initial_prompt = opts.initial_prompt.clone();
        let n_threads_override = opts.n_threads;
        // Streaming pass is greedy regardless of beam_size — pass it
        // through for a uniform call, the Streaming arm ignores it.
        let beam_size_override = opts.beam_size;

        tokio::task::spawn_blocking(move || -> Result<String> {
            run_whisper_blocking(
                &ctx,
                &model_path,
                vad_model_path.as_deref(),
                &samples,
                language,
                initial_prompt,
                n_threads_override,
                beam_size_override,
                DecodeProfile::Streaming,
            )
        })
        .await
        .map_err(|e| VoiceTypeError::Transcription(format!("spawn_blocking: {e}")))?
    }
}

// 8 arguments are acceptable here because they encapsulate different
// concerns (context, audio, language, threads, profile). A config
// struct would just be "8 arguments in different packaging" — the
// helper is called from only two places and is private.
#[allow(clippy::too_many_arguments)]
fn run_whisper_blocking(
    ctx: &Arc<RwLock<Option<WhisperContext>>>,
    model_path: &Path,
    vad_model_path: Option<&Path>,
    samples: &[f32],
    language: Option<String>,
    initial_prompt: Option<String>,
    n_threads_override: Option<u32>,
    beam_size_override: Option<u32>,
    profile: DecodeProfile,
) -> Result<String> {
    let guard = ctx.read();
    let context = guard.as_ref().ok_or_else(|| {
        VoiceTypeError::Transcription(format!(
            "Whisper context not loaded ({})",
            model_path.display()
        ))
    })?;

    let mut state = context
        .create_state()
        .map_err(|e| VoiceTypeError::Transcription(format!("create_state: {e}")))?;

    // Sampling choice depends on the profile:
    // - Final: BeamSearch (patience=1.0) — ~2-3 % WER improvement over
    //   greedy at ~beam× the decode cost. The beam width is
    //   configurable (Settings.whisper_beam_size + per-mode override);
    //   default 5, clamped to 1..=10 (1 ≈ greedy).
    // - Streaming: greedy with best_of=1 — fastest variant, because
    //   partials are overwritten anyway as soon as the next pass
    //   delivers a stable prefix. Quality is caught up in the final
    //   pass. `beam_size_override` is ignored here.
    let sampling = match profile {
        DecodeProfile::Final => SamplingStrategy::BeamSearch {
            beam_size: beam_size_override.unwrap_or(5).clamp(1, 10) as i32,
            patience: 1.0,
        },
        DecodeProfile::Streaming => SamplingStrategy::Greedy { best_of: 1 },
    };
    let mut params = FullParams::new(sampling);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    // `audio_ctx` trick only in the streaming profile: on short audio
    // (<30 s) this shortens the mel encoder, ~2x speedup. On long
    // audio it's a no-op (None) so nothing is truncated.
    if profile == DecodeProfile::Streaming {
        if let Some(ctx_frames) = dynamic_audio_ctx_frames(samples.len()) {
            params.set_audio_ctx(ctx_frames);
            tracing::debug!(audio_ctx = ctx_frames, "streaming pass audio_ctx set");
        }
    }

    // Quality hardening:
    // - suppress_blank: no "empty" tokens at the start of a segment.
    //   Important especially for streaming, harmless in oneshot.
    // - no_speech_thold 0.6 **final only**: segments with no_speech
    //   prob > 0.6 are dropped (prevents silence hallucinations,
    //   additive to VAD). In streaming this would skip too many
    //   uncertain partials, the "single timestamp ending - skip
    //   entire chunk" trap — in streaming we want to see uncertain
    //   outputs TOO, because they get overwritten later anyway.
    // - temperature 0.0 fixed + temperature_inc 0.2 as fallback: if
    //   logprob_thold (default -1.0) trips, Whisper ramps temperature
    //   in 0.2 steps and retries. In streaming without
    //   temperature_inc, because fallback retries double the pass.
    params.set_suppress_blank(true);
    params.set_temperature(0.0);
    match profile {
        DecodeProfile::Final => {
            params.set_no_speech_thold(0.6);
            params.set_temperature_inc(0.2);
        }
        DecodeProfile::Streaming => {
            // no_speech_thold default (1.0 = never skipped), no temp_inc
            params.set_temperature_inc(0.0);
        }
    }

    // n_threads: user override from settings takes precedence.
    // Otherwise auto-detect via `available_parallelism` (logical cores),
    // capped at 8 due to memory-bandwidth diminishing returns.
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
        "Whisper n_threads set"
    );

    if let Some(lang) = language.as_deref() {
        params.set_language(Some(lang));
    }
    if let Some(prompt) = initial_prompt.as_deref() {
        params.set_initial_prompt(prompt);
    }

    // VAD activation (whisper.cpp Silero path). Defaults deliberately
    // more conservative than upstream:
    // - min_silence_duration 500 ms (vs. 100 ms): no mid-sentence cuts
    //   on dictation with short breath pauses.
    // - speech_pad 200 ms (vs. 30 ms): buffer is enough for hard
    //   consonant onsets ("k", "t", "p") that would otherwise be
    //   clipped.
    // If the VAD model file is missing (e.g. because the user has not
    // triggered the download yet), the path silently falls back to
    // "no VAD" — only a WARN log, not an error.
    let vad_path_str: Option<&str> = vad_model_path.and_then(|p| {
        if p.exists() {
            p.to_str()
        } else {
            tracing::warn!(
                vad_model = %p.display(),
                "VAD model file missing — running without VAD"
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
        tracing::debug!(vad_model = vad_path_str, "Silero-VAD active");
    }

    state
        .full(params, samples)
        .map_err(|e| VoiceTypeError::Transcription(format!("whisper full: {e}")))?;

    // whisper-rs 0.16: `full_n_segments` returns i32 directly (no
    // Result); `get_segment(i)` returns `Option<WhisperSegment>` with
    // its own text accessors. `to_str_lossy` replaces invalid UTF-8
    // bytes with U+FFFD instead of crashing — relevant because Whisper
    // output occasionally cuts multi-byte sequences at segment
    // boundaries.
    let n_segments = state.full_n_segments();
    let mut text = String::new();
    for i in 0..n_segments {
        let segment = state.get_segment(i).ok_or_else(|| {
            VoiceTypeError::Transcription(format!("get_segment({i}) returned None"))
        })?;
        let segment_text = segment
            .to_str_lossy()
            .map_err(|e| VoiceTypeError::Transcription(format!("segment {i} to_str: {e}")))?;
        text.push_str(&segment_text);
    }
    Ok(text.trim().to_string())
}

/// Read WAV bytes → 16 kHz mono f32 samples. Also accepts other sample
/// formats (i16, i32, f32) and converts; but rejects wrong sample rate
/// or channel count (the recorder should already have set those).
fn decode_wav_to_f32_mono_16k(wav_bytes: &[u8]) -> Result<Vec<f32>> {
    let cursor = std::io::Cursor::new(wav_bytes);
    let reader = hound::WavReader::new(cursor)
        .map_err(|e| VoiceTypeError::Transcription(format!("WavReader: {e}")))?;
    let spec = reader.spec();
    if spec.sample_rate != 16_000 {
        return Err(VoiceTypeError::Transcription(format!(
            "Expected 16 kHz, got {} Hz",
            spec.sample_rate
        )));
    }
    if spec.channels != 1 {
        return Err(VoiceTypeError::Transcription(format!(
            "Expected mono, got {} channels",
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
