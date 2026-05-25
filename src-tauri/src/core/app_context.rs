// SPDX-License-Identifier: GPL-3.0-or-later
//! `AppContext` — the application's singleton state, managed in
//! Tauri state.
//!
//! All fields are `Send + Sync + 'static` so Tauri commands can read
//! them via `tauri::State<'_, AppContext>` from any async context.

use crate::audio::recorder::RecorderHandle;
use crate::core::config::Settings;
use crate::core::log_buffer::LogRingBuffer;
use crate::core::modes::{Mode, ModesRegistry};
use crate::core::state::StateBus;
use crate::injection::TextInjector;
use crate::processing::embedded::LlamaEmbeddedProcessor;
use crate::transcription::local::LocalTranscriber;
use crate::transcription::Transcriber;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

pub struct AppContext {
    pub state_bus: StateBus,
    pub modes: Arc<ModesRegistry>,
    pub recorder_slot: Arc<Mutex<Option<RecorderHandle>>>,
    /// The mode the currently running recording was started with.
    /// Set in `start_recording`, used in `finish_recording_and_inject`
    /// and cleared on completion. The menu hotkey in the Recording
    /// state reads the mode that should finalize the pipeline here —
    /// without having to pass it along explicitly.
    pub active_mode: Arc<Mutex<Option<Mode>>>,
    /// On Wayland: the effective hotkey trigger as returned by the
    /// compositor after `bind_shortcuts` (e.g. "Meta+Space"). The
    /// `preferred_trigger` from `Settings.menu_hotkey` is only a
    /// suggestion — KDE/GNOME may deviate, and the user can adjust
    /// the hotkey in system settings. `None` until the Wayland portal
    /// session has delivered its first response; on X11/Windows the
    /// value stays `None` — there `Settings.menu_hotkey` is the truth.
    pub effective_menu_hotkey: Arc<RwLock<Option<String>>>,
    pub transcriber: Arc<dyn Transcriber>,
    /// Direct handle on the `LocalTranscriber` — needed by the
    /// streaming worker (phase 2), which calls the non-trait method
    /// `transcribe_streaming_pass`. Points to the same instance as
    /// `transcriber` if the app default is local; otherwise to a
    /// separate `LocalTranscriber` instance in parallel.
    pub local_transcriber: Arc<LocalTranscriber>,
    /// **Phase 3b** — embedded LLM processor. Lazy model load on the
    /// first `process()` call; afterwards held for the app's lifetime.
    /// Only used when a mode sets `local_engine = "embedded"`;
    /// otherwise the model cache stays empty and the file on disk
    /// doesn't have to exist yet.
    pub local_llm_processor: Arc<LlamaEmbeddedProcessor>,
    /// Phase-3b refactor: cache of override `LocalTranscriber`s per
    /// Whisper model slot. A per-mode `mode.whisper_model_slot`
    /// triggers lazy load of a new `LocalTranscriber` for that slot;
    /// all further calls use the cached one. Key is the slot slug,
    /// value is the transcriber.
    pub extra_transcribers: Arc<Mutex<HashMap<String, Arc<LocalTranscriber>>>>,
    /// Analogously for `mode.embedded_llm_slot` — cache of override
    /// `LlamaEmbeddedProcessor`s per GGUF slot.
    pub extra_llm_processors: Arc<Mutex<HashMap<String, Arc<LlamaEmbeddedProcessor>>>>,
    /// Handle of the currently running streaming decode worker
    /// (phase 2, only when `transcription = "local"`). Spawned in
    /// `start_recording`, aborted in `finish_recording_and_inject`
    /// before the final pass runs. `None` = no streaming active. The
    /// type matches the project-wide convention
    /// `tauri::async_runtime::spawn(...)`.
    pub active_streaming_handle: Arc<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
    pub injector: Arc<dyn TextInjector>,
    /// Selection captured eagerly when the menu hotkey opens the menu
    /// in `Idle` (see `pipeline::handle_menu_hotkey`). Consumed by edit
    /// modes (`Mode.input == Selection`) in
    /// `finish_recording_and_inject`; voice modes ignore it. `None` =
    /// nothing captured / nothing selected. Captured before the menu
    /// steals focus, because reading the selection needs the target app
    /// focused.
    pub selection_buffer: Arc<Mutex<Option<String>>>,
    pub settings: Arc<RwLock<Settings>>,
    /// Persistence path for the settings
    /// (`~/.config/.../settings.json`). After each mutating IPC call,
    /// invoke `Settings::save(&settings_path)`.
    pub settings_path: PathBuf,
    pub log_buffer: LogRingBuffer,
    pub model_dir: PathBuf,
    pub modes_dir: PathBuf,
}
