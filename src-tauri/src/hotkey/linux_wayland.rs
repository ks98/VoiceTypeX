// SPDX-License-Identifier: GPL-3.0-or-later
//! Linux Wayland: globaler Hotkey via `xdg-desktop-portal.GlobalShortcuts`.
//!
//! Anders als X11/Windows kann die App auf Wayland NICHT die Tasten
//! selbst greifen. Sie meldet eine Aktion beim Portal an
//! (id + Beschreibung + Wunsch-Trigger); der Compositor zeigt dem User
//! einen Dialog, in dem er die finale Tastenkombination zuweist. Der
//! `Settings.menu_hotkey`-Wert ist auf Wayland also nur ein **Vorschlag**.

use crate::core::error::{Result, VoiceTypeError};
use crate::hotkey::{HotkeyEvent, HotkeyEventKind};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Beschreibung einer App-Action, die als Wayland-Shortcut registriert wird.
#[derive(Debug, Clone)]
pub struct WaylandShortcutSpec {
    pub id: String,
    pub description: String,
    pub preferred_trigger: String,
}

/// Verbinde mit dem GlobalShortcuts-Portal, registriere die uebergebenen
/// Actions, und starte einen Listener, der jede Activation als
/// `HotkeyEvent` ueber den Sender weitergibt. Diese Funktion gibt
/// nicht zurueck — sie ist als langlebige Task gedacht (in
/// `tokio::spawn`).
///
/// `effective_trigger_cache`: optionaler Cache, in den der nach
/// `bind_shortcuts` vom Compositor zurueckgegebene `trigger_description`
/// des ersten Shortcuts geschrieben wird. KDE/GNOME duerfen vom
/// `preferred_trigger` abweichen (User kann den Hotkey in den System-
/// Settings umstellen) — das Frontend liest diesen Cache, um den
/// tatsaechlichen Trigger anzuzeigen.
pub async fn run_global_shortcuts_session(
    shortcuts: Vec<WaylandShortcutSpec>,
    sender: broadcast::Sender<HotkeyEvent>,
    effective_trigger_cache: Option<Arc<RwLock<Option<String>>>>,
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

    // Nach bind_shortcuts list_shortcuts aufrufen, um den tatsaechlich
    // gebundenen Trigger zu lernen. Das ist eigenes Portal-Verhalten:
    // beim allerersten bind_shortcuts wird der preferred_trigger
    // uebernommen, danach merkt sich KDE die User-Zuweisung und meldet
    // sie ueber list_shortcuts zurueck. Wir scheitern hier nicht hart —
    // wenn list_shortcuts fehlschlaegt oder leer ist, bleibt der Cache
    // einfach None und die UI faellt auf den Settings-Wert zurueck.
    if let Some(cache) = effective_trigger_cache.as_ref() {
        match proxy.list_shortcuts(&session).await {
            Ok(req) => match req.response() {
                Ok(list) => {
                    if let Some(first) = list.shortcuts().first() {
                        let trigger = first.trigger_description().to_string();
                        tracing::info!(
                            id = %first.id(),
                            trigger = %trigger,
                            "Wayland-Hotkey effektiver Trigger gelesen"
                        );
                        *cache.write() = Some(trigger);
                    } else {
                        tracing::warn!("list_shortcuts: leere Liste — Cache bleibt None");
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "list_shortcuts.response() fehlgeschlagen");
                }
            },
            Err(e) => {
                tracing::warn!(error = %e, "list_shortcuts(&session) fehlgeschlagen");
            }
        }
    }

    // Beide Streams parallel: Activated (Press) + Deactivated (Release).
    // Activated kommt sofort beim Hotkey-Druck; Deactivated ist
    // Compositor-abhaengig — KDE Plasma 5.27+ und GNOME 45+ liefern
    // zuverlaessig, manche wlroots-Compositors weniger. Falls Deactivated
    // ausbleibt, faellt der User-Pfad zurueck auf das Toggle-Verhalten,
    // konfigurierbar via Settings.ptt_mode.
    let mut activations = proxy
        .receive_activated()
        .await
        .map_err(|e| VoiceTypeError::Hotkey(format!("receive_activated: {e}")))?;
    let mut deactivations = proxy
        .receive_deactivated()
        .await
        .map_err(|e| VoiceTypeError::Hotkey(format!("receive_deactivated: {e}")))?;

    // Auto-Repeat-Dedup: KDE Plasma (und einige andere Compositors) liefern
    // ueber `Activated` kontinuierliche Tastatur-Auto-Repeats waehrend die
    // Taste gedrueckt bleibt — ~25/Sekunde. Ohne Dedup floodet das Log und
    // verschleiert echte Pressed/Released-Zyklen. Wir tracken pro
    // shortcut_id den aktuellen Zustand und reichen Pressed nur beim
    // Uebergang Released->Pressed durch.
    let mut pressed_state: HashMap<String, bool> = HashMap::new();

    loop {
        tokio::select! {
            event = activations.next() => match event {
                Some(ev) => {
                    let shortcut_id = ev.shortcut_id().to_string();
                    let was_pressed = pressed_state.get(&shortcut_id).copied().unwrap_or(false);
                    if was_pressed {
                        // Auto-Repeat — silent ignorieren.
                        continue;
                    }
                    pressed_state.insert(shortcut_id.clone(), true);
                    tracing::info!(shortcut_id = %shortcut_id, "Wayland-Hotkey Pressed");
                    let _ = sender.send(HotkeyEvent {
                        id: shortcut_id,
                        kind: HotkeyEventKind::Pressed,
                    });
                }
                None => {
                    tracing::warn!("Wayland-Hotkey-Activated-Stream beendet");
                    break;
                }
            },
            event = deactivations.next() => match event {
                Some(ev) => {
                    let shortcut_id = ev.shortcut_id().to_string();
                    pressed_state.insert(shortcut_id.clone(), false);
                    tracing::info!(shortcut_id = %shortcut_id, "Wayland-Hotkey Released");
                    let _ = sender.send(HotkeyEvent {
                        id: shortcut_id,
                        kind: HotkeyEventKind::Released,
                    });
                }
                None => {
                    tracing::warn!("Wayland-Hotkey-Deactivated-Stream beendet");
                    break;
                }
            },
        }
    }

    Ok(())
}
