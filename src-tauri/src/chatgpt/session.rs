// SPDX-License-Identifier: GPL-3.0-or-later
//! Sign-in state of the ChatGPT account: stored credentials, the sign-in
//! attempt in progress, and access-token refresh for provider calls. Free
//! of `AppHandle` so it stays testable; the IPC layer emits the status
//! event after each change.

use super::{jwt, oauth};
use crate::core::error::{Result, VoiceTypeError};
use crate::secrets::SecretStore;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// Secret-store entry holding the serialized [`Credentials`]. Not part of
/// `ipc::secrets::PROVIDERS`, so `set_provider_key` can never write it.
const SECRET_KEY: &str = "chatgpt_oauth";

/// Refresh this long before expiry. A transient refresh failure inside the
/// window keeps using the still-valid token.
const REFRESH_WINDOW_SECS: i64 = 5 * 60;

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

impl Credentials {
    fn access(&self) -> Access {
        Access {
            token: self.access_token.clone(),
            account_id: self.account_id.clone(),
        }
    }
}

/// What a provider call needs from the session.
#[derive(Debug, Clone, PartialEq)]
pub struct Access {
    pub token: String,
    pub account_id: String,
}

#[derive(Debug, PartialEq)]
pub enum AccessError {
    NotSignedIn,
    /// The refresh token is dead — the user has to sign in again.
    Expired,
    /// The token is expired and refreshing failed for a transient reason.
    Unavailable(String),
}

impl fmt::Display for AccessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotSignedIn => write!(
                f,
                "ChatGPT is not signed in — sign in under Settings → ChatGPT account"
            ),
            Self::Expired => write!(
                f,
                "ChatGPT sign-in expired — sign in again under Settings → ChatGPT account"
            ),
            Self::Unavailable(detail) => {
                write!(f, "Could not refresh the ChatGPT sign-in ({detail})")
            }
        }
    }
}

#[derive(Debug, PartialEq)]
enum RefreshDecision {
    Fresh,
    /// Inside the refresh window but still valid.
    Soft,
    /// Expired.
    Hard,
}

