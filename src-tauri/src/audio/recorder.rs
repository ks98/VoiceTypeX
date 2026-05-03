// SPDX-License-Identifier: GPL-3.0-or-later
//! Audio-Recorder via cpal.
//!
//! Architektur (CLAUDE.md §4.1, §4.2):
//! `cpal::Stream` ist `!Send`, daher laeuft die eigentliche Aufnahme in einem
//! dedizierten OS-Thread. Der hier exportierte `RecorderHandle` ist
//! `Send + Sync` und kommuniziert per Channel mit dem Worker-Thread.
//!
//! Pipeline pro Recording:
//!   Mikrofon (native Rate, Stereo/Mono)
//!     -> cpal-Callback sammelt f32-Samples in Arc<Mutex<Vec<f32>>>
//!     -> Stop-Signal: Worker-Thread droppt den Stream, wir bekommen Samples
//!     -> Stereo->Mono Downmix (L+R)/2 wenn channels > 1
//!     -> Resampling auf 16 kHz Mono via rubato::SincFixedIn (Sprache-tauglich)
//!     -> WAV-Encoding (s16le) via hound -> Vec<u8>

use crate::core::error::{Result, VoiceTypeError};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use tokio::sync::oneshot;

/// Ziel-Samplerate fuer Whisper.
pub const WHISPER_SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, Clone, Default)]
pub struct RecorderConfig {
    /// Geraete-Name (`Device::name()`). `None` = OS-Default-Eingabegeraet.
    pub device_name: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct StreamMeta {
    sample_rate: u32,
    channels: u16,
}

/// Send-fester Handle zum Recorder-Thread.
pub struct RecorderHandle {
    samples: Arc<Mutex<Vec<f32>>>,
    is_recording: Arc<AtomicBool>,
    stop_tx: Option<oneshot::Sender<()>>,
    meta_rx: Option<oneshot::Receiver<StreamMeta>>,
    meta: Option<StreamMeta>,
    worker: Option<thread::JoinHandle<Result<()>>>,
}

impl RecorderHandle {
    /// Starte die Aufnahme. Liefert sofort zurueck — Audio sammelt sich im
    /// Hintergrund-Thread, bis `stop_and_finalize()` aufgerufen wird.
    pub fn start(config: RecorderConfig) -> Result<Self> {
        let samples: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(16_000 * 30)));
        let is_recording = Arc::new(AtomicBool::new(true));
        let (stop_tx, stop_rx) = oneshot::channel();
        let (meta_tx, meta_rx) = oneshot::channel();

        let samples_clone = Arc::clone(&samples);
        let is_recording_clone = Arc::clone(&is_recording);

        let worker = thread::Builder::new()
            .name("voicetypex-audio".into())
            .spawn(move || {
                run_recorder_thread(config, samples_clone, is_recording_clone, stop_rx, meta_tx)
            })
            .map_err(|e| VoiceTypeError::Audio(format!("Worker-Thread-Spawn: {e}")))?;

