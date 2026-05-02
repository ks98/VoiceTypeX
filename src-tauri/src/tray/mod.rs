// SPDX-License-Identifier: GPL-3.0-or-later
//! Tray-Icon und -Menu. Statusfarben spiegeln `AppState`-Aenderungen.

pub mod icon;

use crate::core::error::Result;
use crate::core::state::AppState;

/// Mappe einen `AppState` auf den Pfad des passenden Tray-Icons.
/// Phase 1.2: nur Konstanten-Mapping. Phase 1.4 verbindet das per Subscriber
/// mit dem `StateBus` und triggert echte Tray-Icon-Updates.
pub fn icon_for_state(state: &AppState) -> &'static str {
    match state {
        AppState::Idle => "icons/tray/idle.png",
        AppState::Recording => "icons/tray/recording.png",
        AppState::Transcribing | AppState::Postprocessing => "icons/tray/processing.png",
        AppState::Injecting => "icons/tray/done.png",
        AppState::Error(_) => "icons/tray/error.png",
    }
}

/// Platzhalter — die wirkliche Tray-Konstruktion erfolgt in Phase 1.4 via
/// `tauri::tray::TrayIconBuilder` an einem `AppHandle`.
pub fn build_tray_placeholder() -> Result<()> {
    Ok(())
}
