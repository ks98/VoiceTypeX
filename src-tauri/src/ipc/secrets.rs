// SPDX-License-Identifier: GPL-3.0-or-later
//! Secrets-IPC.
//!
//! Frontend bekommt **nie** den Klartext-Key zurueck — nur Status-Booleans.
//! Schreiboperationen senden den Key aus dem UI direkt in den OS-Keychain.

use crate::secrets::SecretStore;
use serde::Serialize;

type IpcResult<T> = std::result::Result<T, String>;

/// Provider, fuer die VoiceTypeX einen Keychain-Eintrag verwaltet.
/// xAI nutzt denselben Eintrag fuer STT und LLM (CLAUDE.md §4.4).
const PROVIDERS: &[&str] = &["xai", "openai", "anthropic", "groq", "deepgram"];

#[derive(Serialize)]
pub struct ProviderStatus {
    pub provider: String,
    pub configured: bool,
}

#[tauri::command]
pub async fn get_provider_status() -> IpcResult<Vec<ProviderStatus>> {
    let mut out = Vec::with_capacity(PROVIDERS.len());
    for &provider in PROVIDERS {
        let configured = SecretStore::has(provider).unwrap_or(false);
        out.push(ProviderStatus {
            provider: provider.to_string(),
            configured,
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
