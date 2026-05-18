// SPDX-License-Identifier: GPL-3.0-or-later
//! LocalAgreement-2 — stabilisiert Whisper-Streaming-Outputs.
//!
//! Idee (Machacek et al., arXiv 2307.14743): wenn zwei aufeinanderfolgende
//! Decodes des wachsenden Audio-Buffers denselben Wort-Prefix produzieren,
//! ist dieser Prefix "stabil" und darf committed werden (im Overlay
//! anzeigen). Der divergierende Schwanz bleibt vorlaeufig, wird beim
//! naechsten Pass neu bewertet.
//!
//! Wir tokenisieren auf Whitespace und vergleichen wort-fuer-wort.
//! Interpunktion bleibt am Wort haengen, weil Whisper sie eh als Teil des
//! Tokens emittiert.

/// Liefere den laengsten gemeinsamen Wort-Prefix zweier Whisper-Outputs
/// als ein-Leerzeichen-getrennten String. Bei leerem oder voellig
/// divergierendem Input: `""`.
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
        // split_whitespace abstrahiert Mehrfach-Leerzeichen + Tabs weg.
        assert_eq!(stable_prefix("a   b\tc", "a b c"), "a b c");
    }

    #[test]
    fn punctuation_sticks_to_word() {
        // "Sonne." != "Sonne," — gewollt, weil Whisper Interpunktion noch
        // ueberdenken kann; nur "Heute" ist sicher stabil.
        let prev = "Heute scheint die Sonne.";
        let curr = "Heute scheint die Sonne,";
        assert_eq!(stable_prefix(prev, curr), "Heute scheint die");
    }
}
