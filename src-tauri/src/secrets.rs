// SPDX-License-Identifier: GPL-3.0-or-later
//! BYOK API-Keys with encryption-at-rest.
//!
//! Format on disk (`~/.config/.../secrets.json`, chmod 0600):
//! ```json
//! {
//!   "version": 2,
//!   "method": "aes-256-gcm" | "dpapi" | "plain",
//!   "ciphertext": "<base64>"          // method != plain
//!   "plaintext_entries": { ... }      // method == plain
//! }
//! ```
//!
//! Pre-update files (no `version` key, just a flat `{ provider: key }` map)
//! are detected on load and migrated to v2 with the currently active cipher.
//!
//! Platform-specific cipher selection:
//! - **Linux**: AES-256-GCM with a 32-byte random KEK stored in the OS
//!   keyring (libsecret / kwallet). If no keyring is available the storage
//!   falls back to plaintext and the frontend renders a red banner — see
//!   `is_secrets_encrypted_at_rest()`.
//! - **Windows**: DPAPI via `CryptProtectData` / `CryptUnprotectData`.
//!   User-bound and machine-bound; no fallback needed because DPAPI is
//!   always available on supported Windows versions.
//! - **macOS**: out of scope for the beta — falls back to plaintext with
//!   a FIXME pointing at `Security.framework`/`kSecAttrAccessibleWhenUnlocked`.

use crate::core::error::{Result, VoiceTypeError};
use aes_gcm::aead::rand_core::RngCore;
use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Service identifier for keyring entries. Platform mapping:
/// - Linux (Secret Service): collection attribute `service=voicetypex`.
/// - Windows (Credential Manager): `target_name = "<provider>.voicetypex"`.
///   Relevant for `scripts/uninstall-cleanup.ps1`.
/// - macOS (Keychain): out of scope.
const SERVICE: &str = "voicetypex";

/// Keyring entry name under which the AES-256-GCM key-encryption-key
/// lives. Versioned so a future rotation does not collide with old KEKs.
#[cfg(target_os = "linux")]
const KEK_KEY: &str = "_kek_v1";

const FILE_VERSION_CURRENT: u32 = 2;

static FILE_STATE: OnceLock<RwLock<FileSecrets>> = OnceLock::new();
static FILE_PATH: OnceLock<PathBuf> = OnceLock::new();
static CIPHER: OnceLock<Cipher> = OnceLock::new();

/// Current in-memory representation of all stored secrets.
#[derive(Default)]
struct FileSecrets {
    entries: HashMap<String, String>,
}

/// On-disk format v2.
#[derive(Serialize, Deserialize)]
struct FileSecretsV2 {
    version: u32,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ciphertext: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    plaintext_entries: Option<HashMap<String, String>>,
}

/// Encryption strategy. Selected once at startup and stored in `CIPHER`.
enum Cipher {
    /// AES-256-GCM with a KEK held in the OS keyring. Linux only — the
    /// embedded byte array is the 32-byte KEK.
    AesGcm256([u8; 32]),
    /// Windows DPAPI: encryption is delegated to the OS, no KEK to manage.
    #[allow(dead_code)] // constructed only on Windows
    Dpapi,
    /// Cleartext on disk. Used as a soft fallback when no keychain is
    /// available, and as a hard fallback on macOS until the platform path
    /// is implemented.
    Plain,
}

