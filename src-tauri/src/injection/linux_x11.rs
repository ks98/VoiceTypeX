// SPDX-License-Identifier: GPL-3.0-or-later
//! Linux X11 Text-Injection via XTest (`libxtst`).

use crate::core::error::{Result, VoiceTypeError};
use crate::injection::{InjectOptions, InjectorCapabilities, TextInjector};
use async_trait::async_trait;

pub struct X11Injector;

impl X11Injector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for X11Injector {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TextInjector for X11Injector {
    fn name(&self) -> &str {
        "linux-x11-xtest"
    }

    fn capabilities(&self) -> InjectorCapabilities {
        InjectorCapabilities {
            supports_clipboard: true,
            supports_keystrokes: true,
        }
    }

    async fn inject(&self, _text: &str, _opts: InjectOptions) -> Result<()> {
        Err(VoiceTypeError::Injection(
            "Linux X11 XTest noch nicht implementiert (Phase 1.4)".into(),
        ))
    }
}
