// SPDX-License-Identifier: GPL-3.0-or-later
//! Structured error taxonomy for all pipeline stages.
//!
//! Prefer `VoiceTypeError` at public module boundaries so the caller can
//! pattern-match on the error class (e.g. notification text per stage).
//! For ad-hoc errors inside a stage, `anyhow::Error` with `.context(...)`
//! is legitimate and gets auto-converted via `From`.
//!
//! Every error also has:
//! - `kind()`: machine-readable classification for frontend filtering
//! - `is_retryable()`: whether a retry (backoff) is sensible
//! - `recovery_hint()`: a short user-facing hint string (English; UI
//!   layer localises further if needed)

use thiserror::Error;

/// Structured provider-failure discriminant for the cloud-backed stages.
///
/// `#40` will populate `status` and `provider` at the client call sites
/// (e.g. from the real HTTP response) and then `kind()`/`is_retryable()`
/// will read these exclusively, dropping the substring fallback. Until
/// then every construction site passes `ProviderFault::unknown()`, so the
/// fields stay `None` and the legacy substring classification still runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderFault {
    /// HTTP status from the provider response, when the failure came from
    /// an HTTP exchange. `None` for non-HTTP failures (parse, IO, …).
    pub status: Option<u16>,
    /// Which provider the failure originated from, when known.
    pub provider: Option<ProviderId>,
}

impl ProviderFault {
    /// A fault with no structured information yet — classification falls
    /// back to substring matching on the message. Removed in `#40`.
    pub fn unknown() -> Self {
        Self::default()
    }
}

/// Cloud provider a transcription/processing failure originated from.
/// Populated by `#40`; today every construction site leaves it `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderId {
    Xai,
    OpenAi,
    Groq,
    Deepgram,
    Anthropic,
    Ollama,
}

#[derive(Debug, Error)]
pub enum VoiceTypeError {
    #[error("Audio stage: {0}")]
    Audio(String),

    #[error("Transcription: {message}")]
    Transcription {
        message: String,
        fault: ProviderFault,
    },

    #[error("Post-processing: {message}")]
    Processing {
        message: String,
        fault: ProviderFault,
    },

    #[error("Text injection: {0}")]
    Injection(String),

    #[error("Hotkey: {0}")]
    Hotkey(String),

    #[error("Mode: {0}")]
    Mode(String),

    #[error("Configuration: {0}")]
    Config(String),

    #[error("Secrets / keychain: {0}")]
    Secrets(String),

    #[error("Invalid state transition: {from} -> {to}")]
    InvalidStateTransition { from: String, to: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Machine-readable error class, independent of pipeline stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Configuration missing or invalid (user action needed).
    Configuration,
    /// API key missing or expired.
    Authentication,
    /// HTTP error from the provider — usually 4xx/5xx status.
    HttpStatus,
    /// Network error: timeout, DNS, connection-refused (transient).
    Network,
    /// Hardware issue (microphone, audio device).
    Hardware,
    /// User input invalid (e.g. mode TOML broken).
    InvalidInput,
    /// Platform limitation (e.g. Wayland without phase-5 support).
    Unsupported,
    /// Internal bug or unexpected state.
    Internal,
    /// IO error (filesystem etc.).
    Io,
    /// Other, not further classified.
    Other,
}

impl VoiceTypeError {
    /// Construct a `Transcription` error with no structured fault yet.
    /// `#40` adds variants/overloads that pass a populated `ProviderFault`.
    pub fn transcription(message: impl Into<String>) -> Self {
        Self::Transcription {
            message: message.into(),
            fault: ProviderFault::unknown(),
        }
    }

    /// Construct a `Processing` error with no structured fault yet.
    /// `#40` adds variants/overloads that pass a populated `ProviderFault`.
    pub fn processing(message: impl Into<String>) -> Self {
        Self::Processing {
            message: message.into(),
            fault: ProviderFault::unknown(),
        }
    }

