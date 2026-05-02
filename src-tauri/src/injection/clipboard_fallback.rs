// SPDX-License-Identifier: GPL-3.0-or-later
//! Clipboard-Fallback: aktuellen Inhalt sichern → neuen Text setzen → Paste-
//! Shortcut senden → ~150 ms warten → vorherigen Inhalt wiederherstellen.
//!
//! Phase 1.2: Trait-konforme API. Phase 1.4 implementiert die wirkliche
//! Sequenz mit `tauri-plugin-clipboard-manager` und Plattform-Paste-Shortcuts.

use crate::core::error::{Result, VoiceTypeError};
use crate::injection::{InjectOptions, InjectorCapabilities, TextInjector};
use async_trait::async_trait;

pub struct ClipboardFallbackInjector;

impl ClipboardFallbackInjector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClipboardFallbackInjector {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TextInjector for ClipboardFallbackInjector {
    fn name(&self) -> &str {
        "clipboard-fallback"
    }

    fn capabilities(&self) -> InjectorCapabilities {
        InjectorCapabilities {
            supports_clipboard: true,
            supports_keystrokes: false,
        }
    }

    async fn inject(&self, _text: &str, _opts: InjectOptions) -> Result<()> {
        Err(VoiceTypeError::Injection(
            "Clipboard-Fallback noch nicht implementiert (Phase 1.4)".into(),
        ))
    }
}
