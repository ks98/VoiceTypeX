// SPDX-License-Identifier: GPL-3.0-or-later
//! Groq Whisper — `POST https://api.groq.com/openai/v1/audio/transcriptions`.
//! OpenAI Whisper API-compatible. Default model `whisper-large-v3-turbo`
//! (Groq's fastest variant).

use crate::core::error::{ProviderId, Result};
use crate::transcription::cloud::whisper_compatible::WhisperCompatibleClient;
use crate::transcription::{TranscribeOpts, Transcriber};
use async_trait::async_trait;

pub struct GroqTranscriber {
    inner: WhisperCompatibleClient,
}

impl GroqTranscriber {
    pub fn new(api_key: String, client: reqwest::Client) -> Self {
        Self {
            inner: WhisperCompatibleClient::new(
                ProviderId::Groq,
                "https://api.groq.com/openai/v1",
                "whisper-large-v3-turbo",
                api_key,
                client,
            ),
        }
    }
}

#[async_trait]
impl Transcriber for GroqTranscriber {
    fn name(&self) -> &str {
        "groq"
    }

    async fn transcribe_oneshot(&self, audio: &[u8], opts: TranscribeOpts) -> Result<String> {
        self.inner.transcribe(audio, &opts).await
    }
}
