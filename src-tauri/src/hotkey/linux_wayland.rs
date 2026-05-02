// SPDX-License-Identifier: GPL-3.0-or-later
//! Linux Wayland: `xdg-desktop-portal.GlobalShortcuts`. Phase 5.

use crate::core::error::{Result, VoiceTypeError};
use crate::hotkey::{HotkeyEvent, HotkeyManager};
use async_trait::async_trait;
use tokio::sync::broadcast;

pub struct WaylandHotkeyManager {
    sender: broadcast::Sender<HotkeyEvent>,
}

impl WaylandHotkeyManager {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(16);
        Self { sender }
    }
}

impl Default for WaylandHotkeyManager {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl HotkeyManager for WaylandHotkeyManager {
    async fn register(&self, _id: &str, _accelerator: &str) -> Result<()> {
        Err(VoiceTypeError::Hotkey(
            "Wayland-Support kommt in Phase 5".into(),
        ))
    }

    async fn unregister(&self, _id: &str) -> Result<()> {
        Err(VoiceTypeError::Hotkey(
            "Wayland-Support kommt in Phase 5".into(),
        ))
    }

    fn events(&self) -> broadcast::Receiver<HotkeyEvent> {
        self.sender.subscribe()
    }
}
