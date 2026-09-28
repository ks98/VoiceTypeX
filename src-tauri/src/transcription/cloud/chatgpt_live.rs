// SPDX-License-Identifier: GPL-3.0-or-later
//! Live transcript preview with a ChatGPT subscription (experimental).
//!
//! Undocumented WebSocket that Codex Desktop dictation streams to:
//!   wss://chatgpt.com/backend-api/dictation/stream
//!   subprotocol `chatgpt-dictation`, the honest `chatgpt::api` identity
//!   → {"type":"session.start","config":{…}}   ← session.started
//!   → {"type":"audio.append","audio":"<base64 PCM16 LE mono>"} every 100 ms
//!   ← transcript.segment {utterance_id, text}: the cumulative text of the
//!     current utterance, ~3/s; transcript.final: that utterance is done
//!   ← session.error {fatal, error: {code, message}} / transcript.failed
//!
//! Observed 2026-09-28 (Plus account, honest headers): handshake 101, first
//! segment after ~0.9 s, sample rates from 16 to 96 kHz accepted, 13 min of
//! streaming did not move the plan's usage. It is only a preview: the
//! inserted text still comes from `/transcribe` (see `chatgpt.rs`), so any
//! failure here just ends the preview.

use crate::chatgpt::api;
use crate::chatgpt::oauth::ORIGINATOR;
use crate::chatgpt::session::Access;
use crate::chatgpt::ChatGptSession;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use futures_util::{Sink, SinkExt, Stream, StreamExt};
use parking_lot::Mutex;
use serde::Deserialize;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderValue, Request};
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tokio_tungstenite::Connector;

const URL: &str = "wss://chatgpt.com/backend-api/dictation/stream";
const SUBPROTOCOL: &str = "chatgpt-dictation";
const CHUNK_INTERVAL: Duration = Duration::from_millis(100);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// `session.start`. Same config as observed from Codex Desktop, plus
/// `session_asset_mode: none` (the server keeps no session WAV) and a
/// 10-minute session (accepted; the default is 5).
pub fn start_message(sample_rate: u32) -> String {
    serde_json::json!({
        "type": "session.start",
        "config": {
            "input_audio_format": "pcm16",
            "sample_rate_hz": sample_rate,
            "num_channels": 1,
            "max_buffer_size_bytes": 4 * 1024 * 1024,
            "max_utterance_duration_ms": 30_000,
            "session_ttl_ms": 600_000,
            "provider_mode": "streaming_sse",
            "transcript_delivery_mode": "segment",
            "session_asset_mode": "none",
            "vad": {
                "type": "server_vad",
                "threshold": 0.5,
                "prefix_padding_ms": 300,
                "silence_duration_ms": 500,
            },
        },
    })
    .to_string()
}

