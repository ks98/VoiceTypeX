// SPDX-License-Identifier: GPL-3.0-or-later
//! Audio recorder via cpal.
//!
//! Architecture (CLAUDE.md §4.1, §4.2):
//! `cpal::Stream` is `!Send`, so the actual recording runs in a
//! dedicated OS thread. The `RecorderHandle` exported here is
//! `Send + Sync` and communicates via channels with the worker thread.
//!
//! Pipeline per recording:
//!   microphone (native rate, stereo/mono)
//!     -> cpal callback collects f32 samples into `Arc<Mutex<Vec<f32>>>`
//!     -> stop signal: worker thread drops the stream, we get the samples
//!     -> stereo->mono downmix (L+R)/2 if channels > 1
//!     -> resampling to 16 kHz mono via `rubato::SincFixedIn` (speech-grade)
//!     -> `stop_and_finalize` returns the 16 kHz mono `Vec<f32>` directly.
//!        The local Whisper path consumes f32 as-is; only the cloud
//!        multipart upload lazily wraps it via `encode_wav_16k_mono`
//!        (s16le, hound) — so no f32->WAV->f32 roundtrip on local STT.

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

/// Target sample rate for Whisper.
pub const WHISPER_SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, Clone, Default)]
pub struct RecorderConfig {
    /// Device name (`Device::name()`). `None` = OS default input
    /// device.
    pub device_name: Option<String>,
}

/// Capture stream metadata (sent once by the cpal worker after device
/// open). `pub` so the streaming worker in `pipeline/` can do the
/// conversion without a `RecorderHandle` lock.
#[derive(Debug, Clone, Copy)]
pub struct StreamMeta {
    pub sample_rate: u32,
    pub channels: u16,
}

/// Send-safe handle to the recorder thread.
pub struct RecorderHandle {
    samples: Arc<Mutex<Vec<f32>>>,
    is_recording: Arc<AtomicBool>,
    stop_tx: Option<oneshot::Sender<()>>,
    meta_rx: Option<oneshot::Receiver<StreamMeta>>,
    meta: Option<StreamMeta>,
    worker: Option<thread::JoinHandle<Result<()>>>,
}

impl RecorderHandle {
    /// Start recording. Audio accumulates in the buffer;
    /// `stop_and_finalize` returns the resampled 16 kHz mono f32 samples.
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
            .map_err(|e| VoiceTypeError::Audio(format!("Worker thread spawn: {e}")))?;

        Ok(Self {
            samples,
            is_recording,
            stop_tx: Some(stop_tx),
            meta_rx: Some(meta_rx),
            meta: None,
            worker: Some(worker),
        })
    }

    /// Stop recording, wait for the worker thread, and return the
    /// resampled 16 kHz mono f32 samples.
    ///
    /// The local Whisper path feeds these straight to whisper-rs; the
    /// cloud path wraps them via `encode_wav_16k_mono` (the WAV
    /// container is only needed for the multipart upload). This avoids
    /// the lossy f32->s16-WAV->f32 roundtrip the local path used to pay.
    ///
    /// Async-conformity: three blocking steps (worker join, sample
    /// lock, CPU-bound resampling) run in `spawn_blocking`, so the
    /// tokio runtime doesn't panic ("Cannot block the current thread
    /// from within a runtime"). The meta channel is read with `await`
    /// instead.
    pub async fn stop_and_finalize(mut self) -> Result<Vec<f32>> {
        self.is_recording.store(false, Ordering::SeqCst);
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }

        // Worker `thread.join()` is blocking → blocking thread pool.
        if let Some(handle) = self.worker.take() {
            tokio::task::spawn_blocking(move || handle.join())
                .await
                .map_err(|e| VoiceTypeError::Audio(format!("worker spawn_blocking: {e}")))?
                .map_err(|_| VoiceTypeError::Audio("Worker thread panicked".into()))??;
        }

        let meta = self.resolve_meta().await?;
        let samples_arc = Arc::clone(&self.samples);

        // Resampling is CPU-bound (~hundreds of ms for 30 s of audio).
        // Move to the blocking pool so the async worker stays free.
        tokio::task::spawn_blocking(move || -> Result<Vec<f32>> {
            let raw_samples = std::mem::take(&mut *samples_arc.lock());
            if raw_samples.is_empty() {
                return Err(VoiceTypeError::Audio("No audio data recorded".into()));
            }
            let mono = stereo_to_mono(&raw_samples, meta.channels);
            resample_to_16k(&mono, meta.sample_rate)
        })
        .await
        .map_err(|e| VoiceTypeError::Audio(format!("resampling spawn_blocking: {e}")))?
    }

    async fn resolve_meta(&mut self) -> Result<StreamMeta> {
        if let Some(meta) = self.meta {
            return Ok(meta);
        }
        let rx = self
            .meta_rx
            .take()
            .ok_or_else(|| VoiceTypeError::Audio("Meta already consumed".into()))?;
        let meta = rx
            .await
            .map_err(|_| VoiceTypeError::Audio("Worker did not report stream meta".into()))?;
        self.meta = Some(meta);
        Ok(meta)
    }

    /// Cheap clone of the sample buffer Arc for live snapshots during
    /// recording. The streaming worker in `pipeline/` thus holds the
    /// buffer mutex only briefly (cloning the current samples), while
    /// the CPU work (mono mix + resampling + Whisper decode) runs
    /// lock-free.
    pub fn samples_handle(&self) -> Arc<Mutex<Vec<f32>>> {
        Arc::clone(&self.samples)
    }

    /// Wait once for the `StreamMeta` from the worker thread and
    /// cache it. Subsequent calls return the cached value
    /// immediately. The streaming worker calls this right after
    /// `start()` so the loop needs no further async waits.
    pub async fn await_meta(&mut self) -> Result<StreamMeta> {
        self.resolve_meta().await
    }
}

