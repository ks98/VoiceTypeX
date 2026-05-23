// SPDX-License-Identifier: GPL-3.0-or-later
//! Reset-IPC — kontrollierte Loeschoperationen fuer User-Daten.
//!
//! Drei Stufen, gestaffelt nach Tragweite:
//!
//! 1. `reset_api_keys` — alle Provider-Keys (File + Keychain).
//! 2. `reset_wayland_token` — Wayland-Permission-Token; der naechste
//!    Auto-Paste-Inject triggert wieder den `xdg-desktop-portal`-Dialog.
//! 3. `reset_app_factory` — Settings, Modi (zurueck auf 6 Defaults),
//!    Secrets, Wayland-Token. Modelle und der Models-Cache bleiben
//!    bewusst erhalten (Re-Download waere fuer den User teuer).
//!
//! Alle drei sind als Vorbereitung fuer eine Deinstallation gedacht;
//! das eigentliche Loeschen der Files unter `~/.local/share/...` /
//! `~/.config/...` macht weiterhin der OS-Paket-Manager oder das
//! `scripts/uninstall-cleanup.sh`-Skript. Diese IPCs raeumen nur,
//! was die laufende App selbst geschrieben hat.

use crate::core::config::Settings;
use crate::core::default_modes::bootstrap_defaults_if_empty;
use crate::core::AppContext;
use crate::ipc::secrets::PROVIDERS;
use crate::secrets::SecretStore;
use std::path::Path;
use std::sync::Arc;

type IpcResult<T> = std::result::Result<T, String>;

/// Loescht alle Provider-API-Keys aus File-Storage **und** OS-Keychain.
/// Errors einzelner Provider sind nicht fatal — wir versuchen so viel zu
/// loeschen wie geht und sammeln nur Fehler fuer das letzte Ergebnis.
#[tauri::command]
pub async fn reset_api_keys() -> IpcResult<()> {
    let mut errors: Vec<String> = Vec::new();
    for &provider in PROVIDERS {
        if let Err(e) = SecretStore::delete(provider) {
            errors.push(format!("{provider}: {e}"));
        }
    }
    if errors.is_empty() {
        tracing::info!("Alle Provider-API-Keys geloescht");
        Ok(())
    } else {
        // Selbst bei Teil-Erfolgen melden wir Fehler — der User soll
        // wissen, dass nicht alles geraeumt wurde.
        Err(format!("Teil-Fehler beim Loeschen: {}", errors.join("; ")))
    }
}

/// Loescht die Wayland-Permission-Token-Datei. Effekt: der naechste
/// Auto-Paste-Inject zeigt wieder den Portal-Permission-Dialog.
/// Auf X11/Windows ist das ein No-Op (Datei existiert nie).
#[tauri::command]
pub async fn reset_wayland_token(state: tauri::State<'_, Arc<AppContext>>) -> IpcResult<()> {
    let path = config_dir(&state)?.join("wayland_session.json");
    if !path.exists() {
        tracing::info!(path = %path.display(), "Wayland-Token existiert nicht — No-Op");
        return Ok(());
    }
    std::fs::remove_file(&path).map_err(|e| format!("remove {path:?}: {e}"))?;
    tracing::info!(path = %path.display(), "Wayland-Token geloescht");
    Ok(())
}

/// Vollständiger Werksreset:
/// 1. Alle Provider-Keys raus.
/// 2. Wayland-Token raus.
/// 3. Alle `modes/*.toml`-Files raus, dann Defaults neu bootstrappen.
/// 4. `settings.json` raus, in-memory-Settings auf Default.
///
/// Modelle (`~/.local/share/.../models/`) bleiben **unangetastet**.
/// Re-Download waere fuer den User teuer (bis zu 10 GB GGUF).
#[tauri::command]
pub async fn reset_app_factory(state: tauri::State<'_, Arc<AppContext>>) -> IpcResult<()> {
    // 1. Provider-Keys.
    let mut accumulated_errors: Vec<String> = Vec::new();
    for &provider in PROVIDERS {
        if let Err(e) = SecretStore::delete(provider) {
            accumulated_errors.push(format!("secrets.{provider}: {e}"));
        }
    }

    let cfg_dir = config_dir(&state)?;

    // 2. Wayland-Token.
    let token_path = cfg_dir.join("wayland_session.json");
    if token_path.exists() {
        if let Err(e) = std::fs::remove_file(&token_path) {
            accumulated_errors.push(format!("wayland_session.json: {e}"));
        }
    }

    // 3. Modi: TOMLs raus, dann Defaults neu schreiben. Der notify-
    // Watcher im AppContext.modes pickt die Aenderungen via Hot-Reload
    // auf — kein expliziter In-Memory-Refresh hier noetig.
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
    if let Err(e) = bootstrap_defaults_if_empty(&state.modes_dir) {
        accumulated_errors.push(format!("bootstrap defaults: {e}"));
    }

    // 4. Settings: File raus, In-Memory zurueck auf Default und sofort
    // wieder rausschreiben, damit beim naechsten Start kein Lese-Fail
    // entsteht (nicht-fatal, aber sauberer).
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
        tracing::info!("Factory-Reset komplett — Settings, Modi, Secrets, Token zurueckgesetzt");
        Ok(())
    } else {
        // Reset war Best-effort. Wir geben dem User den Status-Bericht
        // zurueck, damit er entscheiden kann, ob er manuell nachhilft.
        Err(format!(
            "Reset mit Teil-Fehlern abgeschlossen: {}",
            accumulated_errors.join("; ")
        ))
    }
}

/// Helper: aus `state.settings_path` den Config-Dir ableiten. Wir halten
/// `config_dir` nicht extra im AppContext — `settings_path.parent()` ist
/// per Konstruktion derselbe Pfad wie `app_config_dir()` in `lib.rs`.
fn config_dir(state: &Arc<AppContext>) -> IpcResult<&Path> {
    state
        .settings_path
        .parent()
        .ok_or_else(|| format!("settings_path hat kein Parent: {:?}", state.settings_path))
}
