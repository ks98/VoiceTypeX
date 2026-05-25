// SPDX-License-Identifier: GPL-3.0-or-later
//! Mode model and TOML loader.
//!
//! A **mode** is name, hotkey, transcription target, processing target
//! and system prompt (see CLAUDE.md §5). Phase 1.2 provides only the
//! data model and a simple loader; the `notify`-based hot-reload comes
//! in phase 1.4.

use crate::core::error::{Result, VoiceTypeError};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::broadcast;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptionTarget {
    Local,
    Cloud,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProcessingTarget {
    None,
    Local,
    Cloud,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InjectionMethod {
    #[default]
    Clipboard,
    Keystrokes,
}

/// Where the text a mode operates on comes from.
///
/// - `Voice` (default): classic dictation — the spoken audio is the
///   only input; the transcript flows straight into processing/inject.
/// - `Selection`: the text currently selected in the focused app is
///   read first ("Bearbeiten" feature); the spoken audio becomes an
///   optional instruction layered on top. Requires `processing != none`
///   — there is nothing to transform the selection with otherwise.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputSource {
    #[default]
    Voice,
    Selection,
}

/// What happens with the LLM result relative to the selection.
///
/// - `Insert` (default): inject at the cursor — the existing dictation
///   behaviour (over a selection most apps overwrite it on paste).
/// - `Replace`: overwrite the selection with the result.
/// - `Append` / `Prepend`: keep the selection, place the result after /
///   before it (the injector collapses the selection first).
/// - `Auto`: the LLM decides per response via a leading sentinel line
///   (`@@REPLACE` / `@@APPEND` / `@@PREPEND`); unparseable responses
///   fall back to `Mode.output_fallback`. Only valid with
///   `input = selection`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputAction {
    #[default]
    Insert,
    Replace,
    Append,
    Prepend,
    Auto,
}

/// Default for `Mode.output_fallback`: `Replace` is the most common edit
/// action and a safe landing when an `auto` mode emits no sentinel.
fn default_output_fallback() -> OutputAction {
    OutputAction::Replace
}

