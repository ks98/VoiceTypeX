// SPDX-License-Identifier: GPL-3.0-or-later
//! Request headers and failure classification shared by the ChatGPT
//! backend calls (`chatgpt.com/backend-api/…`). The endpoints are
//! undocumented; the error shapes follow what OpenClaw parses
//! (`{"error": {"code"|"type", "message", "resets_at"}}`) plus the
//! `{"detail": "…"}` form observed for rejected requests.

use super::oauth::ORIGINATOR;
use super::session::Access;
use reqwest::RequestBuilder;

const USER_AGENT: &str = concat!("VoiceTypeX/", env!("CARGO_PKG_VERSION"));

/// Honest identification: our own originator and User-Agent — never a
/// browser or another OpenAI client, even if a request gets blocked.
pub fn authed(req: RequestBuilder, access: &Access) -> RequestBuilder {
    req.bearer_auth(&access.token)
        .header("ChatGPT-Account-Id", &access.account_id)
        .header("originator", ORIGINATOR)
        .header("User-Agent", USER_AGENT)
}

#[derive(Debug, PartialEq)]
pub enum FailureKind {
    /// The token was rejected — a new sign-in is needed.
    Auth,
    /// Definitive: usage limit, plan does not include it, blocked.
    Rejected,
    /// Anything else with an HTTP status; 429/5xx are retried.
    Http,
}

#[derive(Debug, PartialEq)]
pub struct Failure {
    pub kind: FailureKind,
    pub status: u16,
    pub message: String,
}

/// `now` (Unix seconds) turns `resets_at` into a relative hint.
pub fn classify_failure(
    status: u16,
    cf_mitigated: bool,
    content_type: &str,
    body: &str,
    now: i64,
) -> Failure {
    let json: Option<serde_json::Value> = serde_json::from_str(body).ok();
    let err = json.as_ref().and_then(|j| j.get("error"));
    let code = err
        .and_then(|e| e.get("code").or_else(|| e.get("type")))
        .and_then(|c| c.as_str())
        .map(str::to_ascii_lowercase);
    let detail = json
        .as_ref()
        .and_then(|j| j.get("detail"))
        .or_else(|| err.and_then(|e| e.get("message")))
        .and_then(|d| d.as_str())
        .map(|d| d.chars().take(200).collect::<String>());
    let fail = |kind, message: String| Failure {
        kind,
        status,
        message,
    };

    match code.as_deref() {
        Some("usage_limit_reached") => {
            let when = err
                .and_then(|e| e.get("resets_at"))
                .and_then(|r| r.as_i64())
                .map(|at| format!(" — resets in {}", human_duration(at - now)))
                .unwrap_or_default();
            return fail(
                FailureKind::Rejected,
                format!("ChatGPT usage limit reached{when}"),
            );
        }
        Some("usage_not_included") => {
            return fail(
                FailureKind::Rejected,
                "Your ChatGPT plan does not include this feature".into(),
            );
        }
        _ => {}
    }
    if status == 403 && (cf_mitigated || content_type.starts_with("text/html")) {
        return fail(
            FailureKind::Rejected,
            "ChatGPT blocked the request (Cloudflare); the unofficial endpoint may be unavailable for VoiceTypeX".into(),
        );
    }
    let suffix = detail.map(|d| format!(": {d}")).unwrap_or_default();
    match status {
        401 | 403 => fail(
            FailureKind::Auth,
            format!("ChatGPT rejected the sign-in (HTTP {status}) — sign in again under Settings → ChatGPT account"),
        ),
        404 | 405 | 415 => fail(
            FailureKind::Http,
            format!("ChatGPT HTTP {status}{suffix} (the unofficial endpoint may have changed)"),
        ),
        _ => fail(FailureKind::Http, format!("ChatGPT HTTP {status}{suffix}")),
    }
}

fn human_duration(secs: i64) -> String {
    let mins = (secs.max(0) + 59) / 60;
    match (mins / 60, mins % 60) {
        (0, m) => format!("~{m} min"),
        (h, 0) => format!("~{h} h"),
        (h, m) => format!("~{h} h {m} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(status: u16, cf: bool, ctype: &str, body: &str) -> Failure {
        classify_failure(status, cf, ctype, body, 1_000)
    }

    #[test]
    fn usage_limit_with_reset_time_is_a_definitive_rejection() {
        let f = classify(
            429,
            false,
            "application/json",
            r#"{"error":{"type":"usage_limit_reached","resets_at":8980}}"#,
        );
        assert_eq!(f.kind, FailureKind::Rejected);
        assert_eq!(f.status, 429);
        assert_eq!(
            f.message,
            "ChatGPT usage limit reached — resets in ~2 h 13 min"
        );
    }

    #[test]
    fn plan_without_the_feature_is_rejected() {
        let f = classify(
            403,
            false,
            "application/json",
            r#"{"error":{"code":"usage_not_included"}}"#,
        );
        assert_eq!(f.kind, FailureKind::Rejected);
    }

    #[test]
    fn cloudflare_block_is_not_an_auth_problem() {
        assert_eq!(
            classify(403, true, "text/plain", "").kind,
            FailureKind::Rejected
        );
        assert_eq!(
            classify(403, false, "text/html; charset=utf-8", "<html>").kind,
            FailureKind::Rejected
        );
        assert_eq!(
            classify(403, false, "application/json", "{}").kind,
            FailureKind::Auth
        );
    }

    #[test]
    fn auth_rate_limit_and_changed_endpoint() {
        assert_eq!(classify(401, false, "", "").kind, FailureKind::Auth);
        let rl = classify(429, false, "application/json", "{}");
        assert_eq!((rl.kind, rl.status), (FailureKind::Http, 429));
        let bad_model = classify(
            400,
            false,
            "application/json",
            r#"{"detail":"The 'gpt-5' model is not supported"}"#,
        );
        assert_eq!(bad_model.kind, FailureKind::Http);
        assert_eq!(
            bad_model.message,
            "ChatGPT HTTP 400: The 'gpt-5' model is not supported"
        );
        assert!(classify(404, false, "text/html", "")
            .message
            .contains("may have changed"));
        assert_eq!(
            classify(502, false, "text/html", "").kind,
            FailureKind::Http
        );
    }

    #[test]
    fn durations_round_up_to_minutes() {
        assert_eq!(human_duration(-5), "~0 min");
        assert_eq!(human_duration(61), "~2 min");
        assert_eq!(human_duration(3600), "~1 h");
        assert_eq!(human_duration(7980), "~2 h 13 min");
    }
}
