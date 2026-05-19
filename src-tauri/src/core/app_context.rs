// SPDX-License-Identifier: GPL-3.0-or-later
//! `AppContext` — der Singleton-Zustand der Anwendung, in Tauri-State gemanagt.
//!
//! Alle Felder sind `Send + Sync + 'static`, damit Tauri-Commands sie via
//! `tauri::State<'_, AppContext>` aus jedem Async-Kontext lesen koennen.

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
use std::path::PathBuf;
use std::sync::Arc;

pub struct AppContext {
    pub state_bus: StateBus,
    pub modes: Arc<ModesRegistry>,
    pub recorder_slot: Arc<Mutex<Option<RecorderHandle>>>,
    /// Der Modus, mit dem das gerade laufende Recording gestartet wurde.
    /// Wird in `start_recording` gesetzt, in `finish_recording_and_inject`
    /// genutzt und beim Abschluss wieder geleert. Der Menue-Hotkey im
    /// Recording-State liest hier den Modus, der die Pipeline finalisieren
    /// soll — ohne dass er ihn explizit mitschicken muss.
    pub active_mode: Arc<Mutex<Option<Mode>>>,
    /// Auf Wayland: der effektive Hotkey-Trigger, wie ihn der Compositor
    /// nach `bind_shortcuts` zurueckliefert (z.B. "Meta+Space"). Der
    /// `preferred_trigger` aus `Settings.menu_hotkey` ist nur ein
    /// Vorschlag — KDE/GNOME koennen davon abweichen, und der User
    /// kann den Hotkey in den System-Settings nachjustieren. `None` bis
    /// die Wayland-Portal-Session ihre erste Antwort geliefert hat;
    /// auf X11/Windows bleibt der Wert `None`, dort ist
    /// `Settings.menu_hotkey` die Wahrheit.
    pub effective_menu_hotkey: Arc<RwLock<Option<String>>>,
    pub transcriber: Arc<dyn Transcriber>,
    /// Direkter Handle auf den `LocalTranscriber` — wird vom Streaming-
    /// Worker (Phase 2) gebraucht, der die nicht-trait-Methode
    /// `transcribe_streaming_pass` aufruft. Zeigt auf die gleiche
    /// Instanz wie `transcriber`, wenn der App-Default lokal ist;
    /// sonst eigene LocalTranscriber-Instanz parallel.
    pub local_transcriber: Arc<LocalTranscriber>,
    /// **Phase 3b** — Embedded-LLM-Processor. Lazy-Modell-Load beim ersten
    /// `process()`-Call; danach gehalten fuer die App-Lebenszeit. Wird
    /// nur dann benutzt, wenn ein Modus `local_engine = "embedded"`
    /// setzt; sonst bleibt der Modell-Cache leer und das File auf Disk
    /// muss noch nicht existieren.
    pub local_llm_processor: Arc<LlamaEmbeddedProcessor>,
    /// Handle des aktuell laufenden Streaming-Decode-Workers (Phase 2,
    /// nur bei `transcription = "local"`). Wird in `start_recording`
    /// gespawnt, in `finish_recording_and_inject` abortet, bevor der
    /// Final-Pass laeuft. `None` = kein Streaming aktiv. Typ matched die
    /// projektweite Konvention `tauri::async_runtime::spawn(...)`.
    pub active_streaming_handle: Arc<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
    pub injector: Arc<dyn TextInjector>,
    pub settings: Arc<RwLock<Settings>>,
    /// Persistenz-Pfad fuer Settings (`~/.config/.../settings.json`).
    /// Nach jedem Mutations-IPC `Settings::save(&settings_path)` aufrufen.
    pub settings_path: PathBuf,
    pub log_buffer: LogRingBuffer,
    pub model_dir: PathBuf,
    pub modes_dir: PathBuf,
}
