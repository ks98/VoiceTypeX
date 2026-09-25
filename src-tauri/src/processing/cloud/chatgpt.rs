// SPDX-License-Identifier: GPL-3.0-or-later
//! LLM post-processing with a ChatGPT subscription (experimental).
//!
//! Undocumented for third parties (the Codex backend):
//!   POST https://chatgpt.com/backend-api/codex/responses
//!   Authorization: Bearer {access token}, ChatGPT-Account-Id: {account}
//!   Body: Responses API — { model, instructions, input: [user message],
//!         store: false, stream: true }
//!   Response: server-sent events; the text arrives as
//!   `response.output_text.delta`, the end as `response.completed`.
//!
//! The backend requires `store: false` and streaming. Sampling parameters
//! (temperature, top_p, …) and max_tokens are not sent. Of the models
//! tried with a ChatGPT account on 2026-09-25 only `gpt-5.5` was accepted.

use crate::chatgpt::api::{self, FailureKind};
use crate::chatgpt::session::{Access, AccessError};
use crate::chatgpt::ChatGptSession;
use crate::core::error::{ProviderId, Result, VoiceTypeError};
use crate::core::retry::with_retry;
use crate::processing::{ProcessOpts, Processor};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const URL: &str = "https://chatgpt.com/backend-api/codex/responses";
const DEFAULT_MODEL: &str = "gpt-5.5";
const TIMEOUT: Duration = Duration::from_secs(90);

pub struct ChatGptProcessor {
    session: Arc<ChatGptSession>,
    client: reqwest::Client,
}

impl ChatGptProcessor {
    pub fn new(session: Arc<ChatGptSession>, client: reqwest::Client) -> Self {
        Self { session, client }
    }

    async fn send(&self, access: &Access, body: &serde_json::Value) -> Result<reqwest::Response> {
        api::authed(self.client.post(URL), access)
            .timeout(TIMEOUT)
            .header("OpenAI-Beta", "responses=experimental")
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .json(body)
            .send()
            .await
            .map_err(|e| {
                VoiceTypeError::processing_network(
                    ProviderId::ChatGpt,
                    format!("ChatGPT responses: {e}"),
                )
            })
    }

    async fn attempt(&self, body: &serde_json::Value) -> Result<String> {
        let access = self
            .session
            .access(&self.client)
            .await
            .map_err(access_error)?;
        let mut resp = self.send(&access, body).await?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            let access = self
                .session
                .refresh_after_401(&self.client, &access.token)
                .await
                .map_err(access_error)?;
            resp = self.send(&access, body).await?;
        }

        let status = resp.status().as_u16();
        let cf_mitigated = resp.headers().contains_key("cf-mitigated");
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let text = resp.text().await.map_err(|e| {
            VoiceTypeError::processing_network(
                ProviderId::ChatGpt,
                format!("ChatGPT responses stream: {e}"),
            )
        })?;
        if !(200..300).contains(&status) {
            let failure =
                api::classify_failure(status, cf_mitigated, &content_type, &text, now_unix());
            tracing::warn!(status, message = %failure.message, "ChatGPT responses failed");
            return Err(match failure.kind {
                FailureKind::Rejected => VoiceTypeError::processing_rejected(
                    status,
                    ProviderId::ChatGpt,
                    failure.message,
                ),
                FailureKind::Auth | FailureKind::Http => {
                    VoiceTypeError::processing_http(status, ProviderId::ChatGpt, failure.message)
                }
            });
        }
        // A stream that fails or ends early never yields partial text —
        // injecting half a rewrite would be worse than an error.
        collect_output_text(&text).map_err(|e| VoiceTypeError::processing(format!("ChatGPT: {e}")))
    }
}

#[async_trait]
impl Processor for ChatGptProcessor {
    fn name(&self) -> &str {
        "chatgpt"
    }

    async fn process(
        &self,
        transcript: &str,
        system_prompt: &str,
        opts: ProcessOpts,
    ) -> Result<String> {
        let body = request_body(
            opts.model.as_deref().unwrap_or(DEFAULT_MODEL),
            system_prompt,
            transcript,
        );
        with_retry(|| self.attempt(&body)).await
    }
}

fn request_body(model: &str, instructions: &str, transcript: &str) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "instructions": instructions,
        "input": [{
            "role": "user",
            "content": [{ "type": "input_text", "text": transcript }],
        }],
        "store": false,
        "stream": true,
    })
}

