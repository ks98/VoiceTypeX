// SPDX-License-Identifier: GPL-3.0-or-later
//! BYOK-API-Keys mit Auto-Backend-Wahl.
//!
//! Beim App-Start macht `init_backend(...)` einen Round-Trip-Test gegen
//! den OS-Keychain (`keyring`-Crate). Wenn write+read+delete erfolgreich
//! ist und der gelesene Wert dem geschriebenen entspricht, nutzen wir
//! Keyring. Sonst (Linux ohne secret-service-Daemon, defekte DBus-
//! Session, etc.) fallen wir automatisch auf einen file-basierten Storage
//! zurueck (`~/.config/.../secrets.json`, mode 0600).
//!
//! Beide Backends erfuellen denselben SecretStore-Vertrag. Der User merkt
//! nichts vom Wechsel — die Logs zeigen aber transparent welches aktiv ist.

use crate::core::error::{Result, VoiceTypeError};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

const SERVICE: &str = "voicetypex";
const HEALTH_CHECK_PROVIDER: &str = "__voicetypex_health_check__";
const HEALTH_CHECK_VALUE: &str = "vtx-health-check-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretBackend {
    Keyring,
    File,
}

static BACKEND: OnceLock<SecretBackend> = OnceLock::new();
static FILE_STATE: OnceLock<RwLock<FileSecrets>> = OnceLock::new();
static FILE_PATH: OnceLock<PathBuf> = OnceLock::new();

#[derive(Default, Serialize, Deserialize)]
struct FileSecrets {
    #[serde(flatten)]
    entries: HashMap<String, String>,
}

/// Bestimme zur Laufzeit, welches Backend zuverlaessig funktioniert.
/// Aufrufen in `lib.rs::run` setup-Closure mit dem app_config_dir.
pub fn init_backend(secrets_dir: PathBuf) -> SecretBackend {
    let file_path = secrets_dir.join("secrets.json");
    let _ = FILE_PATH.set(file_path.clone());

    // File-State immer initialisieren, damit es bei Bedarf bereit ist.
    let initial = load_file_secrets(&file_path).unwrap_or_default();
    let _ = FILE_STATE.set(RwLock::new(initial));

    // Round-Trip-Test gegen Keyring.
    let keyring_works = run_health_check();

    let chosen = if keyring_works {
        SecretBackend::Keyring
    } else {
        SecretBackend::File
    };
    let _ = BACKEND.set(chosen);
    tracing::info!(
        backend = ?chosen,
        keyring_health = keyring_works,
        file_path = %file_path.display(),
        "Secret-Storage-Backend gewaehlt"
    );
    chosen
}

fn run_health_check() -> bool {
    let entry = match keyring::Entry::new(SERVICE, HEALTH_CHECK_PROVIDER) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(error = %e, "keyring::Entry::new fehlgeschlagen — File-Fallback");
            return false;
        }
    };
    if let Err(e) = entry.set_password(HEALTH_CHECK_VALUE) {
        tracing::warn!(error = %e, "keyring::set_password fehlgeschlagen — File-Fallback");
        return false;
    }
    let got = match entry.get_password() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(
                error = %e,
                "keyring::get_password nach erfolgreichem Set fehlgeschlagen — File-Fallback"
            );
            return false;
        }
    };
    let _ = entry.delete_credential();
    if got != HEALTH_CHECK_VALUE {
        tracing::warn!(
            expected = HEALTH_CHECK_VALUE,
            got = %got,
            "keyring Round-Trip-Mismatch — File-Fallback"
        );
        return false;
    }
    true
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
        .ok_or_else(|| VoiceTypeError::Secrets("FILE_PATH nicht initialisiert".into()))?;
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
    // Auf Windows ist die Datei standardmaessig nur fuer den User lesbar
    // (NTFS ACL geerbt von Profile-Dir). Kein expliziter chmod noetig.
    Ok(())
}

