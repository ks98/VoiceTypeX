// SPDX-License-Identifier: GPL-3.0-or-later
//! LLM-Nachbearbeitung: nimmt das rohe Transkript und den modusspezifischen
//! System-Prompt, gibt den finalen Text zurueck.

use crate::core::error::Result;
use async_trait::async_trait;

pub mod cloud;
pub mod local;

use crate::core::error::VoiceTypeError;
use crate::secrets::SecretStore;
use std::sync::Arc;

/// Factory: liefert den passenden Cloud-Processor fuer einen Provider.
/// xAI nutzt denselben Keychain-Eintrag fuer STT und LLM (CLAUDE.md §4.4).
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

/// Factory: liefert den lokalen Ollama-Processor.
/// `keep_alive` ist der Ollama-Duration-String (z.B. `"5m"`, `"0"`, `"-1"`)
/// und wird pro Request mitgeschickt — damit kann der Caller Memory-Pressure
/// pro Aufruf steuern (z.B. `"0"` auf 8-GB-Geraeten fuer sofortiges Unload
/// nach dem Cleanup-Pass).
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
    /// Nucleus-Sampling-Cutoff. `None` = Provider-Default.
    pub top_p: Option<f32>,
    /// Wiederholungs-Penalty (>= 1.0). Werte 1.0-1.1 sind sicher;
    /// hoeher fuehrt zu unnatuerlichen Umformulierungen.
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