/// `None` (no known expiry) counts as fresh: the 401 path refreshes then.
fn refresh_decision(expires_at: Option<i64>, now: i64) -> RefreshDecision {
    match expires_at {
        Some(e) if e <= now => RefreshDecision::Hard,
        Some(e) if e - now <= REFRESH_WINDOW_SECS => RefreshDecision::Soft,
        _ => RefreshDecision::Fresh,
    }
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
    /// Held across a refresh request so concurrent callers never spend the
    /// same rotating refresh token twice (a reuse kills the session).
    refresh_lock: tokio::sync::Mutex<()>,
    token_url: String,
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
            refresh_lock: tokio::sync::Mutex::new(()),
            token_url: oauth::TOKEN_URL.to_string(),
        }
    }

    /// A valid access token for a provider call, refreshed if it expires
    /// within the next minutes.
    pub async fn access(
        &self,
        client: &reqwest::Client,
    ) -> std::result::Result<Access, AccessError> {
        if let Some(access) = self.fresh_access()? {
            return Ok(access);
        }
        let _guard = self.refresh_lock.lock().await;
        let current = self.creds.lock().clone().ok_or(AccessError::NotSignedIn)?;
        match refresh_decision(current.expires_at, now_unix()) {
            RefreshDecision::Fresh => Ok(current.access()),
            RefreshDecision::Soft => match self.refresh_locked(client, &current).await {
                Err(AccessError::Unavailable(detail)) => {
                    tracing::warn!(%detail, "ChatGPT token refresh failed — using the still-valid token");
                    Ok(current.access())
                }
                other => other,
            },
            RefreshDecision::Hard => self.refresh_locked(client, &current).await,
        }
    }

    /// Called after the backend rejected `rejected_token` with 401: refresh
    /// once, unless another caller already replaced that token.
    pub async fn refresh_after_401(
        &self,
        client: &reqwest::Client,
        rejected_token: &str,
    ) -> std::result::Result<Access, AccessError> {
        let _guard = self.refresh_lock.lock().await;
        let current = self.creds.lock().clone().ok_or(AccessError::NotSignedIn)?;
        if current.access_token != rejected_token {
            return Ok(current.access());
        }
        self.refresh_locked(client, &current).await
    }

    fn fresh_access(&self) -> std::result::Result<Option<Access>, AccessError> {
        let creds = self.creds.lock();
        let c = creds.as_ref().ok_or(AccessError::NotSignedIn)?;
        Ok(
            (refresh_decision(c.expires_at, now_unix()) == RefreshDecision::Fresh)
                .then(|| c.access()),
        )
    }

    /// Caller must hold `refresh_lock`.
    async fn refresh_locked(
        &self,
        client: &reqwest::Client,
        current: &Credentials,
    ) -> std::result::Result<Access, AccessError> {
        match oauth::refresh(client, &self.token_url, &current.refresh_token).await {
            Ok(tokens) => {
                let next = apply_refresh(current, tokens, now_unix());
                let mut creds = self.creds.lock();
                // Signed out or signed in anew while the request ran: the
                // result belongs to a session that no longer exists.
                if creds.as_ref().map(|c| &c.refresh_token) != Some(&current.refresh_token) {
                    return creds
                        .as_ref()
                        .map(Credentials::access)
                        .ok_or(AccessError::NotSignedIn);
                }
                persist(&next);
                let access = next.access();
                *creds = Some(next);
                tracing::info!("ChatGPT access token refreshed");
                Ok(access)
            }
            Err(oauth::RefreshError::Permanent(detail)) => {
                tracing::warn!(%detail, "ChatGPT refresh token rejected — signing out");
                if let Err(e) = SecretStore::delete(SECRET_KEY) {
                    tracing::warn!(error = %e, "could not delete the expired ChatGPT sign-in");
                }
                *self.creds.lock() = None;
                *self.last_error.lock() = Some(AccessError::Expired.to_string());
                Err(AccessError::Expired)
            }
            Err(oauth::RefreshError::Transient(detail)) => Err(AccessError::Unavailable(detail)),
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

/// A failed write is logged, not fatal: the refreshed tokens stay usable in
/// memory, only the next app start would need a new sign-in.
fn persist(creds: &Credentials) {
    let result = serde_json::to_string(creds)
        .map_err(|e| VoiceTypeError::Secrets(e.to_string()))
        .and_then(|json| SecretStore::set(SECRET_KEY, &json));
    if let Err(e) = result {
        tracing::warn!(error = %e, "could not persist the refreshed ChatGPT sign-in");
    }
}

/// Merges a refresh response into the stored credentials: the rotated
/// refresh token replaces the old one (kept if none was sent), claims fall
/// back to the previous values.
fn apply_refresh(old: &Credentials, tokens: oauth::TokenResponse, now: i64) -> Credentials {
    let access_claims = jwt::decode_claims(&tokens.access_token).unwrap_or_default();
    let id_claims = tokens
        .id_token
        .as_deref()
        .and_then(jwt::decode_claims)
        .unwrap_or_default();
    Credentials {
        v: 1,
        expires_at: tokens.expires_in.map(|s| now + s).or(access_claims.exp),
        refresh_token: tokens
            .refresh_token
            .unwrap_or_else(|| old.refresh_token.clone()),
        account_id: id_claims
            .account_id
            .or(access_claims.account_id)
            .unwrap_or_else(|| old.account_id.clone()),
        email: id_claims
            .email
            .or(access_claims.email)
            .or_else(|| old.email.clone()),
        plan_type: id_claims
            .plan_type
            .or(access_claims.plan_type)
            .or_else(|| old.plan_type.clone()),
        id_token: tokens.id_token.or_else(|| old.id_token.clone()),
        access_token: tokens.access_token,
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
        session_with(None, "http://127.0.0.1:9/unused")
    }

    fn session_with(creds: Option<Credentials>, token_url: &str) -> ChatGptSession {
        ChatGptSession {
            creds: Mutex::new(creds),
            pending: Mutex::new(None),
            last_error: Mutex::new(None),
            refresh_lock: tokio::sync::Mutex::new(()),
            token_url: token_url.to_string(),
        }
    }

    fn creds(access: &str, refresh: &str, expires_at: Option<i64>) -> Credentials {
        Credentials {
            v: 1,
            access_token: access.into(),
            refresh_token: refresh.into(),
            id_token: None,
            expires_at,
            account_id: "acc".into(),
            email: Some("a@b.de".into()),
            plan_type: Some("plus".into()),
        }
    }

    /// Minimal token endpoint: answers every request with `status`/`body`
    /// and counts the requests.
    async fn mock_token_endpoint(
        status: u16,
        body: String,
    ) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let url = format!("http://{}/oauth/token", listener.local_addr().unwrap());
        let hits = std::sync::Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        tokio::spawn(async move {
            loop {
                let (mut s, _) = listener.accept().await.unwrap();
                counter.fetch_add(1, Ordering::SeqCst);
                let mut buf = Vec::new();
                let mut chunk = [0u8; 1024];
                loop {
                    let n = s.read(&mut chunk).await.unwrap();
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let len = text
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if buf.len() >= end + 4 + len || n == 0 {
                            break;
                        }
                    }
                }
                let resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                s.write_all(resp.as_bytes()).await.unwrap();
            }
        });
        (url, hits)
    }

    #[test]
    fn refresh_decision_boundaries() {
        assert_eq!(refresh_decision(None, 100), RefreshDecision::Fresh);
        assert_eq!(
            refresh_decision(Some(100 + REFRESH_WINDOW_SECS + 1), 100),
            RefreshDecision::Fresh
        );
        assert_eq!(
            refresh_decision(Some(100 + REFRESH_WINDOW_SECS), 100),
            RefreshDecision::Soft
        );
        assert_eq!(refresh_decision(Some(101), 100), RefreshDecision::Soft);
        assert_eq!(refresh_decision(Some(100), 100), RefreshDecision::Hard);
    }

    #[test]
    fn apply_refresh_rotates_and_falls_back() {
        let old = creds("a1", "r1", Some(10));
        let rotated = apply_refresh(
            &old,
            oauth::TokenResponse {
                access_token: fake_jwt(&json!({ "exp": 5000 })),
                refresh_token: Some("r2".into()),
                id_token: None,
                expires_in: None,
            },
            1000,
        );
        assert_eq!(rotated.refresh_token, "r2");
        assert_eq!(rotated.expires_at, Some(5000));
        assert_eq!(rotated.account_id, "acc");
        assert_eq!(rotated.plan_type.as_deref(), Some("plus"));
        let kept = apply_refresh(
            &old,
            oauth::TokenResponse {
                access_token: "opaque".into(),
                refresh_token: None,
                id_token: None,
                expires_in: Some(60),
            },
            1000,
        );
        assert_eq!(kept.refresh_token, "r1");
        assert_eq!(kept.expires_at, Some(1060));
    }

    #[tokio::test]
    async fn concurrent_callers_refresh_exactly_once() {
        let body = json!({
            "access_token": fake_jwt(&json!({ "exp": now_unix() + 3600 })),
            "refresh_token": "r2"
        })
        .to_string();
        let (url, hits) = mock_token_endpoint(200, body).await;
        let s = session_with(Some(creds("old", "r1", Some(now_unix() - 10))), &url);
        let client = reqwest::Client::new();
        let results = futures_util::future::join_all((0..5).map(|_| s.access(&client))).await;
        let tokens: Vec<_> = results.into_iter().map(|r| r.unwrap().token).collect();
        assert!(tokens.iter().all(|t| t != "old" && t == &tokens[0]));
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn permanent_refresh_failure_signs_out() {
        let (url, _) = mock_token_endpoint(400, r#"{"error":"invalid_grant"}"#.into()).await;
        let s = session_with(Some(creds("old", "r1", Some(now_unix() - 10))), &url);
        assert_eq!(
            s.access(&reqwest::Client::new()).await,
            Err(AccessError::Expired)
        );
        let st = s.status();
        assert_eq!(st.state, AuthState::Disconnected);
        assert!(st.error.unwrap().contains("expired"));
    }

    #[tokio::test]
    async fn transient_failure_inside_the_window_keeps_the_valid_token() {
        let (url, hits) = mock_token_endpoint(503, "{}".into()).await;
        let s = session_with(
            Some(creds("still-valid", "r1", Some(now_unix() + 60))),
            &url,
        );
        let access = s.access(&reqwest::Client::new()).await.unwrap();
        assert_eq!(access.token, "still-valid");
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn refresh_after_401_skips_an_already_rotated_token() {
        let (url, hits) = mock_token_endpoint(500, "{}".into()).await;
        let s = session_with(Some(creds("new", "r2", None)), &url);
        let access = s
            .refresh_after_401(&reqwest::Client::new(), "old")
            .await
            .unwrap();
        assert_eq!(access.token, "new");
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn access_without_sign_in_is_not_signed_in() {
        assert_eq!(
            session().access(&reqwest::Client::new()).await,
            Err(AccessError::NotSignedIn)
        );
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
