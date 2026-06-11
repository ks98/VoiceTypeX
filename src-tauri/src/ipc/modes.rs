// SPDX-License-Identifier: GPL-3.0-or-later
//! Modes IPC.
//!
//! CRUD operations write directly to `modes_dir/{id}.toml`. The
//! `notify` watcher in the `ModesRegistry` picks up the change and
//! reloads the list — no explicit refresh needed.

use crate::core::{AppContext, Mode};
use std::path::{Path, PathBuf};
use std::sync::Arc;

type IpcResult<T> = std::result::Result<T, String>;

#[tauri::command]
pub async fn get_modes(state: tauri::State<'_, Arc<AppContext>>) -> IpcResult<Vec<Mode>> {
    Ok(state.modes.current())
}

/// Force an immediate re-read of `modes/` from disk, bypassing the
/// `notify` watcher debounce. Used by the empty-state recovery button in
/// the menu — when the in-memory list is empty/stale, this re-reads the
/// TOMLs without waiting for (or relying on) the file-watcher.
#[tauri::command]
pub async fn reload_modes(state: tauri::State<'_, Arc<AppContext>>) -> IpcResult<Vec<Mode>> {
    state
        .modes
        .reload(&state.modes_dir)
        .map_err(|e| e.to_string())?;
    Ok(state.modes.current())
}

/// Re-seed the bundled default modes in `locale` (overwriting only the default
/// filenames — user-created modes are kept) and reload. Used by the onboarding
/// language picker so the default modes match the chosen UI language (#6).
#[tauri::command]
pub async fn reseed_default_modes(
    state: tauri::State<'_, Arc<AppContext>>,
    locale: String,
) -> IpcResult<Vec<Mode>> {
    crate::core::default_modes::reseed_defaults_for_locale(&state.modes_dir, Some(&locale))
        .map_err(|e| e.to_string())?;
    state
        .modes
        .reload(&state.modes_dir)
        .map_err(|e| e.to_string())?;
    Ok(state.modes.current())
}

#[tauri::command]
pub async fn create_mode(state: tauri::State<'_, Arc<AppContext>>, mode: Mode) -> IpcResult<()> {
    mode.validate().map_err(|e| e.to_string())?;

    let current = state.modes.current();
    if current.iter().any(|m| m.id == mode.id) {
        return Err(format!("Mode with id '{}' already exists", mode.id));
    }

    let path = state
        .modes_dir
        .join(format!("{}.toml", sanitize_id(&mode.id)));
    if path.exists() {
        return Err(format!(
            "File {} already exists — rename the ID or edit via update",
            path.display()
        ));
    }
    write_mode_toml(&path, &mode).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_mode(state: tauri::State<'_, Arc<AppContext>>, mode: Mode) -> IpcResult<()> {
    mode.validate().map_err(|e| e.to_string())?;

    let current = state.modes.current();
    if !current.iter().any(|m| m.id == mode.id) {
        return Err(format!("Mode '{}' does not exist", mode.id));
    }

    let path = find_mode_file(&state.modes_dir, &mode.id).unwrap_or_else(|| {
        state
            .modes_dir
            .join(format!("{}.toml", sanitize_id(&mode.id)))
    });
    write_mode_toml(&path, &mode).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_mode(state: tauri::State<'_, Arc<AppContext>>, id: String) -> IpcResult<()> {
    let path =
        find_mode_file(&state.modes_dir, &id).ok_or_else(|| format!("Mode '{id}' not found"))?;
    std::fs::remove_file(&path).map_err(|e| format!("remove {path:?}: {e}"))
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn write_mode_toml(path: &Path, mode: &Mode) -> std::io::Result<()> {
    let toml_str = toml::to_string_pretty(mode).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("TOML-Serialize: {e}"),
        )
    })?;
    std::fs::write(path, toml_str)
}

fn find_mode_file(modes_dir: &Path, id: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(modes_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(mode) = toml::from_str::<Mode>(&content) {
                if mode.id == id {
                    return Some(path);
                }
            }
        }
    }
    None
}
