// SPDX-License-Identifier: GPL-3.0-or-later
//! Speech-to-Text Abstraktion.
//!
//! Der `Transcriber`-Trait ist Streaming-vorbereitet (siehe CLAUDE.md §4.7),
//! aber die Streaming-Methode wird erst in Phase 4 zum Trait hinzugefuegt;
//! Phase 1+2 implementieren nur One-Shot. Datentypen `TranscriptionMode` und
//! `TranscriptionEvent` existieren bereits, weil sie auch fuer das spaetere
//! IPC-Mapping (Frontend Live-Anzeige) gebraucht werden.

use crate::core::error::Result;
use async_trait::async_trait;

pub mod backend;
pub mod cloud;
pub mod local;
pub mod model_downloader;

use crate::core::error::VoiceTypeError;
use crate::secrets::SecretStore;
use std::path::PathBuf;
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

/// Factory: liefert den lokalen Whisper-Transcriber fuer einen Modellpfad.
pub fn make_local_transcriber(model_path: PathBuf) -> Arc<dyn Transcriber> {
    Arc::new(local::LocalTranscriber::new(model_path))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptionMode {
    OneShot,
    Streaming,
}

#[derive(Debug, Clone)]
pub enum TranscriptionEvent {
    Partial { text: String, is_final: bool },
    Done { text: String, duration_ms: u32 },
    Error(String),
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

    fn supports(&self) -> &'static [TranscriptionMode];

    /// Komplettes WAV/PCM-Buffer rein, kompletter Text raus. Audio-Format
    /// per Konvention 16 kHz Mono PCM s16le als WAV-Container; jeder Provider
    /// dokumentiert Abweichungen.
    async fn transcribe_oneshot(&self, audio: &[u8], opts: TranscribeOpts) -> Result<String>;
}
