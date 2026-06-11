// SPDX-License-Identifier: GPL-3.0-or-later
//! Global state machine.
//!
//! A single `StateBus` holds the current pipeline state and
//! broadcasts every change to all subscribers (tray, frontend via
//! IPC, logs view, ...). Modeled with `tokio::sync::watch` because:
//! 1) **Last value suffices** — new subscribers see the current state
//!    immediately, no "replay" of old events needed.
//! 2) **No backpressure drama** — if a subscriber doesn't read fast
//!    enough, the state isn't lost; on the next read the subscriber
//!    just sees the newest value.
//! 3) **Cheap clone** — the sender is internally an Arc; multiple
//!    producers are possible should several pipelines later run in
//!    parallel.

use crate::core::error::{Result, VoiceTypeError};
use std::sync::Arc;
use tokio::sync::watch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppState {
    Idle,
    Recording,
    Transcribing,
    Postprocessing,
    Injecting,
    Error(String),
}

impl AppState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Recording => "recording",
            Self::Transcribing => "transcribing",
            Self::Postprocessing => "postprocessing",
            Self::Injecting => "injecting",
            Self::Error(_) => "error",
        }
    }

    /// Allowed transitions per CLAUDE.md §4.1 (plus Error as a sink).
    pub fn can_transition_to(&self, next: &AppState) -> bool {
        match (self, next) {
            (Self::Idle, Self::Recording) => true,
            (Self::Recording, Self::Transcribing) => true,
            (Self::Transcribing, Self::Postprocessing) => true,
            (Self::Transcribing, Self::Injecting) => true, // Mode without postprocessing
            (Self::Postprocessing, Self::Injecting) => true,
            (Self::Injecting, Self::Idle) => true,
            // Finish/abort without injection (e.g. the diagnostic test
            // path, or a recording cancelled before transcription).
            (Self::Recording, Self::Idle) => true,
            (Self::Transcribing, Self::Idle) => true,
            (Self::Error(_), Self::Idle) => true,
            (_, Self::Error(_)) => true, // Error is reachable from anywhere
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StateBus {
    sender: Arc<watch::Sender<AppState>>,
}

impl StateBus {
    pub fn new() -> Self {
        let (tx, _rx) = watch::channel(AppState::Idle);
        Self {
            sender: Arc::new(tx),
        }
    }

    pub fn current(&self) -> AppState {
        self.sender.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<AppState> {
        self.sender.subscribe()
    }

    /// Validate and set the next state. Logs every transition.
    pub fn transition(&self, next: AppState) -> Result<()> {
        let current = self.current();
        if !current.can_transition_to(&next) {
            return Err(VoiceTypeError::InvalidStateTransition {
                from: current.label().to_string(),
                to: next.label().to_string(),
            });
        }
        tracing::info!(from = %current.label(), to = %next.label(), "State transition");
        // `send_replace` writes the value always, even without active
        // receivers — crucial because subscribers can attach later via
        // `subscribe()` and the state must still be correct.
        self.sender.send_replace(next);
        Ok(())
    }
}

impl Default for StateBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_flow_idle_to_idle() {
        let bus = StateBus::new();
        assert_eq!(bus.current(), AppState::Idle);
        bus.transition(AppState::Recording).unwrap();
        bus.transition(AppState::Transcribing).unwrap();
        bus.transition(AppState::Postprocessing).unwrap();
        bus.transition(AppState::Injecting).unwrap();
        bus.transition(AppState::Idle).unwrap();
        assert_eq!(bus.current(), AppState::Idle);
    }

    #[test]
    fn flow_without_postprocessing() {
        let bus = StateBus::new();
        bus.transition(AppState::Recording).unwrap();
        bus.transition(AppState::Transcribing).unwrap();
        bus.transition(AppState::Injecting).unwrap();
        bus.transition(AppState::Idle).unwrap();
    }

    #[test]
    fn invalid_idle_to_transcribing_is_rejected() {
        let bus = StateBus::new();
        let err = bus.transition(AppState::Transcribing).unwrap_err();
        assert!(matches!(err, VoiceTypeError::InvalidStateTransition { .. }));
        assert_eq!(bus.current(), AppState::Idle);
    }

    #[test]
    fn error_can_be_reached_from_anywhere() {
        let bus = StateBus::new();
        bus.transition(AppState::Error("test".into())).unwrap();
        assert!(matches!(bus.current(), AppState::Error(_)));
        bus.transition(AppState::Idle).unwrap();
    }

    /// Regression test for the "Transcribing deadlock" (issues #17/#21).
    ///
    /// `run_test_transcription` (ipc/recording.rs) drives the bus
    /// `Idle -> Recording -> Transcribing` and on every exit path tries
    /// to return to `Idle`. #21 legalizes the `Recording -> Idle` and
    /// `Transcribing -> Idle` edges ("finish/abort without injection"),
    /// so each `transition` now succeeds and the bus returns to `Idle`
    /// instead of getting stuck — that was the deadlock mechanism.
    #[test]
    fn transcribing_deadlock_recording_and_transcribing_to_idle_allowed() {
        // Edge-level: both back-to-Idle transitions are legal now.
        assert!(AppState::Recording.can_transition_to(&AppState::Idle));
        assert!(AppState::Transcribing.can_transition_to(&AppState::Idle));

        // Bus-level: replay the run_test_transcription sequence and show
        // the bus returns to Idle on every exit path.
        let bus = StateBus::new();
        bus.transition(AppState::Recording).unwrap();

        // L110/L120 case: Recording -> Idle succeeds, bus is back to Idle.
        bus.transition(AppState::Idle).unwrap();
        assert_eq!(bus.current(), AppState::Idle);

        bus.transition(AppState::Recording).unwrap();
        bus.transition(AppState::Transcribing).unwrap();

        // L153 case: Transcribing -> Idle succeeds, bus is back to Idle.
        bus.transition(AppState::Idle).unwrap();
        assert_eq!(bus.current(), AppState::Idle);
    }
}
