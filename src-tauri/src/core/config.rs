// SPDX-License-Identifier: GPL-3.0-or-later
//! User-Settings (separat von Modi).
//!
//! Persistenz: JSON-File in `~/.config/.../settings.json` (chmod 0644 —
//! kein Secret). Beim App-Start `load_or_default(path)`, nach jedem
//! Mutations-IPC-Aufruf `save(path)`. Bei korruptem JSON faellt der
//! Loader auf `Settings::default()` zurueck (mit Log-Warning) — User
//! verliert einmalig die Settings, App startet aber sauber.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Audio-Eingabegeraet, leer = OS-Default.
    #[serde(default)]
    pub audio_input_device: Option<String>,

    /// Pfad zum lokalen Whisper-GGML-Modell. Leer = Default-Auswahl
    /// (`ggml-large-v3-turbo-q5_0.bin` aus app_data_dir/models/).
    #[serde(default)]
    pub whisper_model_path: Option<String>,

    /// Welches Default-Modell heruntergeladen werden soll, falls keins
    /// vorhanden ist. Erlaubte Werte: "large-v3-turbo-q5_0",
    /// "small-q5_1", "large-v3-turbo".
    #[serde(default = "default_whisper_slot")]
    pub whisper_default_slot: String,

    /// Diagnose-Logging — wenn true, Audio-Metadata, Transkripte und
    /// LLM-Antworten in den Logs sichtbar machen. CLAUDE.md §8: Default OFF.
    #[serde(default)]
    pub diagnostic_logging: bool,

    /// Auto-Start beim System-Login. CLAUDE.md §8: Default OFF.
    #[serde(default)]
    pub autostart: bool,

    /// Ollama-HTTP-Endpunkt (lokales LLM).
    #[serde(default = "default_ollama_url")]
    pub ollama_url: String,

    /// Wird beim ersten erfolgreichen Onboarding-Wizard-Durchlauf auf
    /// `true` gesetzt. Steuert, ob der Wizard beim Start automatisch
    /// erscheint.
    #[serde(default)]
    pub onboarding_done: bool,

    /// Anzahl Threads fuer Whisper-Inferenz. `None` = Auto-Detect via
    /// `available_parallelism()` (deckelt bei 8 — diminishing returns
    /// wegen Memory-Bandwidth). User kann ueberschreiben in Settings-UI,
    /// z.B. "nur 4 Threads damit Browser fluessig bleibt".
    #[serde(default)]
    pub whisper_n_threads: Option<u32>,

    /// Globaler Hotkey, der das Modus-Auswahl-Menue oeffnet. Genau ein
    /// Hotkey fuer die ganze App — die einzelnen Modi haben keine
    /// eigenen Hotkeys mehr.
    #[serde(default = "default_menu_hotkey")]
    pub menu_hotkey: String,

    /// Modus-ID, die beim oeffnen des Menues vorausgewaehlt ist
    /// (Cursor-Position). Wird nach jeder erfolgreichen Auswahl
    /// gespeichert, sodass der "letzte" Modus immer mit einem einzigen
    /// Enter erreichbar ist.
    #[serde(default)]
    pub last_selected_mode_id: Option<String>,
}

/// Manueller `Default`-Impl statt `#[derive(Default)]`: das Derive
/// ignoriert die `#[serde(default = "...")]`-Annotationen und benutzt
/// die Typ-Defaults. Hier setzen wir die echten Anwendungs-Defaults.
impl Default for Settings {
    fn default() -> Self {
        Self {
            audio_input_device: None,
            whisper_model_path: None,
            whisper_default_slot: default_whisper_slot(),
            diagnostic_logging: false,
            autostart: false,
            ollama_url: default_ollama_url(),
            onboarding_done: false,
            whisper_n_threads: None,
            menu_hotkey: default_menu_hotkey(),
            last_selected_mode_id: None,
        }
    }
}

fn default_menu_hotkey() -> String {
    "CommandOrControl+Alt+Space".to_string()
}

impl Settings {
    /// Liest die Settings aus `path` oder gibt `Settings::default()`
    /// zurueck, wenn die Datei fehlt oder korrupt ist. Nicht-fatal —
    /// loggt Warnings, App soll weiterlaufen.
    pub fn load_or_default(path: &Path) -> Self {
        if !path.exists() {
            tracing::info!(path = %path.display(), "Settings-Datei nicht vorhanden — Defaults");
            return Self::default();
        }
        match std::fs::read_to_string(path) {
            Ok(content) => match serde_json::from_str::<Settings>(&content) {
                Ok(settings) => {
                    tracing::info!(path = %path.display(), "Settings aus Disk geladen");
                    settings
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Settings-JSON-Parse fehlgeschlagen — nutze Defaults");
                    Self::default()
                }
            },
            Err(e) => {
                tracing::warn!(error = %e, "Settings-Read fehlgeschlagen — nutze Defaults");
                Self::default()
            }
        }
    }

    /// Schreibt die Settings als JSON nach `path`. Erstellt das
    /// Parent-Verzeichnis bei Bedarf.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create_dir: {e}"))?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| format!("json: {e}"))?;
        std::fs::write(path, json).map_err(|e| format!("write: {e}"))?;
        tracing::debug!(path = %path.display(), "Settings auf Disk geschrieben");
        Ok(())
    }
}

fn default_whisper_slot() -> String {
    "large-v3-turbo-q5_0".to_string()
}

fn default_ollama_url() -> String {
    "http://127.0.0.1:11434".to_string()
}
