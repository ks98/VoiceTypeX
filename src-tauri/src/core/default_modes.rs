// SPDX-License-Identifier: GPL-3.0-or-later
//! Default-Modi werden via `include_str!` ins Binary eingebettet und beim
//! ersten App-Start nach `app_config_dir/modes/` kopiert. Damit kann der
//! User die Modi editieren, ohne System-Schreibrechte zu brauchen, und
//! der Hot-Reload-Watcher beobachtet das User-Verzeichnis.

use crate::core::error::{Result, VoiceTypeError};
use std::path::Path;

const DEFAULTS: &[(&str, &str)] = &[
    (
        "exaktes_diktat.toml",
        include_str!("../../../modes/exaktes_diktat.toml"),
    ),
    (
        "korrigierendes_diktat.toml",
        include_str!("../../../modes/korrigierendes_diktat.toml"),
    ),
    (
        "foermliche_email.toml",
        include_str!("../../../modes/foermliche_email.toml"),
    ),
    (
        "slack_teams.toml",
        include_str!("../../../modes/slack_teams.toml"),
    ),
    (
        "github_issue.toml",
        include_str!("../../../modes/github_issue.toml"),
    ),
    (
        "claude_code_anweisung.toml",
        include_str!("../../../modes/claude_code_anweisung.toml"),
    ),
];

/// Stelle sicher, dass `dir` mindestens die Default-Modi enthaelt. Das
/// Verzeichnis wird angelegt, falls es nicht existiert. Vorhandene Dateien
/// werden NICHT ueberschrieben (User-Anpassungen bleiben erhalten).
pub fn bootstrap_defaults_if_empty(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)
        .map_err(|e| VoiceTypeError::Mode(format!("create_dir_all({}): {e}", dir.display())))?;

    let has_any_toml = std::fs::read_dir(dir)
        .map_err(|e| VoiceTypeError::Mode(format!("read_dir({}): {e}", dir.display())))?
        .flatten()
        .any(|entry| {
            entry
                .path()
                .extension()
                .and_then(|s| s.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("toml"))
                .unwrap_or(false)
        });

    if has_any_toml {
        return Ok(());
    }

    for (name, content) in DEFAULTS {
        let path = dir.join(name);
        std::fs::write(&path, content)
            .map_err(|e| VoiceTypeError::Mode(format!("write({}): {e}", path.display())))?;
        tracing::info!(file = %path.display(), "Default-Modus angelegt");
    }
    Ok(())
}
