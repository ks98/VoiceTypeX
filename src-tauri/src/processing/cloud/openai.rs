// SPDX-License-Identifier: GPL-3.0-or-later
//! OpenAI GPT via Chat-Completions-API.
//! Base-URL `https://api.openai.com/v1`, Default-Model `gpt-4o-mini`.

use crate::core::error::{ProviderId, Result};
use crate::processing::cloud::openai_compatible::OpenAICompatibleClient;
use crate::processing::{ProcessOpts, Processor};
use async_trait::async_trait;

pub struct OpenAIProcessor {
    inner: OpenAICompatibleClient,
}

impl OpenAIProcessor {
    pub fn new(api_key: String) -> Self {
        Self {
            inner: OpenAICompatibleClient::new(
                ProviderId::OpenAi,
                "https://api.openai.com/v1",
                "gpt-4o-mini",
                api_key,
            ),
        }
    }
}

#[async_trait]
impl Processor for OpenAIProcessor {
    fn name(&self) -> &str {
        "openai"
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
