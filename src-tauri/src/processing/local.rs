// SPDX-License-Identifier: GPL-3.0-or-later
//! Lokales LLM via Ollama HTTP API (`POST /api/chat`).
//!
//! Ollama-Default-Endpoint ist `http://127.0.0.1:11434`. Konfigurierbar in
//! Settings (`ollama_url`).

use crate::core::error::{Result, VoiceTypeError};
use crate::processing::{ProcessOpts, Processor};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub struct OllamaProcessor {
    base_url: String,
    default_model: String,
    client: reqwest::Client,
}

impl OllamaProcessor {
    pub fn new(base_url: String, default_model: String) -> Self {
        Self {
            base_url,
            default_model,
            client: reqwest::Client::builder()
                // Lokale Inferenz kann mehr Zeit brauchen als Cloud — 5 min.
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }
}

#[async_trait]
impl Processor for OllamaProcessor {
    fn name(&self) -> &str {
        "ollama"
    }

    async fn process(
        &self,
        transcript: &str,
        system_prompt: &str,
        opts: ProcessOpts,
    ) -> Result<String> {
        let model = opts.model.unwrap_or_else(|| self.default_model.clone());
        let url = format!("{}/api/chat", self.base_url.trim_end_matches('/'));

        let req = OllamaChatRequest {
            model,
            messages: vec![
                OllamaMessage {
                    role: "system",
                    content: system_prompt.to_string(),
                },
                OllamaMessage {
                    role: "user",
                    content: transcript.to_string(),
                },
            ],
            stream: false,
            options: OllamaOptions {
                temperature: opts.temperature,
            },
        };

        let response = self
            .client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| VoiceTypeError::Processing(format!("HTTP {url}: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(VoiceTypeError::Processing(format!(
                "Ollama HTTP {status}: {body}"
            )));
        }

        let parsed: OllamaChatResponse = response
            .json()
            .await
            .map_err(|e| VoiceTypeError::Processing(format!("Ollama-JSON-Parse: {e}")))?;
        Ok(parsed.message.content)
    }
}

#[derive(Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
    options: OllamaOptions,
}

#[derive(Serialize)]
struct OllamaMessage {
    role: &'static str,
    content: String,
}

#[derive(Serialize)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Deserialize)]
struct OllamaChatResponse {
    message: OllamaResponseMessage,
}

#[derive(Deserialize)]
struct OllamaResponseMessage {
    content: String,
}
