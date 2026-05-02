// SPDX-License-Identifier: GPL-3.0-or-later
//! macOS Text-Injection via CGEvent. Vollstaendig in Phase 6.

use crate::core::error::{Result, VoiceTypeError};
use crate::injection::{InjectOptions, InjectorCapabilities, TextInjector};
use async_trait::async_trait;

pub struct MacOsInjector;

impl MacOsInjector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MacOsInjector {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TextInjector for MacOsInjector {
    fn name(&self) -> &str {
        "macos-cgevent"
    }

    fn capabilities(&self) -> InjectorCapabilities {
        InjectorCapabilities::default()
    }

    async fn inject(&self, _text: &str, _opts: InjectOptions) -> Result<()> {
        Err(VoiceTypeError::Injection(
            "macOS CGEvent — Phase 6 (macOS-Port)".into(),
        ))
    }
}
