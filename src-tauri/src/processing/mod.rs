// SPDX-License-Identifier: GPL-3.0-or-later
//! LLM-Nachbearbeitung: nimmt das rohe Transkript und den modusspezifischen
//! System-Prompt, gibt den finalen Text zurueck.

use crate::core::error::Result;
use async_trait::async_trait;

pub mod cloud;
pub mod local;

#[derive(Debug, Clone, Default)]
pub struct ProcessOpts {
    pub model: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub language: Option<String>,
}

#[async_trait]
pub trait Processor: Send + Sync {
    fn name(&self) -> &str;

    async fn process(
        &self,
        transcript: &str,
        system_prompt: &str,
        opts: ProcessOpts,
    ) -> Result<String>;
}
