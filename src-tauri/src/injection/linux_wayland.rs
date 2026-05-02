// SPDX-License-Identifier: GPL-3.0-or-later
//! Linux Wayland Text-Injection via libei (RemoteDesktop-Portal).
//! Vollstaendig in Phase 5.

use crate::core::error::{Result, VoiceTypeError};
use crate::injection::{InjectOptions, InjectorCapabilities, TextInjector};
use async_trait::async_trait;

pub struct WaylandInjector;

impl WaylandInjector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WaylandInjector {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TextInjector for WaylandInjector {
    fn name(&self) -> &str {
        "linux-wayland-libei"
    }

    fn capabilities(&self) -> InjectorCapabilities {
        InjectorCapabilities::default()
    }

    async fn inject(&self, _text: &str, _opts: InjectOptions) -> Result<()> {
        Err(VoiceTypeError::Injection(
            "Wayland-Support kommt in Phase 5".into(),
        ))
    }
}
