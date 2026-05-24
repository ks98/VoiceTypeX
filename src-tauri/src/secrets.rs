// SPDX-License-Identifier: GPL-3.0-or-later
//! BYOK-API-Keys mit Write-Through-/Read-Through-Strategie.
//!
//! **Source of Truth ist die File-Storage** (`~/.config/.../secrets.json`,
//! Mode 0600). Keyring wird **best-effort** zusätzlich beschrieben — falls
//! es funktioniert, profitiert der User vom OS-Keychain-Schutz; wenn
//! nicht, ist die App trotzdem voll funktional.
//!
//! Hintergrund: auf Linux mit zwei konkurrierenden secret-service-Daemons
//! (gnome-keyring + kwallet) liefert keyring nicht-deterministische
//! Ergebnisse — set kann zu Daemon A gehen, get zu Daemon B, der den
//! Eintrag nicht kennt. Die Health-Check-Strategie aus dem vorherigen
//! Commit war nicht ausreichend, weil das Routing zur Laufzeit kippen kann.
//!
//! Lese-Strategie:
//!   1. Versuche Keyring-Read.
//!   2. Wenn NoEntry oder Backend-Error: fall back auf File.
//!   3. Logs zeigen transparent, welcher Pfad die Daten geliefert hat.

use crate::core::error::{Result, VoiceTypeError};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Service-Identifier fuer Keyring-Entries. Plattform-Mapping:
/// - Linux (Secret Service): collection-Attribut `service=voicetypex`.
/// - Windows (Credential Manager): target_name = `<provider>.voicetypex`
///   (Format `<user>.<service>` mit Default-Delimiter ".", siehe
///   windows-native-keyring-store). Relevant fuer
///   `scripts/uninstall-cleanup.ps1`.
/// - macOS (Keychain): out-of-scope.
const SERVICE: &str = "voicetypex";

static FILE_STATE: OnceLock<RwLock<FileSecrets>> = OnceLock::new();
static FILE_PATH: OnceLock<PathBuf> = OnceLock::new();

#[derive(Default, Serialize, Deserialize)]
struct FileSecrets {
    #[serde(flatten)]
    entries: HashMap<String, String>,
}

/// Lade File-Storage. Aufrufen in `lib.rs::run` setup-Closure mit
/// dem app_config_dir. Keyring wird zur Laufzeit best-effort genutzt —
/// kein Health-Check nötig, weil File die Wahrheit ist.
pub fn init_backend(secrets_dir: PathBuf) {
    let file_path = secrets_dir.join("secrets.json");
    let _ = FILE_PATH.set(file_path.clone());

    let initial = load_file_secrets(&file_path).unwrap_or_default();
    let count = initial.entries.len();
    let _ = FILE_STATE.set(RwLock::new(initial));

    tracing::info!(
        file_path = %file_path.display(),
        existing_entries = count,
        "Secrets-File-Storage initialisiert (Source of Truth, Keyring best-effort)"
    );
}

fn load_file_secrets(path: &PathBuf) -> Result<FileSecrets> {
    if !path.exists() {
        return Ok(FileSecrets::default());
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| VoiceTypeError::Secrets(format!("read {path:?}: {e}")))?;
    serde_json::from_str(&content)
        .map_err(|e| VoiceTypeError::Secrets(format!("parse {path:?}: {e}")))
}

fn save_file_secrets(secrets: &FileSecrets) -> Result<()> {
    let path = FILE_PATH
        .get()
        .ok_or_else(|| VoiceTypeError::Secrets("FILE_PATH not initialised".into()))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| VoiceTypeError::Secrets(format!("mkdir {parent:?}: {e}")))?;
    }
    let json = serde_json::to_string_pretty(secrets)
        .map_err(|e| VoiceTypeError::Secrets(format!("serialize: {e}")))?;
    std::fs::write(path, json)
        .map_err(|e| VoiceTypeError::Secrets(format!("write {path:?}: {e}")))?;
    set_file_mode_0600(path)?;
    Ok(())
}

