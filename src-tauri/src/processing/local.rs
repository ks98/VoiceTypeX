// SPDX-License-Identifier: GPL-3.0-or-later
//! Lokales LLM via Ollama HTTP API (`POST /api/chat`).
//!
//! Phase 1.2: Stub. Phase 1.4 (oder 2) implementiert reqwest-Call gegen
//! `${ollama_url}/api/chat` mit `messages: [system, user]`.

use crate::core::error::{Result, VoiceTypeError};
use crate::processing::{ProcessOpts, Processor};
use async_trait::async_trait;

#[allow(dead_code)]
pub struct OllamaProcessor {
    base_url: String,
    default_model: String,
}

impl OllamaProcessor {
    pub fn new(base_url: String, default_model: String) -> Self {
        Self {
            base_url,
            default_model,
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
        _transcript: &str,
        _system_prompt: &str,
        _opts: ProcessOpts,
    ) -> Result<String> {
        Err(VoiceTypeError::Processing(
            "Ollama-Client noch nicht implementiert (Phase 1.4)".into(),
        ))
    }
}
