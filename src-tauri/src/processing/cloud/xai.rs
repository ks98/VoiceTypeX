// SPDX-License-Identifier: GPL-3.0-or-later
//! xAI Grok via the OpenAI-compatible Chat-Completions API.
//! Base URL `https://api.x.ai/v1`, default model
//! `grok-4-fast-non-reasoning` (chosen for fast dictation
//! post-processing: 2 M context, no reasoning overhead, ~6x cheaper
//! than grok-4 — see https://x.ai/news/grok-4-fast). Modes can
//! override this via `cloud_llm_model = "..."` in their TOML.

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
