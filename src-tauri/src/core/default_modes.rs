// SPDX-License-Identifier: GPL-3.0-or-later
//! Default modes embedded via `include_str!` into the binary and copied
//! to `app_config_dir/modes/` on first app start. This way the user can
//! edit modes without needing system write rights, and the hot-reload
//! watcher observes the user directory.
//!
//! Per-locale defaults: each supported UI language ships its own set
//! of 9 mode TOMLs under `modes/defaults/<locale>/` (6 dictation modes
//! plus 3 edit modes — improve/reply/transform). Bootstrap copies
//! the locale-matching set when the user's `modes/` dir is empty.
//! User edits never get overwritten — the function bails as soon as
//! it finds any `.toml`.

use crate::core::error::{Result, VoiceTypeError};
use std::path::Path;

// Locale -> array of (filename, embedded content). Order doesn't
// matter; the bootstrap copies all entries.
type DefaultSet = &'static [(&'static str, &'static str)];

const DEFAULTS_DE: DefaultSet = &[
    (
        "exaktes_diktat.toml",
        include_str!("../../../modes/defaults/de/exaktes_diktat.toml"),
    ),
    (
        "korrigierendes_diktat.toml",
        include_str!("../../../modes/defaults/de/korrigierendes_diktat.toml"),
    ),
    (
        "foermliche_email.toml",
        include_str!("../../../modes/defaults/de/foermliche_email.toml"),
    ),
    (
        "slack_teams.toml",
        include_str!("../../../modes/defaults/de/slack_teams.toml"),
    ),
    (
        "github_issue.toml",
        include_str!("../../../modes/defaults/de/github_issue.toml"),
    ),
    (
        "claude_code_anweisung.toml",
        include_str!("../../../modes/defaults/de/claude_code_anweisung.toml"),
    ),
    (
        "improve.toml",
        include_str!("../../../modes/defaults/de/improve.toml"),
    ),
    (
        "reply.toml",
        include_str!("../../../modes/defaults/de/reply.toml"),
    ),
    (
        "transform.toml",
        include_str!("../../../modes/defaults/de/transform.toml"),
    ),
];

const DEFAULTS_EN: DefaultSet = &[
    (
        "exaktes_diktat.toml",
        include_str!("../../../modes/defaults/en/exaktes_diktat.toml"),
    ),
    (
        "korrigierendes_diktat.toml",
        include_str!("../../../modes/defaults/en/korrigierendes_diktat.toml"),
    ),
    (
        "foermliche_email.toml",
        include_str!("../../../modes/defaults/en/foermliche_email.toml"),
    ),
    (
        "slack_teams.toml",
        include_str!("../../../modes/defaults/en/slack_teams.toml"),
    ),
    (
        "github_issue.toml",
        include_str!("../../../modes/defaults/en/github_issue.toml"),
    ),
    (
        "claude_code_anweisung.toml",
        include_str!("../../../modes/defaults/en/claude_code_anweisung.toml"),
    ),
    (
        "improve.toml",
        include_str!("../../../modes/defaults/en/improve.toml"),
    ),
    (
        "reply.toml",
        include_str!("../../../modes/defaults/en/reply.toml"),
    ),
    (
        "transform.toml",
        include_str!("../../../modes/defaults/en/transform.toml"),
    ),
];

const DEFAULTS_FR: DefaultSet = &[
    (
        "exaktes_diktat.toml",
        include_str!("../../../modes/defaults/fr/exaktes_diktat.toml"),
    ),
    (
        "korrigierendes_diktat.toml",
        include_str!("../../../modes/defaults/fr/korrigierendes_diktat.toml"),
    ),
    (
        "foermliche_email.toml",
        include_str!("../../../modes/defaults/fr/foermliche_email.toml"),
    ),
    (
        "slack_teams.toml",
        include_str!("../../../modes/defaults/fr/slack_teams.toml"),
    ),
    (
        "github_issue.toml",
        include_str!("../../../modes/defaults/fr/github_issue.toml"),
    ),
    (
        "claude_code_anweisung.toml",
        include_str!("../../../modes/defaults/fr/claude_code_anweisung.toml"),
    ),
    (
        "improve.toml",
        include_str!("../../../modes/defaults/fr/improve.toml"),
    ),
    (
        "reply.toml",
        include_str!("../../../modes/defaults/fr/reply.toml"),
    ),
    (
        "transform.toml",
        include_str!("../../../modes/defaults/fr/transform.toml"),
    ),
];

