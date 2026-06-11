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

#[cfg(test)]
mod tests {
    use super::*;

    // Contract test — EVENT-NAME parity (#49, pairs with #48).
    //
    // Pins each backend-emitted `pub const` to its exact wire string. The
    // identical literals are hard-coded on the TS side in
    // `src/lib/events.ts` + `src/lib/events.test.ts`. Both sides are
    // anchored to this canonical list, so a one-sided rename (a changed
    // `pub const` here OR a typo in `events.ts`) breaks its own test
    // instead of silently desyncing the emit/listen channel at runtime.
    //
    // Only the five events the backend actually emits live here; the
    // frontend-internal `app://focus-logs` and `i18n://locale-changed`
    // (window-to-window) have no Rust counterpart.
    //
    // Honest limit (contract-tests-over-codegen, no specta/ts-rs): the two
    // sides do NOT auto-derive — a coordinated rename of the const AND
    // this literal AND the TS literal would pass. Accepted trade-off; the
    // test catches the realistic one-sided-change failure.
    #[test]
    fn wire_names_pinned_to_canonical_strings() {
        assert_eq!(STATE, "app://state");
        assert_eq!(PARTIAL_TRANSCRIPT, "app://partial-transcript");
        assert_eq!(ACTIVE_ENGINE, "app://active-engine");
        assert_eq!(MODEL_DOWNLOAD_PROGRESS, "model-download-progress");
        assert_eq!(LLM_MODEL_DOWNLOAD_PROGRESS, "llm-model-download-progress");
    }
}
