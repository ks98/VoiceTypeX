// SPDX-License-Identifier: GPL-3.0-or-later
//! LocalAgreement-2 — stabilizes Whisper streaming outputs.
//!
//! Idea (Machacek et al., arXiv 2307.14743): if two consecutive
//! decodes of the growing audio buffer produce the same word prefix,
//! that prefix is "stable" and may be committed (shown in the
//! overlay). The diverging tail remains tentative and is re-evaluated
//! on the next pass.
//!
//! We tokenize on whitespace and compare word-by-word. Punctuation
//! sticks to the word because Whisper emits it as part of the token
//! anyway.

/// Return the longest common word prefix of two Whisper outputs as a
/// single-space-separated string. For empty or completely diverging
/// input: `""`.
pub fn stable_prefix(prev: &str, curr: &str) -> String {
    let prev_tokens = prev.split_whitespace();
    let curr_tokens = curr.split_whitespace();
    let mut common: Vec<&str> = Vec::new();
    for (a, b) in prev_tokens.zip(curr_tokens) {
        if a == b {
            common.push(a);
        } else {
            break;
        }
    }
    common.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_strings_yield_full_prefix() {
        let s = "Heute scheint die Sonne.";
        assert_eq!(stable_prefix(s, s), s);
    }

    #[test]
    fn diverging_tail_returns_common_prefix() {
        let prev = "Ich wollte heute pünktlich kommen aber";
        let curr = "Ich wollte heute pünktlich da sein und";
        assert_eq!(stable_prefix(prev, curr), "Ich wollte heute pünktlich");
    }

    #[test]
    fn empty_inputs_return_empty() {
        assert_eq!(stable_prefix("", ""), "");
        assert_eq!(stable_prefix("Hallo", ""), "");
        assert_eq!(stable_prefix("", "Hallo"), "");
    }

    #[test]
    fn first_word_already_diverges() {
        assert_eq!(stable_prefix("Hallo Welt", "Tschuess Welt"), "");
    }

    #[test]
    fn whitespace_variation_is_tolerated() {
        // split_whitespace collapses multiple spaces + tabs.
        assert_eq!(stable_prefix("a   b\tc", "a b c"), "a b c");
    }

    #[test]
    fn punctuation_sticks_to_word() {
        // "Sonne." != "Sonne," — intentional, because Whisper may
        // still revise punctuation; only "Heute" is reliably stable.
        let prev = "Heute scheint die Sonne.";
        let curr = "Heute scheint die Sonne,";
        assert_eq!(stable_prefix(prev, curr), "Heute scheint die");
    }
}
