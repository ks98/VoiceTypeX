// SPDX-License-Identifier: GPL-3.0-or-later
//! Geteilter HTTP-Client fuer alle OpenAI-Chat-Completions-kompatiblen
//! Provider (xAI, OpenAI, perspektivisch andere).
//!
//! Der Client wird per Komposition in `XaiProcessor` und `OpenAIProcessor`
//! eingebettet. Phase 1.2: Stub (konstruieren OK, `complete` returnt Err).
//! Phase 2: echte reqwest-Implementation.

use crate::core::error::{Result, VoiceTypeError};
use crate::processing::ProcessOpts;

#[allow(dead_code)]
pub struct OpenAICompatibleClient {
    pub base_url: String,
    pub default_model: String,
    pub api_key: String,
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
        }
    }

    /// Sende eine Chat-Completion mit System+User-Message und gib den
    /// finalen `assistant`-Content zurueck.
    pub async fn complete(
        &self,
        _transcript: &str,
        _system_prompt: &str,
        _opts: ProcessOpts,
    ) -> Result<String> {
        Err(VoiceTypeError::Processing(format!(
            "OpenAI-kompatibler Client (base_url={}) noch nicht implementiert (Phase 2)",
            self.base_url
        )))
    }
}
