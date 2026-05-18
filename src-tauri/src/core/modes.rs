// SPDX-License-Identifier: GPL-3.0-or-later
//! Mode-Modell und TOML-Loader.
//!
//! Ein **Modus** ist Name, Hotkey, Transkriptions-Ziel, Verarbeitungs-Ziel und
//! System-Prompt (siehe CLAUDE.md §5). Phase 1.2 stellt nur das Datenmodell
//! und einen einfachen Loader bereit; das `notify`-basierte Hot-Reload kommt
//! in Phase 1.4.

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

// Eq entfaellt, weil f32-Sampling-Felder (temperature/top_p/repeat_penalty)
// kein Eq implementieren (NaN != NaN). PartialEq reicht — Mode wird nirgends
// als HashMap/HashSet-Key benutzt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mode {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Legacy-Feld. Wird nur noch zur Backward-Compatibility geparst und
    /// danach ignoriert — seit dem Menue-Hotkey-Umbau gibt es einen
    /// einzigen globalen Hotkey (Settings.menu_hotkey), der das
    /// Modus-Auswahl-Menue oeffnet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotkey: Option<String>,
    pub transcription: TranscriptionTarget,
    pub processing: ProcessingTarget,

    #[serde(default)]
    pub cloud_stt_provider: Option<String>,
    #[serde(default)]
    pub cloud_llm_provider: Option<String>,
    #[serde(default)]
    pub cloud_llm_model: Option<String>,
    #[serde(default)]
    pub local_llm_model: Option<String>,

    #[serde(default)]
    pub injection_method: InjectionMethod,

    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,

    // Sampling-Parameter pro Modus. Greift fuer lokales (Ollama) UND
    // Cloud-LLM (OpenAI-Chat-Completions / Anthropic Messages, soweit der
    // jeweilige Provider die Parameter respektiert). Bei `None` benutzt der
    // Provider seinen Server-Default.
    //
    // Empfohlene Defaults fuer "faithful rewrite, do not extend":
    //   temperature = 0.2, top_p = 0.8, repeat_penalty = 1.05
    // Diese Werte stehen in den default_modes-TOMLs der Modi, die lokales
    // LLM benutzen (siehe Task #8).
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub repeat_penalty: Option<f32>,
}

impl Mode {
    /// Konsistenz-Validierung: ein Modus mit `transcription = "cloud"` braucht
    /// einen `cloud_stt_provider`; analog `processing = "cloud"` →
    /// `cloud_llm_provider`. Diese Konsistenz ist nicht in TOML kodierbar,
    /// also pruefen wir sie nach dem Deserialize.
    pub fn validate(&self) -> Result<()> {
        if self.id.is_empty() {
            return Err(VoiceTypeError::Mode("id darf nicht leer sein".into()));
        }
        if self.id.contains(char::is_whitespace) {
            return Err(VoiceTypeError::Mode(format!(
                "id '{}' enthaelt Leerzeichen",
                self.id
            )));
        }
        if self.transcription == TranscriptionTarget::Cloud && self.cloud_stt_provider.is_none() {
            return Err(VoiceTypeError::Mode(format!(
                "Modus '{}': transcription=cloud, aber kein cloud_stt_provider gesetzt",
                self.id
            )));
        }
        if self.processing == ProcessingTarget::Cloud && self.cloud_llm_provider.is_none() {
            return Err(VoiceTypeError::Mode(format!(
                "Modus '{}': processing=cloud, aber kein cloud_llm_provider gesetzt",
                self.id
            )));
        }
        if self.processing != ProcessingTarget::None && self.system_prompt.is_none() {
            return Err(VoiceTypeError::Mode(format!(
                "Modus '{}': processing != none, aber kein system_prompt gesetzt",
                self.id
            )));
        }
        Ok(())
    }
}

/// Lade einen einzelnen Modus aus einer TOML-Datei.
pub fn load_mode_from_path(path: &Path) -> Result<Mode> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| VoiceTypeError::Mode(format!("{}: {e}", path.display())))?;
    let mode: Mode = toml::from_str(&content)
        .map_err(|e| VoiceTypeError::Mode(format!("{}: TOML-Parse: {e}", path.display())))?;
    mode.validate()?;
    Ok(mode)
}

/// Lade alle `*.toml` aus einem Verzeichnis. Doppelte IDs gelten als
/// Konflikt und produzieren einen Fehler.
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
    /// Modi wurden erfolgreich neu geladen.
    Reloaded,
    /// Beim Reload trat ein Fehler auf — die zuvor geladenen Modi bleiben aktiv.
    Error(String),
}

/// Beobachtbare, in-memory Modi-Liste mit optionalem Hot-Reload.
///
/// Verwendung:
///   let registry = ModesRegistry::load(modes_dir.clone())?;
///   registry.start_watching(modes_dir)?;  // optional
pub struct ModesRegistry {
    modes: Arc<RwLock<Vec<Mode>>>,
    update_tx: broadcast::Sender<ModesEvent>,
    /// Watcher wird hier gehalten, damit er nicht gedroppt wird (was das
    /// File-Watching beendet). Der notify-Watcher selbst wird in
    /// `start_watching` aktualisiert.
    watcher: parking_lot::Mutex<Option<notify::RecommendedWatcher>>,
}

impl ModesRegistry {
    pub fn load(dir: PathBuf) -> Result<Self> {
        let modes = load_modes_from_dir(&dir)?;
        let (tx, _) = broadcast::channel(8);
        Ok(Self {
            modes: Arc::new(RwLock::new(modes)),
            update_tx: tx,
            watcher: parking_lot::Mutex::new(None),
        })
    }

    /// Aktuelle Modi-Liste (Snapshot).
    pub fn current(&self) -> Vec<Mode> {
        self.modes.read().clone()
    }

    pub fn find_by_id(&self, id: &str) -> Option<Mode> {
        self.modes.read().iter().find(|m| m.id == id).cloned()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ModesEvent> {
        self.update_tx.subscribe()
    }

    /// Starte File-Watching. Bei Aenderungen an `*.toml` im Verzeichnis wird
    /// die komplette Modi-Liste neu geladen und Subscriber benachrichtigt.
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
                            tracing::info!("modes/ neu geladen");
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Modes-Reload fehlgeschlagen");
                            let _ = tx.send(ModesEvent::Error(e.to_string()));
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "notify-Watcher meldete Fehler");
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
        let mode: Mode =
            toml::from_str(toml_str).map_err(|e| VoiceTypeError::Mode(e.to_string()))?;
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
        // Bestehende User-TOMLs (vor dem Menue-Hotkey-Umbau) haben ein
        // Pflicht-`hotkey`-Feld. Der Parser muss es weiterhin akzeptieren,
        // damit der erste App-Start nach dem Update nicht alle Modi
        // verwirft.
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
}
