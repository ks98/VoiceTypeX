// SPDX-License-Identifier: GPL-3.0-or-later
//! Speech-to-text abstraction.
//!
//! The `Transcriber` trait defines a one-shot interface: complete
//! audio in, complete text out. That's the most accurate variant (no
//! interim token commits, global audio context during inference) and
//! is used identically by the local `whisper-rs` backend and by all
//! cloud providers.

use crate::core::error::Result;
use async_trait::async_trait;

pub mod backend;
pub mod cloud;
pub mod local;
pub mod local_agreement;
pub mod model_downloader;

use crate::core::error::VoiceTypeError;
use crate::secrets::SecretStore;
use std::sync::Arc;

/// Factory: returns the matching `Transcriber` for a cloud provider.
/// Reads the API key from the OS keychain. Fails with a clear message
/// if the key is not set.
pub fn make_cloud_transcriber(provider: &str) -> Result<Arc<dyn Transcriber>> {
    let key = SecretStore::get(provider)?.ok_or_else(|| {
        VoiceTypeError::transcription(format!(
            "No API key set for provider '{provider}' — add it under Settings"
        ))
    })?;
    match provider {
        "xai" => Ok(Arc::new(cloud::xai::XaiTranscriber::new(key))),
        "openai" => Ok(Arc::new(cloud::openai::OpenAITranscriber::new(key))),
        "groq" => Ok(Arc::new(cloud::groq::GroqTranscriber::new(key))),
        "deepgram" => Ok(Arc::new(cloud::deepgram::DeepgramTranscriber::new(key))),
        other => Err(VoiceTypeError::transcription(format!(
            "Unknown STT provider: {other}"
        ))),
    }
}

#[derive(Debug, Clone, Default)]
pub struct TranscribeOpts {
    pub language: Option<String>,
    pub initial_prompt: Option<String>,
    /// Override for the Whisper thread count. `None` = auto-detect.
    /// Relevant only for `LocalTranscriber`; cloud transcribers
    /// ignore it.
    pub n_threads: Option<u32>,
    /// Beam width for the local final-pass BeamSearch. `None` = the
    /// built-in default (5). Higher = slightly more accurate, markedly
    /// slower (~beam× decode cost); 1 ≈ greedy. Relevant only for
    /// `LocalTranscriber`'s final pass; the streaming pass is greedy
    /// regardless, and cloud transcribers ignore it.
    pub beam_size: Option<u32>,
}

#[async_trait]
pub trait Transcriber: Send + Sync {
    fn name(&self) -> &str;

    /// Complete WAV/PCM buffer in, complete text out. Audio format by
    /// convention 16 kHz mono PCM s16le in a WAV container; every
    /// provider documents deviations.
    async fn transcribe_oneshot(&self, audio: &[u8], opts: TranscribeOpts) -> Result<String>;
}
