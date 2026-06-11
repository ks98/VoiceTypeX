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

use crate::core::error::{ProviderId, Result, VoiceTypeError};
use crate::core::retry::with_retry;
use crate::transcription::TranscribeOpts;
use serde::Deserialize;

#[derive(Clone)]
pub struct WhisperCompatibleClient {
    pub base_url: String,
    pub default_model: String,
    pub api_key: String,
    provider: ProviderId,
    client: reqwest::Client,
}

impl WhisperCompatibleClient {
    pub fn new(
        provider: ProviderId,
        base_url: impl Into<String>,
        default_model: impl Into<String>,
        api_key: String,
        client: reqwest::Client,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            default_model: default_model.into(),
            api_key,
            provider,
            client,
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
                // The shared client carries no default timeout (issue #41);
                // keep the per-request 120 s budget the dedicated client used.
                .timeout(std::time::Duration::from_secs(120))
                .bearer_auth(&self.api_key)
                .multipart(form)
                .send()
                .await
                .map_err(|e| {
                    VoiceTypeError::transcription_network(self.provider, format!("HTTP {url}: {e}"))
                })?;

            let status = response.status();
            if !status.is_success() {
                tracing::warn!(provider = "whisper_compatible", %status, "transcribe call failed");
                return Err(VoiceTypeError::transcription_http(
                    status.as_u16(),
                    self.provider,
                    format!("Whisper-API HTTP {status}"),
                ));
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
