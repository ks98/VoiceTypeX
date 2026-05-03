// SPDX-License-Identifier: GPL-3.0-or-later
//! xAI Speech-to-Text — `POST https://api.x.ai/v1/stt`, multipart/form-data
//! mit `file` als letztem Field. Response: `text`, `language`, `duration`,
//! `words[]` mit Word-Level-Timestamps. Phase 1 nutzt nur `text`.

use crate::core::error::{Result, VoiceTypeError};
use crate::core::retry::with_retry;
use crate::transcription::{
    EventStream, StreamOpts, TranscribeOpts, Transcriber, TranscriptionEvent, TranscriptionMode,
};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

const SUPPORTED: &[TranscriptionMode] =
    &[TranscriptionMode::OneShot, TranscriptionMode::Streaming];
const DEFAULT_MODEL: &str = "stt-1";

pub struct XaiTranscriber {
    api_key: String,
    base_url: String,
    model: String,
    client: reqwest::Client,
}

impl XaiTranscriber {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://api.x.ai/v1".to_string(),
            model: DEFAULT_MODEL.to_string(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }
}

#[async_trait]
impl Transcriber for XaiTranscriber {
    fn name(&self) -> &str {
        "xai"
    }

    fn supports(&self) -> &'static [TranscriptionMode] {
        SUPPORTED
    }

    async fn transcribe_stream(
        &self,
        audio_rx: mpsc::Receiver<Vec<f32>>,
        opts: StreamOpts,
    ) -> Result<EventStream> {
        let ws = self.open_streaming_connection(&opts).await?;
        let (event_tx, event_rx) = mpsc::channel::<TranscriptionEvent>(64);

        // Spawn the streaming loop. event_tx wird am Ende dropped, dann
        // schliesst sich der event-Stream natuerlich.
        tauri::async_runtime::spawn(run_xai_streaming(ws, audio_rx, event_tx));

        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(
            event_rx,
        )))
    }

    async fn transcribe_oneshot(&self, audio: &[u8], opts: TranscribeOpts) -> Result<String> {
        let url = format!("{}/stt", self.base_url.trim_end_matches('/'));

        with_retry(|| async {
            // Wichtig (CLAUDE.md §2): `file` muss laut xAI das LETZTE Field
            // sein. multipart::Form ist nicht Clone — pro Versuch neu bauen.
            let part = reqwest::multipart::Part::bytes(audio.to_vec())
                .file_name("audio.wav")
                .mime_str("audio/wav")
                .map_err(|e| VoiceTypeError::Transcription(format!("multipart-Part: {e}")))?;

            let mut form = reqwest::multipart::Form::new().text("model", self.model.clone());
            if let Some(lang) = opts.language.as_deref() {
                form = form.text("language", lang.to_string());
            }
            if let Some(prompt) = opts.initial_prompt.as_deref() {
                form = form.text("initial_prompt", prompt.to_string());
            }
            let form = form.part("file", part);

            let response = self
                .client
                .post(&url)
                .bearer_auth(&self.api_key)
                .multipart(form)
                .send()
                .await
                .map_err(|e| VoiceTypeError::Transcription(format!("HTTP {url}: {e}")))?;

            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(VoiceTypeError::Transcription(format!(
                    "xAI STT HTTP {status}: {body}"
                )));
            }

            let parsed: SttResponse = response
                .json()
                .await
                .map_err(|e| VoiceTypeError::Transcription(format!("xAI-STT-JSON-Parse: {e}")))?;
            Ok(parsed.text.trim().to_string())
        })
        .await
    }
}

#[derive(Deserialize)]
struct SttResponse {
    text: String,
}

impl XaiTranscriber {
    /// Implementation-Detail-Methode fuer transcribe_stream. Trennt
    /// die WebSocket-Logik vom Trait-Body, damit der Trait kompakt bleibt.
    async fn open_streaming_connection(
        &self,
        opts: &StreamOpts,
    ) -> Result<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    > {
        // URL-Parameter exakt nach xAI-Doku (Stand April 2026):
        //   sample_rate=16000  (wir liefern 16-kHz-PCM aus dem Recorder)
        //   encoding=pcm       (s16le wird vom Server impliziert)
        //   interim_results=true (sonst keine Live-Anzeige)
        //   language=...        (optional, dt. Hinweis fuer Text-Formatting)
        let mut url =
            "wss://api.x.ai/v1/stt?sample_rate=16000&encoding=pcm&interim_results=true".to_string();
        if let Some(lang) = opts.language.as_deref() {
            url.push_str(&format!("&language={lang}"));
        }

        let request = tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(url.as_str())
            .map_err(|e| VoiceTypeError::Transcription(format!("xAI WS request: {e}")))?;
        let mut request = request;
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", self.api_key)
                .parse()
                .map_err(|e| VoiceTypeError::Transcription(format!("auth header: {e}")))?,
        );

        let (ws, _resp) = tokio_tungstenite::connect_async(request)
            .await
            .map_err(|e| VoiceTypeError::Transcription(format!("xAI WS connect: {e}")))?;
        Ok(ws)
    }
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
enum XaiStreamMessage {
    #[serde(rename = "transcript.created")]
    Created,
    #[serde(rename = "transcript.partial")]
    Partial {
        #[serde(default)]
        text: String,
        #[serde(default)]
        is_final: bool,
    },
    #[serde(rename = "transcript.done")]
    Done {
        #[serde(default)]
        text: String,
        #[serde(default)]
        duration: f64,
    },
    #[serde(rename = "error")]
    Error {
        #[serde(default)]
        message: Option<String>,
    },
    #[serde(other)]
    Unknown,
}