impl Cipher {
    fn method_name(&self) -> &'static str {
        match self {
            Cipher::AesGcm256(_) => "aes-256-gcm",
            Cipher::Dpapi => "dpapi",
            Cipher::Plain => "plain",
        }
    }

    fn is_encrypted(&self) -> bool {
        !matches!(self, Cipher::Plain)
    }

    /// Pick the strongest cipher the host system supports.
    fn select_default() -> Cipher {
        #[cfg(target_os = "windows")]
        {
            return Cipher::Dpapi;
        }

        #[cfg(target_os = "linux")]
        {
            match linux_obtain_or_create_kek() {
                Ok(kek) => Cipher::AesGcm256(kek),
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "No OS keyring available — falling back to PLAIN secrets storage. Install libsecret (gnome-keyring) or kwallet to enable encryption-at-rest."
                    );
                    Cipher::Plain
                }
            }
        }

        // FIXME(secrets-macos): wire up Security.framework /
        // kSecAttrAccessibleWhenUnlocked before declaring macOS a
        // supported target.
        #[cfg(target_os = "macos")]
        {
            tracing::warn!("macOS secret encryption not implemented yet — using PLAIN storage.");
            Cipher::Plain
        }
    }

    fn encrypt(&self, plaintext: &[u8]) -> Result<String> {
        match self {
            Cipher::AesGcm256(kek) => aes_gcm_encrypt(kek, plaintext),
            #[cfg(target_os = "windows")]
            Cipher::Dpapi => dpapi_encrypt(plaintext),
            #[cfg(not(target_os = "windows"))]
            Cipher::Dpapi => Err(VoiceTypeError::Secrets(
                "DPAPI selected on non-Windows host".into(),
            )),
            Cipher::Plain => Err(VoiceTypeError::Secrets(
                "encrypt() called on Plain cipher".into(),
            )),
        }
    }

    fn decrypt(&self, ciphertext_b64: &str) -> Result<Vec<u8>> {
        match self {
            Cipher::AesGcm256(kek) => aes_gcm_decrypt(kek, ciphertext_b64),
            #[cfg(target_os = "windows")]
            Cipher::Dpapi => dpapi_decrypt(ciphertext_b64),
            #[cfg(not(target_os = "windows"))]
            Cipher::Dpapi => Err(VoiceTypeError::Secrets(
                "DPAPI selected on non-Windows host".into(),
            )),
            Cipher::Plain => Err(VoiceTypeError::Secrets(
                "decrypt() called on Plain cipher".into(),
            )),
        }
    }
}

#[cfg(target_os = "linux")]
fn linux_obtain_or_create_kek() -> Result<[u8; 32]> {
    let entry = keyring::Entry::new(SERVICE, KEK_KEY)
        .map_err(|e| VoiceTypeError::Secrets(format!("KEK entry: {e}")))?;

    match entry.get_password() {
        Ok(b64) => {
            let bytes = B64
                .decode(b64.as_bytes())
                .map_err(|e| VoiceTypeError::Secrets(format!("KEK decode: {e}")))?;
            if bytes.len() != 32 {
                return Err(VoiceTypeError::Secrets(format!(
                    "KEK length {} != 32",
                    bytes.len()
                )));
            }
            let mut kek = [0u8; 32];
            kek.copy_from_slice(&bytes);
            Ok(kek)
        }
        Err(keyring::Error::NoEntry) => {
            let mut kek = [0u8; 32];
            OsRng.fill_bytes(&mut kek);
            let b64 = B64.encode(kek);
            entry
                .set_password(&b64)
                .map_err(|e| VoiceTypeError::Secrets(format!("KEK set: {e}")))?;
            Ok(kek)
        }
        Err(e) => Err(VoiceTypeError::Secrets(format!("KEK get: {e}"))),
    }
}

fn aes_gcm_encrypt(kek: &[u8; 32], plaintext: &[u8]) -> Result<String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(kek));
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext)
        .map_err(|e| VoiceTypeError::Secrets(format!("aes-gcm encrypt: {e}")))?;
    let mut blob = Vec::with_capacity(12 + ct.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ct);
    Ok(B64.encode(blob))
}

fn aes_gcm_decrypt(kek: &[u8; 32], blob_b64: &str) -> Result<Vec<u8>> {
    let blob = B64
        .decode(blob_b64.as_bytes())
        .map_err(|e| VoiceTypeError::Secrets(format!("aes-gcm decode: {e}")))?;
    // Minimum length: 12-byte nonce + 16-byte GCM tag.
    if blob.len() < 12 + 16 {
        return Err(VoiceTypeError::Secrets("aes-gcm blob too short".into()));
    }
    let (nonce, ct) = blob.split_at(12);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(kek));
    cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|e| VoiceTypeError::Secrets(format!("aes-gcm decrypt: {e}")))
}

