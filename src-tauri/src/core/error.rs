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
/// The client call sites populate this at the point the failure is known
/// (HTTP status from the response, or the connect-before-response case),
/// and `kind()`/`is_retryable()` read it exclusively — there is no
/// substring fallback on the message. The `message` field is for display
/// only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderFault {
    /// What kind of failure this is, independent of the wording.
    pub class: FaultClass,
    /// HTTP status from the provider response, when the failure came from
    /// an HTTP exchange. `None` for non-HTTP failures (parse, IO, …).
    pub status: Option<u16>,
    /// Which provider the failure originated from, when known.
    pub provider: Option<ProviderId>,
}

impl ProviderFault {
    /// Failure from a completed HTTP exchange with a non-success status.
    pub fn http(status: u16, provider: ProviderId) -> Self {
        Self {
            class: FaultClass::Http,
            status: Some(status),
            provider: Some(provider),
        }
    }

    /// The request never produced an HTTP response (timeout, DNS,
    /// connection refused). Transient — retryable.
    pub fn network(provider: ProviderId) -> Self {
        Self {
            class: FaultClass::Network,
            status: None,
            provider: Some(provider),
        }
    }

    /// Opaque/internal failure with no transport signal (response parse,
    /// empty body, unknown-provider factory miss). Not retryable.
    pub fn other() -> Self {
        Self {
            class: FaultClass::Other,
            status: None,
            provider: None,
        }
    }
}

/// What a [`ProviderFault`] represents, decoupled from the display
/// message. Drives `kind()`/`is_retryable()` without any string matching.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FaultClass {
    /// HTTP exchange completed with a non-success status (`status` set).
    Http,
    /// No HTTP response was produced (timeout/DNS/connection refused).
    Network,
    /// Anything else (parse error, empty response, factory miss).
    #[default]
    Other,
}

/// Cloud provider a transcription/processing failure originated from.
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
    /// Construct a `Transcription` error for a non-transport failure
    /// (parse error, missing model, factory miss). Classifies as `Other`.
    pub fn transcription(message: impl Into<String>) -> Self {
        Self::Transcription {
            message: message.into(),
            fault: ProviderFault::other(),
        }
    }

    /// Construct a `Transcription` error from a completed HTTP exchange
    /// with a non-success status.
    pub fn transcription_http(
        status: u16,
        provider: ProviderId,
        message: impl Into<String>,
    ) -> Self {
        Self::Transcription {
            message: message.into(),
            fault: ProviderFault::http(status, provider),
        }
    }

    /// Construct a `Transcription` error for a request that never
    /// produced an HTTP response (timeout/DNS/connection refused).
    pub fn transcription_network(provider: ProviderId, message: impl Into<String>) -> Self {
        Self::Transcription {
            message: message.into(),
            fault: ProviderFault::network(provider),
        }
    }

    /// Construct a `Processing` error for a non-transport failure
    /// (parse error, empty response, factory miss). Classifies as `Other`.
    pub fn processing(message: impl Into<String>) -> Self {
        Self::Processing {
            message: message.into(),
            fault: ProviderFault::other(),
        }
    }

    /// Construct a `Processing` error from a completed HTTP exchange
    /// with a non-success status.
    pub fn processing_http(status: u16, provider: ProviderId, message: impl Into<String>) -> Self {
        Self::Processing {
            message: message.into(),
            fault: ProviderFault::http(status, provider),
        }
    }

    /// Construct a `Processing` error for a request that never produced
    /// an HTTP response (timeout/DNS/connection refused).
    pub fn processing_network(provider: ProviderId, message: impl Into<String>) -> Self {
        Self::Processing {
            message: message.into(),
            fault: ProviderFault::network(provider),
        }
    }

    /// Classify the error into a machine-readable category.
    ///
    /// The cloud stages read the structured `ProviderFault` exclusively —
    /// no substring matching on the message. Audio/Injection/Hotkey keep
    /// their string heuristics because those sources are not yet
    /// structured (tracked separately).
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::Audio(msg) => classify_audio(msg),
            Self::Transcription { fault, .. } => classify_provider(fault),
            Self::Processing { fault, .. } => classify_provider(fault),
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

    /// Worth retrying? Transient errors (network, 5xx, 429) yes,
    /// everything else no.
    pub fn is_retryable(&self) -> bool {
        match self.kind() {
            ErrorKind::Network => true,
            ErrorKind::HttpStatus => {
                // Only the structured status decides; 5xx and 429 are
                // transient. A `HttpStatus` error without a status cannot
                // occur — it always comes from `ProviderFault::http`.
                matches!(self.fault_status(), Some(s) if s >= 500 || s == 429)
            }
            _ => false,
        }
    }

    /// The structured HTTP status carried by a cloud-stage error, if any.
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