/// `audio.append` with mono f32 samples as base64 PCM16 little-endian.
pub fn append_message(mono: &[f32]) -> String {
    let pcm: Vec<u8> = mono
        .iter()
        .flat_map(|s| ((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes())
        .collect();
    serde_json::json!({ "type": "audio.append", "audio": B64.encode(pcm) }).to_string()
}

#[derive(Debug, PartialEq)]
pub enum Event {
    /// Segment or final: the latest text of one utterance.
    Transcript {
        utterance: String,
        text: String,
    },
    /// The session is over (fatal error or transcript failure).
    Fatal(String),
    Other,
}

#[derive(Deserialize)]
struct RawEvent {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    utterance_id: Option<serde_json::Value>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    fatal: Option<bool>,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

pub fn parse_event(raw: &str) -> Event {
    let Ok(ev) = serde_json::from_str::<RawEvent>(raw) else {
        return Event::Other;
    };
    let message = || {
        ev.error
            .as_ref()
            .and_then(|e| e.get("message").or(e.get("code")))
            .and_then(|m| m.as_str())
            .unwrap_or(&ev.kind)
            .chars()
            .take(200)
            .collect::<String>()
    };
    match ev.kind.as_str() {
        "transcript.segment" | "transcript.final" => Event::Transcript {
            utterance: ev
                .utterance_id
                .as_ref()
                .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string))
                .unwrap_or_default(),
            text: ev.text.clone().unwrap_or_default(),
        },
        // A non-fatal session.error keeps the session alive.
        "session.error" if ev.fatal != Some(false) => Event::Fatal(message()),
        "transcript.failed" => Event::Fatal(message()),
        _ => Event::Other,
    }
}

/// Joins the utterances in arrival order; each update replaces the text of
/// its utterance (the server sends cumulative text).
#[derive(Default)]
pub struct Preview {
    utterances: Vec<(String, String)>,
}

impl Preview {
    /// The new preview text, or `None` when nothing visible changed.
    pub fn update(&mut self, utterance: &str, text: &str) -> Option<String> {
        let text = text.trim();
        match self.utterances.iter_mut().find(|(id, _)| id == utterance) {
            Some((_, current)) if current == text => return None,
            Some((_, current)) => *current = text.to_string(),
            None if text.is_empty() => return None,
            None => self
                .utterances
                .push((utterance.to_string(), text.to_string())),
        }
        let joined = self
            .utterances
            .iter()
            .map(|(_, t)| t.as_str())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        Some(joined)
    }
}

/// Handshake request with the honest identification of `chatgpt::api` —
/// never a browser or Codex Desktop, even if the handshake gets blocked.
pub fn handshake_request(access: &Access) -> Result<Request<()>, String> {
    let mut req = URL.into_client_request().map_err(|e| e.to_string())?;
    let header = |v: &str| HeaderValue::from_str(v).map_err(|e| e.to_string());
    let headers = req.headers_mut();
    headers.insert(
        "Authorization",
        header(&format!("Bearer {}", access.token))?,
    );
    headers.insert("ChatGPT-Account-Id", header(&access.account_id)?);
    headers.insert("originator", header(ORIGINATOR)?);
    headers.insert("User-Agent", header(api::USER_AGENT)?);
    headers.insert("Sec-WebSocket-Protocol", header(SUBPROTOCOL)?);
    Ok(req)
}

/// rustls with the ring provider and webpki roots — the stack reqwest uses.
fn tls() -> Result<Connector, String> {
    static CONFIG: OnceLock<Result<Arc<rustls::ClientConfig>, String>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let roots = rustls::RootCertStore {
                roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
            };
            rustls::ClientConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .map(|b| Arc::new(b.with_root_certificates(roots).with_no_client_auth()))
            .map_err(|e| e.to_string())
        })
        .clone()
        .map(Connector::Rustls)
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn open(access: &Access) -> Result<Socket, WsError> {
    let req = handshake_request(access).map_err(|e| WsError::Io(std::io::Error::other(e)))?;
    let connector = tls().map_err(|e| WsError::Io(std::io::Error::other(e)))?;
    let connect =
        tokio_tungstenite::connect_async_tls_with_config(req, None, true, Some(connector));
    match tokio::time::timeout(CONNECT_TIMEOUT, connect).await {
        Ok(result) => result.map(|(ws, _)| ws),
        Err(_) => Err(WsError::Io(std::io::Error::other("handshake timed out"))),
    }
}

/// Opens the stream; a 401 refreshes the token once, like the batch call.
pub async fn connect(session: &ChatGptSession, client: &reqwest::Client) -> Result<Socket, String> {
    let access = session.access(client).await.map_err(|e| e.to_string())?;
    match open(&access).await {
        Err(WsError::Http(resp)) if resp.status() == 401 => {
            let access = session
                .refresh_after_401(client, &access.token)
                .await
                .map_err(|e| e.to_string())?;
            open(&access).await.map_err(describe)
        }
        other => other.map_err(describe),
    }
}

fn describe(e: WsError) -> String {
    let WsError::Http(resp) = e else {
        return format!("ChatGPT dictation stream: {e}");
    };
    let headers = resp.headers();
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let body = resp
        .body()
        .as_deref()
        .map(String::from_utf8_lossy)
        .unwrap_or_default();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    api::classify_failure(
        resp.status().as_u16(),
        headers.contains_key("cf-mitigated"),
        content_type,
        &body,
        now,
    )
    .message
}