#[cfg(target_os = "windows")]
fn dpapi_encrypt(plaintext: &[u8]) -> Result<String> {
    use windows::Win32::Foundation::LocalFree;
    use windows::Win32::Foundation::HLOCAL;
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let mut in_bytes = plaintext.to_vec();
    let in_blob = CRYPT_INTEGER_BLOB {
        cbData: in_bytes.len() as u32,
        pbData: in_bytes.as_mut_ptr(),
    };
    let mut out_blob = CRYPT_INTEGER_BLOB::default();

    unsafe {
        CryptProtectData(
            &in_blob,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out_blob,
        )
        .map_err(|e| VoiceTypeError::Secrets(format!("DPAPI encrypt: {e}")))?;
    }

    let ct =
        unsafe { std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec() };
    unsafe {
        let _ = LocalFree(Some(HLOCAL(out_blob.pbData as *mut _)));
    }
    Ok(B64.encode(ct))
}

#[cfg(target_os = "windows")]
fn dpapi_decrypt(ct_b64: &str) -> Result<Vec<u8>> {
    use windows::Win32::Foundation::LocalFree;
    use windows::Win32::Foundation::HLOCAL;
    use windows::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let mut ct = B64
        .decode(ct_b64.as_bytes())
        .map_err(|e| VoiceTypeError::Secrets(format!("DPAPI decode: {e}")))?;
    let in_blob = CRYPT_INTEGER_BLOB {
        cbData: ct.len() as u32,
        pbData: ct.as_mut_ptr(),
    };
    let mut out_blob = CRYPT_INTEGER_BLOB::default();

    unsafe {
        CryptUnprotectData(
            &in_blob,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out_blob,
        )
        .map_err(|e| VoiceTypeError::Secrets(format!("DPAPI decrypt: {e}")))?;
    }

    let pt =
        unsafe { std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec() };
    unsafe {
        let _ = LocalFree(Some(HLOCAL(out_blob.pbData as *mut _)));
    }
    Ok(pt)
}

/// Initialise the secret store. Must be called once during app startup
/// from `lib.rs::run` with the resolved `app_config_dir`.
pub fn init_backend(secrets_dir: PathBuf) {
    let file_path = secrets_dir.join("secrets.json");
    let _ = FILE_PATH.set(file_path.clone());

    let cipher = Cipher::select_default();
    let method = cipher.method_name();
    let encrypted = cipher.is_encrypted();

    let initial = match load_or_migrate(&file_path, &cipher) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "Secret-file load failed — starting with empty store");
            FileSecrets::default()
        }
    };
    let count = initial.entries.len();
    let _ = FILE_STATE.set(RwLock::new(initial));
    let _ = CIPHER.set(cipher);

    tracing::info!(
        file_path = %file_path.display(),
        method,
        encrypted,
        existing_entries = count,
        "Secret store initialised"
    );
}

/// Public flag for the frontend. Returns `Some(true)` when the cipher is
/// strong (DPAPI / AES-GCM) and `Some(false)` when secrets are written in
/// plaintext. `None` only before `init_backend` ran (should not happen in
/// practice — IPC commands are wired up after init).
pub fn is_encrypted_at_rest() -> Option<bool> {
    CIPHER.get().map(Cipher::is_encrypted)
}

fn load_or_migrate(path: &PathBuf, cipher: &Cipher) -> Result<FileSecrets> {
    if !path.exists() {
        // No file yet — return empty store. We do NOT write a file here;
        // the first `set()` will create it. That avoids touching the
        // disk for users who never enter an API key.
        return Ok(FileSecrets::default());
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| VoiceTypeError::Secrets(format!("read {path:?}: {e}")))?;
    let json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| VoiceTypeError::Secrets(format!("parse {path:?}: {e}")))?;

    // v1 detection: top-level object without a `version` key. Old payload
    // was a flat `{ "provider": "key" }` map.
    let entries = if json.get("version").is_some() {
        load_v2(json, cipher)?
    } else {
        tracing::info!("Migrating v1 secrets file to v2 ({})", cipher.method_name());
        let map: HashMap<String, String> = serde_json::from_value(json)
            .map_err(|e| VoiceTypeError::Secrets(format!("v1 parse: {e}")))?;
        let secrets = FileSecrets { entries: map };
        // Write the freshly-encrypted file back immediately so the v1
        // plaintext does not linger on disk longer than necessary.
        save(&secrets, cipher)?;
        return Ok(secrets);
    };

    Ok(FileSecrets { entries })
}

