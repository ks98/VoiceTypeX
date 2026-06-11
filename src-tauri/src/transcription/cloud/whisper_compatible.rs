// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared STT client for all OpenAI-Whisper-API-compatible providers
//! (OpenAI itself, Groq).
//!
//! API contract:
//!   POST {base_url}/audio/transcriptions
//!   Authorization: Bearer {api_key}
//!   Content-Type: multipart/form-data
//!     - file: WAV bytes
//!     - model: provider-specific
//!     - language: optional ISO code
//!     - response_format: "json" (default)
//!   Response: { "text": "..." }

use crate::core::error::{Result, VoiceTypeError};
use crate::core::retry::with_retry;
use crate::transcription::TranscribeOpts;
use serde::Deserialize;

#[derive(Clone)]
pub struct WhisperCompatibleClient {
    pub base_url: String,
    pub default_model: String,
    pub api_key: String,
    client: reqwest::Client,
}

impl WhisperCompatibleClient {
    pub fn new(
        base_url: impl Into<String>,
        default_model: impl Into<String>,
        api_key: String,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            default_model: default_model.into(),
            api_key,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("reqwest client builder (timeout)"),
        }
    }

    pub async fn transcribe(&self, audio: &[u8], opts: &TranscribeOpts) -> Result<String> {
        let url = format!(
            "{}/audio/transcriptions",
            self.base_url.trim_end_matches('/')
        );

        with_retry(|| async {
            // `multipart::Form` is not Clone — rebuild per attempt.
            let part = reqwest::multipart::Part::bytes(audio.to_vec())
                .file_name("audio.wav")
                .mime_str("audio/wav")
                .map_err(|e| VoiceTypeError::transcription(format!("multipart-Part: {e}")))?;
            let mut form = reqwest::multipart::Form::new()
                .text("model", self.default_model.clone())
                .part("file", part);
            if let Some(lang) = opts.language.as_deref() {
                form = form.text("language", lang.to_string());
            }
            if let Some(prompt) = opts.initial_prompt.as_deref() {
                form = form.text("prompt", prompt.to_string());
            }

            let response = self
                .client
                .post(&url)
                .bearer_auth(&self.api_key)
                .multipart(form)
                .send()
                .await
                .map_err(|e| VoiceTypeError::transcription(format!("HTTP {url}: {e}")))?;

            let status = response.status();
            if !status.is_success() {
                tracing::warn!(provider = "whisper_compatible", %status, "transcribe call failed");
                return Err(VoiceTypeError::transcription(format!(
                    "Whisper-API HTTP {status}"
                )));
            }

            let parsed: WhisperResponse = response
                .json()
                .await
                .map_err(|e| VoiceTypeError::transcription(format!("Whisper-JSON-Parse: {e}")))?;
            Ok(parsed.text.trim().to_string())
        })
        .await
    }
}

#[derive(Deserialize)]
struct WhisperResponse {
    text: String,
}
