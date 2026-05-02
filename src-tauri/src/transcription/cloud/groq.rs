// SPDX-License-Identifier: GPL-3.0-or-later
//! Groq Whisper — `POST https://api.groq.com/openai/v1/audio/transcriptions`.
//! OpenAI-Whisper-API-kompatibel, kann perspektivisch denselben Helper wie
//! `openai.rs` nutzen, falls beide langfristig im Stack bleiben.

use crate::core::error::{Result, VoiceTypeError};
use crate::transcription::{TranscribeOpts, Transcriber, TranscriptionMode};
use async_trait::async_trait;

const SUPPORTED: &[TranscriptionMode] = &[TranscriptionMode::OneShot];

#[allow(dead_code)]
pub struct GroqTranscriber {
    api_key: String,
    base_url: String,
    model: String,
}

impl GroqTranscriber {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://api.groq.com/openai/v1".to_string(),
            model: "whisper-large-v3-turbo".to_string(),
        }
    }
}

#[async_trait]
impl Transcriber for GroqTranscriber {
    fn name(&self) -> &str {
        "groq"
    }

    fn supports(&self) -> &'static [TranscriptionMode] {
        SUPPORTED
    }

    async fn transcribe_oneshot(&self, _audio: &[u8], _opts: TranscribeOpts) -> Result<String> {
        Err(VoiceTypeError::Transcription(
            "Groq Whisper noch nicht implementiert (Phase 2.5)".into(),
        ))
    }
}
