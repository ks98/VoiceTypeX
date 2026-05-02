// SPDX-License-Identifier: GPL-3.0-or-later
//! `AppContext` — der Singleton-Zustand der Anwendung, in Tauri-State gemanagt.
//!
//! Alle Felder sind `Send + Sync + 'static`, damit Tauri-Commands sie via
//! `tauri::State<'_, AppContext>` aus jedem Async-Kontext lesen koennen.

use crate::audio::recorder::RecorderHandle;
use crate::core::config::Settings;
use crate::core::modes::ModesRegistry;
use crate::core::state::StateBus;
use crate::injection::TextInjector;
use crate::transcription::Transcriber;
use parking_lot::{Mutex, RwLock};
use std::path::PathBuf;
use std::sync::Arc;

pub struct AppContext {
    pub state_bus: StateBus,
    pub modes: Arc<ModesRegistry>,
    pub recorder_slot: Arc<Mutex<Option<RecorderHandle>>>,
    pub transcriber: Arc<dyn Transcriber>,
    pub injector: Arc<dyn TextInjector>,
    pub settings: Arc<RwLock<Settings>>,
    pub model_dir: PathBuf,
    pub modes_dir: PathBuf,
}
