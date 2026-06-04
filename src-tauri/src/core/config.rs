// SPDX-License-Identifier: GPL-3.0-or-later
//! User settings (separate from modes).
//!
//! Persistence: a JSON file in `~/.config/.../settings.json` (chmod
//! 0644 — no secret). On app start `load_or_default(path)`, after
//! every mutating IPC call `save(path)`. On corrupt JSON the loader
//! falls back to `Settings::default()` (with a log warning) — the user
//! loses settings once, but the app starts cleanly.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Audio input device; empty = OS default.
    #[serde(default)]
    pub audio_input_device: Option<String>,

    /// Path to the local Whisper GGML model. Empty = default selection
    /// (the file per `whisper_default_slot` from
    /// `app_config_dir/models/`).
    #[serde(default)]
    pub whisper_model_path: Option<String>,

    /// Which default model should be downloaded if none is present.
    /// Allowed values:
    /// - "large-v3-turbo-q8_0" — **default since phase 1** (Q8 instead
    ///   of Q5 for better DE quality at the same latency on modern
    ///   backends).
    /// - "large-v3-turbo-german-q5_0" — **DE Pro**, primeline fine-tune
    ///   (Apache 2.0), ~28 % rel. WER reduction on German.
    /// - "large-v3-turbo-q5_0" — light hardware (~half the disk
    ///   footprint).
    /// - "small-q5_1" — 4 GB devices without GPU.
    /// - "large-v3-turbo" — F16, power users with abundant VRAM.
    #[serde(default = "default_whisper_slot")]
    pub whisper_default_slot: String,

    /// Auto-start on system login. CLAUDE.md §8: default OFF.
    #[serde(default)]
    pub autostart: bool,

    /// Ollama HTTP endpoint (local LLM).
    #[serde(default = "default_ollama_url")]
    pub ollama_url: String,

    /// How long Ollama keeps the model in RAM/VRAM after a call.
    /// Format per Ollama FAQ: duration string (`"5m"`, `"30s"`,
    /// `"1h"`), `"0"` = immediate unload after the response (memory-
    /// pressure profile on 8 GB devices), default `"5m"`. A negative
    /// value (`"-1"`) keeps the model warm indefinitely.
    #[serde(default = "default_ollama_keep_alive")]
    pub ollama_keep_alive: String,

    /// **Phase 3b — embedded LLM**. Which GGUF slot is downloaded to
    /// `app_config_dir/models/` on first start when a mode with
    /// `local_engine = "embedded"` is used. Allowed values:
    /// - `"gemma3-1b-it-q5_k_m"` — **default**, ~850 MB, fits 4 GB RAM
    ///   devices (light tier).
    /// - `"gemma3-4b-it-q5_k_m"` — ~2.8 GB, standard for ≥16 GB setups
    ///   (phase-1 recommendation, strong German quality).
    /// - `"llama3.2-1b-instruct-q5_k_m"` — alternative light tier,
    ///   stronger on English.
    /// - `"qwen2.5-1.5b-instruct-q5_k_m"` — mid-size, structured
    ///   output (code/markdown).
    #[serde(default = "default_llm_slot")]
    pub llm_default_slot: String,

    /// Optional explicit path to a GGUF LLM model. When set, overrides
    /// the `llm_default_slot`-based path construction. Format: an
    /// absolute path to a `.gguf` file. Recommended only for power
    /// users with their own models.
    #[serde(default)]
    pub llm_model_path: Option<String>,

    /// Set to `true` on the first successful onboarding-wizard run.
    /// Controls whether the wizard appears automatically on start.
    #[serde(default)]
    pub onboarding_done: bool,

    /// Number of threads for Whisper inference. `None` = auto-detect via
    /// the physical core count (capped at 8 — diminishing returns due to
    /// memory bandwidth; hyperthreads regress throughput, whisper.cpp
    /// #200). The user can override in the settings UI, e.g. "only 4
    /// threads so the browser stays responsive".
    #[serde(default)]
    pub whisper_n_threads: Option<u32>,

    /// Beam width for the local Whisper **final** pass (BeamSearch).
    /// Default 2. Lower = faster, slightly less accurate (`1` ≈ greedy,
    /// ~beam× cheaper). whisper.cpp runs `beam_size` decoders in
    /// parallel, so cost is ~linear in the width; on short dictation
    /// beam>2-3 buys <2 % WER for a large latency hit. Clamped to
    /// `1..=10` at use. A per-mode `Mode.whisper_beam_size` overrides
    /// this; cloud STT ignores it.
    #[serde(default = "default_whisper_beam_size")]
    pub whisper_beam_size: u32,

    /// Global hotkey that opens the mode-selection menu. Exactly one
    /// hotkey for the whole app — the individual modes no longer have
    /// their own hotkeys.
    #[serde(default = "default_menu_hotkey")]
    pub menu_hotkey: String,

    /// Mode ID that is pre-selected when the menu opens (cursor
    /// position). Stored after every successful selection so the
    /// "last" mode is always reachable with a single Enter.
    #[serde(default)]
    pub last_selected_mode_id: Option<String>,

    /// UI locale (BCP-47, e.g. "de", "en-US", "fr"). `None` = never
    /// set; in that case `lib.rs::run` fills in the detection from
    /// `tauri_plugin_os::locale()` on first app start.
    ///
    /// The raw OS-locale string is persisted — the frontend maps it
    /// via `pickSupported()` onto one of the supported display
    /// languages [en, de, fr, es, it]. This keeps the backend free of
    /// "supported set" knowledge, and a later language expansion only
    /// needs frontend changes.
    ///
    /// **Note — re-detection semantics:** explicitly setting to `None`
    /// triggers a fresh OS detection on the next app start. The UI
    /// switcher "Auto/System" should therefore *store* the current
    /// OS-locale value, not write `None` — otherwise you get log noise
    /// and a file save on every start.
    ///
    /// **Deserialize-time validation:** values coming from the
    /// settings file are checked for BCP-47-ish shape (ASCII
    /// alphanumeric + `-`/`_`, max. 35 chars). Invalid values (e.g.
    /// from hand-editing or a corrupt write) are neutralized to
    /// `None` — the app then re-detects on the next start instead of
    /// crashing or passing the value through unchecked.
    #[serde(default, deserialize_with = "deserialize_locale")]
    pub locale: Option<String>,
}

