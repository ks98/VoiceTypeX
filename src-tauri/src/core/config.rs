// SPDX-License-Identifier: GPL-3.0-or-later
//! User-Settings (separat von Modi).
//!
//! Persistenz: tauri-plugin-store legt das im app_data_dir/settings.json ab,
//! aber die in-process Repraesentation ist diese Struktur. Der Store wird
//! ueber IPC-Commands aus settings.rs synchronisiert.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
}

fn default_whisper_slot() -> String {
    "large-v3-turbo-q5_0".to_string()
}

fn default_ollama_url() -> String {
    "http://127.0.0.1:11434".to_string()
}
