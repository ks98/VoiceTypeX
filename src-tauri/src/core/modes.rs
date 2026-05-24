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

    // --- STT-Konfiguration ---
    #[serde(default)]
    pub cloud_stt_provider: Option<String>,

    /// Optionaler Whisper-Modell-Slot-Override fuer diesen Modus.
    /// `None` (Default) = nutze `Settings.whisper_default_slot`.
    /// Werte: dieselben Slugs wie in Settings (`large-v3-turbo-q8_0`,
    /// `large-v3-turbo-german-q5_0`, `small-q5_1`, …).
    ///
    /// Pro-Modus-Override braucht eine zweite `LocalTranscriber`-
    /// Instanz; die Pipeline cached diese in `AppContext.
    /// extra_transcribers` per Slot-Slug.
    #[serde(default)]
    pub whisper_model_slot: Option<String>,

    /// Whisper-Glossary: Worte/Phrasen die Whisper als Hinweis
    /// bekommt (z.B. Eigennamen, Fachbegriffe). Wird als
    /// `initial_prompt` an die Decode-Stufe gereicht und beeinflusst
    /// die Token-Wahrscheinlichkeiten. Empfehlung: kurze Liste mit
    /// Kommas oder Leerzeichen getrennt.
    #[serde(default)]
    pub initial_prompt: Option<String>,

    // --- LLM-Konfiguration ---
    #[serde(default)]
    pub cloud_llm_provider: Option<String>,
    #[serde(default)]
    pub cloud_llm_model: Option<String>,

    /// **Deprecated** seit Phase-3b-Refactor: verwende stattdessen
    /// `ollama_model_tag` oder `embedded_llm_slot` (je nach Engine).
    /// Wird beim Laden noch geparst und in `ollama_model_tag`
    /// migriert, wenn dieses None ist — Backward-Compat fuer
    /// existierende User-TOMLs.
    #[serde(default)]
    pub local_llm_model: Option<String>,

    /// Welche lokale LLM-Engine soll diesen Modus bedienen, wenn
    /// `processing == "local"`?
    /// - `"embedded"` (Default, seit Mai 2026) — eingebauter
    ///   llama-cpp-2-Pfad ohne externen Daemon, GGUF aus
    ///   `Settings.llm_default_slot` bzw. `Mode.embedded_llm_slot`.
    /// - `"ollama"` — externer Ollama-Daemon (Opt-in fuer User mit
    ///   eigener Installation).
    ///
    /// `None` (Feld weggelassen) faellt in `pipeline/mod.rs` auf
    /// `"embedded"`. Phase-1/2-TOMLs mit `local_llm_model` aber ohne
    /// `local_engine` werden in `migrate_deprecated_fields` automatisch
    /// auf `"ollama"` gesetzt, sonst wuerde der Default-Switch sie auf
    /// den falschen Engine-Pfad umleiten.
    #[serde(default)]
    pub local_engine: Option<String>,

    /// Ollama-Modell-Tag fuer den Opt-in-Pfad (`local_engine =
    /// "ollama"`). Beispiel: `"gemma3:4b"`, `"qwen2.5:7b"`. Bei
    /// `engine == "ollama"` Pflicht.
    #[serde(default)]
    pub ollama_model_tag: Option<String>,

    /// GGUF-Slot fuer den Embedded-Pfad (`local_engine = "embedded"`).
    /// `None` = nutze `Settings.llm_default_slot` (Global-Default).
    /// Werte: `"gemma4-e4b-it-q5_k_m"`, `"gemma4-e2b-it-q5_k_m"`,
    /// `"gemma3-1b-it-q5_k_m"`, … (siehe `LlmModelSlot::from_setting`).
    #[serde(default)]
    pub embedded_llm_slot: Option<String>,

    #[serde(default)]
    pub injection_method: InjectionMethod,

    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,

    // --- Sampling-Parameter pro Modus ---
    // Greifen fuer lokales (Ollama/Embedded) UND Cloud-LLM (OpenAI-
    // Chat-Completions / Anthropic Messages, soweit der Provider die
    // Parameter respektiert). Bei `None` benutzt der Provider/die
    // Engine den Server-Default.
    //
    // Empfohlene Defaults fuer "faithful rewrite, do not extend":
    //   temperature = 0.2, top_p = 0.8, repeat_penalty = 1.05
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub repeat_penalty: Option<f32>,

    /// Maximale Output-Token-Zahl fuer LLM-Cleanup. `None` =
    /// 1024 (Default in `LlamaEmbeddedProcessor`). Slack-Modi
    /// kommen mit 256 aus, lange E-Mails brauchen 2048+.
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

