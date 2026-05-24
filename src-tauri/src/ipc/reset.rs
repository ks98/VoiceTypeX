// SPDX-License-Identifier: GPL-3.0-or-later
//! Reset IPC — controlled delete operations for user data.
//!
//! Three levels, ordered by impact:
//!
//! 1. `reset_api_keys` — all provider keys (file + keychain).
//! 2. `reset_wayland_token` — Wayland permission token; the next
//!    auto-paste inject re-triggers the `xdg-desktop-portal` dialog.
//! 3. `reset_app_factory` — settings, modes (back to the 6 defaults),
//!    secrets, Wayland token. Models and the models cache are
//!    intentionally preserved (re-download would be expensive for the
//!    user).
//!
//! All three are meant as preparation for an uninstall; the actual
//! deletion of files under `~/.local/share/...` / `~/.config/...` is
//! still the job of the OS package manager or the
//! `scripts/uninstall-cleanup.sh` script. These IPCs only clean up
//! what the running app itself has written.

use crate::core::config::Settings;
use crate::core::default_modes::bootstrap_defaults_if_empty;
use crate::core::AppContext;
use crate::ipc::secrets::PROVIDERS;
use crate::secrets::SecretStore;
use std::path::Path;
use std::sync::Arc;

type IpcResult<T> = std::result::Result<T, String>;

/// Deletes all provider API keys from file storage **and** the OS
/// keychain. Errors from individual providers are not fatal — we delete
/// as much as we can and only collect errors for the final result.
#[tauri::command]
pub async fn reset_api_keys() -> IpcResult<()> {
    let mut errors: Vec<String> = Vec::new();
    for &provider in PROVIDERS {
        if let Err(e) = SecretStore::delete(provider) {
            errors.push(format!("{provider}: {e}"));
        }
    }
    if errors.is_empty() {
        tracing::info!("All provider API keys deleted");
        Ok(())
    } else {
        // Even on partial success we report errors — the user should
        // know that not everything was cleaned up.
        Err(format!("Partial delete error: {}", errors.join("; ")))
    }
}

/// Deletes the Wayland permission token file. Effect: the next
/// auto-paste inject again shows the portal permission dialog. On
/// X11/Windows this is a no-op (the file never exists).
#[tauri::command]
pub async fn reset_wayland_token(state: tauri::State<'_, Arc<AppContext>>) -> IpcResult<()> {
    let path = config_dir(&state)?.join("wayland_session.json");
    if !path.exists() {
        tracing::info!(path = %path.display(), "Wayland token does not exist — no-op");
        return Ok(());
    }
    std::fs::remove_file(&path).map_err(|e| format!("remove {path:?}: {e}"))?;
    tracing::info!(path = %path.display(), "Wayland token deleted");
    Ok(())
}

/// Full factory reset:
/// 1. Remove all provider keys.
/// 2. Remove the Wayland token.
/// 3. Remove all `modes/*.toml` files, then re-bootstrap the defaults.
/// 4. Remove `settings.json` and reset in-memory settings to default.
///
/// Models (`~/.local/share/.../models/`) stay **untouched**.
/// Re-download would be expensive for the user (up to 10 GB GGUF).
#[tauri::command]
pub async fn reset_app_factory(state: tauri::State<'_, Arc<AppContext>>) -> IpcResult<()> {
    // 1. Provider keys.
    let mut accumulated_errors: Vec<String> = Vec::new();
    for &provider in PROVIDERS {
        if let Err(e) = SecretStore::delete(provider) {
            accumulated_errors.push(format!("secrets.{provider}: {e}"));
        }
    }

    let cfg_dir = config_dir(&state)?;

    // 2. Wayland token.
    let token_path = cfg_dir.join("wayland_session.json");
    if token_path.exists() {
        if let Err(e) = std::fs::remove_file(&token_path) {
            accumulated_errors.push(format!("wayland_session.json: {e}"));
        }
    }

    // 3. Modes: remove the TOMLs, then write the defaults back. The
    // notify watcher in AppContext.modes picks up the changes via
    // hot-reload — no explicit in-memory refresh needed here.
    if state.modes_dir.exists() {
        match std::fs::read_dir(&state.modes_dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let is_toml = path
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|s| s.eq_ignore_ascii_case("toml"))
                        .unwrap_or(false);
                    if path.is_file() && is_toml {
                        if let Err(e) = std::fs::remove_file(&path) {
                            accumulated_errors.push(format!("modes/{:?}: {e}", path.file_name()));
                        }
                    }
                }
            }
            Err(e) => accumulated_errors.push(format!("read modes_dir: {e}")),
        }
    }
    // Factory reset uses the locale that's currently in Settings (will
    // be reset to Default below, which has `locale = None` → English
    // defaults). If the user wanted DE-specific defaults back, they
    // would need to re-detect via a fresh first-run after deleting the
    // entire profile dir. Acceptable: factory reset is a hard reset.
    let locale_for_bootstrap = state.settings.read().locale.clone();
    if let Err(e) = bootstrap_defaults_if_empty(&state.modes_dir, locale_for_bootstrap.as_deref()) {
        accumulated_errors.push(format!("bootstrap defaults: {e}"));
    }

    // 4. Settings: remove the file, reset in-memory back to default and
    // immediately write it out again so the next start does not hit a
    // read failure (non-fatal, but cleaner).
    let defaults = Settings::default();
    if state.settings_path.exists() {
        if let Err(e) = std::fs::remove_file(&state.settings_path) {
            accumulated_errors.push(format!("settings.json: {e}"));
        }
    }
    {
        let mut guard = state.settings.write();
        *guard = defaults.clone();
    }
    if let Err(e) = defaults.save(&state.settings_path) {
        accumulated_errors.push(format!("settings.save: {e}"));
    }

    if accumulated_errors.is_empty() {
        tracing::info!("Factory reset complete — settings, modes, secrets, token reset");
        Ok(())
    } else {
        // Reset was best-effort. Return the status report so the user
        // can decide whether to clean up manually.
        Err(format!(
            "Reset finished with partial errors: {}",
            accumulated_errors.join("; ")
        ))
    }
}

/// Helper: derive the config directory from `state.settings_path`. We
/// don't keep `config_dir` separately in AppContext —
/// `settings_path.parent()` is by construction the same path as
/// `app_config_dir()` in `lib.rs`.
fn config_dir(state: &Arc<AppContext>) -> IpcResult<&Path> {
    state
        .settings_path
        .parent()
        .ok_or_else(|| format!("settings_path has no parent: {:?}", state.settings_path))
}
