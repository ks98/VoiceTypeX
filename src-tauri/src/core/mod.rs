// SPDX-License-Identifier: GPL-3.0-or-later
//! Kern-Bausteine: State-Machine, Konfiguration, Modus-Modell, Fehler-Taxonomie.

pub mod config;
pub mod error;
pub mod modes;
pub mod state;

pub use error::{Result, VoiceTypeError};
pub use modes::{InjectionMethod, Mode, ProcessingTarget, TranscriptionTarget};
pub use state::{AppState, StateBus};
