// SPDX-License-Identifier: GPL-3.0-or-later
//! Secrets IPC.
//!
//! The frontend **never** receives the cleartext key — only status
//! booleans. Write operations send the key from the UI directly into
//! the OS keychain.

use crate::core::app_context::AppContext;
use crate::core::error::ProviderId;
use crate::processing::cloud::anthropic::AnthropicProcessor;
use crate::processing::cloud::openai_compatible::OpenAICompatibleClient;
use crate::secrets::SecretStore;
use crate::transcription::cloud::deepgram::DeepgramTranscriber;
use serde::Serialize;
use std::sync::Arc;

type IpcResult<T> = std::result::Result<T, String>;

/// Providers for which VoiceTypeX manages a keychain entry. xAI uses
/// the same entry for STT and LLM (CLAUDE.md §4.4).
pub(crate) const PROVIDERS: &[&str] = &["xai", "openai", "anthropic", "groq", "deepgram"];

#[derive(Serialize)]
pub struct ProviderStatus {
    pub provider: String,
    pub configured: bool,
    /// If the keychain backend reports an error (e.g. no
    /// secret-service daemon on Linux), it is exposed here — the
    /// frontend can then show the user a concrete diagnosis instead
    /// of silently displaying "not set".
    pub error: Option<String>,
}

/// True when the secret store can encrypt at rest (Windows DPAPI or
/// Linux AES-256-GCM with a keyring-resident KEK). False when running
/// in the plain fallback, in which case the frontend renders a warning.
#[tauri::command]
pub async fn is_secrets_encrypted_at_rest() -> IpcResult<bool> {
    Ok(crate::secrets::is_encrypted_at_rest().unwrap_or(false))
}

#[tauri::command]
pub async fn get_provider_status() -> IpcResult<Vec<ProviderStatus>> {
    let mut out = Vec::with_capacity(PROVIDERS.len());
    for &provider in PROVIDERS {
        let (configured, error) = match SecretStore::has(provider) {
            Ok(b) => (b, None),
            Err(e) => {
                tracing::warn!(provider, error = %e, "Keychain backend error on has()");
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
pub async fn set_provider_key(
    state: tauri::State<'_, Arc<AppContext>>,
    provider: String,
    key: String,
) -> IpcResult<()> {
    if !PROVIDERS.contains(&provider.as_str()) {
        return Err(format!("Unknown provider: {provider}"));
    }
    if key.trim().is_empty() {
        return Err("API key must not be empty".into());
    }
    SecretStore::set(&provider, key.trim()).map_err(|e| e.to_string())?;
    // Drop the cached cloud transcriber/processor for this provider so
    // the next dictation rebuilds them with the new key (issue #42) — a
    // stale client must never outlive a key change.
    state.invalidate_cloud_provider(&provider);
    Ok(())
}

#[tauri::command]
pub async fn delete_provider_key(
    state: tauri::State<'_, Arc<AppContext>>,
    provider: String,
) -> IpcResult<()> {
    if !PROVIDERS.contains(&provider.as_str()) {
        return Err(format!("Unknown provider: {provider}"));
    }
    SecretStore::delete(&provider).map_err(|e| e.to_string())?;
    // Drop the cached cloud client for this provider (issue #42): the
    // next dictation rebuilds and surfaces the "no key set" error
    // instead of silently using the deleted key's cached wrapper.
    state.invalidate_cloud_provider(&provider);
    Ok(())
}

/// Check provider connectivity with the currently stored API key.
/// Provider-specific endpoints; xAI/OpenAI/Groq share the
/// OpenAI-compatible `GET /models` test. Anthropic/Deepgram follow
/// in phase 2.5+.
#[tauri::command]
pub async fn test_provider_connection(
    state: tauri::State<'_, Arc<AppContext>>,
    provider: String,
) -> IpcResult<()> {
    let key = SecretStore::get(&provider)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("No API key set for '{provider}'"))?;

    // Reuse the app-wide shared HTTP client (issue #41) so the test
    // call warms the same connection pool a later dictation uses.
    let client = state.http_client.clone();

    match provider.as_str() {
        "xai" => OpenAICompatibleClient::new(
            ProviderId::Xai,
            "https://api.x.ai/v1",
            "grok-4-fast-non-reasoning",
            key,
            client,
        )
        .test_connection()
        .await
        .map_err(|e| e.to_string()),
        "openai" => OpenAICompatibleClient::new(
            ProviderId::OpenAi,
            "https://api.openai.com/v1",
            "gpt-4o-mini",
            key,
            client,
        )
        .test_connection()
        .await
        .map_err(|e| e.to_string()),
        "groq" => OpenAICompatibleClient::new(
            ProviderId::Groq,
            "https://api.groq.com/openai/v1",
            "whisper-large-v3-turbo",
            key,
            client,
        )
        .test_connection()
        .await
        .map_err(|e| e.to_string()),
        "anthropic" => AnthropicProcessor::new(key, client)
            .test_connection()
            .await
            .map_err(|e| e.to_string()),
        "deepgram" => DeepgramTranscriber::new(key, client)
            .test_connection()
            .await
            .map_err(|e| e.to_string()),
        other => Err(format!("Unknown provider: {other}")),
    }
}