const DEFAULTS_ES: DefaultSet = &[
    (
        "exaktes_diktat.toml",
        include_str!("../../../modes/defaults/es/exaktes_diktat.toml"),
    ),
    (
        "korrigierendes_diktat.toml",
        include_str!("../../../modes/defaults/es/korrigierendes_diktat.toml"),
    ),
    (
        "foermliche_email.toml",
        include_str!("../../../modes/defaults/es/foermliche_email.toml"),
    ),
    (
        "slack_teams.toml",
        include_str!("../../../modes/defaults/es/slack_teams.toml"),
    ),
    (
        "github_issue.toml",
        include_str!("../../../modes/defaults/es/github_issue.toml"),
    ),
    (
        "claude_code_anweisung.toml",
        include_str!("../../../modes/defaults/es/claude_code_anweisung.toml"),
    ),
    (
        "improve.toml",
        include_str!("../../../modes/defaults/es/improve.toml"),
    ),
    (
        "reply.toml",
        include_str!("../../../modes/defaults/es/reply.toml"),
    ),
    (
        "transform.toml",
        include_str!("../../../modes/defaults/es/transform.toml"),
    ),
];

const DEFAULTS_IT: DefaultSet = &[
    (
        "exaktes_diktat.toml",
        include_str!("../../../modes/defaults/it/exaktes_diktat.toml"),
    ),
    (
        "korrigierendes_diktat.toml",
        include_str!("../../../modes/defaults/it/korrigierendes_diktat.toml"),
    ),
    (
        "foermliche_email.toml",
        include_str!("../../../modes/defaults/it/foermliche_email.toml"),
    ),
    (
        "slack_teams.toml",
        include_str!("../../../modes/defaults/it/slack_teams.toml"),
    ),
    (
        "github_issue.toml",
        include_str!("../../../modes/defaults/it/github_issue.toml"),
    ),
    (
        "claude_code_anweisung.toml",
        include_str!("../../../modes/defaults/it/claude_code_anweisung.toml"),
    ),
    (
        "improve.toml",
        include_str!("../../../modes/defaults/it/improve.toml"),
    ),
    (
        "reply.toml",
        include_str!("../../../modes/defaults/it/reply.toml"),
    ),
    (
        "transform.toml",
        include_str!("../../../modes/defaults/it/transform.toml"),
    ),
];

/// Pick the default set for a given raw locale string (BCP-47 prefix
/// match). Unknown locales fall back to English — same policy as the
/// frontend's `pickSupported`. Keeping the mapping in sync with the
/// frontend is a code-review concern.
fn defaults_for_locale(raw_locale: Option<&str>) -> DefaultSet {
    let prefix = raw_locale
        .and_then(|s| s.split(['-', '_']).next())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    match prefix.as_str() {
        "de" => DEFAULTS_DE,
        "fr" => DEFAULTS_FR,
        "es" => DEFAULTS_ES,
        "it" => DEFAULTS_IT,
        _ => DEFAULTS_EN,
    }
}

/// Ensure that `dir` holds at least the default modes for the given
/// locale. The directory is created if missing. Existing files are NOT
/// overwritten (user edits stay intact).
pub fn bootstrap_defaults_if_empty(dir: &Path, locale: Option<&str>) -> Result<()> {
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

    let defaults = defaults_for_locale(locale);
    for (name, content) in defaults {
        let path = dir.join(name);
        std::fs::write(&path, content)
            .map_err(|e| VoiceTypeError::Mode(format!("write({}): {e}", path.display())))?;
        tracing::info!(file = %path.display(), locale = ?locale, "Default mode created");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::modes::Mode;

    fn all_sets() -> [(&'static str, DefaultSet); 5] {
        [
            ("de", DEFAULTS_DE),
            ("en", DEFAULTS_EN),
            ("fr", DEFAULTS_FR),
            ("es", DEFAULTS_ES),
            ("it", DEFAULTS_IT),
        ]
    }

    /// Every embedded default TOML must parse and pass `Mode::validate`,
    /// and every locale must ship the three edit modes. Guards against a
    /// broken default shipping in the binary (the bootstrap would
    /// otherwise fail at first run, discarding the whole set).
    #[test]
    fn embedded_defaults_parse_validate_and_include_edit_modes() {
        for (locale, set) in all_sets() {
            let mut ids = Vec::new();
            for &(name, content) in set {
                let mode: Mode = toml::from_str(content)
                    .unwrap_or_else(|e| panic!("{locale}/{name}: TOML parse failed: {e}"));
                mode.validate()
                    .unwrap_or_else(|e| panic!("{locale}/{name}: validate failed: {e}"));
                ids.push(mode.id);
            }
            for id in ["improve", "reply", "transform"] {
                assert!(
                    ids.iter().any(|i| i == id),
                    "{locale}: edit mode '{id}' missing from default set"
                );
            }
        }
    }
}
