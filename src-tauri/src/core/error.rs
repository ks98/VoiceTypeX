// SPDX-License-Identifier: GPL-3.0-or-later
//! Strukturierte Fehler-Taxonomie fuer alle Pipeline-Stufen.
//!
//! Bevorzugt `VoiceTypeError` an oeffentlichen Modul-Grenzen, damit der Caller
//! die Fehlerklasse mustermassig behandeln kann (z.B. Notification-Text pro
//! Stufe). Fuer ad-hoc Fehler innerhalb einer Stufe ist `anyhow::Error` mit
//! `.context(...)` legitim und wird via `From` automatisch konvertiert.
//!
//! Jeder Fehler hat zusaetzlich:
//! - `kind()`: maschinenlesbare Klassifikation fuer Frontend-Filterung
//! - `is_retryable()`: ob Wiederholung (Backoff) sinnvoll ist
//! - `recovery_hint()`: deutscher User-facing Hinweistext

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

/// Maschinenlesbare Fehlerklasse, unabhaengig von der Stufe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Konfiguration fehlt oder ist ungueltig (User muss handeln).
    Configuration,
    /// API-Key fehlt oder ist abgelaufen.
    Authentication,
    /// HTTP-Fehler vom Provider — meist 4xx/5xx-Status.
    HttpStatus,
    /// Network-Fehler: Timeout, DNS, Connection-Refused (transient).
    Network,
    /// Hardware-Problem (Mikrofon, Audio-Geraet).
    Hardware,
    /// User-Eingabe ungueltig (z.B. Mode-TOML kaputt).
    InvalidInput,
    /// Plattform-Limitation (z.B. Wayland ohne Phase-5-Support).
    Unsupported,
    /// Interner Bug oder unerwarteter Zustand.
    Internal,
    /// IO-Fehler (Filesystem, etc.).
    Io,
    /// Sonstiges, nicht weiter klassifiziert.
    Other,
}

impl VoiceTypeError {
    /// Klassifiziere den Fehler in eine machine-readable Kategorie.
    /// Heuristik: wir matchen auf das Variant + den Inhalt der String-Message.
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::Audio(msg) => classify_audio(msg),
            Self::Transcription(msg) => classify_network_or_other(msg),
            Self::Processing(msg) => classify_network_or_other(msg),
            Self::Injection(msg) => classify_injection(msg),
            Self::Hotkey(msg) => {
                if msg.contains("Wayland") {
                    ErrorKind::Unsupported
                } else {
                    ErrorKind::Internal
                }
            }
            Self::Mode(_) => ErrorKind::InvalidInput,
            Self::Config(_) => ErrorKind::Configuration,
            Self::Secrets(_) => ErrorKind::Authentication,
            Self::InvalidStateTransition { .. } => ErrorKind::Internal,
            Self::Io(_) => ErrorKind::Io,
            Self::Other(_) => ErrorKind::Other,
        }
    }

    /// Lohnt es sich, die Operation zu wiederholen? Transient-Errors
    /// (Network, 5xx) ja, alles andere nein.
    pub fn is_retryable(&self) -> bool {
        match self.kind() {
            ErrorKind::Network => true,
            ErrorKind::HttpStatus => {
                // 5xx ist retry-bar, 4xx nicht. Heuristik via Message.
                let msg = self.to_string();
                msg.contains("HTTP 5") || msg.contains("HTTP 429")
            }
            _ => false,
        }
    }

    /// Deutscher Hinweistext fuer den User, was er tun kann.
    pub fn recovery_hint(&self) -> &'static str {
        match self.kind() {
            ErrorKind::Configuration => {
                "Pruefe deine Einstellungen — moeglicherweise fehlt ein Pflichtfeld."
            }
            ErrorKind::Authentication => {
                "API-Key fehlt oder ist ungueltig. Setze ihn unter Einstellungen → Cloud-API-Keys."
            }
            ErrorKind::HttpStatus => {
                "Der Provider hat den Request abgelehnt. Pruefe Modell-ID und API-Limits."
            }
            ErrorKind::Network => {
                "Verbindungsproblem. Pruefe Internet/Firewall — wir versuchen es automatisch erneut."
            }
            ErrorKind::Hardware => {
                "Audio-Hardware-Problem. Pruefe das Eingabegeraet in Einstellungen → Audio."
            }
            ErrorKind::InvalidInput => {
                "Eingabe-Validierung fehlgeschlagen. Korrigiere die Felder."
            }
            ErrorKind::Unsupported => {
                "Funktion in deiner Umgebung nicht verfuegbar (z.B. Wayland in Phase 1–3)."
            }
            ErrorKind::Internal => {
                "Interner Fehler — bitte als Bug melden."
            }
            ErrorKind::Io => "Dateisystem-Fehler. Pruefe Schreibrechte und freien Speicherplatz.",
            ErrorKind::Other => "Unspezifischer Fehler. Details siehe Logs-Tab.",
        }
    }
}

