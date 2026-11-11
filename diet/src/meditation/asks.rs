//! The operator's asks a refill carries (N3, the maintainer's ruling of
//! 2026-11-11, overturning F7's "keep nothing by default"): every ask the
//! refill would otherwise compact away, verbatim and in the order it was
//! asked, as Qwen Code's compaction carries "all user messages ... in
//! chronological order" (`diet/dogma/compaction/qwen-code-v0.25.3.txt`).
//! No model is involved: these are the asks' own bytes.
//!
//! An ask still on the trunk after the refill -- the running turn's, or one
//! in the kept tail -- rides there and is not repeated here.
//!
//! The heading and the sentence are the maintainer's wording (2026-11-11):
//! "user", as the render's "user said" and the head's "a person".

use std::fmt::Write as _;

/// The section's heading, in the render's own register.
pub const HEADER: &str = "# user messages\n";

/// What the section says before the asks.
pub const SENTENCE: &str =
    "The user's messages before context was compacted, verbatim, in the order they were sent:";

/// The section a refill carries of `asks` -- each `(turn, text)`, in the
/// order asked -- after the render; `None` when there is none. Each ask is
/// under a heading naming its turn, so an ask's own blank lines stay its.
#[must_use]
pub fn section(asks: &[(u32, &str)]) -> Option<String> {
    if asks.is_empty() {
        return None;
    }
    let mut text = format!("\n{HEADER}{SENTENCE}\n");
    for (turn, ask) in asks {
        let _ = write!(text, "\n## turn {turn}\n{ask}\n");
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::section;

    #[test]
    fn no_asks_is_no_section_and_each_ask_keeps_its_own_bytes() {
        assert_eq!(section(&[]), None);
        let text = section(&[(1, "first"), (2, "two lines\n\nkept")]).expect("a section");
        assert_eq!(
            text,
            "\n# user messages\nThe user's messages before context was compacted, \
             verbatim, in the order they were sent:\n\n## turn 1\nfirst\n\n## turn 2\ntwo \
             lines\n\nkept\n"
        );
    }
}
