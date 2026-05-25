// SPDX-License-Identifier: GPL-3.0-or-later
//! In-memory ring buffer for `tracing` events.
//!
//! A `LogRingBuffer` holds the last N formatted log lines; a
//! `LogHandle` (`Layer` impl) feeds it from the `tracing-subscriber`
//! stack. The frontend Logs view polls via IPC.
//!
//! IMPORTANT (CLAUDE.md §8): audio / transcript / LLM-response data
//! does NOT go through logging by default — we log only control flow
//! and error texts. A diagnostic-logging toggle extension would be
//! additive: one extra log call when the toggle is active.

use parking_lot::Mutex;
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::Arc;
use tracing::field::{Field, Visit};
use tracing_subscriber::Layer;

/// Default capacity of the ring buffer.
pub const DEFAULT_CAPACITY: usize = 500;

#[derive(Clone)]
pub struct LogRingBuffer {
    inner: Arc<Mutex<VecDeque<String>>>,
    capacity: usize,
}

impl LogRingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity,
        }
    }

    /// Returns the last `limit` lines, newest last.
    pub fn lines(&self, limit: usize) -> Vec<String> {
        let buffer = self.inner.lock();
        let take = limit.min(buffer.len());
        let skip = buffer.len() - take;
        buffer.iter().skip(skip).cloned().collect()
    }

    pub fn layer(&self) -> LogHandle {
        LogHandle {
            inner: Arc::clone(&self.inner),
            capacity: self.capacity,
        }
    }
}

impl Default for LogRingBuffer {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

pub struct LogHandle {
    inner: Arc<Mutex<VecDeque<String>>>,
    capacity: usize,
}

impl<S> Layer<S> for LogHandle
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor = StringVisitor::default();
        event.record(&mut visitor);

        let metadata = event.metadata();
        let line = format!(
            "[{:5}] {} - {}",
            metadata.level(),
            metadata.target(),
            visitor.message.trim()
        );

        let mut buffer = self.inner.lock();
        if buffer.len() >= self.capacity {
            buffer.pop_front();
        }
        buffer.push_back(line);
    }
}

#[derive(Default)]
struct StringVisitor {
    message: String,
}

impl Visit for StringVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(&mut self.message, "{value:?}");
        } else {
            let _ = write!(&mut self.message, " {}={value:?}", field.name());
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            let _ = write!(&mut self.message, " {}={}", field.name(), value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_drops_oldest_at_capacity() {
        let buf = LogRingBuffer::new(3);
        let inner = Arc::clone(&buf.inner);
        inner.lock().push_back("a".into());
        inner.lock().push_back("b".into());
        inner.lock().push_back("c".into());
        assert_eq!(buf.lines(10), vec!["a", "b", "c"]);

        // Simulate a layer write via direct push (the Layer impl
        // pop_fronts when full).
        {
            let mut g = inner.lock();
            if g.len() >= buf.capacity {
                g.pop_front();
            }
            g.push_back("d".into());
        }
        assert_eq!(buf.lines(10), vec!["b", "c", "d"]);
    }

    #[test]
    fn lines_returns_at_most_limit() {
        let buf = LogRingBuffer::new(10);
        for i in 0..5 {
            buf.inner.lock().push_back(format!("line {i}"));
        }
        assert_eq!(buf.lines(2), vec!["line 3", "line 4"]);
        assert_eq!(buf.lines(100).len(), 5);
    }
}
