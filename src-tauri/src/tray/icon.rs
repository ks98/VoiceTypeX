// SPDX-License-Identifier: GPL-3.0-or-later
//! Loader helpers for embedded tray-icon bytes. Phase 1.4 uses
//! `tauri::image::Image::from_bytes` with `include_bytes!`.

#[derive(Debug, Clone, Copy)]
pub enum TrayIconSlot {
    Idle,
    Recording,
    Processing,
    Done,
    Error,
}

impl TrayIconSlot {
    pub fn relative_path(self) -> &'static str {
        match self {
            Self::Idle => "icons/tray/idle.png",
            Self::Recording => "icons/tray/recording.png",
            Self::Processing => "icons/tray/processing.png",
            Self::Done => "icons/tray/done.png",
            Self::Error => "icons/tray/error.png",
        }
    }
}
