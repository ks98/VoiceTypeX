// SPDX-License-Identifier: GPL-3.0-or-later
//! Anthropic Claude via Messages-API — eigene Konventionen
//! (System-Prompt-Feld, Content-Blocks). Daher keine Wiederverwendung von
//! `OpenAICompatibleClient`.

use crate::core::error::{Result, VoiceTypeError};
use crate::processing::{ProcessOpts, Processor};
use async_trait::async_trait;

#[allow(dead_code)]
pub struct AnthropicProcessor {
    api_key: String,
    base_url: String,
    default_model: String,
}

impl AnthropicProcessor {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://api.anthropic.com/v1".to_string(),
            default_model: "claude-sonnet-4-6".to_string(),
        }
    }
}

#[async_trait]
impl Processor for AnthropicProcessor {
    fn name(&self) -> &str {
        "anthropic"
    }

    async fn process(
        &self,
        _transcript: &str,
        _system_prompt: &str,
        _opts: ProcessOpts,
    ) -> Result<String> {
        Err(VoiceTypeError::Processing(
            "Anthropic Messages-API noch nicht implementiert (Phase 2.5)".into(),
        ))
    }
}
