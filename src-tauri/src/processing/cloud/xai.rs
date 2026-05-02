// SPDX-License-Identifier: GPL-3.0-or-later
//! xAI Grok via OpenAI-kompatible Chat-Completions-API.
//! Base-URL `https://api.x.ai/v1`, Default-Model `grok-4`.

use crate::core::error::Result;
use crate::processing::cloud::openai_compatible::OpenAICompatibleClient;
use crate::processing::{ProcessOpts, Processor};
use async_trait::async_trait;

pub struct XaiProcessor {
    inner: OpenAICompatibleClient,
}

impl XaiProcessor {
    pub fn new(api_key: String) -> Self {
        Self {
            inner: OpenAICompatibleClient::new("https://api.x.ai/v1", "grok-4", api_key),
        }
    }
}

#[async_trait]
impl Processor for XaiProcessor {
    fn name(&self) -> &str {
        "xai"
    }

    async fn process(
        &self,
        transcript: &str,
        system_prompt: &str,
        opts: ProcessOpts,
    ) -> Result<String> {
        self.inner.complete(transcript, system_prompt, opts).await
    }
}
