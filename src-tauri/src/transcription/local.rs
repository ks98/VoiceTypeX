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

/// Welche Sampling- und Latenz-Charakteristik der aktuelle Pass haben soll.
/// Steuert: Sampling-Strategie (Beam vs. Greedy), audio_ctx-Trick,
/// Log-Verbosity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeProfile {
    /// Finaler Pass nach Stop-Hotkey — Quality first. BeamSearch (size=5),
    /// kein audio_ctx-Trick. ~3x langsamer als Streaming, aber definitiver
    /// Output.
    Final,
    /// Live-Pass waehrend der Aufnahme — Latenz first. Greedy-Sampling,
    /// dynamischer audio_ctx (kuerzt den Mel-Encoder bei kurzem Audio).
    /// Wird vom Streaming-Worker alle ~800 ms aufgerufen.
    Streaming,
}

/// Whisper-Encoder verarbeitet immer eine 30-s-Mel-Spec mit 1500 Frames.
/// Bei kuerzerem Audio koennen wir `audio_ctx` reduzieren — Whisper
/// processiert dann effektiv nur die relevanten Frames.
///
/// Aggressive Defaults fuer CPU-only-Hardware: Untergrenze 256 Frames
/// (5 s Kontext). Auf CPU+BLAS dominiert der Encoder die Latenz; ein
/// kleinerer audio_ctx halbiert oder drittelt die Decode-Zeit. Ab 25 s
/// Audio gibt's `None` zurueck, damit nichts abgeschnitten wird.
///
/// 50 Frames/Sek. + 64 Frames Padding fuer Whisper's interne
/// Token-Buffer-Reserve.
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
                DecodeProfile::Final,
            )
        })
        .await
        .map_err(|e| VoiceTypeError::Transcription(format!("spawn_blocking: {e}")))?
    }
}

impl LocalTranscriber {
    /// Streaming-Pass: nimmt bereits konvertierte 16-kHz-Mono-f32-Samples,
    /// laeuft mit Greedy + dynamischem audio_ctx, gibt den aktuellen Decode
    /// als String zurueck. Idempotent — der WhisperContext bleibt zwischen
    /// Aufrufen warm; nur der WhisperState (Decoder-Buffer) wird pro Pass
    /// neu erstellt. VAD bleibt aktiv.
    ///
    /// Wird vom Streaming-Worker (`pipeline/`) alle ~800 ms waehrend der
    /// Aufnahme aufgerufen. Das Ergebnis geht durch LocalAgreement-2,
    /// nur der stabile Prefix wird ins Overlay emittiert.
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

        tokio::task::spawn_blocking(move || -> Result<String> {
            run_whisper_blocking(
                &ctx,
                &model_path,
                vad_model_path.as_deref(),
                &samples,
                language,
                initial_prompt,
                n_threads_override,
                DecodeProfile::Streaming,
            )
        })
        .await
        .map_err(|e| VoiceTypeError::Transcription(format!("spawn_blocking: {e}")))?
    }
}

// 8 Argumente sind hier vertretbar, weil sie verschiedene Concerns
// kapseln (Context, Audio, Sprache, Threads, Profile). Eine Konfig-
// Struct waere nur "8 Argumente in einer anderen Verpackung" — der
// Helper wird nur von zwei Stellen gerufen und ist privat.
#[allow(clippy::too_many_arguments)]
fn run_whisper_blocking(
    ctx: &Arc<RwLock<Option<WhisperContext>>>,
    model_path: &Path,
    vad_model_path: Option<&Path>,
    samples: &[f32],
    language: Option<String>,
    initial_prompt: Option<String>,
    n_threads_override: Option<u32>,
    profile: DecodeProfile,
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

    // Sampling-Wahl haengt am Profil:
    // - Final: BeamSearch (size=5, patience=1.0) — ~2-3 % WER-Verbesserung
    //   gegenueber Greedy, ~3x langsamer pro Decode-Step. OK fuer Oneshot.
    // - Streaming: Greedy mit best_of=1 — schnellste Variante, weil
    //   Partials sowieso ueberschrieben werden, sobald der naechste Pass
    //   stabilen Prefix liefert. Quality wird im Final-Pass eingeholt.
    let sampling = match profile {
        DecodeProfile::Final => SamplingStrategy::BeamSearch {
            beam_size: 5,
            patience: 1.0,
        },
        DecodeProfile::Streaming => SamplingStrategy::Greedy { best_of: 1 },
    };
    let mut params = FullParams::new(sampling);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    // audio_ctx-Trick nur im Streaming-Profil: bei kurzem Audio (<30 s)
    // kuerzt das den Mel-Encoder, ~2x Speedup. Bei langem Audio kein-op
    // (None), damit nichts abgeschnitten wird.
    if profile == DecodeProfile::Streaming {
        if let Some(ctx_frames) = dynamic_audio_ctx_frames(samples.len()) {
            params.set_audio_ctx(ctx_frames);
            tracing::debug!(audio_ctx = ctx_frames, "Streaming-Pass audio_ctx gesetzt");
        }
    }

    // Quality-Hardening:
    // - suppress_blank: keine "leeren" Tokens am Segment-Anfang. Wichtig
    //   vor allem fuer Streaming, schadet im Oneshot nicht.
    // - no_speech_thold 0.6 **nur Final**: Segmente mit no_speech-Prob
    //   > 0.6 fallen weg (verhindert Stille-Halluzinationen, additiv zu
    //   VAD). Im Streaming wuerden zu viele unsichere Partials geskipped,
    //   "single timestamp ending - skip entire chunk"-Falle — wir wollen
    //   im Streaming AUCH unsichere Outputs sehen, weil sie spaeter eh
    //   ueberschrieben werden.
    // - temperature 0.0 fix + temperature_inc 0.2 als Fallback: wenn
    //   logprob_thold (Default -1.0) reisst, dreht Whisper temperature
    //   in 0.2er Schritten hoch und versucht erneut. Im Streaming ohne
    //   temperature_inc, weil Fallback-Retries den Pass verdoppeln.
    params.set_suppress_blank(true);
    params.set_temperature(0.0);
    match profile {
        DecodeProfile::Final => {
            params.set_no_speech_thold(0.6);
            params.set_temperature_inc(0.2);
        }
        DecodeProfile::Streaming => {
            // no_speech_thold default (1.0 = nie skipped), kein temp_inc
            params.set_temperature_inc(0.0);
        }
    }

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
