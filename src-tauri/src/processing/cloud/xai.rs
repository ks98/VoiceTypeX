// SPDX-License-Identifier: GPL-3.0-or-later
//! xAI Grok via OpenAI-kompatible Chat-Completions-API.
//! Base-URL `https://api.x.ai/v1`, Default-Model `grok-4-fast-non-reasoning`
//! (gewaehlt fuer schnelle Diktat-Postprocessing-Tasks: 2 M Context,
//! kein Reasoning-Overhead, ~6x guenstiger als grok-4 — siehe
//! https://x.ai/news/grok-4-fast). Modi koennen das per
//! `cloud_llm_model = "..."` in ihrer TOML ueberschreiben.

use crate::core::error::Result;
use crate::processing::cloud::openai_compatible::OpenAICompatibleClient;
use crate::processing::{ProcessOpts, Processor};
use async_trait::async_trait;

const DEFAULT_MODEL: &str = "grok-4-fast-non-reasoning";

pub struct XaiProcessor {
    inner: OpenAICompatibleClient,
}

impl XaiProcessor {
    pub fn new(api_key: String) -> Self {
        Self {
            inner: OpenAICompatibleClient::new("https://api.x.ai/v1", DEFAULT_MODEL, api_key),
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
