// SPDX-License-Identifier: GPL-3.0-or-later
//! Globale State-Machine.
//!
//! Ein einziger `StateBus` haelt den aktuellen Pipeline-Zustand und broadcastet
//! jede Aenderung an alle Subscriber (Tray, Frontend via IPC, Logs-View, ...).
//! Modelliert mit `tokio::sync::watch`, weil:
//! 1) **Letzter Wert reicht** — neue Subscriber sehen sofort den aktuellen
//!    Zustand, kein "Replay" alter Events noetig.
//! 2) **Kein Backpressure-Drama** — wenn ein Subscriber nicht schnell genug
//!    liest, geht der State nicht verloren, der Subscriber sieht beim naechsten
//!    Read einfach den neuesten Wert.
//! 3) **Cheap clone** — Sender ist intern Arc, mehrere Producer moeglich falls
//!    spaeter mehrere Pipelines parallel laufen sollen.

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

    /// Erlaubte Uebergaenge laut CLAUDE.md §4.1 (zzgl. Error als Sink).
    pub fn can_transition_to(&self, next: &AppState) -> bool {
        match (self, next) {
            (Self::Idle, Self::Recording) => true,
            (Self::Recording, Self::Transcribing) => true,
            (Self::Transcribing, Self::Postprocessing) => true,
            (Self::Transcribing, Self::Injecting) => true, // Modus ohne Postprocessing
            (Self::Postprocessing, Self::Injecting) => true,
            (Self::Injecting, Self::Idle) => true,
            (Self::Error(_), Self::Idle) => true,
            (_, Self::Error(_)) => true, // Error ist von ueberall erreichbar
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

    /// Validiere und setze den naechsten State. Loggt jeden Uebergang.
    pub fn transition(&self, next: AppState) -> Result<()> {
        let current = self.current();
        if !current.can_transition_to(&next) {
            return Err(VoiceTypeError::InvalidStateTransition {
                from: current.label().to_string(),
                to: next.label().to_string(),
            });
        }
        tracing::info!(from = %current.label(), to = %next.label(), "State-Uebergang");
        // `send_replace` schreibt den Wert immer, auch ohne aktive Receiver —
        // entscheidend, weil Subscriber sich erst spaeter ueber `subscribe()`
        // anhaengen koennen und der State trotzdem korrekt sein muss.
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
}