fn current_backend() -> SecretBackend {
    BACKEND.get().copied().unwrap_or(SecretBackend::Keyring)
}

pub struct SecretStore;

impl SecretStore {
    pub fn get(provider: &str) -> Result<Option<String>> {
        match current_backend() {
            SecretBackend::Keyring => keyring_get(provider),
            SecretBackend::File => file_get(provider),
        }
    }

    pub fn set(provider: &str, value: &str) -> Result<()> {
        match current_backend() {
            SecretBackend::Keyring => keyring_set(provider, value),
            SecretBackend::File => file_set(provider, value),
        }
    }

    pub fn delete(provider: &str) -> Result<()> {
        match current_backend() {
            SecretBackend::Keyring => keyring_delete(provider),
            SecretBackend::File => file_delete(provider),
        }
    }

    pub fn has(provider: &str) -> Result<bool> {
        Ok(Self::get(provider)?.is_some())
    }

    pub fn active_backend() -> SecretBackend {
        current_backend()
    }
}

// --- Keyring backend ---

fn keyring_get(provider: &str) -> Result<Option<String>> {
    let entry = keyring::Entry::new(SERVICE, provider).map_err(|e| {
        tracing::error!(provider, error = %e, "keyring::Entry::new fehlgeschlagen");
        VoiceTypeError::Secrets(format!("Entry({provider}): {e}"))
    })?;
    match entry.get_password() {
        Ok(p) => {
            tracing::info!(provider, len = p.len(), "secret get (keyring): gefunden");
            Ok(Some(p))
        }
        Err(keyring::Error::NoEntry) => {
            tracing::info!(provider, "secret get (keyring): NoEntry");
            Ok(None)
        }
        Err(e) => {
            tracing::error!(provider, error = %e, "secret get (keyring): Backend-Fehler");
            Err(VoiceTypeError::Secrets(format!("get({provider}): {e}")))
        }
    }
}

fn keyring_set(provider: &str, value: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, provider)
        .map_err(|e| VoiceTypeError::Secrets(format!("Entry({provider}): {e}")))?;
    entry
        .set_password(value)
        .map_err(|e| VoiceTypeError::Secrets(format!("set({provider}): {e}")))?;
    tracing::info!(
        provider,
        len = value.len(),
        "secret set (keyring): gespeichert"
    );
    Ok(())
}

fn keyring_delete(provider: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, provider)
        .map_err(|e| VoiceTypeError::Secrets(format!("Entry({provider}): {e}")))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {
            tracing::info!(provider, "secret delete (keyring): ok");
            Ok(())
        }
        Err(e) => Err(VoiceTypeError::Secrets(format!("delete({provider}): {e}"))),
    }
}

// --- File backend ---

fn file_state() -> Result<&'static RwLock<FileSecrets>> {
    FILE_STATE
        .get()
        .ok_or_else(|| VoiceTypeError::Secrets("FILE_STATE nicht initialisiert".into()))
}

fn file_get(provider: &str) -> Result<Option<String>> {
    let state = file_state()?.read();
    let v = state.entries.get(provider).cloned();
    tracing::info!(
        provider,
        found = v.is_some(),
        "secret get (file): {}",
        if v.is_some() {
            "gefunden"
        } else {
            "nicht vorhanden"
        }
    );
    Ok(v)
}

fn file_set(provider: &str, value: &str) -> Result<()> {
    let state = file_state()?;
    {
        let mut guard = state.write();
        guard
            .entries
            .insert(provider.to_string(), value.to_string());
        save_file_secrets(&guard)?;
    }
    tracing::info!(
        provider,
        len = value.len(),
        "secret set (file): gespeichert"
    );
    Ok(())
}

fn file_delete(provider: &str) -> Result<()> {
    let state = file_state()?;
    {
        let mut guard = state.write();
        guard.entries.remove(provider);
        save_file_secrets(&guard)?;
    }
    tracing::info!(provider, "secret delete (file): ok");
    Ok(())
}
