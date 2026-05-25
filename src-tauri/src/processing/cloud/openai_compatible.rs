// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared HTTP client for all OpenAI-Chat-Completions-compatible
//! providers (xAI, OpenAI, prospectively others like Together,
//! Mistral, …).
//!
//! API requirements:
//!   POST {base_url}/chat/completions
//!   Authorization: Bearer {api_key}
//!   Body: { model, messages: [system, user], temperature?, max_tokens? }
//!   Response: { choices: [{ message: { content } }] }

use crate::core::error::{Result, VoiceTypeError};
use crate::core::retry::with_retry;
use crate::processing::ProcessOpts;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct OpenAICompatibleClient {
    pub base_url: String,
    pub default_model: String,
    pub api_key: String,
    client: reqwest::Client,
}

impl OpenAICompatibleClient {
    pub fn new(
        base_url: impl Into<String>,
        default_model: impl Into<String>,
        api_key: String,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            default_model: default_model.into(),
            api_key,
            // A 60 s timeout covers almost all chat-completion calls.
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .expect("reqwest client builder (timeout)"),
        }
    }

    /// Send a chat completion with system + user message and return
    /// the final `assistant` content. Retries on transient errors
    /// (5xx, 429, network) with exponential backoff.
    pub async fn complete(
        &self,
        transcript: &str,
        system_prompt: &str,
        opts: ProcessOpts,
    ) -> Result<String> {
        let model = opts.model.unwrap_or_else(|| self.default_model.clone());
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let req = ChatCompletionRequest {
            model,
            messages: vec![
                ChatMessage {
                    role: "system",
                    content: system_prompt.to_string(),
                },
                ChatMessage {
                    role: "user",
                    content: transcript.to_string(),
                },
            ],
            temperature: opts.temperature,
            max_tokens: opts.max_tokens,
        };

        with_retry(|| async {
            let response = self
                .client
                .post(&url)
                .bearer_auth(&self.api_key)
                .json(&req)
                .send()
                .await
                .map_err(|e| VoiceTypeError::Processing(format!("HTTP {url}: {e}")))?;

            let status = response.status();
            if !status.is_success() {
                tracing::warn!(provider = "openai_compatible", %status, "process call failed");
                return Err(VoiceTypeError::Processing(format!("HTTP {status}")));
            }

            let parsed: ChatCompletionResponse = response
                .json()
                .await
                .map_err(|e| VoiceTypeError::Processing(format!("Response-JSON-Parse: {e}")))?;

            parsed
                .choices
                .into_iter()
                .next()
                .map(|c| c.message.content)
                .ok_or_else(|| VoiceTypeError::Processing("Keine choices in Response".into()))
        })
        .await
    }

    /// Check connectivity and auth via `GET /models` — the cheapest
    /// endpoint that OpenAI-compatible providers support.
    pub async fn test_connection(&self) -> Result<()> {
        let url = format!("{}/models", self.base_url.trim_end_matches('/'));
        let response = self
            .client
            .get(&url)
            .bearer_auth(&self.api_key)
            .send()
            .await
            .map_err(|e| VoiceTypeError::Processing(format!("HTTP {url}: {e}")))?;
        let status = response.status();
        if !status.is_success() {
            tracing::warn!(provider = "openai_compatible", %status, "test_connection failed");
            return Err(VoiceTypeError::Processing(format!("HTTP {status}")));
        }
        Ok(())
    }
}

#[derive(Serialize, Clone)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Serialize, Clone)]
struct ChatMessage {
    role: &'static str,
    content: String,
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Deserialize)]
struct ResponseMessage {
    content: String,
}
