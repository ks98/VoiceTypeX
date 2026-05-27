// SPDX-License-Identifier: GPL-3.0-or-later
//! Linux Wayland: global hotkey via
//! `xdg-desktop-portal.GlobalShortcuts`.
//!
//! Unlike X11/Windows, the app CANNOT grab the keys itself on Wayland.
//! It registers an action with the portal (id + description +
//! preferred trigger); the compositor shows the user a dialog where
//! they assign the final key combination. The `Settings.menu_hotkey`
//! value is therefore only a **suggestion** on Wayland.

use crate::core::error::{Result, VoiceTypeError};
use crate::hotkey::{HotkeyEvent, HotkeyEventKind};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::broadcast;

/// Description of an app action registered as a Wayland shortcut.
#[derive(Debug, Clone)]
pub struct WaylandShortcutSpec {
    pub id: String,
    pub description: String,
    pub preferred_trigger: String,
}

/// Connects to the GlobalShortcuts portal, registers the passed
/// actions, and starts a listener that forwards every activation as a
/// `HotkeyEvent` via the sender. This function does not return — it
/// is meant as a long-lived task (in `tokio::spawn`).
///
/// `effective_trigger_cache`: optional cache that receives the
/// `trigger_description` of the first shortcut returned by the
/// compositor after `bind_shortcuts`. KDE/GNOME may deviate from the
/// `preferred_trigger` (the user can change the hotkey in system
/// settings) — the frontend reads this cache to show the actual
/// trigger.
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

    // Spec-recommended pattern: list first, bind only what isn't bound
    // yet. `bind_shortcuts` is the call that triggers the compositor's
    // assignment dialog, and "an application can only attempt to bind
    // shortcuts of a session once". On a fresh first run the list is
    // empty → we bind (one-time dialog). On later starts the portal
    // returns the previously-bound shortcuts → we skip the bind and KDE
    // shows no dialog. Without this gate, the unconditional bind
    // re-prompted on every start.
    let already_bound: HashSet<String> = match proxy.list_shortcuts(&session).await {
        Ok(req) => match req.response() {
            Ok(list) => list
                .shortcuts()
                .iter()
                .map(|s| s.id().to_string())
                .collect(),
            Err(e) => {
                tracing::warn!(error = %e, "list_shortcuts.response() before bind failed — assuming none bound");
                HashSet::new()
            }
        },
        Err(e) => {
            tracing::warn!(error = %e, "list_shortcuts before bind failed — assuming none bound");
            HashSet::new()
        }
    };

    let new_shortcuts: Vec<NewShortcut> = shortcuts
        .iter()
        .filter(|s| !already_bound.contains(&s.id))
        .map(|s| {
            NewShortcut::new(&s.id, &s.description)
                .preferred_trigger(Some(s.preferred_trigger.as_str()))
        })
        .collect();

    if new_shortcuts.is_empty() {
        tracing::info!(
            count = shortcuts.len(),
            "Wayland hotkeys already bound — skipping bind_shortcuts (no portal dialog)"
        );
    } else {
        proxy
            .bind_shortcuts(&session, &new_shortcuts, None)
            .await
            .map_err(|e| VoiceTypeError::Hotkey(format!("bind_shortcuts: {e}")))?;
        tracing::info!(count = new_shortcuts.len(), "Wayland hotkeys registered");
    }

    // Call `list_shortcuts` after `bind_shortcuts` to learn the
    // actually bound trigger. This is portal behavior: on the very
    // first `bind_shortcuts` the preferred trigger is taken,
    // afterwards KDE remembers the user assignment and reports it via
    // `list_shortcuts`. We don't fail hard here — if `list_shortcuts`
    // fails or returns empty, the cache just stays None and the UI
    // falls back to the settings value.
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
                    tracing::warn!(error = %e, "list_shortcuts.response() failed");
                }
            },
            Err(e) => {
                tracing::warn!(error = %e, "list_shortcuts(&session) failed");
            }
        }
    }

    // Both streams in parallel: Activated (press) + Deactivated
    // (release). Activated arrives immediately on hotkey press;
    // Deactivated is compositor-dependent — KDE Plasma 5.27+ and
    // GNOME 45+ deliver reliably, some wlroots compositors less so. If
    // Deactivated doesn't arrive, the user path falls back to toggle
    // behavior, configurable via `Settings.ptt_mode`.
    let mut activations = proxy
        .receive_activated()
        .await
        .map_err(|e| VoiceTypeError::Hotkey(format!("receive_activated: {e}")))?;
    let mut deactivations = proxy
        .receive_deactivated()
        .await
        .map_err(|e| VoiceTypeError::Hotkey(format!("receive_deactivated: {e}")))?;

    // Auto-repeat dedup: KDE Plasma (and some other compositors)
    // deliver continuous keyboard auto-repeats via `Activated` while
    // the key stays pressed — ~25/sec. Without dedup, the log floods
    // and obscures real Pressed/Released cycles. We track the current
    // state per `shortcut_id` and only forward Pressed on the
    // Released->Pressed transition.
    let mut pressed_state: HashMap<String, bool> = HashMap::new();

    loop {
        tokio::select! {
            event = activations.next() => match event {
                Some(ev) => {
                    let shortcut_id = ev.shortcut_id().to_string();
                    let was_pressed = pressed_state.get(&shortcut_id).copied().unwrap_or(false);
                    if was_pressed {
                        // Auto-repeat — silently ignore.
                        continue;
                    }
                    pressed_state.insert(shortcut_id.clone(), true);
                    tracing::info!(shortcut_id = %shortcut_id, "Wayland hotkey pressed");
                    let _ = sender.send(HotkeyEvent {
                        id: shortcut_id,
                        kind: HotkeyEventKind::Pressed,
                    });
                }
                None => {
                    tracing::warn!("Wayland hotkey activated stream ended");
                    break;
                }
            },
            event = deactivations.next() => match event {
                Some(ev) => {
                    let shortcut_id = ev.shortcut_id().to_string();
                    pressed_state.insert(shortcut_id.clone(), false);
                    tracing::info!(shortcut_id = %shortcut_id, "Wayland hotkey released");
                    let _ = sender.send(HotkeyEvent {
                        id: shortcut_id,
                        kind: HotkeyEventKind::Released,
                    });
                }
                None => {
                    tracing::warn!("Wayland hotkey deactivated stream ended");
                    break;
                }
            },
        }
    }

    Ok(())
}
