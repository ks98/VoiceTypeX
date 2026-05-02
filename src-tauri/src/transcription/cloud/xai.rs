// SPDX-License-Identifier: GPL-3.0-or-later
//! xAI Speech-to-Text — `POST https://api.x.ai/v1/stt`, multipart/form-data
//! mit `file` als letztem Field. Response: `text`, `language`, `duration`,
//! `words[]` mit Word-Level-Timestamps.

use crate::core::error::{Result, VoiceTypeError};
use crate::transcription::{TranscribeOpts, Transcriber, TranscriptionMode};
use async_trait::async_trait;

const SUPPORTED: &[TranscriptionMode] = &[TranscriptionMode::OneShot];

#[allow(dead_code)]
pub struct XaiTranscriber {
    api_key: String,
    base_url: String,
}

impl XaiTranscriber {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://api.x.ai/v1".to_string(),
        }
    }
}

#[async_trait]
impl Transcriber for XaiTranscriber {
    fn name(&self) -> &str {
        "xai"
    }

    fn supports(&self) -> &'static [TranscriptionMode] {
        SUPPORTED
    }

    async fn transcribe_oneshot(&self, _audio: &[u8], _opts: TranscribeOpts) -> Result<String> {
        Err(VoiceTypeError::Transcription(
            "xAI Speech-to-Text noch nicht implementiert (Phase 2)".into(),
        ))
    }
}