#[cfg(unix)]
fn set_file_mode_0600(path: &PathBuf) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(0o600);
    std::fs::set_permissions(path, perms)
        .map_err(|e| VoiceTypeError::Secrets(format!("chmod 0600 {path:?}: {e}")))
}

#[cfg(not(unix))]
fn set_file_mode_0600(_path: &PathBuf) -> Result<()> {
    Ok(())
}

fn file_state() -> Result<&'static RwLock<FileSecrets>> {
    FILE_STATE
        .get()
        .ok_or_else(|| VoiceTypeError::Secrets("FILE_STATE not initialised".into()))
}

pub struct SecretStore;

impl SecretStore {
    /// Lese-Strategie: erst Keyring versuchen (eventuell schneller +
    /// OS-Keychain-Schutz), dann File-Storage als Fallback.
    pub fn get(provider: &str) -> Result<Option<String>> {
        // 1. Keyring-Read versuchen, Errors als "nicht gefunden" behandeln.
        match keyring_get(provider) {
            Ok(Some(v)) => {
                tracing::info!(provider, len = v.len(), "secret get: Keyring");
                return Ok(Some(v));
            }
            Ok(None) => {
                // NoEntry — fallback auf File
            }
            Err(e) => {
                tracing::warn!(provider, error = %e, "Keyring read failed — falling back to file");
            }
        }

        // 2. File-Read als Source of Truth.
        let state = file_state()?.read();
        let v = state.entries.get(provider).cloned();
        tracing::info!(
            provider,
            found = v.is_some(),
            "secret get: File ({})",
            if v.is_some() {
                "gefunden"
            } else {
                "nicht vorhanden"
            }
        );
        Ok(v)
    }

    /// Schreib-Strategie: File **immer** (Source of Truth), Keyring
    /// best-effort. Wenn Keyring fehlschlaegt, kein Error nach aussen —
    /// File ist erfolgreich, das reicht funktional.
    pub fn set(provider: &str, value: &str) -> Result<()> {
        // 1. File schreiben (Source of Truth, muss klappen).
        {
            let state = file_state()?;
            let mut guard = state.write();
            guard
                .entries
                .insert(provider.to_string(), value.to_string());
            save_file_secrets(&guard)?;
        }
        tracing::info!(provider, len = value.len(), "secret set: File ok");

        // 2. Keyring best-effort.
        match keyring_set(provider, value) {
            Ok(()) => tracing::info!(provider, "secret set: Keyring ok"),
            Err(e) => {
                tracing::warn!(provider, error = %e, "Keyring set failed (irrelevant — file is source of truth)")
            }
        }
        Ok(())
    }

    /// Loesche aus beiden Speichern.
    pub fn delete(provider: &str) -> Result<()> {
        {
            let state = file_state()?;
            let mut guard = state.write();
            guard.entries.remove(provider);
            save_file_secrets(&guard)?;
        }
        tracing::info!(provider, "secret delete: File ok");

        if let Err(e) = keyring_delete(provider) {
            tracing::warn!(provider, error = %e, "Keyring delete failed (irrelevant)");
        }
        Ok(())
    }

    pub fn has(provider: &str) -> Result<bool> {
        Ok(Self::get(provider)?.is_some())
    }
}

// --- Keyring backend (best-effort, kein Source of Truth mehr) ---

fn keyring_get(provider: &str) -> Result<Option<String>> {
    let entry = keyring::Entry::new(SERVICE, provider)
        .map_err(|e| VoiceTypeError::Secrets(format!("Entry({provider}): {e}")))?;
    match entry.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(VoiceTypeError::Secrets(format!("get({provider}): {e}"))),
    }
}

fn keyring_set(provider: &str, value: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, provider)
        .map_err(|e| VoiceTypeError::Secrets(format!("Entry({provider}): {e}")))?;
    entry
        .set_password(value)
        .map_err(|e| VoiceTypeError::Secrets(format!("set({provider}): {e}")))
}

fn keyring_delete(provider: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, provider)
        .map_err(|e| VoiceTypeError::Secrets(format!("Entry({provider}): {e}")))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(VoiceTypeError::Secrets(format!("delete({provider}): {e}"))),
    }
}
