// SPDX-License-Identifier: GPL-3.0-or-later
//! OAuth 2.0 authorization-code flow with PKCE against auth.openai.com,
//! mirroring OpenAI's Codex CLI (`codex-rs/login/src/server.rs`).

use crate::core::error::{Result, VoiceTypeError};
use aes_gcm::aead::rand_core::RngCore;
use aes_gcm::aead::OsRng;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use reqwest::Url;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fmt;
use std::time::Duration;

/// Client id of OpenAI's Codex CLI; there is no public registration for
/// third-party apps, which is why this provider is experimental.
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
pub const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const SCOPE: &str = "openid profile email offline_access";
/// Sent honestly as ourselves — never impersonate another client.
pub const ORIGINATOR: &str = "voicetypex";
pub const CALLBACK_PATH: &str = "/auth/callback";
const TOKEN_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

pub fn generate_pkce() -> Pkce {
    let verifier = random_urlsafe(32);
    let challenge = challenge_for(&verifier);
    Pkce {
        verifier,
        challenge,
    }
}

pub fn generate_state() -> String {
    random_urlsafe(16)
}

fn random_urlsafe(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn challenge_for(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Codex registers the loopback redirect as `127.0.0.1`, not `localhost`
/// (which may resolve to `::1` while we only bind IPv4).
pub fn redirect_uri(port: u16) -> String {
    format!("http://127.0.0.1:{port}{CALLBACK_PATH}")
}

pub fn authorize_url(redirect_uri: &str, challenge: &str, state: &str) -> String {
    Url::parse_with_params(
        AUTHORIZE_URL,
        [
            ("response_type", "code"),
            ("client_id", CLIENT_ID),
            ("redirect_uri", redirect_uri),
            ("scope", SCOPE),
            ("code_challenge", challenge),
            ("code_challenge_method", "S256"),
            ("state", state),
            ("id_token_add_organizations", "true"),
            ("codex_cli_simplified_flow", "true"),
            ("originator", ORIGINATOR),
        ],
    )
    .expect("static authorize URL is valid")
    .to_string()
}

#[derive(Debug, PartialEq)]
pub enum CallbackError {
    NotACallbackUrl,
    StateMismatch,
    Denied(String),
    MissingCode,
}

impl fmt::Display for CallbackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotACallbackUrl => write!(
                f,
                "Not a sign-in callback address (expected …{CALLBACK_PATH}?code=…)"
            ),
            Self::StateMismatch => write!(
                f,
                "This sign-in response belongs to a different sign-in attempt"
            ),
            Self::Denied(reason) => write!(f, "Sign-in was declined: {reason}"),
            Self::MissingCode => write!(f, "The sign-in response contains no code"),
        }
    }
}

/// Extracts the authorization code from a callback, given either the full
/// redirect URL (pasted by the user) or the request target the loopback
/// server received (`/auth/callback?code=…`).
pub fn parse_callback(
    input: &str,
    expected_state: &str,
) -> std::result::Result<String, CallbackError> {
    let input = input.trim();
    let url = if input.starts_with('/') {
        Url::parse(&format!("http://127.0.0.1{input}"))
    } else {
        Url::parse(input)
    }
    .map_err(|_| CallbackError::NotACallbackUrl)?;
    if url.path() != CALLBACK_PATH {
        return Err(CallbackError::NotACallbackUrl);
    }
    let param = |name: &str| {
        url.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
    };
    if let Some(error) = param("error") {
        return Err(CallbackError::Denied(
            param("error_description").unwrap_or(error),
        ));
    }
    if param("state").as_deref() != Some(expected_state) {
        return Err(CallbackError::StateMismatch);
    }
    match param("code") {
        Some(code) if !code.is_empty() => Ok(code),
        _ => Err(CallbackError::MissingCode),
    }
}

#[derive(Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub id_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<i64>,
}

pub async fn exchange_code(
    client: &reqwest::Client,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<TokenResponse> {
    let resp = client
        .post(TOKEN_URL)
        .timeout(TOKEN_TIMEOUT)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", CLIENT_ID),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", redirect_uri),
        ])
        .send()
        .await
        .map_err(|e| other(format!("ChatGPT sign-in: token request failed: {e}")))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(other(format!(
            "ChatGPT sign-in: token exchange rejected (HTTP {}{})",
            status.as_u16(),
            error_code(&body)
                .map(|c| format!(": {c}"))
                .unwrap_or_default()
        )));
    }
    serde_json::from_str(&body)
        .map_err(|e| other(format!("ChatGPT sign-in: unexpected token response: {e}")))
}

#[derive(Debug, PartialEq)]
pub enum RefreshError {
    /// The refresh token is dead (expired, reused, revoked): only a new
    /// sign-in helps.
    Permanent(String),
    /// Network trouble or an unexpected server answer: worth trying again.
    Transient(String),
}

/// Exchanges the refresh token for a new token set. JSON body, like the
/// Codex CLI (`codex-rs/login/src/auth/manager.rs`). The refresh token
/// rotates: the caller must persist the new one or the session dies.
pub async fn refresh(
    client: &reqwest::Client,
    token_url: &str,
    refresh_token: &str,
) -> std::result::Result<TokenResponse, RefreshError> {
    let resp = client
        .post(token_url)
        .timeout(TOKEN_TIMEOUT)
        .json(&serde_json::json!({
            "client_id": CLIENT_ID,
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
        }))
        .send()
        .await
        .map_err(|e| RefreshError::Transient(format!("token refresh request failed: {e}")))?;
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(classify_refresh_failure(
            status,
            error_code(&body).as_deref(),
        ));
    }
    serde_json::from_str(&body)
        .map_err(|e| RefreshError::Transient(format!("unexpected token refresh response: {e}")))
}

