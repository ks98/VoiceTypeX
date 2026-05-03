// SPDX-License-Identifier: GPL-3.0-or-later
//! Anthropic Claude via Messages-API. Eigene Konventionen:
//! - Header `x-api-key` statt `Authorization: Bearer`
//! - Header `anthropic-version: 2023-06-01` (aktuell stabil)
//! - System-Prompt ist eigenes Top-Level-Field (NICHT in messages)
//! - Response ist `content: [{type: "text", text: ...}]`-Array

use crate::core::error::{Result, VoiceTypeError};
use crate::core::retry::with_retry;
use crate::processing::{ProcessOpts, Processor};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

const API_VERSION: &str = "2023-06-01";

pub struct AnthropicProcessor {
    api_key: String,
    base_url: String,
    default_model: String,
    client: reqwest::Client,
}

impl AnthropicProcessor {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://api.anthropic.com/v1".to_string(),
            default_model: "claude-sonnet-4-6".to_string(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    /// Pruefe Auth via einer minimalen `POST /messages`-Anfrage (max_tokens=1)
    /// gegen das billigste Modell. Anthropic hat keinen kostenlosen
    /// Auth-Endpoint, daher kostet der Test ~1 Token (Cent-Bruchteil).
    pub async fn test_connection(&self) -> Result<()> {
        let url = format!("{}/messages", self.base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": "claude-3-haiku-20240307",
            "max_tokens": 1,
            "messages": [{"role": "user", "content": "ok"}],
        });
        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .json(&body)
            .send()
            .await
            .map_err(|e| VoiceTypeError::Processing(format!("HTTP {url}: {e}")))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(VoiceTypeError::Processing(format!(
                "Anthropic HTTP {status}: {body}"
            )));
        }
        Ok(())
    }
}

#[async_trait]
impl Processor for AnthropicProcessor {
    fn name(&self) -> &str {
        "anthropic"
    }

    async fn process(
        &self,
        transcript: &str,
        system_prompt: &str,
        opts: ProcessOpts,
    ) -> Result<String> {
        let model = opts.model.unwrap_or_else(|| self.default_model.clone());
        let url = format!("{}/messages", self.base_url.trim_end_matches('/'));

        let req = MessagesRequest {
            model,
            system: system_prompt.to_string(),
            messages: vec![Message {
                role: "user",
                content: transcript.to_string(),
            }],
            max_tokens: opts.max_tokens.unwrap_or(2048),
            temperature: opts.temperature,
        };

        with_retry(|| async {
            let response = self
                .client
                .post(&url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", API_VERSION)
                .json(&req)
                .send()
                .await
                .map_err(|e| VoiceTypeError::Processing(format!("HTTP {url}: {e}")))?;

            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(VoiceTypeError::Processing(format!(
                    "Anthropic HTTP {status}: {body}"
                )));
            }

            let parsed: MessagesResponse = response
                .json()
                .await
                .map_err(|e| VoiceTypeError::Processing(format!("Anthropic-JSON-Parse: {e}")))?;

            let text = parsed
                .content
                .into_iter()
                .map(|block| match block {
                    ContentBlock::Text { text } => text,
                })
                .collect::<Vec<_>>()
                .join("");

            if text.is_empty() {
                return Err(VoiceTypeError::Processing(
                    "Anthropic-Antwort enthielt keinen Text-Block".into(),
                ));
            }
            Ok(text)
        })
        .await
    }
}

#[derive(Serialize)]
struct MessagesRequest {
    model: String,
    system: String,
    messages: Vec<Message>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Serialize)]
struct Message {
    role: &'static str,
    content: String,
}

#[derive(Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentBlock {
    Text { text: String },
}