/// Validates deserialized locale strings as BCP-47-ish.
/// Defense-in-depth: the actual mapping to the supported language
/// happens in the frontend (`pickSupported`), but if someone tampered
/// with the settings file, the value should already be filtered here
/// — otherwise it lands unchecked e.g. in a future
/// `Intl.PluralRules` constructor (`RangeError`) or as a substring in
/// a file path (path traversal).
fn deserialize_locale<'de, D>(d: D) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(d)?;
    Ok(opt.filter(|s| {
        !s.is_empty()
            && s.len() <= 35
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    }))
}

/// Manual `Default` impl instead of `#[derive(Default)]`: the derive
/// ignores the `#[serde(default = "...")]` annotations and uses the
/// type defaults. Here we set the real application defaults.
impl Default for Settings {
    fn default() -> Self {
        Self {
            audio_input_device: None,
            whisper_model_path: None,
            whisper_default_slot: default_whisper_slot(),
            autostart: false,
            ollama_url: default_ollama_url(),
            ollama_keep_alive: default_ollama_keep_alive(),
            llm_default_slot: default_llm_slot(),
            llm_model_path: None,
            onboarding_done: false,
            whisper_n_threads: None,
            whisper_beam_size: default_whisper_beam_size(),
            menu_hotkey: default_menu_hotkey(),
            last_selected_mode_id: None,
            locale: None,
        }
    }
}

fn default_menu_hotkey() -> String {
    "CommandOrControl+Alt+Space".to_string()
}

impl Settings {
    /// Reads the settings from `path`, or returns
    /// `Settings::default()` if the file is missing or corrupt.
    /// Non-fatal — logs warnings; the app should keep running.
    pub fn load_or_default(path: &Path) -> Self {
        if !path.exists() {
            tracing::info!(path = %path.display(), "Settings file not present — using defaults");
            return Self::default();
        }
        match std::fs::read_to_string(path) {
            Ok(content) => match serde_json::from_str::<Settings>(&content) {
                Ok(settings) => {
                    tracing::info!(path = %path.display(), "Settings loaded from disk");
                    settings
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Settings JSON parse failed — using defaults");
                    Self::default()
                }
            },
            Err(e) => {
                tracing::warn!(error = %e, "Settings read failed — using defaults");
                Self::default()
            }
        }
    }

    /// Writes the settings as JSON to `path`. Creates the parent
    /// directory if needed.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create_dir: {e}"))?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| format!("json: {e}"))?;
        std::fs::write(path, json).map_err(|e| format!("write: {e}"))?;
        tracing::debug!(path = %path.display(), "Settings written to disk");
        Ok(())
    }
}

fn default_ollama_keep_alive() -> String {
    "5m".to_string()
}

fn default_llm_slot() -> String {
    // Light-tier default: fits 4 GB devices and delivers sub-second
    // latency on 16 GB setups. Power users can switch to
    // "gemma3-4b-it-q5_k_m" in settings once they have 16+ GB RAM.
    "gemma3-1b-it-q5_k_m".to_string()
}

fn default_whisper_beam_size() -> u32 {
    2
}

fn default_whisper_slot() -> String {
    // Phase 1: the default moved from Q5_0 (547 MB) to Q8_0 (874 MB).
    // Q8 is equally fast on modern backends and qualitatively much
    // closer to F16 — Q5 remains selectable as a light-hardware option.
    "large-v3-turbo-q8_0".to_string()
}

fn default_ollama_url() -> String {
    "http://127.0.0.1:11434".to_string()
}
