// SPDX-License-Identifier: GPL-3.0-or-later
//! Tray-Icon und -Menu.
//!
//! Tray hat Kontextmenue mit "Einstellungen oeffnen" und "Beenden". Das Icon
//! wird per StateBus-Subscriber live aktualisiert (siehe
//! `pipeline::spawn_tray_state_listener`).

pub mod icon;

use crate::core::error::{Result, VoiceTypeError};
use crate::core::state::AppState;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

const ICON_IDLE: &[u8] = include_bytes!("../../icons/tray/idle.png");
const ICON_RECORDING: &[u8] = include_bytes!("../../icons/tray/recording.png");
const ICON_PROCESSING: &[u8] = include_bytes!("../../icons/tray/processing.png");
const ICON_DONE: &[u8] = include_bytes!("../../icons/tray/done.png");
const ICON_ERROR: &[u8] = include_bytes!("../../icons/tray/error.png");

pub fn icon_bytes_for_state(state: &AppState) -> &'static [u8] {
    match state {
        AppState::Idle => ICON_IDLE,
        AppState::Recording => ICON_RECORDING,
        AppState::Transcribing | AppState::Postprocessing => ICON_PROCESSING,
        AppState::Injecting => ICON_DONE,
        AppState::Error(_) => ICON_ERROR,
    }
}

/// Baue Tray mit Icon und Kontextmenue. Wird in `lib.rs::run` waehrend
/// `setup` aufgerufen.
pub fn setup_tray(app: &AppHandle) -> Result<()> {
    let item_open = MenuItem::with_id(
        app,
        "open_settings",
        "Einstellungen oeffnen",
        true,
        None::<&str>,
    )
    .map_err(|e| VoiceTypeError::Hotkey(format!("MenuItem 'open_settings': {e}")))?;
    let item_quit = MenuItem::with_id(app, "quit", "Beenden", true, None::<&str>)
        .map_err(|e| VoiceTypeError::Hotkey(format!("MenuItem 'quit': {e}")))?;

    let menu = Menu::with_items(app, &[&item_open, &item_quit])
        .map_err(|e| VoiceTypeError::Hotkey(format!("Menu::with_items: {e}")))?;

    let icon = tauri::image::Image::from_bytes(ICON_IDLE)
        .map_err(|e| VoiceTypeError::Hotkey(format!("Image::from_bytes: {e}")))?;

    TrayIconBuilder::with_id("main")
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("VoiceTypeX")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open_settings" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .build(app)
        .map_err(|e| VoiceTypeError::Hotkey(format!("TrayIconBuilder::build: {e}")))?;

    Ok(())
}
