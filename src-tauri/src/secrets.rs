// SPDX-License-Identifier: GPL-3.0-or-later
//! BYOK-API-Keys im OS-Keychain.
//!
//! Service-Name `voicetypex` plus pro Provider ein Key-Eintrag (`xai`,
//! `openai`, `anthropic`, `groq`, `deepgram`). xAI nutzt **denselben**
//! Eintrag fuer STT und LLM (CLAUDE.md §4.4).
//!
//! WICHTIG: Diese Funktionen sollen aus IPC-Commands aufgerufen werden, **die
//! nur den Provider-Namen ans Frontend zurueckgeben** — nie den API-Key
//! selbst. Settings-UI zeigt maskierte Werte ("xai-…1234") durch separat
//! gespeicherte Hash-Praefixe oder einfach "✓ gesetzt".

use crate::core::error::{Result, VoiceTypeError};

const SERVICE: &str = "voicetypex";

pub struct SecretStore;

impl SecretStore {
    /// Lese den Key fuer einen Provider. `Ok(None)` bedeutet "kein Eintrag",
    /// nicht "Fehler" — das Frontend nutzt das fuer "API-Key noch nicht gesetzt".
    pub fn get(provider: &str) -> Result<Option<String>> {
        let entry = keyring::Entry::new(SERVICE, provider)
            .map_err(|e| VoiceTypeError::Secrets(format!("Entry({provider}): {e}")))?;
        match entry.get_password() {
            Ok(p) => Ok(Some(p)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(VoiceTypeError::Secrets(format!("get({provider}): {e}"))),
        }
    }

    pub fn set(provider: &str, value: &str) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE, provider)
            .map_err(|e| VoiceTypeError::Secrets(format!("Entry({provider}): {e}")))?;
        entry
            .set_password(value)
            .map_err(|e| VoiceTypeError::Secrets(format!("set({provider}): {e}")))?;
        Ok(())
    }

    pub fn delete(provider: &str) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE, provider)
            .map_err(|e| VoiceTypeError::Secrets(format!("Entry({provider}): {e}")))?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(VoiceTypeError::Secrets(format!("delete({provider}): {e}"))),
        }
    }

    pub fn has(provider: &str) -> Result<bool> {
        Ok(Self::get(provider)?.is_some())
    }
}
