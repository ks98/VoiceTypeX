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

pub mod cloud;
pub mod local;
pub mod model_downloader;

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