fn load_v2(json: serde_json::Value, cipher: &Cipher) -> Result<HashMap<String, String>> {
    let parsed: FileSecretsV2 = serde_json::from_value(json)
        .map_err(|e| VoiceTypeError::Secrets(format!("v2 parse: {e}")))?;
    if parsed.version != FILE_VERSION_CURRENT {
        return Err(VoiceTypeError::Secrets(format!(
            "unsupported secrets file version: {}",
            parsed.version
        )));
    }

    let stored_method = parsed.method.as_str();
    let stored_is_plain = stored_method == "plain";
    let current_is_plain = matches!(cipher, Cipher::Plain);

    if stored_is_plain {
        let entries = parsed.plaintext_entries.unwrap_or_default();
        // If we now have a strong cipher available but the file is still
        // plain, re-encrypt on the way in. Same intent as v1 migration:
        // do not leave plaintext lying around once we can avoid it.
        if !current_is_plain {
            tracing::info!("Migrating plain secrets file to {}", cipher.method_name());
            let secrets = FileSecrets {
                entries: entries.clone(),
            };
            save(&secrets, cipher)?;
        }
        return Ok(entries);
    }

    // Stored as ciphertext.
    let ct = parsed
        .ciphertext
        .ok_or_else(|| VoiceTypeError::Secrets("v2 missing ciphertext".into()))?;
    if stored_method != cipher.method_name() {
        // Stored cipher does not match the host's current cipher (e.g.
        // user lost their keyring after a system change). We cannot
        // decrypt, so we surface a clear error and start with an empty
        // store. The user will need to re-enter their API keys.
        return Err(VoiceTypeError::Secrets(format!(
            "secrets file was encrypted with '{}', host now offers '{}' — re-enter API keys",
            stored_method,
            cipher.method_name()
        )));
    }
    // Same method but decryption fails → the KEK is gone or changed
    // (e.g. OS keyring locked/unavailable, or a pre-fix mock-store KEK
    // that never persisted). Surface this as actionable instead of
    // letting the raw GCM error bubble up and read like "no key was
    // ever set".
    let plaintext_bytes = cipher.decrypt(&ct).map_err(|e| {
        VoiceTypeError::Secrets(format!(
            "secrets file is encrypted with '{}' but could not be decrypted ({e}) — \
             the encryption key (OS keyring) is unavailable or changed; re-enter your API keys",
            stored_method
        ))
    })?;
    let entries: HashMap<String, String> = serde_json::from_slice(&plaintext_bytes)
        .map_err(|e| VoiceTypeError::Secrets(format!("decrypted JSON parse: {e}")))?;
    Ok(entries)
}

fn save(secrets: &FileSecrets, cipher: &Cipher) -> Result<()> {
    let path = FILE_PATH
        .get()
        .ok_or_else(|| VoiceTypeError::Secrets("FILE_PATH not initialised".into()))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| VoiceTypeError::Secrets(format!("mkdir {parent:?}: {e}")))?;
    }

    let payload = match cipher {
        Cipher::Plain => FileSecretsV2 {
            version: FILE_VERSION_CURRENT,
            method: "plain".into(),
            ciphertext: None,
            plaintext_entries: Some(secrets.entries.clone()),
        },
        _ => {
            let plaintext_json = serde_json::to_vec(&secrets.entries)
                .map_err(|e| VoiceTypeError::Secrets(format!("serialise entries: {e}")))?;
            let ct = cipher.encrypt(&plaintext_json)?;
            FileSecretsV2 {
                version: FILE_VERSION_CURRENT,
                method: cipher.method_name().into(),
                ciphertext: Some(ct),
                plaintext_entries: None,
            }
        }
    };

    let json = serde_json::to_string_pretty(&payload)
        .map_err(|e| VoiceTypeError::Secrets(format!("serialise: {e}")))?;
    std::fs::write(path, json)
        .map_err(|e| VoiceTypeError::Secrets(format!("write {path:?}: {e}")))?;
    set_file_mode_0600(path)?;
    Ok(())
}

