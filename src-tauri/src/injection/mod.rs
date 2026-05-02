// SPDX-License-Identifier: GPL-3.0-or-later
//! Text-Injection an die aktive Cursor-Position.
//!
//! Default-Strategie ist Clipboard-Fallback (siehe CLAUDE.md §4.5):
//!   1. Clipboard-Inhalt sichern
//!   2. Neuen Text auf Clipboard setzen
//!   3. Plattform-Paste-Shortcut senden (Cmd/Ctrl + V)
//!   4. Nach 200 ms vorherigen Inhalt wiederherstellen
//!
//! Direkte Keystroke-Injection ist Opt-in pro Modus
//! (`injection_method = "keystrokes"`). In Phase 1 ignorieren wir die Wahl
//! und nutzen immer Clipboard; ein Hinweis wird geloggt.

use crate::core::error::Result;
use async_trait::async_trait;

pub mod clipboard_fallback;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "linux")]
pub mod linux_x11;

#[cfg(target_os = "linux")]
pub mod linux_wayland;

#[derive(Debug, Clone, Copy, Default)]
pub struct InjectorCapabilities {
    pub supports_clipboard: bool,
    pub supports_keystrokes: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectionStrategy {
    Clipboard,
    Keystrokes,
}

#[derive(Debug, Clone)]
pub struct InjectOptions {
    pub strategy: InjectionStrategy,
}

#[async_trait]
pub trait TextInjector: Send + Sync {
    fn name(&self) -> &str;
    fn capabilities(&self) -> InjectorCapabilities;
    async fn inject(&self, text: &str, opts: InjectOptions) -> Result<()>;
}

/// Liefert den Default-Injector. Phase 1: immer Clipboard-Fallback.
/// `app_handle` wird fuer den Zugriff auf `tauri-plugin-clipboard-manager` benoetigt.
pub fn make_default_injector(app_handle: tauri::AppHandle) -> Box<dyn TextInjector> {
    Box::new(clipboard_fallback::ClipboardFallbackInjector::new(
        app_handle,
    ))
}
