// SPDX-License-Identifier: GPL-3.0-or-later
//! Windows: globaler Hotkey via `tauri-plugin-global-shortcut`.

use crate::core::error::{Result, VoiceTypeError};
use crate::hotkey::{HotkeyEvent, HotkeyManager};
use async_trait::async_trait;
use tokio::sync::broadcast;

pub struct WindowsHotkeyManager {
    sender: broadcast::Sender<HotkeyEvent>,
}

impl WindowsHotkeyManager {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(16);
        Self { sender }
    }
}

impl Default for WindowsHotkeyManager {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl HotkeyManager for WindowsHotkeyManager {
    async fn register(&self, _id: &str, _accelerator: &str) -> Result<()> {
        Err(VoiceTypeError::Hotkey(
            "Windows Hotkey-Registrierung — Phase 1.4".into(),
        ))
    }

    async fn unregister(&self, _id: &str) -> Result<()> {
        Err(VoiceTypeError::Hotkey(
            "Windows Hotkey-Deregistrierung — Phase 1.4".into(),
        ))
    }

    fn events(&self) -> broadcast::Receiver<HotkeyEvent> {
        self.sender.subscribe()
    }
}
