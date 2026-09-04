//! The words that decide when to ask.
//!
//! A trigger vocabulary is dogma as much as a template is: it sets what the
//! program considers worth interrupting for. Kept as lowercased substrings
//! rather than a regex so this crate stays free of a regex dependency; the
//! matching rule differs per set and lives with the mechanism that applies it.
//!
//! Both lists are the research program's, word for word, and the pin below
//! is that program's own pin: the same digest function over the same
//! concatenation gives the same sixteen hex digits, so the port is checkable
//! from either side.

/// Words in a turn's text that trip the surprise asks. Matched at WORD
/// BOUNDARIES by the mechanism, so `require` fires and `requirement` does not.
pub const SURPRISE_WORDS: &[&str] = &[
    "actually",
    "instead",
    "turns out",
    "however",
    "wait,",
    "wait.",
    "doesn't support",
    "not supported",
    "unsupported",
    "can't",
    "cannot",
    "must be",
    "must use",
    "must not",
    // Both forms listed, matched as whole words, so "expect"/"expects" and
    // "require"/"requires" fire but "requirement"/"required"/"expected" do not.
    "expect",
    "expects",
    "require",
    "requires",
    "error",
    "failed",
    "failure",
    "mismatch",
    "deprecated",
    "confirmed via",
    "it's called",
    "renamed",
];

/// Words in a tool output that mark an error; an error always fires the
/// asks, cooldown or not. Matched as PLAIN SUBSTRINGS, without word
/// boundaries, so `TS2345` and `ENOENT` hit.
pub const ERROR_WORDS: &[&str] = &[
    "error",
    "traceback",
    "failed",
    "cannot find",
    "not found",
    "eexist",
    "enoent",
];

/// The pin: how many words each list holds and the digest of their
/// concatenation, as the research program pinned them.
pub const SURPRISE_PIN: (usize, &str) = (26, "53a54875041d2c8e");
/// See [`SURPRISE_PIN`].
pub const ERROR_PIN: (usize, &str) = (7, "2f500e2563deb160");

#[cfg(test)]
mod tests {
    use super::{ERROR_PIN, ERROR_WORDS, SURPRISE_PIN, SURPRISE_WORDS};
    use crate::dogma::digest;

    // A stray edit here changes which turns get interrupted, which is a
    // silent change to what every fire measures.
    #[test]
    fn the_trigger_vocabulary_is_pinned() {
        assert_eq!(
            (SURPRISE_WORDS.len(), digest(&SURPRISE_WORDS.concat())),
            (SURPRISE_PIN.0, SURPRISE_PIN.1.to_owned()),
            "the surprise vocabulary changed: that is a dogma version bump"
        );
        assert_eq!(
            (ERROR_WORDS.len(), digest(&ERROR_WORDS.concat())),
            (ERROR_PIN.0, ERROR_PIN.1.to_owned()),
            "the error vocabulary changed: that is a dogma version bump"
        );
    }

    #[test]
    fn every_word_is_lowercase_and_distinct() {
        for list in [SURPRISE_WORDS, ERROR_WORDS] {
            let mut seen = std::collections::BTreeSet::new();
            for word in list {
                assert_eq!(*word, word.to_lowercase(), "{word} is not lowercase");
                assert!(!word.trim().is_empty());
                assert!(seen.insert(*word), "{word} is listed twice");
            }
        }
    }
}