fn classify_audio(msg: &str) -> ErrorKind {
    let m = msg.to_ascii_lowercase();
    if m.contains("kein standard-eingabegeraet") || m.contains("nicht gefunden") {
        ErrorKind::Hardware
    } else if m.contains("permission") || m.contains("zugriff") {
        ErrorKind::Configuration
    } else {
        ErrorKind::Hardware
    }
}

fn classify_network_or_other(msg: &str) -> ErrorKind {
    let m = msg.to_ascii_lowercase();
    if m.contains("http 4") || m.contains("http 5") {
        // Aus den http {status}-Formatstrings unserer Clients.
        if m.contains("http 401") || m.contains("http 403") {
            ErrorKind::Authentication
        } else {
            ErrorKind::HttpStatus
        }
    } else if m.contains("http") || m.contains("timeout") || m.contains("connection") {
        ErrorKind::Network
    } else if m.contains("api-key") || m.contains("nicht gesetzt") {
        ErrorKind::Authentication
    } else if m.contains("nicht implementiert") || m.contains("phase ") {
        ErrorKind::Unsupported
    } else {
        ErrorKind::Other
    }
}

fn classify_injection(msg: &str) -> ErrorKind {
    let m = msg.to_ascii_lowercase();
    if m.contains("wayland") || m.contains("nicht implementiert") {
        ErrorKind::Unsupported
    } else {
        ErrorKind::Internal
    }
}

pub type Result<T> = std::result::Result<T, VoiceTypeError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_no_device_classifies_as_hardware() {
        let e = VoiceTypeError::Audio("Kein Standard-Eingabegeraet".into());
        assert_eq!(e.kind(), ErrorKind::Hardware);
        assert!(!e.is_retryable());
    }

    #[test]
    fn http_500_is_retryable() {
        let e = VoiceTypeError::Processing("HTTP 502: Bad Gateway".into());
        assert_eq!(e.kind(), ErrorKind::HttpStatus);
        assert!(e.is_retryable());
    }

    #[test]
    fn http_401_is_authentication_not_retryable() {
        let e = VoiceTypeError::Processing("HTTP 401: Unauthorized".into());
        assert_eq!(e.kind(), ErrorKind::Authentication);
        assert!(!e.is_retryable());
    }

    #[test]
    fn http_429_rate_limit_is_retryable() {
        let e = VoiceTypeError::Transcription("HTTP 429: Too Many Requests".into());
        assert!(e.is_retryable());
    }

    #[test]
    fn missing_api_key_classifies_as_authentication() {
        let e = VoiceTypeError::Transcription("API-Key fuer Provider 'xai' nicht gesetzt".into());
        assert_eq!(e.kind(), ErrorKind::Authentication);
    }

    #[test]
    fn wayland_hotkey_is_unsupported() {
        let e = VoiceTypeError::Hotkey("Wayland-Support kommt in Phase 5".into());
        assert_eq!(e.kind(), ErrorKind::Unsupported);
    }

    #[test]
    fn invalid_state_transition_is_internal() {
        let e = VoiceTypeError::InvalidStateTransition {
            from: "idle".into(),
            to: "transcribing".into(),
        };
        assert_eq!(e.kind(), ErrorKind::Internal);
        assert!(!e.is_retryable());
    }

    #[test]
    fn mode_validation_is_invalid_input() {
        let e = VoiceTypeError::Mode("id darf nicht leer sein".into());
        assert_eq!(e.kind(), ErrorKind::InvalidInput);
    }

    #[test]
    fn recovery_hints_are_distinct_per_kind() {
        let kinds = [
            ErrorKind::Configuration,
            ErrorKind::Authentication,
            ErrorKind::Network,
            ErrorKind::Hardware,
        ];
        let mut hints: Vec<&str> = kinds
            .iter()
            .map(|k| {
                let dummy = match k {
                    ErrorKind::Configuration => VoiceTypeError::Config("x".into()),
                    ErrorKind::Authentication => VoiceTypeError::Secrets("x".into()),
                    ErrorKind::Network => VoiceTypeError::Processing("HTTP timeout".into()),
                    ErrorKind::Hardware => VoiceTypeError::Audio("x".into()),
                    _ => unreachable!(),
                };
                dummy.recovery_hint()
            })
            .collect();
        hints.sort();
        hints.dedup();
        assert_eq!(hints.len(), 4);
    }
}