// `Eq` is dropped because the f32 sampling fields (temperature / top_p
// / repeat_penalty) do not implement `Eq` (NaN != NaN). `PartialEq` is
// enough — `Mode` is not used as a HashMap/HashSet key anywhere.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mode {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Legacy field. Parsed only for backward compatibility and
    /// ignored afterwards — since the menu-hotkey rework there is a
    /// single global hotkey (`Settings.menu_hotkey`) that opens the
    /// mode-selection menu.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotkey: Option<String>,
    pub transcription: TranscriptionTarget,
    pub processing: ProcessingTarget,

    // --- STT configuration ---
    #[serde(default)]
    pub cloud_stt_provider: Option<String>,

    /// Optional Whisper model slot override for this mode. `None`
    /// (default) = use `Settings.whisper_default_slot`. Values: the
    /// same slugs as in settings (`large-v3-turbo-q8_0`,
    /// `large-v3-turbo-german-q5_0`, `small-q5_1`, …).
    ///
    /// A per-mode override needs a second `LocalTranscriber` instance;
    /// the pipeline caches these in `AppContext.extra_transcribers`
    /// per slot slug.
    #[serde(default)]
    pub whisper_model_slot: Option<String>,

    /// Whisper glossary: words/phrases passed to Whisper as a hint
    /// (e.g. proper names, jargon). Forwarded as `initial_prompt` to
    /// the decode stage and influences token probabilities.
    /// Recommendation: a short list, comma- or space-separated.
    #[serde(default)]
    pub initial_prompt: Option<String>,

    // --- LLM configuration ---
    #[serde(default)]
    pub cloud_llm_provider: Option<String>,
    #[serde(default)]
    pub cloud_llm_model: Option<String>,

    /// **Deprecated** since the phase-3b refactor: use
    /// `ollama_model_tag` or `embedded_llm_slot` instead (depending on
    /// the engine). Still parsed at load time and migrated into
    /// `ollama_model_tag` if that is `None` — backward-compat for
    /// existing user TOMLs.
    #[serde(default)]
    pub local_llm_model: Option<String>,

    /// Which local LLM engine should serve this mode when
    /// `processing == "local"`?
    /// - `"embedded"` (default, since May 2026) — built-in llama-cpp-2
    ///   path without an external daemon, GGUF from
    ///   `Settings.llm_default_slot` or `Mode.embedded_llm_slot`.
    /// - `"ollama"` — external Ollama daemon (opt-in for users with
    ///   their own installation).
    ///
    /// `None` (field omitted) falls back to `"embedded"` in
    /// `pipeline/mod.rs`. Phase-1/2 TOMLs with `local_llm_model` but
    /// without `local_engine` are set to `"ollama"` automatically by
    /// `migrate_deprecated_fields`, otherwise the default switch would
    /// route them to the wrong engine path.
    #[serde(default)]
    pub local_engine: Option<String>,

    /// Ollama model tag for the opt-in path (`local_engine =
    /// "ollama"`). Example: `"gemma3:4b"`, `"qwen2.5:7b"`. Required
    /// when `engine == "ollama"`.
    #[serde(default)]
    pub ollama_model_tag: Option<String>,

    /// GGUF slot for the embedded path (`local_engine = "embedded"`).
    /// `None` = use `Settings.llm_default_slot` (global default).
    /// Values: `"gemma4-e4b-it-q5_k_m"`, `"gemma4-e2b-it-q5_k_m"`,
    /// `"gemma3-1b-it-q5_k_m"`, … (see `LlmModelSlot::from_setting`).
    #[serde(default)]
    pub embedded_llm_slot: Option<String>,

    #[serde(default)]
    pub injection_method: InjectionMethod,

    /// Where the operated-on text comes from — `voice` (default,
    /// dictation) or `selection` ("Bearbeiten"). See `InputSource`.
    #[serde(default)]
    pub input: InputSource,

    /// What to do with the LLM result relative to the selection. See
    /// `OutputAction`. Only meaningful when `input = selection`.
    #[serde(default)]
    pub output: OutputAction,

    /// Fallback action when `output = auto` and the LLM emits no
    /// recognizable sentinel line. Must not itself be `auto`.
    #[serde(default = "default_output_fallback")]
    pub output_fallback: OutputAction,

    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,

    // --- Per-mode sampling parameters ---
    // Apply to local (Ollama/embedded) AND cloud LLMs (OpenAI Chat
    // Completions / Anthropic Messages, as long as the provider
    // respects the parameter). On `None` the provider/engine uses the
    // server default.
    //
    // Recommended defaults for "faithful rewrite, do not extend":
    //   temperature = 0.2, top_p = 0.8, repeat_penalty = 1.05
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub repeat_penalty: Option<f32>,

    /// Maximum output token count for LLM cleanup. `None` = 1024
    /// (default in `LlamaEmbeddedProcessor`). Slack modes get by with
    /// 256, long emails need 2048+.
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