        Ok(Self {
            samples,
            is_recording,
            stop_tx: Some(stop_tx),
            meta_rx: Some(meta_rx),
            meta: None,
            worker: Some(worker),
        })
    }

    /// Stoppt die Aufnahme, wartet auf den Worker-Thread und liefert das
    /// fertige WAV-File (16 kHz Mono PCM s16le) als Byte-Buffer.
    ///
    /// Async-Konformitaet: drei blockierende Schritte (Worker-Join,
    /// Sample-Lock, CPU-bound Resampling/Encoding) laufen in
    /// `spawn_blocking`, damit die tokio-Runtime nicht panickt
    /// ("Cannot block the current thread from within a runtime").
    /// Das Meta-Channel wird hingegen mit `await` gelesen.
    pub async fn stop_and_finalize(mut self) -> Result<Vec<u8>> {
        self.is_recording.store(false, Ordering::SeqCst);
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }

        // Worker-Thread.join() ist blockierend → blocking-Thread-Pool.
        if let Some(handle) = self.worker.take() {
            tokio::task::spawn_blocking(move || handle.join())
                .await
                .map_err(|e| VoiceTypeError::Audio(format!("worker spawn_blocking: {e}")))?
                .map_err(|_| VoiceTypeError::Audio("Worker-Thread panicked".into()))??;
        }

        let meta = self.resolve_meta().await?;
        let samples_arc = Arc::clone(&self.samples);

        // Resampling + WAV-Encoding sind CPU-bound (~hundert ms bei 30 s
        // Audio). Auf den blocking-Pool verlagern, damit der async-Worker frei bleibt.
        tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
            let raw_samples = std::mem::take(&mut *samples_arc.lock());
            if raw_samples.is_empty() {
                return Err(VoiceTypeError::Audio(
                    "Keine Audio-Daten aufgenommen".into(),
                ));
            }
            let mono = stereo_to_mono(&raw_samples, meta.channels);
            let resampled = resample_to_16k(&mono, meta.sample_rate)?;
            encode_wav_16k_mono(&resampled)
        })
        .await
        .map_err(|e| VoiceTypeError::Audio(format!("encoding spawn_blocking: {e}")))?
    }

    async fn resolve_meta(&mut self) -> Result<StreamMeta> {
        if let Some(meta) = self.meta {
            return Ok(meta);
        }
        let rx = self
            .meta_rx
            .take()
            .ok_or_else(|| VoiceTypeError::Audio("Meta bereits konsumiert".into()))?;
        let meta = rx
            .await
            .map_err(|_| VoiceTypeError::Audio("Worker meldete keine Stream-Meta".into()))?;
        self.meta = Some(meta);
        Ok(meta)
    }
}

/// Worker-Thread: baut den cpal-Stream, fuettert Samples in den Buffer,
/// wartet auf Stop-Signal, droppt dann den Stream (was die Aufnahme beendet).
fn run_recorder_thread(
    config: RecorderConfig,
    samples: Arc<Mutex<Vec<f32>>>,
    is_recording: Arc<AtomicBool>,
    stop_rx: oneshot::Receiver<()>,
    meta_tx: oneshot::Sender<StreamMeta>,
) -> Result<()> {
    let host = cpal::default_host();
    let device = match config.device_name {
        Some(name) => host
            .input_devices()
            .map_err(|e| VoiceTypeError::Audio(format!("Geraeteliste: {e}")))?
            .find(|d| d.name().map(|n| n == name).unwrap_or(false))
            .ok_or_else(|| VoiceTypeError::Audio(format!("Geraet '{name}' nicht gefunden")))?,
        None => host
            .default_input_device()
            .ok_or_else(|| VoiceTypeError::Audio("Kein Standard-Eingabegeraet".into()))?,
    };

    let supported = device
        .default_input_config()
        .map_err(|e| VoiceTypeError::Audio(format!("Geraete-Config: {e}")))?;

    let sample_rate = supported.sample_rate().0;
    let channels = supported.channels();
    let _ = meta_tx.send(StreamMeta {
        sample_rate,
        channels,
    });

    let err_fn =
        |err: cpal::StreamError| tracing::error!(target: "voicetypex::audio", "cpal: {err}");
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => {
            let samples_cb = Arc::clone(&samples);
            let recording_cb = Arc::clone(&is_recording);
            device.build_input_stream(
                &supported.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if recording_cb.load(Ordering::Relaxed) {
                        samples_cb.lock().extend_from_slice(data);
                    }
                },
                err_fn,
                None,
            )
        }
        cpal::SampleFormat::I16 => {
            let samples_cb = Arc::clone(&samples);
            let recording_cb = Arc::clone(&is_recording);
            device.build_input_stream(
                &supported.into(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if recording_cb.load(Ordering::Relaxed) {
                        let mut buf = samples_cb.lock();
                        buf.extend(data.iter().map(|&s| s as f32 / i16::MAX as f32));
                    }
                },
                err_fn,
                None,
            )
        }
        cpal::SampleFormat::U16 => {
            let samples_cb = Arc::clone(&samples);
            let recording_cb = Arc::clone(&is_recording);
            device.build_input_stream(
                &supported.into(),
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    if recording_cb.load(Ordering::Relaxed) {
                        let mut buf = samples_cb.lock();
                        buf.extend(data.iter().map(|&s| (s as f32 - 32_768.0) / 32_768.0));
                    }
                },
                err_fn,
                None,
            )
        }
        other => {
            return Err(VoiceTypeError::Audio(format!(
                "Sample-Format {other:?} nicht unterstuetzt"
            )));
        }
    }
    .map_err(|e| VoiceTypeError::Audio(format!("build_input_stream: {e}")))?;

    stream
        .play()
        .map_err(|e| VoiceTypeError::Audio(format!("stream.play: {e}")))?;

    // Blockiere bis Stop-Signal — Stream wird beim Drop gestoppt.
    let _ = stop_rx.blocking_recv();
    drop(stream);
    Ok(())
}

