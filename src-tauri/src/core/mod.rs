// SPDX-License-Identifier: GPL-3.0-or-later
//! Kern-Bausteine: State-Machine, Konfiguration, Modus-Modell, Fehler-Taxonomie.

pub mod app_context;
pub mod config;
pub mod default_modes;
pub mod error;
pub mod log_buffer;
pub mod modes;
pub mod retry;
pub mod session;
pub mod state;

pub use app_context::AppContext;
pub use error::{Result, VoiceTypeError};
pub use modes::{InjectionMethod, Mode, ProcessingTarget, TranscriptionTarget};
pub use state::{AppState, StateBus};
