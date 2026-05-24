// SPDX-License-Identifier: GPL-3.0-or-later
//! Deepgram STT.
//!
//! Eigene API: POST https://api.deepgram.com/v1/listen
//!   Header `Authorization: Token {api_key}` (NICHT Bearer)
//!   Header `Content-Type: audio/wav`
//!   Body: RAW Audio-Bytes (kein multipart!)
//! Response (json): results.channels[0].alternatives[0].transcript

use crate::core::error::{Result, VoiceTypeError};
use crate::core::retry::with_retry;
use crate::transcription::{TranscribeOpts, Transcriber};
use async_trait::async_trait;
use serde::Deserialize;

pub struct DeepgramTranscriber {
    api_key: String,
    base_url: String,
    client: reqwest::Client,
}

impl DeepgramTranscriber {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://api.deepgram.com/v1".to_string(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("reqwest client builder (timeout)"),
        }
    }

    pub async fn test_connection(&self) -> Result<()> {
        let url = format!("{}/projects", self.base_url.trim_end_matches('/'));
        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Token {}", self.api_key))
            .send()
            .await
            .map_err(|e| VoiceTypeError::Transcription(format!("HTTP {url}: {e}")))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(VoiceTypeError::Transcription(format!(
                "Deepgram HTTP {status}: {body}"
            )));
        }
        Ok(())
    }
}

#[async_trait]
impl Transcriber for DeepgramTranscriber {
    fn name(&self) -> &str {
        "deepgram"
    }

    async fn transcribe_oneshot(&self, audio: &[u8], opts: TranscribeOpts) -> Result<String> {
        let url = format!("{}/listen", self.base_url.trim_end_matches('/'));
        let mut query: Vec<(&str, String)> = vec![
            ("model", "nova-3".to_string()),
            ("smart_format", "true".to_string()),
        ];
        if let Some(lang) = opts.language.as_deref() {
            query.push(("language", lang.to_string()));
        }

        with_retry(|| async {
            let response = self
                .client
                .post(&url)
                .query(&query)
                .header("Authorization", format!("Token {}", self.api_key))
                .header("Content-Type", "audio/wav")
                .body(audio.to_vec())
                .send()
                .await
                .map_err(|e| VoiceTypeError::Transcription(format!("HTTP {url}: {e}")))?;

            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(VoiceTypeError::Transcription(format!(
                    "Deepgram HTTP {status}: {body}"
                )));
            }

            let parsed: DeepgramResponse = response
                .json()
                .await
                .map_err(|e| VoiceTypeError::Transcription(format!("Deepgram-JSON-Parse: {e}")))?;
            let transcript = parsed
                .results
                .and_then(|r| r.channels.into_iter().next())
                .and_then(|c| c.alternatives.into_iter().next())
                .map(|a| a.transcript)
                .unwrap_or_default();
            Ok(transcript.trim().to_string())
        })
        .await
    }
}

#[derive(Deserialize)]
struct DeepgramResponse {
    results: Option<Results>,
}

#[derive(Deserialize)]
struct Results {
    channels: Vec<Channel>,
}

#[derive(Deserialize)]
struct Channel {
    alternatives: Vec<Alternative>,
}

#[derive(Deserialize)]
struct Alternative {
    transcript: String,
}
