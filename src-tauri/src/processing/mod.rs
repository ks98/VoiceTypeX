// SPDX-License-Identifier: GPL-3.0-or-later
//! LLM post-processing: takes the raw transcript and the
//! mode-specific system prompt, returns the final text.

use crate::core::error::Result;
use async_trait::async_trait;

pub mod cloud;
// Embedded LLM (llama-cpp-2) is Linux/macOS-only — it collides with
// whisper's ggml at link time on Windows/MSVC (issue #1).
#[cfg(not(target_os = "windows"))]
pub mod embedded;
pub mod local;

use crate::core::error::VoiceTypeError;
use crate::secrets::SecretStore;
use std::sync::Arc;

/// Factory: returns the matching cloud processor for a provider. xAI
/// uses the same keychain entry for STT and LLM (CLAUDE.md §4.4).
pub fn make_cloud_processor(provider: &str) -> Result<Arc<dyn Processor>> {
    let key = SecretStore::get(provider)?.ok_or_else(|| {
        VoiceTypeError::Processing(format!(
            "API-Key fuer Provider '{provider}' nicht gesetzt — bitte in den Einstellungen hinterlegen"
        ))
    })?;
    match provider {
        "xai" => Ok(Arc::new(cloud::xai::XaiProcessor::new(key))),
        "openai" => Ok(Arc::new(cloud::openai::OpenAIProcessor::new(key))),
        "anthropic" => Ok(Arc::new(cloud::anthropic::AnthropicProcessor::new(key))),
        other => Err(VoiceTypeError::Processing(format!(
            "Unbekannter LLM-Provider: {other}"
        ))),
    }
}

/// Factory: returns the local Ollama processor. `keep_alive` is the
/// Ollama duration string (e.g. `"5m"`, `"0"`, `"-1"`) and is sent
/// with every request — so the caller can drive memory pressure per
/// call (e.g. `"0"` on 8 GB devices for immediate unload after the
/// cleanup pass).
pub fn make_local_processor(
    ollama_url: String,
    default_model: String,
    keep_alive: String,
) -> Arc<dyn Processor> {
    Arc::new(local::OllamaProcessor::new(
        ollama_url,
        default_model,
        keep_alive,
    ))
}

#[derive(Debug, Clone, Default)]
pub struct ProcessOpts {
    pub model: Option<String>,
    pub temperature: Option<f32>,
    /// Nucleus-sampling cutoff. `None` = provider default.
    pub top_p: Option<f32>,
    /// Repetition penalty (>= 1.0). Values 1.0-1.1 are safe; higher
    /// leads to unnatural rephrasings.
    pub repeat_penalty: Option<f32>,
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
