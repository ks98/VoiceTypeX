// SPDX-License-Identifier: GPL-3.0-or-later
//! Globale Hotkey-Registrierung pro Plattform.

use crate::core::error::Result;
use async_trait::async_trait;
use tokio::sync::broadcast;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "linux")]
pub mod linux_x11;

#[cfg(target_os = "linux")]
pub mod linux_wayland;

#[derive(Debug, Clone)]
pub struct HotkeyEvent {
    pub id: String,
}

#[async_trait]
pub trait HotkeyManager: Send + Sync {
    async fn register(&self, id: &str, accelerator: &str) -> Result<()>;
    async fn unregister(&self, id: &str) -> Result<()>;
    fn events(&self) -> broadcast::Receiver<HotkeyEvent>;
}

pub fn detect_hotkey_manager() -> Result<Box<dyn HotkeyManager>> {
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(windows::WindowsHotkeyManager::new()))
    }
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacOsHotkeyManager::new()))
    }
    #[cfg(target_os = "linux")]
    {
        if std::env::var("WAYLAND_DISPLAY").is_ok() {
            Ok(Box::new(linux_wayland::WaylandHotkeyManager::new()))
        } else {
            Ok(Box::new(linux_x11::X11HotkeyManager::new()))
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Err(crate::core::error::VoiceTypeError::Hotkey(
            "Unsupported platform".into(),
        ))
    }
}
