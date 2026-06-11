// SPDX-License-Identifier: GPL-3.0-or-later
//! Core building blocks: state machine, configuration, mode model, error taxonomy.

pub mod app_context;
pub mod bounded_lru;
pub mod config;
pub mod default_modes;
pub mod edit;
pub mod error;
pub mod events;
pub mod hardware;
pub mod log_buffer;
pub mod modes;
pub mod retry;
pub mod session;
pub mod state;

pub use app_context::AppContext;
pub use error::{Result, VoiceTypeError};
pub use modes::{
    InjectionMethod, InputSource, Mode, OutputAction, ProcessingTarget, TranscriptionTarget,
};
pub use state::{AppState, StateBus};
