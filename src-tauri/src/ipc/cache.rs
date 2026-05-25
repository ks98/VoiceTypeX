// SPDX-License-Identifier: GPL-3.0-or-later
//! Cache management — list and delete downloaded models and aborted
//! downloads.
//!
//! Scope is deliberately tight: only `app_config_dir/models/*`.
//! Settings, modes, secrets and the Wayland token are not "cache"
//! data and are handled via separate reset flows (user data, not
//! regenerable).
//!
//! Files are classified by filename pattern:
//! - `ggml-*.bin` → Whisper model
//! - `ggml-silero-*.bin` → VAD model
//! - `*.gguf` → LLM model
//! - `*.partial` → aborted download
//! - otherwise → "other" (e.g. files manually placed by the user)

use crate::core::AppContext;
use serde::Serialize;
use std::sync::Arc;

type IpcResult<T> = std::result::Result<T, String>;

#[derive(Serialize, Clone)]
pub struct CachedFile {
    pub filename: String,
    /// "whisper", "vad", "llm", "partial", "other".
    pub kind: &'static str,
    pub size_bytes: u64,
}

fn classify(filename: &str) -> &'static str {
    let lower = filename.to_lowercase();
    if lower.ends_with(".partial") {
        "partial"
    } else if lower.starts_with("ggml-silero") {
        "vad"
    } else if lower.starts_with("ggml-") && lower.ends_with(".bin") {
        "whisper"
    } else if lower.ends_with(".gguf") {
        "llm"
    } else {
        "other"
    }
}

#[tauri::command]
pub async fn list_cached_files(
    state: tauri::State<'_, Arc<AppContext>>,
) -> IpcResult<Vec<CachedFile>> {
    let model_dir = state.model_dir.clone();
    if !model_dir.exists() {
        return Ok(Vec::new());
    }
    let entries = std::fs::read_dir(&model_dir)
        .map_err(|e| format!("read_dir({}): {e}", model_dir.display()))?;
    let mut out: Vec<CachedFile> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(filename) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
        out.push(CachedFile {
            filename: filename.to_string(),
            kind: classify(filename),
            size_bytes,
        });
    }
    // Stable sort: type group, then filename. Makes the UI
    // predictable (all Whispers grouped, then LLMs, etc.).
    out.sort_by(|a, b| a.kind.cmp(b.kind).then(a.filename.cmp(&b.filename)));
    Ok(out)
}

/// Deletes a single file in `model_dir`. Path-traversal guard:
/// `filename` must not contain slashes or `..`.
#[tauri::command]
pub async fn delete_cached_file(
    state: tauri::State<'_, Arc<AppContext>>,
    filename: String,
) -> IpcResult<u64> {
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Err(format!("Invalid filename: {filename}"));
    }
    let path = state.model_dir.join(&filename);
    if !path.exists() {
        return Err(format!("File not present: {filename}"));
    }
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    std::fs::remove_file(&path).map_err(|e| format!("remove_file({filename}): {e}"))?;
    tracing::info!(
        file = %filename,
        freed_bytes = size,
        "Cache file deleted"
    );
    Ok(size)
}

/// Deletes all model files (Whisper, VAD, LLM) plus partials. Keeps
/// "other" files, e.g. things the user placed there manually.
#[tauri::command]
pub async fn delete_all_models(state: tauri::State<'_, Arc<AppContext>>) -> IpcResult<u64> {
    let model_dir = state.model_dir.clone();
    if !model_dir.exists() {
        return Ok(0);
    }
    let entries = std::fs::read_dir(&model_dir)
        .map_err(|e| format!("read_dir({}): {e}", model_dir.display()))?;
    let mut freed = 0u64;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(filename) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let kind = classify(filename);
        if !matches!(kind, "whisper" | "vad" | "llm" | "partial") {
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if let Err(e) = std::fs::remove_file(&path) {
            tracing::warn!(file = %filename, error = %e, "Delete failed");
            continue;
        }
        freed += size;
    }
    tracing::info!(freed_bytes = freed, "All models deleted");
    Ok(freed)
}

/// Deletes only `*.partial` files (aborted downloads).
#[tauri::command]
pub async fn clean_partial_downloads(state: tauri::State<'_, Arc<AppContext>>) -> IpcResult<u64> {
    let model_dir = state.model_dir.clone();
    if !model_dir.exists() {
        return Ok(0);
    }
    let entries = std::fs::read_dir(&model_dir)
        .map_err(|e| format!("read_dir({}): {e}", model_dir.display()))?;
    let mut freed = 0u64;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(filename) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if classify(filename) != "partial" {
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if let Err(e) = std::fs::remove_file(&path) {
            tracing::warn!(file = %filename, error = %e, "Delete failed");
            continue;
        }
        freed += size;
    }
    tracing::info!(freed_bytes = freed, "Partial downloads cleaned up");
    Ok(freed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_recognizes_common_patterns() {
        assert_eq!(classify("ggml-large-v3-turbo-q8_0.bin"), "whisper");
        assert_eq!(classify("ggml-small-q5_1.bin"), "whisper");
        assert_eq!(classify("ggml-model-q5_0.bin"), "whisper"); // primeline DE
        assert_eq!(classify("ggml-silero-v6.2.0.bin"), "vad");
        assert_eq!(classify("gemma-4-E4B-it-Q5_K_M.gguf"), "llm");
        assert_eq!(classify("Llama-3.2-1B-Instruct-Q5_K_M.gguf"), "llm");
        assert_eq!(classify("ggml-large-v3-turbo-q5_0.bin.partial"), "partial");
        assert_eq!(classify("readme.txt"), "other");
    }
}