    /// Classify the error into a machine-readable category.
    ///
    /// For the cloud stages this reads the structured `ProviderFault`
    /// first and only falls back to substring matching on the message
    /// when no status is set. `#40` populates `fault` at every client and
    /// then removes the substring fallback entirely.
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::Audio(msg) => classify_audio(msg),
            Self::Transcription { message, fault } => classify_provider(message, fault),
            Self::Processing { message, fault } => classify_provider(message, fault),
            Self::Injection(msg) => classify_injection(msg),
            Self::Hotkey(msg) => {
                if msg.to_ascii_lowercase().contains("wayland") {
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

    /// Worth retrying? Transient errors (network, 5xx) yes,
    /// everything else no.
    pub fn is_retryable(&self) -> bool {
        match self.kind() {
            ErrorKind::Network => true,
            ErrorKind::HttpStatus => {
                // Prefer the structured status; 5xx and 429 are retryable.
                // `#40` removes the substring fallback once every client
                // populates `fault.status`.
                if let Some(status) = self.fault_status() {
                    return status >= 500 || status == 429;
                }
                let msg = self.to_string();
                msg.contains("HTTP 5") || msg.contains("HTTP 429")
            }
            _ => false,
        }
    }

    /// The structured HTTP status carried by a cloud-stage error, if any.
    /// `None` until `#40` populates `ProviderFault` at the call sites.
    fn fault_status(&self) -> Option<u16> {
        match self {
            Self::Transcription { fault, .. } | Self::Processing { fault, .. } => fault.status,
            _ => None,
        }
    }

    /// Short English user-facing hint for what to do. The frontend
    /// banner currently shows this string directly; full per-locale
    /// translation of recovery hints is a follow-up refactor.
    pub fn recovery_hint(&self) -> &'static str {
        match self.kind() {
            ErrorKind::Configuration => "Check your settings — a required field may be missing.",
            ErrorKind::Authentication => {
                "API key missing or invalid. Set it under Settings → Cloud API keys."
            }
            ErrorKind::HttpStatus => {
                "The provider rejected the request. Check the model ID and API limits."
            }
            ErrorKind::Network => {
                "Connection problem. Check internet/firewall — we'll retry automatically."
            }
            ErrorKind::Hardware => {
                "Audio hardware problem. Check the input device under Settings → Audio."
            }
            ErrorKind::InvalidInput => "Input validation failed. Please correct the fields.",
            ErrorKind::Unsupported => {
                "Feature not available in your environment (e.g. Wayland in early phases)."
            }
            ErrorKind::Internal => "Internal error — please report as a bug.",
            ErrorKind::Io => "Filesystem error. Check write permissions and free disk space.",
            ErrorKind::Other => "Unspecified error. See the Logs tab for details.",
        }
    }
}

/// Heuristic for Audio-stage errors. Accepts both the new English
/// strings ("no default input device", "not found") and the old
/// German ones ("kein standard-eingabegeraet", "nicht gefunden") so
/// historical error sources don't regress to the `else` fallback.
fn classify_audio(msg: &str) -> ErrorKind {
    let m = msg.to_ascii_lowercase();
    if m.contains("no default input device")
        || m.contains("kein standard-eingabegeraet")
        || m.contains("not found")
        || m.contains("nicht gefunden")
    {
        ErrorKind::Hardware
    } else if m.contains("permission") || m.contains("zugriff") || m.contains("denied") {
        ErrorKind::Configuration
    } else {
        ErrorKind::Hardware
    }
}

/// Classify a cloud-stage (Transcription/Processing) failure.
///
/// Reads the structured `ProviderFault` first; only when it carries no
/// status do we fall back to substring matching on the message. `#40`
/// populates `fault` at every client and drops the substring branch.
fn classify_provider(msg: &str, fault: &ProviderFault) -> ErrorKind {
    if let Some(status) = fault.status {
        return if status == 401 || status == 403 {
            ErrorKind::Authentication
        } else {
            ErrorKind::HttpStatus
        };
    }
    classify_network_or_other(msg)
}

fn classify_network_or_other(msg: &str) -> ErrorKind {
    let m = msg.to_ascii_lowercase();
    if m.contains("http 4") || m.contains("http 5") {
        // From the http {status}-format strings in our clients.
        if m.contains("http 401") || m.contains("http 403") {
            ErrorKind::Authentication
        } else {
            ErrorKind::HttpStatus
        }
    } else if m.contains("http") || m.contains("timeout") || m.contains("connection") {
        ErrorKind::Network
    } else if m.contains("api key")
        || m.contains("api-key")
        || m.contains("nicht gesetzt")
        || m.contains("not set")
    {
        ErrorKind::Authentication
    } else if m.contains("not implemented")
        || m.contains("nicht implementiert")
        || m.contains("phase ")
    {
        ErrorKind::Unsupported
    } else {
        ErrorKind::Other
    }
}

fn classify_injection(msg: &str) -> ErrorKind {
    let m = msg.to_ascii_lowercase();
    if m.contains("wayland") || m.contains("not implemented") || m.contains("nicht implementiert") {
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
    fn audio_no_device_classifies_as_hardware_en() {
        let e = VoiceTypeError::Audio("No default input device".into());
        assert_eq!(e.kind(), ErrorKind::Hardware);
        assert!(!e.is_retryable());
    }

    #[test]
    fn audio_no_device_classifies_as_hardware_de_legacy() {
        // Backwards-compat: pre-phase-4 audio code still produces
        // German strings (see audio/recorder.rs). Classifier must
        // accept both until those are normalised too.
        let e = VoiceTypeError::Audio("Kein Standard-Eingabegeraet".into());
        assert_eq!(e.kind(), ErrorKind::Hardware);
    }

    #[test]
    fn audio_permission_classifies_as_configuration() {
        let e = VoiceTypeError::Audio("permission denied".into());
        assert_eq!(e.kind(), ErrorKind::Configuration);
    }

    #[test]
    fn http_500_is_retryable() {
        let e = VoiceTypeError::processing("HTTP 502: Bad Gateway");
        assert_eq!(e.kind(), ErrorKind::HttpStatus);
        assert!(e.is_retryable());
    }

    #[test]
    fn http_401_is_authentication_not_retryable() {
        let e = VoiceTypeError::processing("HTTP 401: Unauthorized");
        assert_eq!(e.kind(), ErrorKind::Authentication);
        assert!(!e.is_retryable());
    }

    #[test]
    fn http_429_rate_limit_is_retryable() {
        let e = VoiceTypeError::transcription("HTTP 429: Too Many Requests");
        assert!(e.is_retryable());
    }

    #[test]
    fn missing_api_key_classifies_as_authentication_en() {
        let e = VoiceTypeError::transcription("No API key set for 'xai'");
        assert_eq!(e.kind(), ErrorKind::Authentication);
    }

    #[test]
    fn missing_api_key_classifies_as_authentication_de_legacy() {
        let e = VoiceTypeError::transcription("API-Key fuer Provider 'xai' nicht gesetzt");
        assert_eq!(e.kind(), ErrorKind::Authentication);
    }

    #[test]
    fn structured_status_classifies_without_substring_match() {
        // `#40` will populate `fault`; verify the structured path works
        // and takes precedence over the (here absent) substring signal.
        let e = VoiceTypeError::Transcription {
            message: "opaque provider error".into(),
            fault: ProviderFault {
                status: Some(503),
                provider: Some(ProviderId::Groq),
            },
        };
        assert_eq!(e.kind(), ErrorKind::HttpStatus);
        assert!(e.is_retryable());

        let e = VoiceTypeError::Processing {
            message: "opaque provider error".into(),
            fault: ProviderFault {
                status: Some(401),
                provider: Some(ProviderId::Anthropic),
            },
        };
        assert_eq!(e.kind(), ErrorKind::Authentication);
        assert!(!e.is_retryable());
    }

    #[test]
    fn wayland_hotkey_is_unsupported() {
        let e = VoiceTypeError::Hotkey("Wayland support is coming later".into());
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
        let e = VoiceTypeError::Mode("id must not be empty".into());
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
                    ErrorKind::Network => VoiceTypeError::processing("HTTP timeout"),
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
