// SPDX-License-Identifier: GPL-3.0-or-later
//! Local LLM via the Ollama HTTP API (`POST /api/chat`).
//!
//! The default Ollama endpoint is `http://127.0.0.1:11434`.
//! Configurable in settings (`ollama_url`).
//!
//! Sampling parameters (temperature, top_p, repeat_penalty) come from
//! the `Mode` TOML, forwarded via `ProcessOpts`. `keep_alive` is
//! Ollama-specific and comes from `Settings.ollama_keep_alive`
//! (default `"5m"`, `"0"` for immediate unload after every call on
//! memory-pressure profiles).

use crate::core::error::{Result, VoiceTypeError};
use crate::processing::{ProcessOpts, Processor};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub struct OllamaProcessor {
    base_url: String,
    default_model: String,
    keep_alive: String,
    client: reqwest::Client,
}

impl OllamaProcessor {
    pub fn new(base_url: String, default_model: String, keep_alive: String) -> Self {
        Self {
            base_url,
            default_model,
            keep_alive,
            client: reqwest::Client::builder()
                // Local inference can take longer than cloud — 5 min.
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .expect("reqwest client builder (timeout)"),
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
            keep_alive: self.keep_alive.clone(),
            options: OllamaOptions {
                temperature: opts.temperature,
                top_p: opts.top_p,
                repeat_penalty: opts.repeat_penalty,
            },
        };

        let response = self
            .client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| VoiceTypeError::processing(format!("HTTP {url}: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            tracing::warn!(provider = "ollama", %status, "process call failed");
            return Err(VoiceTypeError::processing(format!("Ollama HTTP {status}")));
        }

        let parsed: OllamaChatResponse = response
            .json()
            .await
            .map_err(|e| VoiceTypeError::processing(format!("Ollama-JSON-Parse: {e}")))?;
        Ok(parsed.message.content)
    }
}

#[derive(Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
    /// Ollama duration string: `"5m"`, `"0"`, `"-1"`, etc. Controls
    /// how long the model stays in RAM/VRAM after this call.
    keep_alive: String,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    repeat_penalty: Option<f32>,
}

#[derive(Deserialize)]
struct OllamaChatResponse {
    message: OllamaResponseMessage,
}

#[derive(Deserialize)]
struct OllamaResponseMessage {
    content: String,
}
