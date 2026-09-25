// SPDX-License-Identifier: GPL-3.0-or-later
//! Claim extraction from the OAuth tokens.
//!
//! The signature is deliberately not verified: the tokens come straight
//! from the token endpoint over TLS in response to our own PKCE exchange
//! (OIDC Core §3.1.3.7 allows TLS server validation instead), the claims
//! are only used for display and the account-id header, and the backend
//! validates the token on every request anyway.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Deserialize;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Claims {
    pub account_id: Option<String>,
    pub plan_type: Option<String>,
    pub email: Option<String>,
    /// Expiry as Unix seconds (`exp`).
    pub exp: Option<i64>,
}

#[derive(Deserialize)]
struct RawClaims {
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    exp: Option<i64>,
    #[serde(rename = "https://api.openai.com/profile", default)]
    profile: Option<RawProfile>,
    #[serde(rename = "https://api.openai.com/auth", default)]
    auth: Option<RawAuth>,
}

#[derive(Deserialize)]
struct RawProfile {
    #[serde(default)]
    email: Option<String>,
}

#[derive(Deserialize)]
struct RawAuth {
    #[serde(default)]
    chatgpt_account_id: Option<String>,
    #[serde(default)]
    chatgpt_plan_type: Option<String>,
}

/// Decodes the payload segment of a JWT. `None` for anything that is not
/// a three-segment token with a JSON payload.
pub fn decode_claims(jwt: &str) -> Option<Claims> {
    let mut parts = jwt.split('.');
    let (_, payload, _) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let raw: RawClaims = serde_json::from_slice(&bytes).ok()?;
    let (account_id, plan_type) = raw
        .auth
        .map(|a| (a.chatgpt_account_id, a.chatgpt_plan_type))
        .unwrap_or_default();
    Some(Claims {
        account_id,
        plan_type,
        email: raw.email.or(raw.profile.and_then(|p| p.email)),
        exp: raw.exp,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn fake_jwt(payload: &serde_json::Value) -> String {
        let body = URL_SAFE_NO_PAD.encode(serde_json::to_vec(payload).unwrap());
        format!("eyJhbGciOiJub25lIn0.{body}.sig")
    }

    #[test]
    fn reads_account_plan_email_and_exp() {
        let jwt = fake_jwt(&serde_json::json!({
            "email": "a@b.de",
            "exp": 1_900_000_000,
            "https://api.openai.com/auth": {
                "chatgpt_account_id": "acc-1",
                "chatgpt_plan_type": "plus"
            }
        }));
        assert_eq!(
            decode_claims(&jwt),
            Some(Claims {
                account_id: Some("acc-1".into()),
                plan_type: Some("plus".into()),
                email: Some("a@b.de".into()),
                exp: Some(1_900_000_000),
            })
        );
    }

    #[test]
    fn falls_back_to_profile_email() {
        let jwt = fake_jwt(&serde_json::json!({
            "https://api.openai.com/profile": { "email": "p@b.de" }
        }));
        let claims = decode_claims(&jwt).unwrap();
        assert_eq!(claims.email.as_deref(), Some("p@b.de"));
        assert_eq!(claims.account_id, None);
    }

    #[test]
    fn tolerates_padded_payload() {
        let jwt = fake_jwt(&serde_json::json!({ "email": "x@y.z" }));
        let mut parts: Vec<&str> = jwt.split('.').collect();
        let padded = format!("{}==", parts[1]);
        parts[1] = &padded;
        assert!(decode_claims(&parts.join(".")).is_some());
    }

    #[test]
    fn rejects_malformed_tokens() {
        assert_eq!(decode_claims(""), None);
        assert_eq!(decode_claims("a.b"), None);
        assert_eq!(decode_claims("a.b.c.d"), None);
        assert_eq!(decode_claims("a.!!!.c"), None);
        let not_json = format!("a.{}.c", URL_SAFE_NO_PAD.encode(b"not json"));
        assert_eq!(decode_claims(&not_json), None);
    }
}
