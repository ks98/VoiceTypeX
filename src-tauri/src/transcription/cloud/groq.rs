// SPDX-License-Identifier: GPL-3.0-or-later
//! Groq Whisper — `POST https://api.groq.com/openai/v1/audio/transcriptions`.
//! OpenAI-Whisper-API-kompatibel. Default-Modell `whisper-large-v3-turbo`
//! (Groq's schnellste Variante).

use crate::core::error::Result;
use crate::transcription::cloud::whisper_compatible::WhisperCompatibleClient;
use crate::transcription::{TranscribeOpts, Transcriber, TranscriptionMode};
use async_trait::async_trait;

const SUPPORTED: &[TranscriptionMode] = &[TranscriptionMode::OneShot];

pub struct GroqTranscriber {
    inner: WhisperCompatibleClient,
}

impl GroqTranscriber {
    pub fn new(api_key: String) -> Self {
        Self {
            inner: WhisperCompatibleClient::new(
                "https://api.groq.com/openai/v1",
                "whisper-large-v3-turbo",
                api_key,
            ),
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

    async fn transcribe_oneshot(&self, audio: &[u8], opts: TranscribeOpts) -> Result<String> {
        self.inner.transcribe(audio, &opts).await
    }
}
