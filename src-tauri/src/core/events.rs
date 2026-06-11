// SPDX-License-Identifier: GPL-3.0-or-later
//! Tauri event-name constants.
//!
//! Single source of truth for the wire names of the events the backend
//! emits to the frontend. The frontend mirrors these in `src/lib/events.ts`;
//! the string values on both sides MUST stay identical or the emit/listen
//! channel silently desyncs (#48).

/// Pipeline-phase updates for the overlay (`Phase` payload).
pub const STATE: &str = "app://state";

/// Streaming partial transcript (stable word prefixes) during recording.
pub const PARTIAL_TRANSCRIPT: &str = "app://partial-transcript";

/// Active STT/LLM engine + model for the overlay status line (#8).
pub const ACTIVE_ENGINE: &str = "app://active-engine";

/// Whisper model download progress.
pub const MODEL_DOWNLOAD_PROGRESS: &str = "model-download-progress";

/// LLM (GGUF) model download progress — a separate channel from Whisper
/// so both downloads can report progress in parallel.
pub const LLM_MODEL_DOWNLOAD_PROGRESS: &str = "llm-model-download-progress";
