// SPDX-License-Identifier: GPL-3.0-or-later
//! Edit-mode ("Bearbeiten") helpers.
//!
//! Two pure functions that frame the LLM round-trip for selection-based
//! modes, kept here so they are unit-testable without any platform or
//! pipeline glue:
//!
//! - [`compose_edit_input`] builds the user message from the selected
//!   text and the (optional) spoken instruction.
//! - [`resolve_output_action`] turns the LLM response into the effective
//!   [`OutputAction`] plus the text to inject — including the `auto`
//!   sentinel convention with a safe fallback.

use crate::core::modes::OutputAction;

/// Sentinel tokens an `auto`-output mode may emit as the first token of
/// its response to choose the injection action. Compared
/// case-insensitively (see [`resolve_output_action`]).
const SENTINEL_REPLACE: &str = "@@REPLACE";
const SENTINEL_APPEND: &str = "@@APPEND";
const SENTINEL_PREPEND: &str = "@@PREPEND";

/// Compose the user message for an edit mode from the selected text and
/// the spoken instruction.
///
/// The selection is embedded verbatim (formatting preserved); the
/// instruction is trimmed because voice transcripts carry leading and
/// trailing whitespace. Both blocks are always present so a mode's
/// `system_prompt` can rely on a stable structure — an empty
/// `<instruction>` simply means "apply the system prompt as-is".
pub fn compose_edit_input(selection: &str, instruction: &str) -> String {
    format!(
        "<selected_text>\n{selection}\n</selected_text>\n\n<instruction>\n{}\n</instruction>",
        instruction.trim()
    )
}

/// Resolve the effective injection action and the text to inject from
/// the LLM response.
///
/// - For a fixed `mode_output` (`replace`/`append`/`prepend`/`insert`)
///   the response is returned unchanged.
/// - For `auto` the first token of the response is matched (case-
///   insensitively) against the `@@REPLACE` / `@@APPEND` / `@@PREPEND`
///   sentinels. On a match the sentinel is stripped and the remainder
///   returned; on no match nothing is stripped and `fallback` decides
///   the action.
///
/// `fallback` is expected to never be `Auto` (enforced by
/// `Mode::validate`), so this function cannot recurse into the sentinel
/// branch.
pub fn resolve_output_action(
    mode_output: OutputAction,
    fallback: OutputAction,
    llm_output: &str,
) -> (OutputAction, String) {
    if mode_output != OutputAction::Auto {
        return (mode_output, llm_output.to_string());
    }

    let trimmed = llm_output.trim_start();
    let (token, rest) = match trimmed.split_once(char::is_whitespace) {
        Some((t, r)) => (t, r),
        None => (trimmed, ""),
    };

    let action = match token.to_ascii_uppercase().as_str() {
        SENTINEL_REPLACE => Some(OutputAction::Replace),
        SENTINEL_APPEND => Some(OutputAction::Append),
        SENTINEL_PREPEND => Some(OutputAction::Prepend),
        _ => None,
    };

    match action {
        Some(a) => (a, rest.trim_start().to_string()),
        None => (fallback, llm_output.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_embeds_selection_verbatim_and_trims_instruction() {
        let out = compose_edit_input("Hello\nWorld", "  make it formal  ");
        assert!(out.contains("<selected_text>\nHello\nWorld\n</selected_text>"));
        assert!(out.contains("<instruction>\nmake it formal\n</instruction>"));
    }

    #[test]
    fn compose_keeps_empty_instruction_block() {
        let out = compose_edit_input("text", "");
        assert!(out.contains("<instruction>\n\n</instruction>"));
    }

    #[test]
    fn fixed_action_returns_output_unchanged() {
        let (action, text) =
            resolve_output_action(OutputAction::Replace, OutputAction::Replace, "the result");
        assert_eq!(action, OutputAction::Replace);
        assert_eq!(text, "the result");
    }

    #[test]
    fn fixed_action_does_not_strip_sentinel_like_prefix() {
        // A non-auto mode must never treat a leading @@REPLACE as a
        // control token — it is literal content there.
        let (action, text) = resolve_output_action(
            OutputAction::Append,
            OutputAction::Replace,
            "@@REPLACE stays literal",
        );
        assert_eq!(action, OutputAction::Append);
        assert_eq!(text, "@@REPLACE stays literal");
    }

    #[test]
    fn auto_parses_sentinel_on_own_line() {
        let (action, text) = resolve_output_action(
            OutputAction::Auto,
            OutputAction::Replace,
            "@@APPEND\nDear Sir,\nthank you.",
        );
        assert_eq!(action, OutputAction::Append);
        assert_eq!(text, "Dear Sir,\nthank you.");
    }

    #[test]
    fn auto_parses_sentinel_inline_and_case_insensitive() {
        let (action, text) = resolve_output_action(
            OutputAction::Auto,
            OutputAction::Replace,
            "@@prepend Summary:",
        );
        assert_eq!(action, OutputAction::Prepend);
        assert_eq!(text, "Summary:");
    }

    #[test]
    fn auto_tolerates_leading_whitespace_before_sentinel() {
        let (action, text) = resolve_output_action(
            OutputAction::Auto,
            OutputAction::Replace,
            "\n  @@REPLACE done",
        );
        assert_eq!(action, OutputAction::Replace);
        assert_eq!(text, "done");
    }

    #[test]
    fn auto_without_sentinel_uses_fallback_and_keeps_full_text() {
        let (action, text) = resolve_output_action(
            OutputAction::Auto,
            OutputAction::Append,
            "Just some text without a tag.",
        );
        assert_eq!(action, OutputAction::Append);
        assert_eq!(text, "Just some text without a tag.");
    }

    #[test]
    fn auto_with_glued_token_falls_back() {
        // No whitespace after the token → not a recognized sentinel.
        let (action, text) =
            resolve_output_action(OutputAction::Auto, OutputAction::Replace, "@@APPENDnow");
        assert_eq!(action, OutputAction::Replace);
        assert_eq!(text, "@@APPENDnow");
    }
}
