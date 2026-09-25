// SPDX-License-Identifier: GPL-3.0-or-later
//! ChatGPT subscription sign-in (experimental).
//!
//! Signs in with the OAuth flow of OpenAI's Codex CLI ("Sign in with
//! ChatGPT": PKCE authorization code + loopback callback) so a ChatGPT
//! plan can be used instead of an API key. This is **not** an official
//! API for third-party apps — it reuses the Codex client id and may break
//! or be restricted at any time. See docs/PROVIDERS.md.

pub mod jwt;
pub mod loopback;
pub mod oauth;
pub mod session;

pub use session::{AuthState, ChatGptSession, ChatGptStatus};
