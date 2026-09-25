// SPDX-License-Identifier: GPL-3.0-or-later
//! One-shot loopback HTTP listener that receives the OAuth redirect.

use super::oauth::CALLBACK_PATH;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Same ports as the Codex CLI (1455, fallback 1457): the redirect URI
/// has to be one that the Codex client id accepts.
const PORTS: [u16; 2] = [1455, 1457];
const MAX_REQUEST_BYTES: usize = 8 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(5);

const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>VoiceTypeX</title>\
<body style=\"font-family:sans-serif;margin:3em\"><h2>VoiceTypeX</h2>\
<p>The sign-in response was received. You can close this tab and return to VoiceTypeX.</p>";

/// Binds 127.0.0.1 only — never reachable from the network. `None` when
/// both ports are taken (e.g. by a running Codex login); the user can
/// then paste the redirect URL instead.
pub async fn bind() -> Option<(TcpListener, u16)> {
    for port in PORTS {
        match TcpListener::bind(("127.0.0.1", port)).await {
            Ok(listener) => return Some((listener, port)),
            Err(e) => tracing::warn!(port, error = %e, "ChatGPT sign-in: callback port busy"),
        }
    }
    None
}

/// Serves requests until the callback path arrives and returns its request
/// target (`/auth/callback?…`). Other requests (e.g. `/favicon.ico`) get a
/// 404 and the listener keeps waiting. The response never echoes the query.
pub async fn wait_for_callback(listener: TcpListener) -> std::io::Result<String> {
    loop {
        let (mut stream, _) = listener.accept().await?;
        let Some(target) = read_request_target(&mut stream).await else {
            continue;
        };
        if is_callback(&target) {
            let _ = respond(&mut stream, "200 OK", DONE_PAGE).await;
            return Ok(target);
        }
        let _ = respond(&mut stream, "404 Not Found", "").await;
    }
}

async fn read_request_target(stream: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    let read_head = async {
        while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < MAX_REQUEST_BYTES {
            let n = stream.read(&mut chunk).await.ok()?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        Some(())
    };
    tokio::time::timeout(READ_TIMEOUT, read_head).await.ok()??;
    request_target(&String::from_utf8_lossy(&buf)).map(str::to_owned)
}

/// Target of a `GET` request line; anything else is ignored.
fn request_target(head: &str) -> Option<&str> {
    let mut parts = head.lines().next()?.split_whitespace();
    match (parts.next()?, parts.next()?) {
        ("GET", target) => Some(target),
        _ => None,
    }
}

fn is_callback(target: &str) -> bool {
    target.split('?').next() == Some(CALLBACK_PATH)
}

async fn respond(stream: &mut TcpStream, status: &str, body: &str) -> std::io::Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_get_targets_only() {
        assert_eq!(
            request_target("GET /auth/callback?code=a HTTP/1.1\r\nHost: x\r\n\r\n"),
            Some("/auth/callback?code=a")
        );
        assert_eq!(request_target("POST /auth/callback HTTP/1.1\r\n"), None);
        assert_eq!(request_target(""), None);
    }

    #[test]
    fn recognises_callback_path() {
        assert!(is_callback("/auth/callback?code=a&state=b"));
        assert!(is_callback("/auth/callback"));
        assert!(!is_callback("/favicon.ico"));
        assert!(!is_callback("/auth/callbackx?code=a"));
    }

    async fn get(port: u16, target: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        out
    }

    #[tokio::test]
    async fn skips_other_requests_until_the_callback_arrives() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(wait_for_callback(listener));

        let favicon = get(port, "/favicon.ico").await;
        assert!(favicon.starts_with("HTTP/1.1 404"));

        let page = get(port, "/auth/callback?code=c&state=s").await;
        assert!(page.starts_with("HTTP/1.1 200"));
        assert!(!page.contains("code=c"), "response must not echo the query");

        assert_eq!(
            server.await.unwrap().unwrap(),
            "/auth/callback?code=c&state=s"
        );
    }
}
