// SPDX-License-Identifier: GPL-3.0-or-later
//! Tray icon and tray menu.
//!
//! The tray exposes a context menu with "Open settings" and "Quit". The
//! icon is updated live by a StateBus subscriber (see
//! `pipeline::spawn_tray_state_listener`). Menu labels are localized
//! against the persisted `Settings.locale`; an in-flight locale change
//! does NOT re-render the menu — the user has to restart the app for
//! tray-menu labels to update. Trade-off: Tauri 2 has no public API to
//! swap menu items at runtime without tearing down the whole tray.

pub mod icon;

use crate::core::error::{Result, VoiceTypeError};
use crate::core::state::AppState;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

const ICON_IDLE: &[u8] = include_bytes!("../../icons/tray/idle.png");
const ICON_RECORDING: &[u8] = include_bytes!("../../icons/tray/recording.png");
const ICON_RECORDING_PULSE: &[u8] = include_bytes!("../../icons/tray/recording_pulse.png");
const ICON_PROCESSING: &[u8] = include_bytes!("../../icons/tray/processing.png");
const ICON_DONE: &[u8] = include_bytes!("../../icons/tray/done.png");
const ICON_ERROR: &[u8] = include_bytes!("../../icons/tray/error.png");

/// Pulse variant of the recording icon (lighter red) for the recording
/// animation (CLAUDE.md DoD §6.1: "tray icon pulses red").
pub fn icon_bytes_recording_pulse() -> &'static [u8] {
    ICON_RECORDING_PULSE
}

pub fn icon_bytes_for_state(state: &AppState) -> &'static [u8] {
    match state {
        AppState::Idle => ICON_IDLE,
        AppState::Recording => ICON_RECORDING,
        AppState::Transcribing | AppState::Postprocessing => ICON_PROCESSING,
        AppState::Injecting => ICON_DONE,
        AppState::Error(_) => ICON_ERROR,
    }
}

struct TrayLabels {
    open_settings: &'static str,
    quit: &'static str,
}

/// Map BCP-47 locale prefix to tray labels. Backend-side i18n: tiny
/// inline lookup to avoid pulling a full Rust-side i18n stack for two
/// strings. Mirrors the supported set in `src/i18n/detect.ts`.
fn labels_for_locale(raw_locale: Option<&str>) -> TrayLabels {
    let prefix = raw_locale
        .and_then(|s| s.split(['-', '_']).next())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    match prefix.as_str() {
        "de" => TrayLabels {
            open_settings: "Einstellungen öffnen",
            quit: "Beenden",
        },
        "fr" => TrayLabels {
            open_settings: "Ouvrir les paramètres",
            quit: "Quitter",
        },
        "es" => TrayLabels {
            open_settings: "Abrir ajustes",
            quit: "Salir",
        },
        "it" => TrayLabels {
            open_settings: "Apri impostazioni",
            quit: "Esci",
        },
        // "en" and unknown locales fall back to English.
        _ => TrayLabels {
            open_settings: "Open settings",
            quit: "Quit",
        },
    }
}

/// Build tray with icon and context menu. Called from `lib.rs::run`
/// inside the `setup` hook. `locale` is the raw value from
/// `Settings.locale` (may be `None` if first-run detection failed).
pub fn setup_tray(app: &AppHandle, locale: Option<&str>) -> Result<()> {
    let labels = labels_for_locale(locale);

    let item_open = MenuItem::with_id(
        app,
        "open_settings",
        labels.open_settings,
        true,
        None::<&str>,
    )
    .map_err(|e| VoiceTypeError::Hotkey(format!("MenuItem 'open_settings': {e}")))?;
    let item_quit = MenuItem::with_id(app, "quit", labels.quit, true, None::<&str>)
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
                    reveal_main_window(&window);
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        // Left-click on the tray icon shows the main window — standard
        // tray-app expectation. The main window starts hidden (see
        // tauri.conf.json `visible: false`), so that Plasma's global-
        // hotkey trigger doesn't steal focus from the target app.
        .on_tray_icon_event(|tray, event| {
            use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    reveal_main_window(&window);
                }
            }
        })
        .build(app)
        .map_err(|e| VoiceTypeError::Hotkey(format!("TrayIconBuilder::build: {e}")))?;

    Ok(())
}

/// Reveal the settings (main) window from the tray.
///
/// On Linux/Wayland the WM close (X) button is dead until the window's
/// first `configure` event (tao 0.35.3, tauri#13440 — open upstream).
/// Because the window starts `visible:false` (focus-steal guard) and is
/// re-mapped fresh on every tray reveal, the X is dead again each time.
/// We replicate the manual maximize→restore the user would otherwise do:
/// once the fresh map has settled, briefly `maximize()` + `unmaximize()`,
/// which fires the `configure` that binds the close affordance. Deferred on
/// a short-lived thread so the initial map completes first. Window ops are
/// dispatched to the main loop, so calling them off-thread is fine. Drop
/// this once tauri#13440 / tao ship a fix.
fn reveal_main_window(window: &tauri::WebviewWindow) {
    let _ = window.show();
    // center() after show(): the config `center:true` flag is unreliable for a
    // window created `visible:false` — the runtime computes the centered
    // coordinate from the still-unmapped window's (default) position/size, so
    // it lands top-left on Windows. After show() the window is mapped with a
    // real size + monitor, so centering resolves correctly. (#5)
    let _ = window.center();
    let _ = window.set_focus();

    #[cfg(target_os = "linux")]
    {
        let w = window.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(120));
            let _ = w.maximize();
            std::thread::sleep(std::time::Duration::from_millis(80));
            let _ = w.unmaximize();
        });
    }
}