impl Mode {
    /// Konsistenz-Validierung: ein Modus mit `transcription = "cloud"` braucht
    /// einen `cloud_stt_provider`; analog `processing = "cloud"` →
    /// `cloud_llm_provider`. Diese Konsistenz ist nicht in TOML kodierbar,
    /// also pruefen wir sie nach dem Deserialize.
    pub fn validate(&self) -> Result<()> {
        if self.id.is_empty() {
            return Err(VoiceTypeError::Mode("id must not be empty".into()));
        }
        if self.id.contains(char::is_whitespace) {
            return Err(VoiceTypeError::Mode(format!(
                "id '{}' contains whitespace",
                self.id
            )));
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
        // Phase 3b: Engine-Type-Check + Ollama-Tag-Pflicht.
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
            // Default-Engine ist "embedded" (siehe pipeline/mod.rs). Nur
            // wenn der Modus explizit Ollama ausgewählt hat, ist ein
            // Modell-Tag Pflicht — Embedded laeuft mit dem globalen
            // `Settings.llm_default_slot` ohne weitere Pflicht-Konfig.
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
        // Sampling-Parameter-Ranges (Best-effort-Check, kein Hard-Fail
        // bei minimalen Ueberschreitungen — User-Frust vs.
        // Hilfsbereitschaft).
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

    /// Migration: alte TOMLs (Phase 1/2, vor Embedded-Default-Switch).
    ///
    /// Zwei Faelle:
    ///
    /// 1. **`local_llm_model` ohne `ollama_model_tag`**: kopiere den Wert
    ///    nach `ollama_model_tag` (das ist die neue Pflicht-Schluessel-
    ///    Position fuer den Ollama-Pfad).
    ///
    /// 2. **`local_engine` nicht gesetzt + Indizien fuer Ollama-Konfig
    ///    vorhanden** (`local_llm_model` oder `ollama_model_tag`): setze
    ///    `local_engine = "ollama"` explizit. Sonst wuerde der neue Code-
    ///    Default ("embedded", siehe `pipeline/mod.rs::run_local_processing`)
    ///    den Modus auf Embedded umleiten — und ein Wert wie `"gemma3:4b"`
    ///    ist ein Ollama-Tag, kein GGUF-Slot, was zu Lade-Fehlern fuehrt.
    ///
    /// Modi ohne jegliche Engine-Hinweise lassen wir unangetastet und der
    /// Code-Default ("embedded") greift — das ist der gewuenschte Effekt
    /// fuer neue/frische TOMLs ab dem Embedded-Default-Switch.
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

/// Lade einen einzelnen Modus aus einer TOML-Datei.
pub fn load_mode_from_path(path: &Path) -> Result<Mode> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| VoiceTypeError::Mode(format!("{}: {e}", path.display())))?;
    let mut mode: Mode = toml::from_str(&content)
        .map_err(|e| VoiceTypeError::Mode(format!("{}: TOML-Parse: {e}", path.display())))?;
    mode.migrate_deprecated_fields();
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
        // Spiegelt `load_mode_from_path`: erst Migration, dann Validation.
        // Sonst greift die Engine-Migration in den Tests nicht.
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

    #[test]
    fn migration_sets_local_engine_ollama_for_deprecated_local_llm_model() {
        // Phase-1/2-TOML: `local_llm_model` ohne `local_engine` und ohne
        // `ollama_model_tag`. Nach Migration muessen beide Felder gesetzt
        // sein, damit der neue Embedded-Default solche Modi nicht in den
        // falschen Engine-Pfad zwingt.
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
        // TOML mit `ollama_model_tag` aber ohne `local_engine` — gleiche
        // Migration: Engine wird explizit auf "ollama" gesetzt.
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
        // Frischer Mode ohne jegliche Ollama-Indizien: `local_engine`
        // bleibt `None`. Der Code-Default in `run_local_processing`
        // greift dann auf "embedded".
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
}