/// Collects the output text from the server-sent event stream: deltas are
/// concatenated; `response.completed` must arrive, `response.failed` and
/// `error` events end with an error.
fn collect_output_text(sse: &str) -> std::result::Result<String, String> {
    let mut out = String::new();
    let mut completed: Option<serde_json::Value> = None;
    for event in sse.replace("\r\n", "\n").split("\n\n") {
        let data: Vec<&str> = event
            .lines()
            .filter_map(|l| l.strip_prefix("data:").map(str::trim_start))
            .collect();
        if data.is_empty() {
            continue;
        }
        let payload = data.join("\n");
        if payload == "[DONE]" {
            continue;
        }
        let Ok(ev) = serde_json::from_str::<serde_json::Value>(&payload) else {
            continue;
        };
        match ev["type"].as_str() {
            Some("response.output_text.delta") => out.push_str(ev["delta"].as_str().unwrap_or("")),
            Some("response.completed") => completed = Some(ev),
            Some("response.failed") | Some("error") => {
                let err = if ev["response"]["error"].is_object() {
                    &ev["response"]["error"]
                } else if ev["error"].is_object() {
                    &ev["error"]
                } else {
                    &ev
                };
                let msg = err["message"]
                    .as_str()
                    .or_else(|| err["code"].as_str())
                    .unwrap_or("unknown error");
                return Err(format!("the response failed: {msg}"));
            }
            _ => {}
        }
    }
    let completed = completed.ok_or("the response stream ended before it completed")?;
    if out.is_empty() {
        out = final_output_text(&completed);
    }
    Ok(out.trim().to_string())
}

/// Fallback when no deltas were streamed: the text parts of the final
/// response object.
fn final_output_text(completed: &serde_json::Value) -> String {
    completed["response"]["output"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|item| item["content"].as_array().into_iter().flatten())
        .filter(|part| part["type"] == "output_text")
        .filter_map(|part| part["text"].as_str())
        .collect()
}

fn access_error(e: AccessError) -> VoiceTypeError {
    match e {
        AccessError::NotSignedIn | AccessError::Expired => {
            VoiceTypeError::processing_http(401, ProviderId::ChatGpt, e.to_string())
        }
        AccessError::Unavailable(_) => {
            VoiceTypeError::processing_network(ProviderId::ChatGpt, e.to_string())
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

    fn sse(events: &[serde_json::Value]) -> String {
        events
            .iter()
            .map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap()))
            .collect()
    }

    #[test]
    fn concatenates_deltas_until_completed() {
        let body = sse(&[
            serde_json::json!({"type": "response.created"}),
            serde_json::json!({"type": "response.output_text.delta", "delta": "Hello "}),
            serde_json::json!({"type": "response.output_text.delta", "delta": "wörld."}),
            serde_json::json!({"type": "response.completed", "response": {}}),
        ]);
        assert_eq!(collect_output_text(&body), Ok("Hello wörld.".into()));
        assert_eq!(
            collect_output_text(&body.replace('\n', "\r\n")),
            Ok("Hello wörld.".into())
        );
    }

    #[test]
    fn falls_back_to_the_completed_response_output() {
        let body = sse(&[serde_json::json!({
            "type": "response.completed",
            "response": {"output": [{"type": "message", "content": [
                {"type": "output_text", "text": "Final text."}
            ]}]}
        })]);
        assert_eq!(collect_output_text(&body), Ok("Final text.".into()));
    }

    #[test]
    fn failed_or_truncated_streams_are_errors() {
        let failed = sse(&[
            serde_json::json!({"type": "response.output_text.delta", "delta": "Half"}),
            serde_json::json!({"type": "response.failed",
                "response": {"error": {"code": "server_error", "message": "boom"}}}),
        ]);
        assert_eq!(
            collect_output_text(&failed),
            Err("the response failed: boom".into())
        );
        let truncated = sse(&[serde_json::json!({
            "type": "response.output_text.delta", "delta": "Half"
        })]);
        assert!(collect_output_text(&truncated)
            .unwrap_err()
            .contains("ended before"));
    }

    #[test]
    fn request_uses_instructions_and_disables_storage() {
        let b = request_body("gpt-5.5", "Fix punctuation.", "hello world");
        assert_eq!(b["model"], "gpt-5.5");
        assert_eq!(b["instructions"], "Fix punctuation.");
        assert_eq!(b["input"][0]["content"][0]["text"], "hello world");
        assert_eq!(b["store"], false);
        assert_eq!(b["stream"], true);
    }

    #[test]
    fn missing_sign_in_is_an_auth_error() {
        let e = access_error(AccessError::Expired);
        assert_eq!(e.kind(), ErrorKind::Authentication);
        assert!(e.recovery_hint().contains("ChatGPT account"));
    }
}