/// Streams the growing recording buffer (interleaved f32 at the device
/// rate) and reports every preview change, until `recording()` turns false
/// or the server ends the session.
pub async fn stream_preview<S>(
    mut ws: S,
    samples: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    channels: u16,
    mut on_preview: impl FnMut(&str),
    recording: impl Fn() -> bool,
) -> Result<(), String>
where
    S: Stream<Item = Result<Message, WsError>> + Sink<Message, Error = WsError> + Unpin,
{
    let send_err = |e: WsError| format!("ChatGPT dictation stream: {e}");
    ws.send(Message::text(start_message(sample_rate)))
        .await
        .map_err(send_err)?;

    let channels = usize::from(channels.max(1));
    let mut sent = 0usize;
    let mut preview = Preview::default();
    let mut tick = tokio::time::interval(CHUNK_INTERVAL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = tick.tick() => {
                if !recording() {
                    return Ok(());
                }
                let chunk = {
                    let buf = samples.lock();
                    let end = sent + (buf.len().saturating_sub(sent) / channels) * channels;
                    let chunk = buf[sent..end].to_vec();
                    sent = end;
                    chunk
                };
                if !chunk.is_empty() {
                    let mono = crate::audio::recorder::stereo_to_mono(&chunk, channels as u16);
                    ws.send(Message::text(append_message(&mono))).await.map_err(send_err)?;
                }
            }
            msg = ws.next() => match msg {
                Some(Ok(Message::Text(text))) => match parse_event(text.as_str()) {
                    Event::Transcript { utterance, text } => {
                        if let Some(p) = preview.update(&utterance, &text) {
                            on_preview(&p);
                        }
                    }
                    Event::Fatal(message) => return Err(format!("ChatGPT dictation stream: {message}")),
                    Event::Other => {}
                },
                Some(Ok(Message::Close(frame))) => {
                    return Err(format!("ChatGPT dictation stream closed by the server ({frame:?})"));
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(send_err(e)),
                None => return Err("ChatGPT dictation stream ended".into()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::accept_async;

    fn access() -> Access {
        Access {
            token: "tok".into(),
            account_id: "acc".into(),
        }
    }

    #[test]
    fn handshake_identifies_honestly() {
        let req = handshake_request(&access()).unwrap();
        let h = req.headers();
        assert_eq!(req.uri(), URL);
        assert_eq!(h["Authorization"], "Bearer tok");
        assert_eq!(h["ChatGPT-Account-Id"], "acc");
        assert_eq!(h["originator"], "voicetypex");
        assert!(h["User-Agent"].to_str().unwrap().starts_with("VoiceTypeX/"));
        assert_eq!(h["Sec-WebSocket-Protocol"], "chatgpt-dictation");
        assert!(!h.contains_key("Origin"));
    }

    #[test]
    fn start_message_carries_the_rate_and_no_session_asset() {
        let v: serde_json::Value = serde_json::from_str(&start_message(44_100)).unwrap();
        assert_eq!(v["type"], "session.start");
        assert_eq!(v["config"]["sample_rate_hz"], 44_100);
        assert_eq!(v["config"]["input_audio_format"], "pcm16");
        assert_eq!(v["config"]["transcript_delivery_mode"], "segment");
        assert_eq!(v["config"]["session_asset_mode"], "none");
    }

    #[test]
    fn append_message_encodes_clamped_pcm16_le() {
        let v: serde_json::Value =
            serde_json::from_str(&append_message(&[0.0, 1.0, -1.0, 2.0, 0.5])).unwrap();
        assert_eq!(v["type"], "audio.append");
        let bytes = B64.decode(v["audio"].as_str().unwrap()).unwrap();
        let pcm: Vec<i16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes(*b))
            .collect();
        assert_eq!(pcm, [0, 32767, -32767, 32767, 16383]);
    }

    #[test]
    fn parses_transcripts_and_failures() {
        assert_eq!(
            parse_event(
                r#"{"type":"transcript.segment","utterance_id":"u1","revision":3,"text":" Hallo"}"#
            ),
            Event::Transcript {
                utterance: "u1".into(),
                text: " Hallo".into()
            }
        );
        assert_eq!(
            parse_event(r#"{"type":"transcript.final","utterance_id":7,"text":"Hi."}"#),
            Event::Transcript {
                utterance: "7".into(),
                text: "Hi.".into()
            }
        );
        assert_eq!(
            parse_event(
                r#"{"type":"session.error","fatal":true,"error":{"code":"x","message":"boom","retryable":false}}"#
            ),
            Event::Fatal("boom".into())
        );
        assert_eq!(
            parse_event(r#"{"type":"session.error","fatal":false,"error":{"message":"meh"}}"#),
            Event::Other
        );
        assert_eq!(
            parse_event(r#"{"type":"transcript.failed"}"#),
            Event::Fatal("transcript.failed".into())
        );
        assert_eq!(
            parse_event(r#"{"type":"session.updated","session":{}}"#),
            Event::Other
        );
        assert_eq!(parse_event("not json"), Event::Other);
    }

    #[test]
    fn preview_replaces_per_utterance_and_joins_in_order() {
        let mut p = Preview::default();
        assert_eq!(p.update("a", " Hallo"), Some("Hallo".into()));
        assert_eq!(p.update("a", "Hallo Welt"), Some("Hallo Welt".into()));
        assert_eq!(p.update("a", "Hallo Welt"), None);
        assert_eq!(p.update("b", ""), None);
        assert_eq!(
            p.update("b", "Zweiter Satz"),
            Some("Hallo Welt Zweiter Satz".into())
        );
        assert_eq!(
            p.update("a", "Hallo Welt."),
            Some("Hallo Welt. Zweiter Satz".into())
        );
    }

    /// End to end against a local mock server: start message, audio as
    /// mono PCM16, preview updates, and a fatal error ending the stream.
    #[tokio::test]
    async fn streams_audio_and_reports_previews() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(tcp).await.unwrap();
            let start: serde_json::Value =
                serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            let append: serde_json::Value =
                serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            for ev in [
                r#"{"type":"session.started"}"#,
                r#"{"type":"transcript.segment","utterance_id":"u1","text":"Hallo"}"#,
                r#"{"type":"transcript.final","utterance_id":"u1","text":"Hallo Welt."}"#,
                r#"{"type":"session.error","fatal":true,"error":{"message":"done"}}"#,
            ] {
                ws.send(Message::text(ev)).await.unwrap();
            }
            (start, append)
        });

        let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}"))
            .await
            .unwrap();
        // Stereo frames (L, R): the preview sends their mono mix.
        let samples = Arc::new(Mutex::new(vec![0.5, 0.5, 1.0, 0.0, 0.25]));
        let mut seen = Vec::new();
        let result = stream_preview(
            ws,
            samples,
            48_000,
            2,
            |p| seen.push(p.to_string()),
            || true,
        )
        .await;

        assert_eq!(result, Err("ChatGPT dictation stream: done".into()));
        assert_eq!(seen, ["Hallo", "Hallo Welt."]);
        let (start, append) = server.await.unwrap();
        assert_eq!(start["config"]["sample_rate_hz"], 48_000);
        let bytes = B64.decode(append["audio"].as_str().unwrap()).unwrap();
        // Two whole frames; the trailing half frame waits for its partner.
        assert_eq!(bytes.len(), 4);
        assert_eq!(i16::from_le_bytes([bytes[0], bytes[1]]), 16383);
        assert_eq!(i16::from_le_bytes([bytes[2], bytes[3]]), 16383);
    }

    #[tokio::test]
    async fn stops_when_recording_ends() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(tcp).await.unwrap();
            while ws.next().await.is_some() {}
        });
        let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}"))
            .await
            .unwrap();
        let samples = Arc::new(Mutex::new(Vec::new()));
        let result = stream_preview(ws, samples, 16_000, 1, |_| {}, || false).await;
        assert_eq!(result, Ok(()));
    }
}
