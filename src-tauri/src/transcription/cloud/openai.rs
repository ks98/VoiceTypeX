// SPDX-License-Identifier: GPL-3.0-or-later
//! OpenAI Whisper API — `POST https://api.openai.com/v1/audio/transcriptions`.
//! Whisper-API-Standard, multipart/form-data. Default-Modell `whisper-1`.

use crate::core::error::Result;
use crate::transcription::cloud::whisper_compatible::WhisperCompatibleClient;
use crate::transcription::{TranscribeOpts, Transcriber};
use async_trait::async_trait;

pub struct OpenAITranscriber {
    inner: WhisperCompatibleClient,
}

impl OpenAITranscriber {
    pub fn new(api_key: String) -> Self {
        Self {
            inner: WhisperCompatibleClient::new("https://api.openai.com/v1", "whisper-1", api_key),
        }
    }
}

#[async_trait]
impl Transcriber for OpenAITranscriber {
    fn name(&self) -> &str {
        "openai"
    }

    async fn transcribe_oneshot(&self, audio: &[u8], opts: TranscribeOpts) -> Result<String> {
        self.inner.transcribe(audio, &opts).await
    }
}
