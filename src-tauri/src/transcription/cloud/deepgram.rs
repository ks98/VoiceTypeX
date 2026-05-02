// SPDX-License-Identifier: GPL-3.0-or-later
//! Deepgram STT — eigenes API-Format, nicht Whisper-kompatibel.

use crate::core::error::{Result, VoiceTypeError};
use crate::transcription::{TranscribeOpts, Transcriber, TranscriptionMode};
use async_trait::async_trait;

const SUPPORTED: &[TranscriptionMode] = &[TranscriptionMode::OneShot];

#[allow(dead_code)]
pub struct DeepgramTranscriber {
    api_key: String,
    base_url: String,
}

impl DeepgramTranscriber {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://api.deepgram.com/v1".to_string(),
        }
    }
}

#[async_trait]
impl Transcriber for DeepgramTranscriber {
    fn name(&self) -> &str {
        "deepgram"
    }

    fn supports(&self) -> &'static [TranscriptionMode] {
        SUPPORTED
    }

    async fn transcribe_oneshot(&self, _audio: &[u8], _opts: TranscribeOpts) -> Result<String> {
        Err(VoiceTypeError::Transcription(
            "Deepgram noch nicht implementiert (Phase 2.5)".into(),
        ))
    }
}