/// Same rule as the Codex CLI: 401, a known dead-token code, or
/// `400 invalid_grant` are permanent; everything else is transient.
fn classify_refresh_failure(status: u16, code: Option<&str>) -> RefreshError {
    let code_lc = code.map(str::to_ascii_lowercase);
    let dead_token = matches!(
        code_lc.as_deref(),
        Some("refresh_token_expired" | "refresh_token_reused" | "refresh_token_invalidated")
    );
    let invalid_grant = status == 400 && code_lc.as_deref() == Some("invalid_grant");
    let detail = format!(
        "HTTP {status}{}",
        code.map(|c| format!(": {c}")).unwrap_or_default()
    );
    if status == 401 || dead_token || invalid_grant {
        RefreshError::Permanent(detail)
    } else {
        RefreshError::Transient(detail)
    }
}

/// Only the OAuth error code is surfaced — the body of a token response
/// must never end up in logs or messages.
fn error_code(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let err = v.get("error")?;
    err.as_str()
        .or_else(|| err.get("code").and_then(|c| c.as_str()))
        .map(str::to_owned)
}

fn other(msg: String) -> VoiceTypeError {
    VoiceTypeError::Other(anyhow::anyhow!(msg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_matches_rfc7636_appendix_b() {
        assert_eq!(
            challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn generated_pkce_is_consistent_and_urlsafe() {
        let p = generate_pkce();
        assert_eq!(p.verifier.len(), 43);
        assert_eq!(p.challenge, challenge_for(&p.verifier));
        assert!(p
            .verifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_ne!(generate_state(), generate_state());
    }

    #[test]
    fn authorize_url_carries_all_parameters() {
        let url = Url::parse(&authorize_url(&redirect_uri(1455), "chal", "st")).unwrap();
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(
            url.origin().ascii_serialization(),
            "https://auth.openai.com"
        );
        assert_eq!(url.path(), "/oauth/authorize");
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["client_id"], CLIENT_ID);
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:1455/auth/callback");
        assert_eq!(q["scope"], SCOPE);
        assert_eq!(q["code_challenge"], "chal");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["state"], "st");
        assert_eq!(q["originator"], "voicetypex");
        assert_eq!(q["codex_cli_simplified_flow"], "true");
        assert_eq!(q["id_token_add_organizations"], "true");
    }

    #[test]
    fn parses_full_url_and_request_target() {
        let full = "http://127.0.0.1:1455/auth/callback?code=abc&state=st";
        assert_eq!(parse_callback(full, "st"), Ok("abc".into()));
        assert_eq!(
            parse_callback("/auth/callback?state=st&code=xyz", "st"),
            Ok("xyz".into())
        );
        let localhost = "  http://localhost:1455/auth/callback?code=abc&state=st\n";
        assert_eq!(parse_callback(localhost, "st"), Ok("abc".into()));
    }

    #[test]
    fn rejects_bad_callbacks() {
        assert_eq!(
            parse_callback(
                "http://127.0.0.1:1455/auth/callback?code=abc&state=other",
                "st"
            ),
            Err(CallbackError::StateMismatch)
        );
        assert_eq!(
            parse_callback("http://127.0.0.1:1455/favicon.ico?code=abc&state=st", "st"),
            Err(CallbackError::NotACallbackUrl)
        );
        assert_eq!(
            parse_callback("not a url", "st"),
            Err(CallbackError::NotACallbackUrl)
        );
        assert_eq!(
            parse_callback("/auth/callback?state=st", "st"),
            Err(CallbackError::MissingCode)
        );
        assert_eq!(
            parse_callback("/auth/callback?state=st&code=", "st"),
            Err(CallbackError::MissingCode)
        );
        assert_eq!(
            parse_callback(
                "/auth/callback?error=access_denied&error_description=User%20cancelled&state=st",
                "st"
            ),
            Err(CallbackError::Denied("User cancelled".into()))
        );
    }

    #[test]
    fn error_code_never_leaks_the_body() {
        assert_eq!(
            error_code(r#"{"error":"invalid_grant","error_description":"x"}"#),
            Some("invalid_grant".into())
        );
        assert_eq!(
            error_code(r#"{"error":{"code":"refresh_token_reused","message":"m"}}"#),
            Some("refresh_token_reused".into())
        );
        assert_eq!(error_code("<html>nope</html>"), None);
    }

    #[test]
    fn refresh_failures_follow_the_codex_rule() {
        use RefreshError::*;
        assert!(matches!(classify_refresh_failure(401, None), Permanent(_)));
        assert!(matches!(
            classify_refresh_failure(400, Some("invalid_grant")),
            Permanent(_)
        ));
        for code in [
            "refresh_token_expired",
            "refresh_token_reused",
            "REFRESH_TOKEN_INVALIDATED",
        ] {
            assert!(matches!(
                classify_refresh_failure(403, Some(code)),
                Permanent(_)
            ));
        }
        assert!(matches!(classify_refresh_failure(500, None), Transient(_)));
        assert!(matches!(
            classify_refresh_failure(400, Some("invalid_request")),
            Transient(_)
        ));
        assert_eq!(
            classify_refresh_failure(400, Some("invalid_grant")),
            Permanent("HTTP 400: invalid_grant".into())
        );
    }

    #[test]
    fn token_response_tolerates_missing_optionals() {
        let t: TokenResponse = serde_json::from_str(r#"{"access_token":"a"}"#).unwrap();
        assert_eq!(t.access_token, "a");
        assert!(t.refresh_token.is_none() && t.id_token.is_none() && t.expires_in.is_none());
    }
}
