// SPDX-License-Identifier: GPL-3.0-or-later
//! Sign-in state of the ChatGPT account: stored credentials plus the
//! sign-in attempt in progress. Free of `AppHandle` so it stays testable;
//! the IPC layer emits the status event after each change.

use super::{jwt, oauth};
use crate::core::error::{Result, VoiceTypeError};
use crate::secrets::SecretStore;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Secret-store entry holding the serialized [`Credentials`]. Not part of
/// `ipc::secrets::PROVIDERS`, so `set_provider_key` can never write it.
const SECRET_KEY: &str = "chatgpt_oauth";

/// Access and refresh token are stored together: the refresh token
/// rotates, so writing them separately could strand a stale pair.
#[derive(Serialize, Deserialize, Clone)]
struct Credentials {
    v: u8,
    access_token: String,
    refresh_token: String,
    #[serde(default)]
    id_token: Option<String>,
    /// Unix seconds; `None` when neither `expires_in` nor `exp` was given.
    #[serde(default)]
    expires_at: Option<i64>,
    account_id: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    plan_type: Option<String>,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AuthState {
    Disconnected,
    Pending,
    Connected,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ChatGptStatus {
    pub state: AuthState,
    pub email: Option<String>,
    pub plan: Option<String>,
    /// Only while a sign-in is pending — lets the UI offer "copy link".
    pub auth_url: Option<String>,
    /// Last sign-in failure or a problem reading the stored sign-in.
    pub error: Option<String>,
}

struct PendingLogin {
    verifier: String,
    state: String,
    redirect_uri: String,
    auth_url: String,
    /// Loopback listener task; aborting it frees the callback port.
    task: Option<tauri::async_runtime::JoinHandle<()>>,
}

impl Drop for PendingLogin {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

pub struct ChatGptSession {
    creds: Mutex<Option<Credentials>>,
    pending: Mutex<Option<PendingLogin>>,
    last_error: Mutex<Option<String>>,
}

impl ChatGptSession {
    /// Loads the stored sign-in, if any. A corrupt entry is reported via
    /// `status().error` instead of failing app start.
    pub fn load() -> Self {
        let (creds, error) = match SecretStore::get(SECRET_KEY) {
            Ok(Some(json)) => match serde_json::from_str::<Credentials>(&json) {
                Ok(c) => (Some(c), None),
                Err(e) => (
                    None,
                    Some(format!("Stored ChatGPT sign-in is unreadable: {e}")),
                ),
            },
            Ok(None) => (None, None),
            Err(e) => (None, Some(e.to_string())),
        };
        Self {
            creds: Mutex::new(creds),
            pending: Mutex::new(None),
            last_error: Mutex::new(error),
        }
    }

    pub fn status(&self) -> ChatGptStatus {
        let error = self.last_error.lock().clone();
        if let Some(p) = self.pending.lock().as_ref() {
            return ChatGptStatus {
                state: AuthState::Pending,
                email: None,
                plan: None,
                auth_url: Some(p.auth_url.clone()),
                error,
            };
        }
        match self.creds.lock().as_ref() {
            Some(c) => ChatGptStatus {
                state: AuthState::Connected,
                email: c.email.clone(),
                plan: c.plan_type.clone(),
                auth_url: None,
                error,
            },
            None => ChatGptStatus {
                state: AuthState::Disconnected,
                email: None,
                plan: None,
                auth_url: None,
                error,
            },
        }
    }

    /// Starts a new attempt (replacing and aborting any previous one) and
    /// returns `(authorize_url, state)`.
    pub fn begin_login(&self, redirect_uri: String) -> (String, String) {
        let pkce = oauth::generate_pkce();
        let state = oauth::generate_state();
        let auth_url = oauth::authorize_url(&redirect_uri, &pkce.challenge, &state);
        *self.pending.lock() = Some(PendingLogin {
            verifier: pkce.verifier,
            state: state.clone(),
            redirect_uri,
            auth_url: auth_url.clone(),
            task: None,
        });
        *self.last_error.lock() = None;
        (auth_url, state)
    }

    /// Hands the listener task to the attempt so cancel/replace can abort
    /// it. A task whose attempt is already gone is aborted right away.
    pub fn attach_task(&self, state: &str, task: tauri::async_runtime::JoinHandle<()>) {
        match self.pending.lock().as_mut() {
            Some(p) if p.state == state => p.task = Some(task),
            _ => task.abort(),
        }
    }

    /// Called by the listener task once the callback arrived: from then on
    /// clearing the attempt must not abort the task that is completing it.
    pub fn detach_task(&self, state: &str) {
        if let Some(p) = self.pending.lock().as_mut() {
            if p.state == state {
                // Dropping a JoinHandle detaches instead of aborting.
                drop(p.task.take());
            }
        }
    }

    /// Completes the pending attempt from a callback URL or request target.
    /// On failure the attempt stays pending, so a wrongly pasted URL can
    /// simply be pasted again.
    pub async fn complete_login(&self, client: &reqwest::Client, callback: &str) -> Result<()> {
        let (verifier, state, redirect_uri) = {
            let pending = self.pending.lock();
            let p = pending
                .as_ref()
                .ok_or_else(|| other("No ChatGPT sign-in is in progress".into()))?;
            (p.verifier.clone(), p.state.clone(), p.redirect_uri.clone())
        };
        let code = oauth::parse_callback(callback, &state).map_err(|e| other(e.to_string()))?;
        let tokens = oauth::exchange_code(client, &code, &verifier, &redirect_uri).await?;
        let creds = credentials_from(tokens, now_unix())?;
        let json =
            serde_json::to_string(&creds).map_err(|e| VoiceTypeError::Secrets(e.to_string()))?;
        SecretStore::set(SECRET_KEY, &json)?;
        *self.creds.lock() = Some(creds);
        self.clear_pending_if(&state);
        *self.last_error.lock() = None;
        tracing::info!("ChatGPT account connected");
        Ok(())
    }

    /// Ends the attempt `state` with an error shown in the status. Ignored
    /// if a newer attempt replaced it meanwhile.
    pub fn fail_login(&self, state: &str, message: String) {
        if self.clear_pending_if(state) {
            tracing::warn!(%message, "ChatGPT sign-in failed");
            *self.last_error.lock() = Some(message);
        }
    }

    /// Drops the attempt and aborts its listener. The returned handle
    /// resolves once the listener is gone, i.e. the callback port is free
    /// again — await it before binding a new listener.
    pub fn cancel_login(&self) -> Option<tauri::async_runtime::JoinHandle<()>> {
        let task = self.pending.lock().take()?.task.take()?;
        task.abort();
        Some(task)
    }

    pub fn logout(&self) -> Result<()> {
        drop(self.cancel_login());
        SecretStore::delete(SECRET_KEY)?;
        *self.creds.lock() = None;
        *self.last_error.lock() = None;
        tracing::info!("ChatGPT account disconnected");
        Ok(())
    }

    fn clear_pending_if(&self, state: &str) -> bool {
        let mut pending = self.pending.lock();
        if pending.as_ref().is_some_and(|p| p.state == state) {
            *pending = None;
            true
        } else {
            false
        }
    }
}

fn credentials_from(tokens: oauth::TokenResponse, now: i64) -> Result<Credentials> {
    let refresh_token = tokens.refresh_token.ok_or_else(|| {
        other("ChatGPT sign-in: the token response contains no refresh token".into())
    })?;
    let access_claims = jwt::decode_claims(&tokens.access_token).unwrap_or_default();
    let claims = tokens
        .id_token
        .as_deref()
        .and_then(jwt::decode_claims)
        .unwrap_or_default();
    let account_id = claims
        .account_id
        .or(access_claims.account_id)
        .ok_or_else(|| other("ChatGPT sign-in: no ChatGPT account id in the token".into()))?;
    Ok(Credentials {
        v: 1,
        expires_at: tokens.expires_in.map(|s| now + s).or(access_claims.exp),
        access_token: tokens.access_token,
        refresh_token,
        id_token: tokens.id_token,
        account_id,
        email: claims.email.or(access_claims.email),
        plan_type: claims.plan_type.or(access_claims.plan_type),
    })
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn other(msg: String) -> VoiceTypeError {
    VoiceTypeError::Other(anyhow::anyhow!(msg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chatgpt::jwt::tests::fake_jwt;
    use serde_json::json;

    fn session() -> ChatGptSession {
        ChatGptSession {
            creds: Mutex::new(None),
            pending: Mutex::new(None),
            last_error: Mutex::new(None),
        }
    }

    fn tokens(id: Option<serde_json::Value>, access: serde_json::Value) -> oauth::TokenResponse {
        oauth::TokenResponse {
            access_token: fake_jwt(&access),
            refresh_token: Some("r".into()),
            id_token: id.map(|v| fake_jwt(&v)),
            expires_in: None,
        }
    }

    #[test]
    fn credentials_prefer_id_token_claims_and_fall_back_to_access_token() {
        let t = tokens(
            Some(json!({ "email": "id@x.de",
                "https://api.openai.com/auth": { "chatgpt_plan_type": "pro" } })),
            json!({ "exp": 2000,
                "https://api.openai.com/auth": { "chatgpt_account_id": "acc" } }),
        );
        let c = credentials_from(t, 1000).unwrap();
        assert_eq!(c.account_id, "acc");
        assert_eq!(c.email.as_deref(), Some("id@x.de"));
        assert_eq!(c.plan_type.as_deref(), Some("pro"));
        assert_eq!(c.expires_at, Some(2000));
    }

    #[test]
    fn expires_in_wins_over_jwt_exp() {
        let mut t = tokens(
            None,
            json!({ "exp": 2000, "https://api.openai.com/auth": { "chatgpt_account_id": "a" } }),
        );
        t.expires_in = Some(3600);
        assert_eq!(credentials_from(t, 1000).unwrap().expires_at, Some(4600));
    }

    #[test]
    fn missing_account_id_or_refresh_token_is_an_error() {
        assert!(credentials_from(tokens(None, json!({})), 0).is_err());
        let mut t = tokens(
            None,
            json!({ "https://api.openai.com/auth": { "chatgpt_account_id": "a" } }),
        );
        t.refresh_token = None;
        assert!(credentials_from(t, 0).is_err());
    }

    #[test]
    fn pending_status_exposes_the_authorize_url() {
        let s = session();
        assert_eq!(s.status().state, AuthState::Disconnected);
        let (url, _) = s.begin_login(oauth::redirect_uri(1455));
        let st = s.status();
        assert_eq!(st.state, AuthState::Pending);
        assert_eq!(st.auth_url.as_deref(), Some(url.as_str()));
        assert!(s.cancel_login().is_none(), "no listener task was attached");
        assert_eq!(s.status().state, AuthState::Disconnected);
    }

    #[test]
    fn fail_login_ignores_superseded_attempts() {
        let s = session();
        let (_, old) = s.begin_login(oauth::redirect_uri(1455));
        let (_, new) = s.begin_login(oauth::redirect_uri(1455));
        s.fail_login(&old, "late failure".into());
        assert_eq!(s.status().state, AuthState::Pending);
        assert_eq!(s.status().error, None);
        s.fail_login(&new, "timed out".into());
        let st = s.status();
        assert_eq!(st.state, AuthState::Disconnected);
        assert_eq!(st.error.as_deref(), Some("timed out"));
        // A new attempt clears the previous error.
        s.begin_login(oauth::redirect_uri(1455));
        assert_eq!(s.status().error, None);
    }

    #[tokio::test]
    async fn complete_login_rejects_a_foreign_state_and_stays_pending() {
        let s = session();
        s.begin_login(oauth::redirect_uri(1455));
        let err = s
            .complete_login(
                &reqwest::Client::new(),
                "/auth/callback?code=c&state=forged",
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("different sign-in attempt"));
        assert_eq!(s.status().state, AuthState::Pending);
    }

    #[test]
    fn stored_credentials_round_trip() {
        let c = credentials_from(
            tokens(
                None,
                json!({ "https://api.openai.com/auth": { "chatgpt_account_id": "a" } }),
            ),
            0,
        )
        .unwrap();
        let back: Credentials = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back.account_id, "a");
        assert_eq!(back.refresh_token, "r");
    }

    #[test]
    fn status_serializes_with_lowercase_state() {
        let v = serde_json::to_value(session().status()).unwrap();
        assert_eq!(v["state"], "disconnected");
        let mut keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["auth_url", "email", "error", "plan", "state"]);
    }
}
