// SPDX-License-Identifier: GPL-3.0-or-later
//! xAI speech-to-text — `POST https://api.x.ai/v1/stt`,
//! multipart/form-data with `file` as the last field. Response:
//! `text`, `language`, `duration`, `words[]` with word-level
//! timestamps. We only use `text`.

use crate::core::error::{Result, VoiceTypeError};
use crate::core::retry::with_retry;
use crate::transcription::{TranscribeOpts, Transcriber};
use async_trait::async_trait;
use serde::Deserialize;

const DEFAULT_MODEL: &str = "stt-1";

pub struct XaiTranscriber {
    api_key: String,
    base_url: String,
    model: String,
    client: reqwest::Client,
}

impl XaiTranscriber {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://api.x.ai/v1".to_string(),
            model: DEFAULT_MODEL.to_string(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("reqwest client builder (timeout)"),
        }
    }
}

#[async_trait]
impl Transcriber for XaiTranscriber {
    fn name(&self) -> &str {
        "xai"
    }

    async fn transcribe_oneshot(&self, audio: &[u8], opts: TranscribeOpts) -> Result<String> {
        let url = format!("{}/stt", self.base_url.trim_end_matches('/'));

        with_retry(|| async {
            // According to xAI `file` must be the LAST field.
            // `multipart::Form` is not Clone — rebuild per attempt.
            let part = reqwest::multipart::Part::bytes(audio.to_vec())
                .file_name("audio.wav")
                .mime_str("audio/wav")
                .map_err(|e| VoiceTypeError::Transcription(format!("multipart-Part: {e}")))?;

            let mut form = reqwest::multipart::Form::new().text("model", self.model.clone());
            if let Some(lang) = opts.language.as_deref() {
                form = form.text("language", lang.to_string());
            }
            if let Some(prompt) = opts.initial_prompt.as_deref() {
                form = form.text("initial_prompt", prompt.to_string());
            }
            let form = form.part("file", part);

            let response = self
                .client
                .post(&url)
                .bearer_auth(&self.api_key)
                .multipart(form)
                .send()
                .await
                .map_err(|e| VoiceTypeError::Transcription(format!("HTTP {url}: {e}")))?;

            let status = response.status();
            if !status.is_success() {
                tracing::warn!(provider = "xai", %status, "transcribe call failed");
                return Err(VoiceTypeError::Transcription(format!(
                    "xAI STT HTTP {status}"
                )));
            }

            let parsed: SttResponse = response
                .json()
                .await
                .map_err(|e| VoiceTypeError::Transcription(format!("xAI-STT-JSON-Parse: {e}")))?;
            Ok(parsed.text.trim().to_string())
        })
        .await
    }
}

#[derive(Deserialize)]
struct SttResponse {
    text: String,
}
