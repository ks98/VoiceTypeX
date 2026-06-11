// SPDX-License-Identifier: GPL-3.0-or-later
//! Exponential-backoff retry for operations that can fail transiently
//! (HTTP calls against cloud providers).
//!
//! Convention:
//! - Only retryable errors (see `VoiceTypeError::is_retryable`) are
//!   retried; everything else (4xx auth, InvalidInput, Internal)
//!   gives up immediately.
//! - Backoff doubles per attempt: 100 → 400 → 1600 ms.
//! - Default: 3 attempts. Use `with_retry_n` for different values.

use crate::core::error::{Result, VoiceTypeError};
use std::future::Future;
use std::time::Duration;

const DEFAULT_MAX_ATTEMPTS: u32 = 3;
const INITIAL_BACKOFF_MS: u64 = 100;
const BACKOFF_MULTIPLIER: u64 = 4;

pub async fn with_retry<F, Fut, T>(operation: F) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    with_retry_n(operation, DEFAULT_MAX_ATTEMPTS).await
}

pub async fn with_retry_n<F, Fut, T>(operation: F, max_attempts: u32) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut backoff_ms = INITIAL_BACKOFF_MS;
    let mut last_err: Option<VoiceTypeError> = None;

    for attempt in 0..max_attempts {
        match operation().await {
            Ok(value) => {
                if attempt > 0 {
                    tracing::info!(attempt, "retry succeeded");
                }
                return Ok(value);
            }
            Err(e) if e.is_retryable() && attempt + 1 < max_attempts => {
                tracing::warn!(
                    attempt = attempt + 1,
                    max = max_attempts,
                    backoff_ms,
                    kind = ?e.kind(),
                    "transient error — retrying"
                );
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                backoff_ms = backoff_ms.saturating_mul(BACKOFF_MULTIPLIER);
                last_err = Some(e);
            }
            Err(e) => return Err(e),
        }
    }

    // Only reachable when all attempts were retryable and failed.
    Err(last_err.unwrap_or_else(|| {
        VoiceTypeError::Other(anyhow::anyhow!(
            "retry loop ended without a success or error path"
        ))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::error::ProviderId;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn first_attempt_succeeds_no_retry() {
        let counter = Arc::new(AtomicU32::new(0));
        let c = Arc::clone(&counter);
        let result = with_retry(|| {
            let c = Arc::clone(&c);
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok::<_, VoiceTypeError>(42)
            }
        })
        .await
        .unwrap();
        assert_eq!(result, 42);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn transient_failure_retries_and_succeeds() {
        let counter = Arc::new(AtomicU32::new(0));
        let c = Arc::clone(&counter);
        let result = with_retry(move || {
            let c = Arc::clone(&c);
            async move {
                let n = c.fetch_add(1, Ordering::SeqCst) + 1;
                if n < 3 {
                    Err(VoiceTypeError::processing_http(
                        503,
                        ProviderId::OpenAi,
                        "HTTP 503: Service Unavailable",
                    ))
                } else {
                    Ok(n)
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(result, 3);
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn auth_failure_no_retry() {
        let counter = Arc::new(AtomicU32::new(0));
        let c = Arc::clone(&counter);
        let err = with_retry(move || {
            let c = Arc::clone(&c);
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err::<i32, _>(VoiceTypeError::processing_http(
                    401,
                    ProviderId::OpenAi,
                    "HTTP 401: Unauthorized",
                ))
            }
        })
        .await
        .unwrap_err();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert!(err.to_string().contains("401"));
    }

    #[tokio::test]
    async fn exhausts_attempts_returns_last_error() {
        let counter = Arc::new(AtomicU32::new(0));
        let c = Arc::clone(&counter);
        let err = with_retry_n(
            move || {
                let c = Arc::clone(&c);
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Err::<i32, _>(VoiceTypeError::processing_http(
                        502,
                        ProviderId::OpenAi,
                        "HTTP 502: Bad Gateway",
                    ))
                }
            },
            3,
        )
        .await
        .unwrap_err();
        assert_eq!(counter.load(Ordering::SeqCst), 3);
        assert!(err.to_string().contains("502"));
    }
}
