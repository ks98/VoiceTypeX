// SPDX-License-Identifier: GPL-3.0-or-later
//! Text-Injection an die aktive Cursor-Position.
//!
//! Default-Strategie ist Clipboard-Fallback (siehe CLAUDE.md §4.5).
//! Direkte Keystroke-Injection (SendInput / XTest / libei) ist Opt-in pro
//! Modus. Plattform-Selektion zur Laufzeit via `detect_injector`.

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

/// Plattform-Selektion zur Laufzeit. Auf Linux entscheidet die Anwesenheit
/// von `WAYLAND_DISPLAY` zwischen Wayland- und X11-Pfad.
pub fn detect_injector() -> Result<Box<dyn TextInjector>> {
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(windows::WindowsInjector::new()))
    }
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacOsInjector::new()))
    }
    #[cfg(target_os = "linux")]
    {
        if std::env::var("WAYLAND_DISPLAY").is_ok() {
            Ok(Box::new(linux_wayland::WaylandInjector::new()))
        } else {
            Ok(Box::new(linux_x11::X11Injector::new()))
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Err(crate::core::error::VoiceTypeError::Injection(
            "Unsupported platform".into(),
        ))
    }
}
