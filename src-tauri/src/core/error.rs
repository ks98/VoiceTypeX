// SPDX-License-Identifier: GPL-3.0-or-later
//! Strukturierte Fehler-Taxonomie fuer alle Pipeline-Stufen.
//!
//! Bevorzugt `VoiceTypeError` an oeffentlichen Modul-Grenzen, damit der Caller
//! die Fehlerklasse mustermassig behandeln kann (z.B. Notification-Text pro
//! Stufe). Fuer ad-hoc Fehler innerhalb einer Stufe ist `anyhow::Error` mit
//! `.context(...)` legitim und wird via `From` automatisch konvertiert.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum VoiceTypeError {
    #[error("Audio-Stufe: {0}")]
    Audio(String),

    #[error("Transkription: {0}")]
    Transcription(String),

    #[error("Nachbearbeitung: {0}")]
    Processing(String),

    #[error("Text-Injection: {0}")]
    Injection(String),

    #[error("Hotkey: {0}")]
    Hotkey(String),

    #[error("Modus: {0}")]
    Mode(String),

    #[error("Konfiguration: {0}")]
    Config(String),

    #[error("Secrets / Keychain: {0}")]
    Secrets(String),

    #[error("State-Uebergang ungueltig: {from} -> {to}")]
    InvalidStateTransition { from: String, to: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, VoiceTypeError>;