const AUDIO_DONE_FRAME: &str = r#"{"type":"audio.done"}"#;

/// Zentrale Streaming-Logic. Protokoll exakt nach xAI-Spec (April 2026):
/// 1) Server schickt zuerst `transcript.created` — vor diesem Event darf
///    KEIN Binary-Audio rausgehen (sonst Reset). Bis dahin puffern wir
///    eingehende Recorder-Chunks.
/// 2) Audio als raw s16le binary frames.
/// 3) Stream-Ende: Text-Frame `{"type":"audio.done"}` (NICHT Close-Frame).
/// 4) Server beendet mit `transcript.done` und schliesst die Connection.
async fn run_xai_streaming(
    mut ws: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    mut audio_rx: mpsc::Receiver<Vec<f32>>,
    event_tx: mpsc::Sender<TranscriptionEvent>,
) {
    let started = std::time::Instant::now();
    let mut accumulated_final = String::new();
    let mut server_ready = false;
    let mut audio_buffer: Vec<Vec<f32>> = Vec::new();
    let mut audio_done_sent = false;
    let mut done_sent = false;

    loop {
        tokio::select! {
            chunk = audio_rx.recv() => match chunk {
                Some(samples) => {
                    if server_ready {
                        let bytes = pcm_f32_to_s16le(&samples);
                        if let Err(e) = ws.send(Message::Binary(bytes)).await {
                            let _ = event_tx.send(TranscriptionEvent::Error(
                                format!("xAI WS send: {e}")
                            )).await;
                            done_sent = true;
                            break;
                        }
                    } else {
                        // Server hat noch nicht `transcript.created` geschickt —
                        // Audio puffern, sonst kickt der Server uns sofort.
                        audio_buffer.push(samples);
                    }
                }
                None => {
                    // Recorder zu (Hotkey losgelassen). Falls Server noch
                    // nicht ready: kurzes Timeout, dann harter Close.
                    if server_ready && !audio_done_sent {
                        if let Err(e) = ws.send(Message::Text(AUDIO_DONE_FRAME.into())).await {
                            tracing::warn!(error = %e, "audio.done konnte nicht gesendet werden");
                        }
                        audio_done_sent = true;
                    } else if !server_ready {
                        // Notfall — Server hat sich nie gemeldet.
                        let _ = ws.send(Message::Close(None)).await;
                        let _ = event_tx.send(TranscriptionEvent::Error(
                            "xAI WS: kein transcript.created erhalten".into()
                        )).await;
                        done_sent = true;
                        break;
                    }
                }
            },
            msg = ws.next() => match msg {
                Some(Ok(Message::Text(text))) => {
                    match serde_json::from_str::<XaiStreamMessage>(&text) {
                        Ok(XaiStreamMessage::Created) => {
                            server_ready = true;
                            // Gepufferte Chunks rausschicken — beibehaltene
                            // Reihenfolge ist wichtig fuer Sprach-Kontext.
                            let buffered = std::mem::take(&mut audio_buffer);
                            for chunk in buffered {
                                let bytes = pcm_f32_to_s16le(&chunk);
                                if let Err(e) = ws.send(Message::Binary(bytes)).await {
                                    let _ = event_tx.send(TranscriptionEvent::Error(
                                        format!("xAI WS send (flush): {e}")
                                    )).await;
                                    done_sent = true;
                                    break;
                                }
                            }
                            if done_sent { break; }
                        }
                        Ok(XaiStreamMessage::Partial { text, is_final }) => {
                            if is_final && !text.trim().is_empty() {
                                if !accumulated_final.is_empty() {
                                    accumulated_final.push(' ');
                                }
                                accumulated_final.push_str(text.trim());
                            }
                            let _ = event_tx.send(TranscriptionEvent::Partial {
                                text,
                                is_final,
                            }).await;
                        }
                        Ok(XaiStreamMessage::Done { text, duration }) => {
                            let final_text = if text.trim().is_empty() {
                                accumulated_final.trim().to_string()
                            } else {
                                text.trim().to_string()
                            };
                            let duration_ms = if duration > 0.0 {
                                (duration * 1000.0) as u32
                            } else {
                                started.elapsed().as_millis() as u32
                            };
                            let _ = event_tx.send(TranscriptionEvent::Done {
                                text: final_text,
                                duration_ms,
                            }).await;
                            done_sent = true;
                            break;
                        }
                        Ok(XaiStreamMessage::Error { message }) => {
                            let msg = message.unwrap_or_else(|| "xAI Streaming-Error".into());
                            tracing::warn!(message = %msg, "xAI Streaming-Error-Event");
                            let _ = event_tx.send(TranscriptionEvent::Error(msg)).await;
                            done_sent = true;
                            break;
                        }
                        Ok(XaiStreamMessage::Unknown) => {
                            tracing::debug!(payload = %text, "xAI Streaming: unbekannte Message");
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, payload = %text, "xAI Streaming JSON-Parse fehlgeschlagen");
                        }
                    }
                }
                Some(Ok(Message::Close(_))) => break,
                Some(Ok(_)) => {} // Ping/Pong/Binary ignorieren
                Some(Err(e)) => {
                    let _ = event_tx.send(TranscriptionEvent::Error(
                        format!("xAI WS recv: {e}")
                    )).await;
                    done_sent = true;
                    break;
                }
                None => break,
            },
        }
    }

    if !done_sent {
        let _ = event_tx
            .send(TranscriptionEvent::Done {
                text: accumulated_final.trim().to_string(),
                duration_ms: started.elapsed().as_millis() as u32,
            })
            .await;
    }
}

fn pcm_f32_to_s16le(samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        let q = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.extend_from_slice(&q.to_le_bytes());
    }
    out
}