/// Liefert die Liste verfuegbarer Eingabegeraete. Gibt nur Geraete zurueck,
/// die einen Namen haben (anonyme Geraete werden uebersprungen).
pub fn list_input_devices() -> Result<Vec<String>> {
    let host = cpal::default_host();
    let devices = host
        .input_devices()
        .map_err(|e| VoiceTypeError::Audio(format!("input_devices: {e}")))?;
    let mut names = Vec::new();
    for d in devices {
        if let Ok(name) = d.name() {
            names.push(name);
        }
    }
    Ok(names)
}

fn stereo_to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }
    let ch = channels as usize;
    samples
        .chunks_exact(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

/// Resample f32-Mono auf 16 kHz. Wenn die Quelle bereits 16 kHz ist, kein-op.
fn resample_to_16k(mono: &[f32], source_rate: u32) -> Result<Vec<f32>> {
    if source_rate == WHISPER_SAMPLE_RATE {
        return Ok(mono.to_vec());
    }

    let params = SincInterpolationParameters {
        sinc_len: 128,
        f_cutoff: 0.95,
        oversampling_factor: 256,
        interpolation: SincInterpolationType::Linear,
        window: WindowFunction::BlackmanHarris2,
    };

    let resample_ratio = WHISPER_SAMPLE_RATE as f64 / source_rate as f64;
    let chunk_size = mono.len();

    let mut resampler = SincFixedIn::<f32>::new(resample_ratio, 2.0, params, chunk_size, 1)
        .map_err(|e| VoiceTypeError::Audio(format!("rubato::new: {e}")))?;

    let input = vec![mono.to_vec()];
    let output = resampler
        .process(&input, None)
        .map_err(|e| VoiceTypeError::Audio(format!("rubato::process: {e}")))?;

    output
        .into_iter()
        .next()
        .ok_or_else(|| VoiceTypeError::Audio("Resampler lieferte 0 Channels".into()))
}

/// Encodiere f32-Mono-Samples (16 kHz) als WAV (PCM s16le).
fn encode_wav_16k_mono(samples: &[f32]) -> Result<Vec<u8>> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: WHISPER_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut buf = Cursor::new(Vec::with_capacity(samples.len() * 2 + 44));
    {
        let mut writer = hound::WavWriter::new(&mut buf, spec)
            .map_err(|e| VoiceTypeError::Audio(format!("hound::WavWriter: {e}")))?;
        for &s in samples {
            let clamped = s.clamp(-1.0, 1.0);
            let q = (clamped * i16::MAX as f32) as i16;
            writer
                .write_sample(q)
                .map_err(|e| VoiceTypeError::Audio(format!("hound::write_sample: {e}")))?;
        }
        writer
            .finalize()
            .map_err(|e| VoiceTypeError::Audio(format!("hound::finalize: {e}")))?;
    }
    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_downmix_averages_channels() {
        let stereo = vec![1.0, -1.0, 0.5, 0.5];
        let mono = stereo_to_mono(&stereo, 2);
        assert_eq!(mono, vec![0.0, 0.5]);
    }

    #[test]
    fn mono_passthrough() {
        let mono = vec![0.1, 0.2, 0.3];
        let out = stereo_to_mono(&mono, 1);
        assert_eq!(out, mono);
    }

    #[test]
    fn resample_passthrough_when_already_16k() {
        let samples: Vec<f32> = (0..1000).map(|i| (i as f32) / 1000.0).collect();
        let out = resample_to_16k(&samples, WHISPER_SAMPLE_RATE).unwrap();
        assert_eq!(out, samples);
    }

    #[test]
    fn wav_encode_has_header_and_payload() {
        let samples = vec![0.0; 16_000]; // 1 sec stille
        let wav = encode_wav_16k_mono(&samples).unwrap();
        // RIFF-header
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        // 16k samples * 2 bytes + 44 byte header
        assert_eq!(wav.len(), 16_000 * 2 + 44);
    }
}