/// Convert raw samples (cpal-native: f32, interleaved per channel) to
/// Whisper's expectation of 16 kHz mono f32. Used both in the final
/// path (`stop_and_finalize`) and in the streaming worker, so exposed
/// here as a free function.
pub fn to_16k_mono(raw: &[f32], meta: StreamMeta) -> Result<Vec<f32>> {
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    let mono = stereo_to_mono(raw, meta.channels);
    resample_to_16k(&mono, meta.sample_rate)
}

/// Worker thread: builds the cpal stream, feeds samples into the
/// buffer, waits for the stop signal, then drops the stream (which
/// ends the recording).
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
            .map_err(|e| VoiceTypeError::Audio(format!("Device list: {e}")))?
            .find(|d| d.name().map(|n| n == name).unwrap_or(false))
            .ok_or_else(|| VoiceTypeError::Audio(format!("Device '{name}' not found")))?,
        None => host
            .default_input_device()
            .ok_or_else(|| VoiceTypeError::Audio("No default input device".into()))?,
    };

    let supported = device
        .default_input_config()
        .map_err(|e| VoiceTypeError::Audio(format!("Device config: {e}")))?;

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
                "Sample format {other:?} not supported"
            )));
        }
    }
    .map_err(|e| VoiceTypeError::Audio(format!("build_input_stream: {e}")))?;

    stream
        .play()
        .map_err(|e| VoiceTypeError::Audio(format!("stream.play: {e}")))?;

    // Block until the stop signal — the stream is stopped on drop.
    let _ = stop_rx.blocking_recv();
    drop(stream);
    Ok(())
}

/// Returns the list of available input devices. Returns only devices
/// that have a name (anonymous devices are skipped).
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

/// Resample f32 mono to 16 kHz. No-op if the source is already 16 kHz.
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
        .ok_or_else(|| VoiceTypeError::Audio("resampler returned 0 channels".into()))
}

/// Encode f32 mono samples (16 kHz) as WAV (PCM s16le).
///
/// Only the cloud STT path needs this — its multipart upload requires
/// a WAV container. The local Whisper path consumes the f32 samples
/// from `stop_and_finalize` directly, skipping the lossy s16 quantize.
pub fn encode_wav_16k_mono(samples: &[f32]) -> Result<Vec<u8>> {
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
        let samples = vec![0.0; 16_000]; // 1 sec of silence
        let wav = encode_wav_16k_mono(&samples).unwrap();
        // RIFF-header
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        // 16k samples * 2 bytes + 44 byte header
        assert_eq!(wav.len(), 16_000 * 2 + 44);
    }
}