/// Classify a cloud-stage (Transcription/Processing) failure purely from
/// its structured `ProviderFault` — no substring matching on the message.
fn classify_provider(fault: &ProviderFault) -> ErrorKind {
    match fault.class {
        FaultClass::Http => match fault.status {
            Some(401) | Some(403) => ErrorKind::Authentication,
            _ => ErrorKind::HttpStatus,
        },
        FaultClass::Network => ErrorKind::Network,
        FaultClass::Other => ErrorKind::Other,
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
    fn http_5xx_is_retryable() {
        let e = VoiceTypeError::processing_http(502, ProviderId::Anthropic, "Anthropic HTTP 502");
        assert_eq!(e.kind(), ErrorKind::HttpStatus);
        assert!(e.is_retryable());
    }

    #[test]
    fn http_401_is_authentication_not_retryable() {
        let e = VoiceTypeError::processing_http(401, ProviderId::OpenAi, "HTTP 401");
        assert_eq!(e.kind(), ErrorKind::Authentication);
        assert!(!e.is_retryable());
    }

    #[test]
    fn http_403_is_authentication_not_retryable() {
        let e = VoiceTypeError::transcription_http(403, ProviderId::Xai, "xAI STT HTTP 403");
        assert_eq!(e.kind(), ErrorKind::Authentication);
        assert!(!e.is_retryable());
    }

    #[test]
    fn http_429_rate_limit_is_retryable() {
        let e = VoiceTypeError::transcription_http(429, ProviderId::Groq, "Whisper-API HTTP 429");
        assert_eq!(e.kind(), ErrorKind::HttpStatus);
        assert!(e.is_retryable());
    }

    #[test]
    fn http_400_is_http_status_not_retryable() {
        let e = VoiceTypeError::processing_http(400, ProviderId::OpenAi, "HTTP 400");
        assert_eq!(e.kind(), ErrorKind::HttpStatus);
        assert!(!e.is_retryable());
    }

    #[test]
    fn network_fault_is_retryable() {
        // Connect-before-response: no HTTP status, transient.
        let e =
            VoiceTypeError::transcription_network(ProviderId::Deepgram, "HTTP <url>: timed out");
        assert_eq!(e.kind(), ErrorKind::Network);
        assert!(e.is_retryable());
    }

    #[test]
    fn missing_api_key_classifies_as_authentication() {
        // Factory routes the missing-key case through `Secrets`.
        let e = VoiceTypeError::Secrets("No API key set for provider 'xai'".into());
        assert_eq!(e.kind(), ErrorKind::Authentication);
        assert!(!e.is_retryable());
    }

    #[test]
    fn opaque_provider_error_classifies_as_other() {
        // Non-transport failures (parse, empty response, factory miss)
        // carry `FaultClass::Other` and classify as `Other` — no
        // substring inspection of the message.
        let e = VoiceTypeError::transcription("xAI-STT-JSON-Parse: trailing data");
        assert_eq!(e.kind(), ErrorKind::Other);
        assert!(!e.is_retryable());
    }

    #[test]
    fn structured_status_classifies_without_substring_match() {
        // The message is opaque on purpose: classification is purely
        // structural.
        let e = VoiceTypeError::Transcription {
            message: "opaque provider error".into(),
            fault: ProviderFault::http(503, ProviderId::Groq),
        };
        assert_eq!(e.kind(), ErrorKind::HttpStatus);
        assert!(e.is_retryable());

        let e = VoiceTypeError::Processing {
            message: "opaque provider error".into(),
            fault: ProviderFault::http(401, ProviderId::Anthropic),
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
                    ErrorKind::Network => {
                        VoiceTypeError::processing_network(ProviderId::OpenAi, "HTTP timeout")
                    }
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
