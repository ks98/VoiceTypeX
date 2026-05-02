// SPDX-License-Identifier: GPL-3.0-or-later
//! OpenAI Whisper API — `POST https://api.openai.com/v1/audio/transcriptions`.
//! Whisper-API-Standard, multipart/form-data.

use crate::core::error::{Result, VoiceTypeError};
use crate::transcription::{TranscribeOpts, Transcriber, TranscriptionMode};
use async_trait::async_trait;

const SUPPORTED: &[TranscriptionMode] = &[TranscriptionMode::OneShot];

#[allow(dead_code)]
pub struct OpenAITranscriber {
    api_key: String,
    base_url: String,
    model: String,
}

impl OpenAITranscriber {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://api.openai.com/v1".to_string(),
            model: "whisper-1".to_string(),
        }
    }
}

#[async_trait]
impl Transcriber for OpenAITranscriber {
    fn name(&self) -> &str {
        "openai"
    }

    fn supports(&self) -> &'static [TranscriptionMode] {
        SUPPORTED
    }

    async fn transcribe_oneshot(&self, _audio: &[u8], _opts: TranscribeOpts) -> Result<String> {
        Err(VoiceTypeError::Transcription(
            "OpenAI Whisper noch nicht implementiert (Phase 2.5)".into(),
        ))
    }
}
