// SPDX-License-Identifier: GPL-3.0-or-later
//! Usage limits of the ChatGPT plan (Codex budget).
//!
//! Two undocumented sources, both observed with a Plus account on
//! 2026-09-28:
//! - `GET https://chatgpt.com/backend-api/wham/usage` (what OpenClaw and the
//!   Codex CLI use) — fetched on demand, see `fetch`.
//! - `x-codex-*` headers on every `/codex/responses` answer — read for free
//!   after each post-processing call, see `from_headers`. `/transcribe`
//!   sends none.
//!
//! Observed windows: primary = 5 hours (300 min), secondary = 1 week
//! (10080 min). Either may be missing; the UI renders what is reported.

use super::api::{self, authed};
use super::session::Access;
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct UsageWindow {
    pub used_percent: f64,
    pub window_minutes: Option<i64>,
    /// Unix seconds.
    pub resets_at: Option<i64>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ChatGptUsage {
    pub primary: Option<UsageWindow>,
    pub secondary: Option<UsageWindow>,
    pub limit_reached: bool,
    /// Unix seconds when this snapshot was taken.
    pub fetched_at: i64,
}

#[derive(Deserialize)]
struct UsageBody {
    #[serde(default)]
    rate_limit: Option<RateLimit>,
}

#[derive(Deserialize)]
struct RateLimit {
    #[serde(default)]
    limit_reached: bool,
    #[serde(default)]
    primary_window: Option<BodyWindow>,
    #[serde(default)]
    secondary_window: Option<BodyWindow>,
}

#[derive(Deserialize)]
struct BodyWindow {
    used_percent: f64,
    #[serde(default)]
    limit_window_seconds: Option<i64>,
    #[serde(default)]
    reset_at: Option<i64>,
    #[serde(default)]
    reset_after_seconds: Option<i64>,
}

impl BodyWindow {
    fn into_window(self, now: i64) -> UsageWindow {
        UsageWindow {
            used_percent: self.used_percent,
            window_minutes: self.limit_window_seconds.map(|s| s / 60),
            resets_at: self.reset_at.or(self.reset_after_seconds.map(|s| now + s)),
        }
    }
}

/// `None` for an unknown shape — the UI then shows "unavailable".
fn parse_usage_body(body: &str, now: i64) -> Option<ChatGptUsage> {
    let rl = serde_json::from_str::<UsageBody>(body).ok()?.rate_limit?;
    let primary = rl.primary_window.map(|w| w.into_window(now));
    let secondary = rl.secondary_window.map(|w| w.into_window(now));
    let limit_reached = rl.limit_reached || any_full(&primary, &secondary);
    Some(ChatGptUsage {
        primary,
        secondary,
        limit_reached,
        fetched_at: now,
    })
}

/// Rate-limit headers of a `/codex/responses` answer (names as read by the
/// Codex CLI, `codex-api/src/rate_limits.rs`). `None` if none are present.
pub fn from_headers(headers: &HeaderMap, now: i64) -> Option<ChatGptUsage> {
    let primary = header_window(headers, "primary");
    let secondary = header_window(headers, "secondary");
    if primary.is_none() && secondary.is_none() {
        return None;
    }
    let limit_reached = any_full(&primary, &secondary);
    Some(ChatGptUsage {
        primary,
        secondary,
        limit_reached,
        fetched_at: now,
    })
}

fn header_window(headers: &HeaderMap, which: &str) -> Option<UsageWindow> {
    let get = |suffix: &str| {
        headers
            .get(format!("x-codex-{which}-{suffix}"))
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
    };
    let used_percent = get("used-percent")?.parse::<f64>().ok()?;
    let window_minutes = get("window-minutes").and_then(|v| v.parse().ok());
    let resets_at = get("reset-at").and_then(|v| v.parse().ok());
    // Same rule as the Codex CLI: an all-empty window is no window.
    if used_percent == 0.0 && window_minutes.is_none() && resets_at.is_none() {
        return None;
    }
    Some(UsageWindow {
        used_percent,
        window_minutes,
        resets_at,
    })
}

fn any_full(primary: &Option<UsageWindow>, secondary: &Option<UsageWindow>) -> bool {
    [primary, secondary]
        .into_iter()
        .flatten()
        .any(|w| w.used_percent >= 100.0)
}

pub enum FetchError {
    /// 401: the caller refreshes the token and retries once.
    Unauthorized,
    Other(String),
}

pub async fn fetch(
    client: &reqwest::Client,
    access: &Access,
    now: i64,
) -> Result<ChatGptUsage, FetchError> {
    let resp = authed(client.get(USAGE_URL), access)
        .header(reqwest::header::ACCEPT, "application/json")
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|e| FetchError::Other(format!("ChatGPT usage: {e}")))?;
    let status = resp.status().as_u16();
    if status == 401 {
        return Err(FetchError::Unauthorized);
    }
    let cf_mitigated = resp.headers().contains_key("cf-mitigated");
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = resp.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        let failure = api::classify_failure(status, cf_mitigated, &content_type, &body, now);
        return Err(FetchError::Other(failure.message));
    }
    parse_usage_body(&body, now)
        .ok_or_else(|| FetchError::Other("ChatGPT usage: unexpected response".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderName, HeaderValue};

    /// Shape of a live response (2026-09-28, Plus account), ids redacted.
    const LIVE_BODY: &str = r#"{
        "account_id": "<redacted>", "plan_type": "plus", "email": "<redacted>",
        "credits": {"balance": "0", "has_credits": false, "unlimited": false},
        "model_usage": {"gpt-6-astra": {"available": true}},
        "rate_limit": {
            "allowed": true, "limit_reached": false,
            "primary_window": {"limit_window_seconds": 18000, "reset_after_seconds": 16762,
                               "reset_at": 1790607587, "used_percent": 0},
            "secondary_window": {"limit_window_seconds": 604800, "reset_after_seconds": 603562,
                                 "reset_at": 1791194387, "used_percent": 81}
        },
        "rate_limit_reached_type": null
    }"#;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    #[test]
    fn parses_the_live_body() {
        let u = parse_usage_body(LIVE_BODY, 100).unwrap();
        assert_eq!(
            u.primary,
            Some(UsageWindow {
                used_percent: 0.0,
                window_minutes: Some(300),
                resets_at: Some(1790607587)
            })
        );
        assert_eq!(u.secondary.as_ref().unwrap().window_minutes, Some(10080));
        assert_eq!(u.secondary.as_ref().unwrap().used_percent, 81.0);
        assert!(!u.limit_reached);
        assert_eq!(u.fetched_at, 100);
    }

    #[test]
    fn falls_back_to_reset_after_and_detects_a_full_window() {
        let body = r#"{"rate_limit": {"primary_window": {"used_percent": 100,
                        "reset_after_seconds": 60}}}"#;
        let u = parse_usage_body(body, 1_000).unwrap();
        assert_eq!(u.primary.as_ref().unwrap().resets_at, Some(1_060));
        assert_eq!(u.secondary, None);
        assert!(u.limit_reached);
    }

    #[test]
    fn unknown_shapes_are_unavailable() {
        assert_eq!(parse_usage_body("{}", 0), None);
        assert_eq!(parse_usage_body("<html>", 0), None);
        assert_eq!(
            parse_usage_body(
                r#"{"rate_limit": {"primary_window": {"used_percent": "x"}}}"#,
                0
            ),
            None
        );
    }

    #[test]
    fn reads_the_rate_limit_headers() {
        let h = headers(&[
            ("x-codex-primary-used-percent", "12.5"),
            ("x-codex-primary-window-minutes", "300"),
            ("x-codex-primary-reset-at", "1790607587"),
            ("x-codex-secondary-used-percent", "100"),
            ("x-codex-secondary-window-minutes", "10080"),
            ("x-codex-secondary-reset-at", "1791194387"),
        ]);
        let u = from_headers(&h, 5).unwrap();
        assert_eq!(u.primary.as_ref().unwrap().used_percent, 12.5);
        assert_eq!(u.primary.as_ref().unwrap().window_minutes, Some(300));
        assert!(u.limit_reached);
        assert_eq!(u.fetched_at, 5);
    }

    #[test]
    fn missing_or_empty_headers_yield_nothing() {
        assert_eq!(from_headers(&HeaderMap::new(), 0), None);
        let empty = headers(&[("x-codex-primary-used-percent", "0")]);
        assert_eq!(from_headers(&empty, 0), None);
    }

    #[test]
    fn usage_serializes_with_the_pinned_keys() {
        let u = parse_usage_body(LIVE_BODY, 1).unwrap();
        let v = serde_json::to_value(&u).unwrap();
        let mut keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            ["fetched_at", "limit_reached", "primary", "secondary"]
        );
        let mut wkeys: Vec<_> = v["primary"].as_object().unwrap().keys().cloned().collect();
        wkeys.sort();
        assert_eq!(wkeys, ["resets_at", "used_percent", "window_minutes"]);
    }
}