#[cfg(unix)]
fn set_file_mode_0600(path: &PathBuf) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
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

fn current_cipher() -> Result<&'static Cipher> {
    CIPHER
        .get()
        .ok_or_else(|| VoiceTypeError::Secrets("CIPHER not initialised".into()))
}

pub struct SecretStore;

impl SecretStore {
    pub fn get(provider: &str) -> Result<Option<String>> {
        let state = file_state()?.read();
        let v = state.entries.get(provider).cloned();
        tracing::info!(provider, found = v.is_some(), "secret get");
        Ok(v)
    }

    pub fn set(provider: &str, value: &str) -> Result<()> {
        let cipher = current_cipher()?;
        {
            let state = file_state()?;
            let mut guard = state.write();
            guard
                .entries
                .insert(provider.to_string(), value.to_string());
            save(&guard, cipher)?;
        }
        tracing::info!(provider, "secret set");
        Ok(())
    }

    pub fn delete(provider: &str) -> Result<()> {
        let cipher = current_cipher()?;
        {
            let state = file_state()?;
            let mut guard = state.write();
            guard.entries.remove(provider);
            save(&guard, cipher)?;
        }
        tracing::info!(provider, "secret delete");
        Ok(())
    }

    pub fn has(provider: &str) -> Result<bool> {
        Ok(Self::get(provider)?.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aes_gcm_roundtrip() {
        let kek = [0x42u8; 32];
        let pt = b"hello world, this is a secret";
        let ct = aes_gcm_encrypt(&kek, pt).expect("encrypt");
        let decrypted = aes_gcm_decrypt(&kek, &ct).expect("decrypt");
        assert_eq!(decrypted, pt);
    }

    #[test]
    fn aes_gcm_wrong_key_fails() {
        let kek_a = [0x01u8; 32];
        let kek_b = [0x02u8; 32];
        let ct = aes_gcm_encrypt(&kek_a, b"secret").expect("encrypt");
        assert!(aes_gcm_decrypt(&kek_b, &ct).is_err());
    }

    #[test]
    fn aes_gcm_short_blob_fails() {
        let kek = [0u8; 32];
        let short = B64.encode([1u8; 10]);
        assert!(aes_gcm_decrypt(&kek, &short).is_err());
    }

    #[test]
    fn v1_format_parses_as_flat_map() {
        let json = r#"{"openai":"sk-test","groq":"gsk-test"}"#;
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        assert!(value.get("version").is_none());
        let map: HashMap<String, String> = serde_json::from_value(value).unwrap();
        assert_eq!(map.get("openai"), Some(&"sk-test".to_string()));
    }

    #[test]
    fn v2_plain_roundtrip_via_serde() {
        let mut entries = HashMap::new();
        entries.insert("openai".to_string(), "sk-test".to_string());
        let payload = FileSecretsV2 {
            version: 2,
            method: "plain".into(),
            ciphertext: None,
            plaintext_entries: Some(entries.clone()),
        };
        let json = serde_json::to_string(&payload).unwrap();
        let back: FileSecretsV2 = serde_json::from_str(&json).unwrap();
        assert_eq!(back.version, 2);
        assert_eq!(back.method, "plain");
        assert_eq!(back.plaintext_entries.as_ref(), Some(&entries));
        assert!(back.ciphertext.is_none());
    }

    #[test]
    fn cipher_method_names_are_stable() {
        // The method name is part of the on-disk format. Renaming a
        // variant must not silently rename the on-disk method string,
        // otherwise existing files become unreadable.
        let kek = [0u8; 32];
        assert_eq!(Cipher::AesGcm256(kek).method_name(), "aes-256-gcm");
        assert_eq!(Cipher::Dpapi.method_name(), "dpapi");
        assert_eq!(Cipher::Plain.method_name(), "plain");
    }
}