impl Mode {
    /// Consistency validation: a mode with `transcription = "cloud"`
    /// needs a `cloud_stt_provider`; analogously `processing =
    /// "cloud"` → `cloud_llm_provider`. This consistency cannot be
    /// encoded in TOML, so we check it after deserialize.
    pub fn validate(&self) -> Result<()> {
        const MAX_SYSTEM_PROMPT_LEN: usize = 32 * 1024;
        const MAX_DESCRIPTION_LEN: usize = 4 * 1024;

        if self.id.is_empty() {
            return Err(VoiceTypeError::Mode("id must not be empty".into()));
        }
        if self.id.contains(char::is_whitespace) {
            return Err(VoiceTypeError::Mode(format!(
                "id '{}' contains whitespace",
                self.id
            )));
        }
        if self.description.len() > MAX_DESCRIPTION_LEN {
            return Err(VoiceTypeError::Mode(format!(
                "Mode '{}': description exceeds {} bytes",
                self.id, MAX_DESCRIPTION_LEN
            )));
        }
        if let Some(prompt) = self.system_prompt.as_deref() {
            if prompt.len() > MAX_SYSTEM_PROMPT_LEN {
                return Err(VoiceTypeError::Mode(format!(
                    "Mode '{}': system_prompt exceeds {} bytes",
                    self.id, MAX_SYSTEM_PROMPT_LEN
                )));
            }
        }
        if self.transcription == TranscriptionTarget::Cloud && self.cloud_stt_provider.is_none() {
            return Err(VoiceTypeError::Mode(format!(
                "Mode '{}': transcription=cloud, but no cloud_stt_provider set",
                self.id
            )));
        }
        if self.processing == ProcessingTarget::Cloud && self.cloud_llm_provider.is_none() {
            return Err(VoiceTypeError::Mode(format!(
                "Mode '{}': processing=cloud, but no cloud_llm_provider set",
                self.id
            )));
        }
        if self.processing != ProcessingTarget::None && self.system_prompt.is_none() {
            return Err(VoiceTypeError::Mode(format!(
                "Mode '{}': processing != none, but no system_prompt set",
                self.id
            )));
        }
        // Edit-mode ("Bearbeiten") consistency: a selection-based mode
        // must have an LLM to transform the selection with, and the
        // selection-relative output actions only make sense there.
        if self.input == InputSource::Selection && self.processing == ProcessingTarget::None {
            return Err(VoiceTypeError::Mode(format!(
                "Mode '{}': input=selection requires processing != none (transforming a selection needs an LLM)",
                self.id
            )));
        }
        if self.output != OutputAction::Insert && self.input != InputSource::Selection {
            return Err(VoiceTypeError::Mode(format!(
                "Mode '{}': output={:?} requires input=selection (voice modes inject at the cursor)",
                self.id, self.output
            )));
        }
        if self.output_fallback == OutputAction::Auto {
            return Err(VoiceTypeError::Mode(format!(
                "Mode '{}': output_fallback must not be 'auto'",
                self.id
            )));
        }
        // Phase 3b: engine-type check + Ollama-tag requirement.
        if let Some(engine) = self.local_engine.as_deref() {
            match engine {
                "embedded" | "ollama" => {}
                other => {
                    return Err(VoiceTypeError::Mode(format!(
                        "Mode '{}': local_engine '{other}' unbekannt (erlaubt: \"embedded\", \"ollama\")",
                        self.id
                    )));
                }
            }
        }
        if self.processing == ProcessingTarget::Local {
            // The default engine is "embedded" (see pipeline/mod.rs).
            // A model tag is only required when the mode explicitly
            // selects Ollama — embedded runs with the global
            // `Settings.llm_default_slot` and needs no further required
            // config.
            let engine = self.local_engine.as_deref().unwrap_or("embedded");
            if engine == "ollama"
                && self.ollama_model_tag.is_none()
                && self.local_llm_model.is_none()
            {
                return Err(VoiceTypeError::Mode(format!(
                    "Mode '{}': local_engine=ollama, aber weder ollama_model_tag noch local_llm_model gesetzt",
                    self.id
                )));
            }
        }
        // Sampling parameter ranges (best-effort check, no hard-fail
        // on minor overruns — user frustration vs. helpfulness).
        if let Some(t) = self.temperature {
            if !(0.0..=2.0).contains(&t) {
                return Err(VoiceTypeError::Mode(format!(
                    "Mode '{}': temperature {t} ausserhalb [0.0, 2.0]",
                    self.id
                )));
            }
        }
        if let Some(p) = self.top_p {
            if !(0.0..=1.0).contains(&p) {
                return Err(VoiceTypeError::Mode(format!(
                    "Mode '{}': top_p {p} ausserhalb [0.0, 1.0]",
                    self.id
                )));
            }
        }
        if let Some(r) = self.repeat_penalty {
            if !(0.5..=2.0).contains(&r) {
                return Err(VoiceTypeError::Mode(format!(
                    "Mode '{}': repeat_penalty {r} ausserhalb [0.5, 2.0]",
                    self.id
                )));
            }
        }
        if let Some(m) = self.max_tokens {
            if !(1..=8192).contains(&m) {
                return Err(VoiceTypeError::Mode(format!(
                    "Mode '{}': max_tokens {m} ausserhalb [1, 8192]",
                    self.id
                )));
            }
        }
        Ok(())
    }

