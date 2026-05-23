// SPDX-License-Identifier: GPL-3.0-or-later
//! Secrets-IPC.
//!
//! Frontend bekommt **nie** den Klartext-Key zurueck — nur Status-Booleans.
//! Schreiboperationen senden den Key aus dem UI direkt in den OS-Keychain.

use crate::processing::cloud::anthropic::AnthropicProcessor;
use crate::processing::cloud::openai_compatible::OpenAICompatibleClient;
use crate::secrets::SecretStore;
use crate::transcription::cloud::deepgram::DeepgramTranscriber;
use serde::Serialize;

type IpcResult<T> = std::result::Result<T, String>;

/// Provider, fuer die VoiceTypeX einen Keychain-Eintrag verwaltet.
/// xAI nutzt denselben Eintrag fuer STT und LLM (CLAUDE.md §4.4).
pub(crate) const PROVIDERS: &[&str] = &["xai", "openai", "anthropic", "groq", "deepgram"];

#[derive(Serialize)]
pub struct ProviderStatus {
    pub provider: String,
    pub configured: bool,
    /// Wenn das Keychain-Backend einen Fehler liefert (z.B. kein
    /// secret-service-Daemon auf Linux), wird der Fehler hier exposed —
    /// das Frontend kann dem User dann eine konkrete Diagnose zeigen,
    /// statt stillschweigend "nicht gesetzt" anzuzeigen.
    pub error: Option<String>,
}

#[tauri::command]
pub async fn get_provider_status() -> IpcResult<Vec<ProviderStatus>> {
    let mut out = Vec::with_capacity(PROVIDERS.len());
    for &provider in PROVIDERS {
        let (configured, error) = match SecretStore::has(provider) {
            Ok(b) => (b, None),
            Err(e) => {
                tracing::warn!(provider, error = %e, "Keychain-Backend-Fehler bei has()");
                (false, Some(e.to_string()))
            }
        };
        out.push(ProviderStatus {
            provider: provider.to_string(),
            configured,
            error,
        });
    }
    Ok(out)
}

#[tauri::command]
pub async fn set_provider_key(provider: String, key: String) -> IpcResult<()> {
    if !PROVIDERS.contains(&provider.as_str()) {
        return Err(format!("Unbekannter Provider: {provider}"));
    }
    if key.trim().is_empty() {
        return Err("API-Key darf nicht leer sein".into());
    }
    SecretStore::set(&provider, key.trim()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_provider_key(provider: String) -> IpcResult<()> {
    if !PROVIDERS.contains(&provider.as_str()) {
        return Err(format!("Unbekannter Provider: {provider}"));
    }
    SecretStore::delete(&provider).map_err(|e| e.to_string())
}

/// Pruefe Provider-Verbindung mit dem aktuell gespeicherten API-Key.
/// Provider-spezifische Endpoints; xAI/OpenAI/Groq teilen den OpenAI-
/// kompatiblen `GET /models`-Test. Anthropic/Deepgram folgen in Phase 2.5+.
#[tauri::command]
pub async fn test_provider_connection(provider: String) -> IpcResult<()> {
    let key = SecretStore::get(&provider)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Kein API-Key fuer '{provider}' gesetzt"))?;

    match provider.as_str() {
        "xai" => {
            OpenAICompatibleClient::new("https://api.x.ai/v1", "grok-4-fast-non-reasoning", key)
                .test_connection()
                .await
                .map_err(|e| e.to_string())
        }
        "openai" => OpenAICompatibleClient::new("https://api.openai.com/v1", "gpt-4o-mini", key)
            .test_connection()
            .await
            .map_err(|e| e.to_string()),
        "groq" => OpenAICompatibleClient::new(
            "https://api.groq.com/openai/v1",
            "whisper-large-v3-turbo",
            key,
        )
        .test_connection()
        .await
        .map_err(|e| e.to_string()),
        "anthropic" => AnthropicProcessor::new(key)
            .test_connection()
            .await
            .map_err(|e| e.to_string()),
        "deepgram" => DeepgramTranscriber::new(key)
            .test_connection()
            .await
            .map_err(|e| e.to_string()),
        other => Err(format!("Unbekannter Provider: {other}")),
    }
}
