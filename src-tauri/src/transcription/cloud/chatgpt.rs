// SPDX-License-Identifier: GPL-3.0-or-later
//! Speech-to-text with a ChatGPT subscription (experimental).
//!
//! Undocumented endpoint (the one Codex Desktop dictation uses):
//!   POST https://chatgpt.com/backend-api/transcribe
//!   Authorization: Bearer {access token}, ChatGPT-Account-Id: {account}
//!   Content-Type: multipart/form-data
//!     - file: WAV bytes
//!     - language: optional ISO code (reportedly ignored by the backend)
//!   Response: { "text": "..." }
//!
//! Observed 2026-09-25: no truncation up to 326 s of speech, so the whole
//! recording is sent in one request. Identical repeated sentences are
//! collapsed by the backend. The initial prompt is not supported.

use crate::chatgpt::api::{self, FailureKind};
use crate::chatgpt::session::{Access, AccessError};
use crate::chatgpt::ChatGptSession;
use crate::core::error::{ProviderId, Result, VoiceTypeError};
use crate::core::retry::with_retry;
use crate::transcription::{TranscribeOpts, Transcriber};
use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const URL: &str = "https://chatgpt.com/backend-api/transcribe";
const TIMEOUT: Duration = Duration::from_secs(120);

pub struct ChatGptTranscriber {
    session: Arc<ChatGptSession>,
    client: reqwest::Client,
}

impl ChatGptTranscriber {
    pub fn new(session: Arc<ChatGptSession>, client: reqwest::Client) -> Self {
        Self { session, client }
    }

    async fn send(
        &self,
        access: &Access,
        audio: &[u8],
        opts: &TranscribeOpts,
    ) -> Result<reqwest::Response> {
        // `multipart::Form` is not Clone — rebuild per attempt.
        let part = reqwest::multipart::Part::bytes(audio.to_vec())
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|e| VoiceTypeError::transcription(format!("multipart part: {e}")))?;
        let mut form = reqwest::multipart::Form::new().part("file", part);
        if let Some(lang) = opts.language.as_deref() {
            form = form.text("language", lang.to_string());
        }
        api::authed(self.client.post(URL), access)
            .timeout(TIMEOUT)
            .multipart(form)
            .send()
            .await
            .map_err(|e| {
                VoiceTypeError::transcription_network(
                    ProviderId::ChatGpt,
                    format!("ChatGPT transcribe: {e}"),
                )
            })
    }

    async fn attempt(&self, audio: &[u8], opts: &TranscribeOpts) -> Result<String> {
        let access = self
            .session
            .access(&self.client)
            .await
            .map_err(access_error)?;
        let mut resp = self.send(&access, audio, opts).await?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            let access = self
                .session
                .refresh_after_401(&self.client, &access.token)
                .await
                .map_err(access_error)?;
            resp = self.send(&access, audio, opts).await?;
        }

        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let cf_mitigated = resp.headers().contains_key("cf-mitigated");
            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            let body = resp.text().await.unwrap_or_default();
            let failure =
                api::classify_failure(status, cf_mitigated, &content_type, &body, now_unix());
            tracing::warn!(status, message = %failure.message, "ChatGPT transcribe failed");
            return Err(match failure.kind {
                FailureKind::Rejected => VoiceTypeError::transcription_rejected(
                    status,
                    ProviderId::ChatGpt,
                    failure.message,
                ),
                FailureKind::Auth | FailureKind::Http => {
                    VoiceTypeError::transcription_http(status, ProviderId::ChatGpt, failure.message)
                }
            });
        }
        let parsed: TranscribeResponse = resp.json().await.map_err(|e| {
            VoiceTypeError::transcription(format!("ChatGPT transcribe response: {e}"))
        })?;
        Ok(parsed.text.trim().to_string())
    }
}

#[async_trait]
impl Transcriber for ChatGptTranscriber {
    fn name(&self) -> &str {
        "chatgpt"
    }

    async fn transcribe_oneshot(&self, audio: &[u8], opts: TranscribeOpts) -> Result<String> {
        with_retry(|| self.attempt(audio, &opts)).await
    }
}

#[derive(Deserialize)]
struct TranscribeResponse {
    text: String,
}

/// Missing/expired sign-in behaves like a 401 (auth, not retried); a
/// transient refresh failure like a network error (retried).
fn access_error(e: AccessError) -> VoiceTypeError {
    match e {
        AccessError::NotSignedIn | AccessError::Expired => {
            VoiceTypeError::transcription_http(401, ProviderId::ChatGpt, e.to_string())
        }
        AccessError::Unavailable(_) => {
            VoiceTypeError::transcription_network(ProviderId::ChatGpt, e.to_string())
        }
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::error::ErrorKind;

    #[test]
    fn missing_sign_in_is_an_auth_error_pointing_to_settings() {
        let e = access_error(AccessError::NotSignedIn);
        assert_eq!(e.kind(), ErrorKind::Authentication);
        assert!(!e.is_retryable());
        assert!(e.to_string().contains("Settings → ChatGPT account"));
        assert!(e.recovery_hint().contains("ChatGPT account"));
    }

    #[test]
    fn transient_refresh_failure_is_retried() {
        let e = access_error(AccessError::Unavailable("HTTP 503".into()));
        assert_eq!(e.kind(), ErrorKind::Network);
        assert!(e.is_retryable());
    }

    #[test]
    fn response_text_is_parsed() {
        let r: TranscribeResponse = serde_json::from_str(r#"{"text":" Hallo Welt. "}"#).unwrap();
        assert_eq!(r.text.trim(), "Hallo Welt.");
    }
}
