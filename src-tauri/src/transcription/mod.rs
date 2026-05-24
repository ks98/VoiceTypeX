// SPDX-License-Identifier: GPL-3.0-or-later
//! Speech-to-Text Abstraktion.
//!
//! Der `Transcriber`-Trait definiert eine One-Shot-Schnittstelle: komplettes
//! Audio rein, kompletter Text raus. Das ist die genaueste Variante (keine
//! interim Token-Commits, globaler Audio-Kontext bei der Inferenz) und wird
//! sowohl vom lokalen `whisper-rs`-Backend als auch von allen Cloud-Providern
//! gleich genutzt.

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

/// Factory: liefert den passenden `Transcriber` fuer einen Cloud-Provider.
/// Liest den API-Key aus dem OS-Keychain. Schlaegt mit klarer Meldung fehl,
/// wenn der Key nicht gesetzt ist.
pub fn make_cloud_transcriber(provider: &str) -> Result<Arc<dyn Transcriber>> {
    let key = SecretStore::get(provider)?.ok_or_else(|| {
        VoiceTypeError::Transcription(format!(
            "API-Key fuer Provider '{provider}' nicht gesetzt — bitte in den Einstellungen hinterlegen"
        ))
    })?;
    match provider {
        "xai" => Ok(Arc::new(cloud::xai::XaiTranscriber::new(key))),
        "openai" => Ok(Arc::new(cloud::openai::OpenAITranscriber::new(key))),
        "groq" => Ok(Arc::new(cloud::groq::GroqTranscriber::new(key))),
        "deepgram" => Ok(Arc::new(cloud::deepgram::DeepgramTranscriber::new(key))),
        other => Err(VoiceTypeError::Transcription(format!(
            "Unbekannter STT-Provider: {other}"
        ))),
    }
}

#[derive(Debug, Clone, Default)]
pub struct TranscribeOpts {
    pub language: Option<String>,
    pub initial_prompt: Option<String>,
    /// Override fuer Whisper-Thread-Anzahl. `None` = Auto-Detect.
    /// Nur fuer LocalTranscriber relevant; Cloud-Transcriber ignorieren es.
    pub n_threads: Option<u32>,
}

#[async_trait]
pub trait Transcriber: Send + Sync {
    fn name(&self) -> &str;

    /// Komplettes WAV/PCM-Buffer rein, kompletter Text raus. Audio-Format
    /// per Konvention 16 kHz Mono PCM s16le als WAV-Container; jeder Provider
    /// dokumentiert Abweichungen.
    async fn transcribe_oneshot(&self, audio: &[u8], opts: TranscribeOpts) -> Result<String>;
}
