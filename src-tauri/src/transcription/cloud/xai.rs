// SPDX-License-Identifier: GPL-3.0-or-later
//! xAI speech-to-text — `POST https://api.x.ai/v1/stt`,
//! multipart/form-data with `file` as the last field. Response:
//! `text`, `language`, `duration`, `words[]` with word-level
//! timestamps. We only use `text`.

use crate::core::error::{ProviderId, Result, VoiceTypeError};
use crate::core::retry::with_retry;
use crate::transcription::{TranscribeOpts, Transcriber};
use async_trait::async_trait;
use serde::Deserialize;

pub struct XaiTranscriber {
    api_key: String,
    base_url: String,
    client: reqwest::Client,
}

impl XaiTranscriber {
    pub fn new(api_key: String, client: reqwest::Client) -> Self {
        Self {
            api_key,
            base_url: "https://api.x.ai/v1".to_string(),
            client,
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
                .map_err(|e| VoiceTypeError::transcription(format!("multipart-Part: {e}")))?;

            // xAI `/v1/stt` accepts no `model` and no `initial_prompt`
            // field (verified against the official docs — only `file`/
            // `url` plus optional flags like `language`). Sending them
            // was a no-op at best and a latent 4xx risk. `file` must be
            // the LAST field.
            let mut form = reqwest::multipart::Form::new();
            if let Some(lang) = opts.language.as_deref() {
                form = form.text("language", lang.to_string());
            }
            let form = form.part("file", part);

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
                    VoiceTypeError::transcription_network(
                        ProviderId::Xai,
                        format!("HTTP {url}: {e}"),
                    )
                })?;

            let status = response.status();
            if !status.is_success() {
                tracing::warn!(provider = "xai", %status, "transcribe call failed");
                return Err(VoiceTypeError::transcription_http(
                    status.as_u16(),
                    ProviderId::Xai,
                    format!("xAI STT HTTP {status}"),
                ));
            }

            let parsed: SttResponse = response
                .json()
                .await
                .map_err(|e| VoiceTypeError::transcription(format!("xAI-STT-JSON-Parse: {e}")))?;
            Ok(parsed.text.trim().to_string())
        })
        .await
    }
}

#[derive(Deserialize)]
struct SttResponse {
    text: String,
}
