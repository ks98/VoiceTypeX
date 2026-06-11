// SPDX-License-Identifier: GPL-3.0-or-later
//
// Single source of truth for Tauri event wire names.
//
// The backend mirrors the Rust-emitted subset in
// `src-tauri/src/core/events.rs`; the string values on both sides MUST stay
// identical or the emit/listen channel silently desyncs (#48). `focus-logs`
// and `locale-changed` are frontend-internal (window-to-window) and have no
// Rust counterpart.

export const EVENTS = {
  /** Pipeline-phase updates for the overlay (backend → overlay). */
  STATE: "app://state",
  /** Streaming partial transcript during recording (backend → overlay). */
  PARTIAL_TRANSCRIPT: "app://partial-transcript",
  /** Active STT/LLM engine + model for the overlay status line, #8. */
  ACTIVE_ENGINE: "app://active-engine",
  /** Overlay error click → main window switches to the Logs tab. */
  FOCUS_LOGS: "app://focus-logs",
  /** Whisper model download progress (backend → settings/onboarding). */
  MODEL_DOWNLOAD_PROGRESS: "model-download-progress",
  /** LLM (GGUF) model download progress (backend → settings/onboarding). */
  LLM_MODEL_DOWNLOAD_PROGRESS: "llm-model-download-progress",
  /** Cross-window locale sync (settings/onboarding → every webview window). */
  LOCALE_CHANGED: "i18n://locale-changed",
} as const;
