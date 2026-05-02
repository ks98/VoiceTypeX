// SPDX-License-Identifier: GPL-3.0-or-later
//! Modi-IPC.
//!
//! CRUD-Operationen schreiben direkt in `modes_dir/{id}.toml`. Der
//! `notify`-Watcher in der ModesRegistry pickt die Aenderung auf und laedt
//! die Liste neu — kein expliziter Refresh noetig.

use crate::core::{AppContext, Mode};
use std::path::{Path, PathBuf};
use std::sync::Arc;

type IpcResult<T> = std::result::Result<T, String>;

#[tauri::command]
pub async fn get_modes(state: tauri::State<'_, Arc<AppContext>>) -> IpcResult<Vec<Mode>> {
    Ok(state.modes.current())
}

#[tauri::command]
pub async fn reload_modes(state: tauri::State<'_, Arc<AppContext>>) -> IpcResult<Vec<Mode>> {
    Ok(state.modes.current())
}

#[tauri::command]
pub async fn create_mode(state: tauri::State<'_, Arc<AppContext>>, mode: Mode) -> IpcResult<()> {
    mode.validate().map_err(|e| e.to_string())?;

    // Doppelte ID oder Hotkey gegen aktuelle Liste pruefen.
    let current = state.modes.current();
    if current.iter().any(|m| m.id == mode.id) {
        return Err(format!("Modus mit id '{}' existiert bereits", mode.id));
    }
    if let Some(conflict) = current.iter().find(|m| m.hotkey == mode.hotkey) {
        return Err(format!(
            "Hotkey '{}' bereits durch '{}' belegt",
            mode.hotkey, conflict.name
        ));
    }

    let path = state
        .modes_dir
        .join(format!("{}.toml", sanitize_id(&mode.id)));
    if path.exists() {
        return Err(format!(
            "Datei {} existiert schon — entweder ID umbenennen oder via Update editieren",
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
        return Err(format!("Modus '{}' existiert nicht", mode.id));
    }
    if let Some(conflict) = current
        .iter()
        .find(|m| m.hotkey == mode.hotkey && m.id != mode.id)
    {
        return Err(format!(
            "Hotkey '{}' bereits durch '{}' belegt",
            mode.hotkey, conflict.name
        ));
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
    let path = find_mode_file(&state.modes_dir, &id)
        .ok_or_else(|| format!("Modus '{id}' nicht gefunden"))?;
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
