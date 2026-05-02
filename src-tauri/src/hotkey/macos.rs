// SPDX-License-Identifier: GPL-3.0-or-later
//! macOS-Hotkey-Registrierung. Vollstaendig in Phase 6.

use crate::core::error::{Result, VoiceTypeError};
use crate::hotkey::{HotkeyEvent, HotkeyManager};
use async_trait::async_trait;
use tokio::sync::broadcast;

pub struct MacOsHotkeyManager {
    sender: broadcast::Sender<HotkeyEvent>,
}

impl MacOsHotkeyManager {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(16);
        Self { sender }
    }
}

impl Default for MacOsHotkeyManager {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl HotkeyManager for MacOsHotkeyManager {
    async fn register(&self, _id: &str, _accelerator: &str) -> Result<()> {
        Err(VoiceTypeError::Hotkey(
            "macOS — Phase 6 (macOS-Port)".into(),
        ))
    }

    async fn unregister(&self, _id: &str) -> Result<()> {
        Err(VoiceTypeError::Hotkey(
            "macOS — Phase 6 (macOS-Port)".into(),
        ))
    }

    fn events(&self) -> broadcast::Receiver<HotkeyEvent> {
        self.sender.subscribe()
    }
}
