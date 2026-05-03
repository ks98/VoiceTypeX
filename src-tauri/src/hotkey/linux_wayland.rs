// SPDX-License-Identifier: GPL-3.0-or-later
//! Linux Wayland: globale Hotkeys via `xdg-desktop-portal.GlobalShortcuts`.
//!
//! Anders als X11/Windows kann die App auf Wayland NICHT die Tasten selbst
//! greifen. Sie meldet eine Liste **Aktionen** beim Portal an
//! (id + Beschreibung + Wunsch-Trigger); der Compositor zeigt dem User einen
//! Dialog, in dem er die finale Tastenkombination zuweist. Daher heisst das
//! TOML-Feld auch `hotkey` weiterhin, aber auf Wayland ist es nur ein
//! **Vorschlag**.

use crate::core::error::{Result, VoiceTypeError};
use crate::hotkey::{HotkeyEvent, HotkeyManager};
use async_trait::async_trait;
use tokio::sync::broadcast;

/// Beschreibung einer App-Action, die als Wayland-Shortcut registriert wird.
#[derive(Debug, Clone)]
pub struct WaylandShortcutSpec {
    pub id: String,
    pub description: String,
    pub preferred_trigger: String,
}

/// Stub fuer das HotkeyManager-Trait. Auf Wayland nutzen wir den
/// dedizierten Session-Pfad aus `pipeline::wayland_hotkey`, daher gibt
/// dieses Trait-Impl nur Receiver-Stubs zurueck.
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
            "Wayland: nutze pipeline::wayland_hotkey::start_session statt register".into(),
        ))
    }

    async fn unregister(&self, _id: &str) -> Result<()> {
        Err(VoiceTypeError::Hotkey(
            "Wayland: nutze pipeline::wayland_hotkey::stop_session".into(),
        ))
    }

    fn events(&self) -> broadcast::Receiver<HotkeyEvent> {
        self.sender.subscribe()
    }
}

/// Verbinde mit dem GlobalShortcuts-Portal, registriere die uebergebenen
/// Actions, und starte einen Listener, der jede Activation als
/// `HotkeyEvent` ueber den Sender weitergibt. Diese Funktion gibt
/// nicht zurueck — sie ist als langlebige Task gedacht (in
/// `tokio::spawn`).
pub async fn run_global_shortcuts_session(
    shortcuts: Vec<WaylandShortcutSpec>,
    sender: broadcast::Sender<HotkeyEvent>,
) -> Result<()> {
    use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
    use futures_util::StreamExt;

    let proxy = GlobalShortcuts::new()
        .await
        .map_err(|e| VoiceTypeError::Hotkey(format!("GlobalShortcuts::new: {e}")))?;

    let session = proxy
        .create_session()
        .await
        .map_err(|e| VoiceTypeError::Hotkey(format!("create_session: {e}")))?;

    let new_shortcuts: Vec<NewShortcut> = shortcuts
        .iter()
        .map(|s| {
            NewShortcut::new(&s.id, &s.description)
                .preferred_trigger(Some(s.preferred_trigger.as_str()))
        })
        .collect();

    proxy
        .bind_shortcuts(&session, &new_shortcuts, None)
        .await
        .map_err(|e| VoiceTypeError::Hotkey(format!("bind_shortcuts: {e}")))?;

    tracing::info!(count = shortcuts.len(), "Wayland-Hotkeys registriert");

    let mut activations = proxy
        .receive_activated()
        .await
        .map_err(|e| VoiceTypeError::Hotkey(format!("receive_activated: {e}")))?;

    while let Some(event) = activations.next().await {
        let shortcut_id = event.shortcut_id().to_string();
        tracing::info!(shortcut_id = %shortcut_id, "Wayland-Hotkey ausgeloest");
        let _ = sender.send(HotkeyEvent { id: shortcut_id });
    }

    tracing::warn!("Wayland-Hotkey-Activated-Stream beendet");
    Ok(())
}