    /// Migration: old TOMLs (Phase 1/2, before the embedded-default
    /// switch).
    ///
    /// Two cases:
    ///
    /// 1. **`local_llm_model` without `ollama_model_tag`**: copy the
    ///    value over to `ollama_model_tag` (the new required key
    ///    position for the Ollama path).
    ///
    /// 2. **`local_engine` unset + indications of an Ollama config
    ///    present** (`local_llm_model` or `ollama_model_tag`): set
    ///    `local_engine = "ollama"` explicitly. Otherwise the new code
    ///    default ("embedded", see
    ///    `pipeline/mod.rs::run_local_processing`) would route the mode
    ///    to embedded — and a value like `"gemma3:4b"` is an Ollama tag,
    ///    not a GGUF slot, which would cause load errors.
    ///
    /// Modes without any engine hints are left untouched and the code
    /// default ("embedded") wins — that is the desired effect for new
    /// or fresh TOMLs from the embedded-default switch onwards.
    fn migrate_deprecated_fields(&mut self) {
        if self.ollama_model_tag.is_none() && self.local_llm_model.is_some() {
            let val = self.local_llm_model.clone();
            tracing::warn!(
                mode_id = %self.id,
                value = ?val,
                "TOML-Feld `local_llm_model` ist deprecated — automatisch nach `ollama_model_tag` migriert"
            );
            self.ollama_model_tag = val;
        }

        if self.local_engine.is_none()
            && (self.ollama_model_tag.is_some() || self.local_llm_model.is_some())
        {
            tracing::warn!(
                mode_id = %self.id,
                "Mode hat Ollama-Indizien aber kein `local_engine` — setze explizit auf \"ollama\" (Migration: Embedded ist neuer Default)"
            );
            self.local_engine = Some("ollama".to_string());
        }
    }
}

/// Load a single mode from a TOML file.
pub fn load_mode_from_path(path: &Path) -> Result<Mode> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| VoiceTypeError::Mode(format!("{}: {e}", path.display())))?;
    let mut mode: Mode = toml::from_str(&content)
        .map_err(|e| VoiceTypeError::Mode(format!("{}: TOML-Parse: {e}", path.display())))?;
    mode.migrate_deprecated_fields();
    mode.validate()?;
    Ok(mode)
}

/// Load every `*.toml` from a directory. Duplicate IDs are treated as
/// a conflict and produce an error.
pub fn load_modes_from_dir(dir: &Path) -> Result<Vec<Mode>> {
    let mut by_id: std::collections::HashMap<String, Mode> = std::collections::HashMap::new();

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let mode = load_mode_from_path(&path)?;

        if let Some(prev) = by_id.get(&mode.id) {
            return Err(VoiceTypeError::Mode(format!(
                "Doppelte Modus-ID '{}' in {} und (vorher) {}",
                mode.id,
                path.display(),
                prev.name
            )));
        }

        by_id.insert(mode.id.clone(), mode);
    }

    let mut modes: Vec<Mode> = by_id.into_values().collect();
    modes.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(modes)
}

#[derive(Debug, Clone)]
pub enum ModesEvent {
    /// Modes were reloaded successfully.
    Reloaded,
    /// An error occurred during reload — the previously loaded modes
    /// stay active.
    Error(String),
}

/// Observable in-memory mode list with optional hot-reload.
///
/// Usage:
///   let registry = ModesRegistry::load(modes_dir.clone())?;
///   registry.start_watching(modes_dir)?;  // optional
pub struct ModesRegistry {
    modes: Arc<RwLock<Vec<Mode>>>,
    update_tx: broadcast::Sender<ModesEvent>,
    /// The watcher is held here so it isn't dropped (which would end
    /// file watching). The notify watcher itself is updated in
    /// `start_watching`.
    watcher: parking_lot::Mutex<Option<notify::RecommendedWatcher>>,
}

impl ModesRegistry {
    /// Loads every `*.toml` under `dir` and returns a registry holding
    /// the snapshot. File watching is opt-in via `start_watching`.
    pub fn load(dir: PathBuf) -> Result<Self> {
        let modes = load_modes_from_dir(&dir)?;
        let (tx, _) = broadcast::channel(8);
        Ok(Self {
            modes: Arc::new(RwLock::new(modes)),
            update_tx: tx,
            watcher: parking_lot::Mutex::new(None),
        })
    }

    /// Current modes list (snapshot).
    pub fn current(&self) -> Vec<Mode> {
        self.modes.read().clone()
    }

    /// Snapshot lookup by mode ID. Returns `None` if the ID is unknown.
    pub fn find_by_id(&self, id: &str) -> Option<Mode> {
        self.modes.read().iter().find(|m| m.id == id).cloned()
    }

    /// Subscribes to hot-reload events emitted by `start_watching`.
    pub fn subscribe(&self) -> broadcast::Receiver<ModesEvent> {
        self.update_tx.subscribe()
    }

