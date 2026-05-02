// SPDX-License-Identifier: GPL-3.0-or-later
//! Windows Text-Injection via SendInput (winapi).

use crate::core::error::{Result, VoiceTypeError};
use crate::injection::{InjectOptions, InjectorCapabilities, TextInjector};
use async_trait::async_trait;

pub struct WindowsInjector;

impl WindowsInjector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsInjector {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TextInjector for WindowsInjector {
    fn name(&self) -> &str {
        "windows-sendinput"
    }

    fn capabilities(&self) -> InjectorCapabilities {
        InjectorCapabilities {
            supports_clipboard: true,
            supports_keystrokes: true,
        }
    }

    async fn inject(&self, _text: &str, _opts: InjectOptions) -> Result<()> {
        Err(VoiceTypeError::Injection(
            "Windows SendInput noch nicht implementiert (Phase 1.4)".into(),
        ))
    }
}