    /// Start file watching. On changes to `*.toml` in the directory
    /// the entire mode list is reloaded and subscribers are notified.
    pub fn start_watching(&self, dir: PathBuf) -> Result<()> {
        use notify::Watcher;

        let modes = Arc::clone(&self.modes);
        let tx = self.update_tx.clone();
        let dir_for_load = dir.clone();

        let mut watcher =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| match res {
                Ok(event) => {
                    if !event_touches_toml(&event) {
                        return;
                    }
                    match load_modes_from_dir(&dir_for_load) {
                        Ok(new_modes) => {
                            *modes.write() = new_modes;
                            let _ = tx.send(ModesEvent::Reloaded);
                            tracing::info!("modes/ reloaded");
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Modes reload failed");
                            let _ = tx.send(ModesEvent::Error(e.to_string()));
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "notify watcher reported error");
                }
            })
            .map_err(|e| VoiceTypeError::Mode(format!("notify::watcher: {e}")))?;

        watcher
            .watch(&dir, notify::RecursiveMode::NonRecursive)
            .map_err(|e| VoiceTypeError::Mode(format!("watch({dir:?}): {e}")))?;

        *self.watcher.lock() = Some(watcher);
        Ok(())
    }
}

fn event_touches_toml(event: &notify::Event) -> bool {
    event
        .paths
        .iter()
        .any(|p| p.extension().and_then(|s| s.to_str()) == Some("toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml_str: &str) -> Result<Mode> {
        // Mirrors `load_mode_from_path`: migration first, then
        // validation. Otherwise the engine migration would not apply
        // in tests.
        let mut mode: Mode =
            toml::from_str(toml_str).map_err(|e| VoiceTypeError::Mode(e.to_string()))?;
        mode.migrate_deprecated_fields();
        mode.validate()?;
        Ok(mode)
    }

    #[test]
    fn local_only_mode_parses() {
        let m = parse(
            r#"
            id = "exakt"
            name = "Exaktes Diktat"
            transcription = "local"
            processing = "none"
            language = "de"
        "#,
        )
        .unwrap();
        assert_eq!(m.id, "exakt");
        assert_eq!(m.transcription, TranscriptionTarget::Local);
        assert_eq!(m.processing, ProcessingTarget::None);
        assert_eq!(m.injection_method, InjectionMethod::Clipboard);
        assert!(m.hotkey.is_none());
    }

    #[test]
    fn legacy_hotkey_field_is_accepted_and_ignored() {
        // Existing user TOMLs (before the menu-hotkey rework) have a
        // required `hotkey` field. The parser must keep accepting it,
        // so the first app start after the update doesn't discard all
        // modes.
        let m = parse(
            r#"
            id = "exakt"
            name = "Exaktes Diktat"
            hotkey = "CommandOrControl+Alt+D"
            transcription = "local"
            processing = "none"
        "#,
        )
        .unwrap();
        assert_eq!(m.hotkey.as_deref(), Some("CommandOrControl+Alt+D"));
    }

    #[test]
    fn cloud_mode_without_provider_fails() {
        let err = parse(
            r#"
            id = "email"
            name = "Email"
            transcription = "cloud"
            processing = "cloud"
            system_prompt = "test"
        "#,
        )
        .unwrap_err();
        assert!(matches!(err, VoiceTypeError::Mode(_)));
    }

    #[test]
    fn id_with_whitespace_fails() {
        let err = parse(
            r#"
            id = "exakt diktat"
            name = "x"
            transcription = "local"
            processing = "none"
        "#,
        )
        .unwrap_err();
        assert!(matches!(err, VoiceTypeError::Mode(_)));
    }

    #[test]
    fn migration_sets_local_engine_ollama_for_deprecated_local_llm_model() {
        // Phase-1/2 TOML: `local_llm_model` without `local_engine`
        // and without `ollama_model_tag`. After migration both fields
        // must be set so the new embedded default doesn't force such
        // modes onto the wrong engine path.
        let m = parse(
            r#"
            id = "korr-alt"
            name = "Korrektur (alt)"
            transcription = "local"
            processing = "local"
            local_llm_model = "gemma3:4b"
            system_prompt = "x"
        "#,
        )
        .unwrap();
        assert_eq!(m.local_engine.as_deref(), Some("ollama"));
        assert_eq!(m.ollama_model_tag.as_deref(), Some("gemma3:4b"));
    }

    #[test]
    fn migration_sets_local_engine_ollama_for_explicit_ollama_tag() {
        // TOML with `ollama_model_tag` but without `local_engine` —
        // same migration: the engine is explicitly set to "ollama".
        let m = parse(
            r#"
            id = "korr-tag"
            name = "Korrektur (tag)"
            transcription = "local"
            processing = "local"
            ollama_model_tag = "llama3.2:3b"
            system_prompt = "x"
        "#,
        )
        .unwrap();
        assert_eq!(m.local_engine.as_deref(), Some("ollama"));
    }

    #[test]
    fn fresh_local_mode_keeps_engine_none_for_default_embedded() {
        // Fresh mode without any Ollama hints: `local_engine` stays
        // `None`. The code default in `run_local_processing` then
        // falls back to "embedded".
        let m = parse(
            r#"
            id = "korr-neu"
            name = "Korrektur (neu)"
            transcription = "local"
            processing = "local"
            embedded_llm_slot = "gemma4-e4b-it-q5_k_m"
            system_prompt = "x"
        "#,
        )
        .unwrap();
        assert!(m.local_engine.is_none());
    }

    #[test]
    fn voice_mode_defaults_to_insert_and_replace_fallback() {
        // A plain dictation mode sets none of the edit fields. The
        // defaults must keep it a Voice/Insert mode so existing TOMLs
        // behave exactly as before.
        let m = parse(
            r#"
            id = "exakt"
            name = "Exaktes Diktat"
            transcription = "local"
            processing = "none"
        "#,
        )
        .unwrap();
        assert_eq!(m.input, InputSource::Voice);
        assert_eq!(m.output, OutputAction::Insert);
        assert_eq!(m.output_fallback, OutputAction::Replace);
    }

    #[test]
    fn edit_mode_with_selection_and_processing_parses() {
        let m = parse(
            r#"
            id = "verbessern"
            name = "Verbessern"
            transcription = "cloud"
            processing = "cloud"
            cloud_stt_provider = "xai"
            cloud_llm_provider = "xai"
            input = "selection"
            output = "replace"
            system_prompt = "Improve the selected text."
        "#,
        )
        .unwrap();
        assert_eq!(m.input, InputSource::Selection);
        assert_eq!(m.output, OutputAction::Replace);
    }

    #[test]
    fn auto_output_mode_parses_with_fallback() {
        let m = parse(
            r#"
            id = "frei"
            name = "Frei bearbeiten"
            transcription = "cloud"
            processing = "cloud"
            cloud_stt_provider = "xai"
            cloud_llm_provider = "xai"
            input = "selection"
            output = "auto"
            output_fallback = "append"
            system_prompt = "Apply the instruction."
        "#,
        )
        .unwrap();
        assert_eq!(m.output, OutputAction::Auto);
        assert_eq!(m.output_fallback, OutputAction::Append);
    }

    #[test]
    fn selection_input_without_processing_fails() {
        // input=selection but processing=none — there is no LLM to
        // transform the selection with.
        let err = parse(
            r#"
            id = "broken"
            name = "Broken"
            transcription = "local"
            processing = "none"
            input = "selection"
        "#,
        )
        .unwrap_err();
        assert!(matches!(err, VoiceTypeError::Mode(_)));
    }

    #[test]
    fn selection_output_on_voice_mode_fails() {
        // output=replace without input=selection — a voice mode injects
        // at the cursor and has no selection to replace.
        let err = parse(
            r#"
            id = "broken"
            name = "Broken"
            transcription = "cloud"
            processing = "cloud"
            cloud_stt_provider = "xai"
            cloud_llm_provider = "xai"
            output = "replace"
            system_prompt = "x"
        "#,
        )
        .unwrap_err();
        assert!(matches!(err, VoiceTypeError::Mode(_)));
    }

    #[test]
    fn output_fallback_auto_fails() {
        let err = parse(
            r#"
            id = "broken"
            name = "Broken"
            transcription = "cloud"
            processing = "cloud"
            cloud_stt_provider = "xai"
            cloud_llm_provider = "xai"
            input = "selection"
            output = "auto"
            output_fallback = "auto"
            system_prompt = "x"
        "#,
        )
        .unwrap_err();
        assert!(matches!(err, VoiceTypeError::Mode(_)));
    }
}
