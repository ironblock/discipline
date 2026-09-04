//! Tier 0 of the collector: literal nomination.
//!
//! An entry's **anchors** are the things in it that a later turn would have
//! to write the same way to be talking about the same thing: identifiers,
//! file paths, quoted terms. When new prose or tool output carries one of an
//! entry's anchors, the entry is nominated -- not superseded, nominated: a
//! fork then judges, in the verdict format, and the reconciler applies.
//!
//! Free, deterministic, and high-precision on purpose. The precision is in
//! two rules the tests pin:
//!
//! * An anchor is never an English word. `plan`, `parser` and `record` are
//!   in every third sentence of a drive; an entry anchored on them would be
//!   nominated by everything. Anchors are shapes prose does not produce by
//!   accident: `snake_case`, `CamelCase`, `dotted.paths`, `a::b`, `path/to/x`,
//!   `x.rs`, and whatever the author put in quotes or backticks.
//! * A match is at a **word boundary**. `plan_a` inside `plan_ab` and `Parser`
//!   inside `ParserError` are not the same thing, and a substring match would
//!   nominate the entry on every mention of the longer name. The multi-pattern
//!   search here is a plain scan with boundary checks; the automaton in the
//!   design's name is an optimisation this corpus size does not need, and the
//!   boundary rule is the part that changes what is found.
//!
//! An entry is never nominated by the turn that created it: its own prose
//! carries its own anchors by construction.

use std::collections::BTreeSet;

use crate::object::WorkingObject;

use super::{Evidence, Nomination};

/// What kind of thing an anchor is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AnchorKind {
    /// `snake_case`, `CamelCase`, `dotted.path`, `a::b`.
    Identifier,
    /// A path, or a file name with an extension.
    Path,
    /// A span the author quoted or backticked.
    Quoted,
}

impl AnchorKind {
    /// Every kind.
    pub const ALL: &'static [Self] = &[Self::Identifier, Self::Path, Self::Quoted];

    /// A stable name.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Identifier => "identifier",
            Self::Path => "path",
            Self::Quoted => "quoted",
        }
    }
}

/// One anchor of an entry.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Anchor {
    /// The text, exactly as it must recur.
    pub text: String,
    /// What kind of thing it is.
    pub kind: AnchorKind,
}

/// The shortest text worth anchoring on. Below this, a shape is a
/// coincidence: `a.b` occurs in version numbers and `io` in every word.
const MIN_ANCHOR_LEN: usize = 3;

/// A character that can be part of an identifier or a path.
fn is_token_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '/' | ':' | '-' | '~')
}

/// A character that is part of a word, for boundary purposes.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Strip the punctuation prose hangs on a token: a trailing period, comma,
/// colon, semicolon, bracket.
fn trim_token(token: &str) -> &str {
    token.trim_matches(|c: char| {
        matches!(
            c,
            '.' | ',' | ';' | ':' | ')' | '(' | '[' | ']' | '"' | '\''
        )
    })
}

/// Whether a bare token has the shape of an identifier rather than a word.
fn identifier_shape(token: &str) -> bool {
    let has_alpha = token.chars().any(char::is_alphabetic);
    if !has_alpha {
        return false;
    }
    // snake_case with a letter on both sides of the underscore.
    if token.contains('_') && !token.starts_with('_') && !token.ends_with('_') {
        return true;
    }
    // a::b
    if token.contains("::") && !token.starts_with(':') && !token.ends_with(':') {
        return true;
    }
    // dotted.path -- letters on both sides of a dot, not a version number and
    // not a sentence boundary.
    if let Some((left, right)) = token.split_once('.')
        && !left.is_empty()
        && !right.is_empty()
        && left.chars().last().is_some_and(char::is_alphabetic)
        && right.chars().next().is_some_and(char::is_alphabetic)
    {
        return true;
    }
    // CamelCase: a lower-case letter followed by an upper-case one inside the
    // token, so `Parser` alone is a word and `ParseError` is a name.
    token
        .chars()
        .zip(token.chars().skip(1))
        .any(|(a, b)| a.is_lowercase() && b.is_uppercase())
}

/// Whether a bare token has the shape of a path or a file name.
fn path_shape(token: &str) -> bool {
    if token.contains('/') {
        return token.chars().any(char::is_alphanumeric);
    }
    // `name.ext`: a short alphabetic extension after a stem, and not a
    // sentence's last word with its period.
    token.rsplit_once('.').is_some_and(|(stem, ext)| {
        !stem.is_empty()
            && (1..=4).contains(&ext.len())
            && ext.chars().all(|c| c.is_ascii_alphanumeric())
            && ext.chars().any(char::is_alphabetic)
            && stem
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-'))
    })
}

/// Spans the author set off in backticks or double quotes.
fn quoted_spans(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for delimiter in ['`', '"'] {
        let mut rest = text;
        while let Some(open) = rest.find(delimiter) {
            let after = &rest[open + delimiter.len_utf8()..];
            let Some(close) = after.find(delimiter) else {
                break;
            };
            let span = after[..close].trim();
            if span.len() >= MIN_ANCHOR_LEN && !span.contains('\n') {
                found.push(span.to_owned());
            }
            rest = &after[close + delimiter.len_utf8()..];
        }
    }
    found
}

/// The anchors of an entry's content.
///
/// Deterministic and deduplicated, in first-occurrence order.
#[must_use]
pub fn anchors(content: &str) -> Vec<Anchor> {
    let mut seen = BTreeSet::new();
    let mut found = Vec::new();
    let mut push = |text: String, kind: AnchorKind| {
        if text.len() >= MIN_ANCHOR_LEN && seen.insert(text.clone()) {
            found.push(Anchor { text, kind });
        }
    };
    for span in quoted_spans(content) {
        push(span, AnchorKind::Quoted);
    }
    for raw in content.split(|c: char| c.is_whitespace() || matches!(c, '`' | '"')) {
        let token = trim_token(raw);
        if token.is_empty() || !token.chars().all(is_token_char) {
            continue;
        }
        if path_shape(token) {
            push(token.to_owned(), AnchorKind::Path);
        } else if identifier_shape(token) {
            push(token.to_owned(), AnchorKind::Identifier);
        }
    }
    found
}

/// Where in a haystack an anchor recurred.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// Byte offset of the occurrence.
    pub offset: usize,
}

/// Whether `needle` occurs in `haystack` at `at` as a whole token: neither
/// side continues the word. For a path, the boundary is a non-path
/// character, so `src/lib.rs` inside `diet/src/lib.rs` is a hit and
/// `lib.rs` inside `mylib.rs` is not.
fn bounded(haystack: &str, at: usize, needle: &str) -> bool {
    let before = haystack[..at].chars().next_back();
    let after = haystack[at + needle.len()..].chars().next();
    let continues = |c: Option<char>| c.is_some_and(is_word_char);
    !continues(before) && !continues(after)
}

/// Every whole-token occurrence of `needle` in `haystack`.
#[must_use]
pub fn find(needle: &str, haystack: &str) -> Vec<Hit> {
    let mut hits = Vec::new();
    if needle.is_empty() {
        return hits;
    }
    let mut from = 0;
    while let Some(found) = haystack[from..].find(needle) {
        let at = from + found;
        if bounded(haystack, at, needle) {
            hits.push(Hit { offset: at });
        }
        from = at + needle.len();
    }
    hits
}

/// Which text an anchor recurred in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    /// The model's own prose.
    Prose,
    /// A tool's output.
    ToolOutput,
}

impl Source {
    /// A stable name.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Prose => "prose",
            Self::ToolOutput => "tool_output",
        }
    }
}

/// What a turn brought that an entry's anchors are matched against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewText<'a> {
    /// The turn the text belongs to.
    pub turn: u32,
    /// The model's prose for the turn.
    pub prose: &'a str,
    /// Tool output the turn saw.
    pub tool_output: &'a str,
}

/// Nominate every live entry one of whose anchors recurs in the new text.
///
/// One nomination per entry, by its first anchor to recur, prose before tool
/// output. An entry born in the same turn as the text is never nominated by
/// it. Entries are visited in id order, so the result does not depend on the
/// object's insertion order.
#[must_use]
pub fn nominate(object: &WorkingObject, new: NewText<'_>) -> Vec<Nomination> {
    let mut nominations = Vec::new();
    let mut live: Vec<_> = object.live().collect();
    live.sort_by(|a, b| a.id.cmp(&b.id));
    for entry in live {
        if entry.provenances.iter().any(|p| p.turn >= new.turn) {
            continue;
        }
        let found = anchors(&entry.content).into_iter().find_map(|anchor| {
            [
                (Source::Prose, new.prose),
                (Source::ToolOutput, new.tool_output),
            ]
            .into_iter()
            .find_map(|(source, haystack)| {
                find(&anchor.text, haystack)
                    .into_iter()
                    .next()
                    .map(|hit| (anchor.clone(), source, hit))
            })
        });
        if let Some((anchor, source, hit)) = found {
            nominations.push(Nomination {
                entry: entry.id.clone(),
                evidence: Evidence::Literal {
                    anchor,
                    source,
                    hit,
                },
            });
        }
    }
    nominations
}

#[cfg(test)]
mod tests {
    use super::{
        Anchor, AnchorKind, Evidence, NewText, Nomination, Source, anchors, find, nominate,
    };
    use crate::formats::record::{Reasoning, Regime, Substrate};
    use crate::object::{EntryId, Patch, Provenance, WorkingObject};
    use std::collections::BTreeMap;

    /// The literal evidence a nomination carries, or a panic naming what it
    /// carried instead: this tier answers in the shared shape, and a sense
    /// nomination reaching a literal assertion is a bug worth the panic.
    fn literal(nomination: &Nomination) -> (&Anchor, Source) {
        match &nomination.evidence {
            Evidence::Literal { anchor, source, .. } => (anchor, *source),
            other @ Evidence::Sense { .. } => panic!("tier 0 produced {other:?}"),
        }
    }

    fn texts(found: &[Anchor]) -> Vec<(&str, AnchorKind)> {
        found.iter().map(|a| (a.text.as_str(), a.kind)).collect()
    }

    #[test]
    fn anchors_are_shapes_prose_does_not_produce_by_accident() {
        let found = anchors(
            "The parser in diet/src/formats/interview.rs keeps every line; \
             `Answer::accounted_text` returns what it kept, and plan_a is the plan. \
             See config.toml and the ParseError type.",
        );
        assert_eq!(
            texts(&found),
            vec![
                ("Answer::accounted_text", AnchorKind::Quoted),
                ("diet/src/formats/interview.rs", AnchorKind::Path),
                ("plan_a", AnchorKind::Identifier),
                ("config.toml", AnchorKind::Path),
                ("ParseError", AnchorKind::Identifier),
            ]
        );
    }

    #[test]
    fn an_english_sentence_has_no_anchors() {
        assert!(
            anchors("The plan is to fix the parser before the record changes.").is_empty(),
            "an English word became an anchor"
        );
        assert!(anchors("Version 0.3 shipped. Done.").is_empty());
    }

    #[test]
    fn a_match_is_at_a_word_boundary() {
        assert_eq!(find("plan_a", "we chose plan_a over plan_b").len(), 1);
        assert!(
            find("plan_a", "we chose plan_ab").is_empty(),
            "an anchor matched inside a longer word"
        );
        assert!(find("Parser", "a ParserError").is_empty());
        assert_eq!(find("lib.rs", "see diet/src/lib.rs").len(), 1);
        assert!(find("lib.rs", "see mylib.rs").is_empty());
        assert_eq!(find("x", "x x x").len(), 3);
    }

    fn regime() -> Regime {
        Regime {
            arm: "baseline".to_owned(),
            dogma_version: 0,
            substrate: Substrate {
                name: "local".to_owned(),
                model: "a-model".to_owned(),
                quantization: "q4".to_owned(),
                sampler: BTreeMap::new(),
                reasoning: Reasoning::On,
                hardware: "one-gpu".to_owned(),
            },
        }
    }

    fn add(object: &mut WorkingObject, id: &str, content: &str, turn: u32) {
        object
            .apply(&Patch::Add {
                id: EntryId::new(id).expect("an id"),
                content: content.to_owned(),
                provenance: Provenance {
                    turn,
                    lane: "interview".to_owned(),
                    fork: None,
                    index: 0,
                },
            })
            .expect("applied");
    }

    // The acceptance case: an entry at turn 5, prose at turn 18 that names
    // its anchor, and the pair is nominated.
    #[test]
    fn an_entry_is_nominated_when_a_later_turn_names_its_anchor() {
        let mut object = WorkingObject::open(regime());
        add(
            &mut object,
            "e1",
            "`check_record` is missing from the CLI, so callers reparse",
            5,
        );
        add(
            &mut object,
            "e2",
            "the build takes four minutes on this box",
            6,
        );
        let nominations = nominate(
            &object,
            NewText {
                turn: 18,
                prose: "Actually, check_record exists now: the CLI gained it in the resolver change.",
                tool_output: "",
            },
        );
        assert_eq!(nominations.len(), 1, "{nominations:?}");
        assert_eq!(nominations[0].entry.as_str(), "e1");
        assert_eq!(literal(&nominations[0]).0.text, "check_record");
    }

    #[test]
    fn a_mention_of_an_unrelated_name_nominates_nothing() {
        let mut object = WorkingObject::open(regime());
        add(
            &mut object,
            "e1",
            "`check_record` is missing from the CLI",
            5,
        );
        let nominations = nominate(
            &object,
            NewText {
                turn: 18,
                prose: "The check_records table in the plan was never the CLI's business.",
                tool_output: "",
            },
        );
        assert!(nominations.is_empty(), "{nominations:?}");
    }

    #[test]
    fn an_entry_is_not_nominated_by_the_turn_that_made_it() {
        let mut object = WorkingObject::open(regime());
        add(
            &mut object,
            "e1",
            "`check_record` is missing from the CLI",
            18,
        );
        let nominations = nominate(
            &object,
            NewText {
                turn: 18,
                prose: "I found that check_record is missing from the CLI.",
                tool_output: "",
            },
        );
        assert!(
            nominations.is_empty(),
            "an entry nominated itself: {nominations:?}"
        );
    }

    #[test]
    fn tool_output_nominates_too_and_prose_is_preferred() {
        let mut object = WorkingObject::open(regime());
        add(
            &mut object,
            "e1",
            "diet/src/object.rs holds the reconciler",
            3,
        );
        let by_tool = nominate(
            &object,
            NewText {
                turn: 9,
                prose: "Reading the reconciler.",
                tool_output: "diet/src/object.rs:474:    pub fn apply_turn",
            },
        );
        assert_eq!(by_tool.len(), 1);
        assert_eq!(literal(&by_tool[0]).1, Source::ToolOutput);
        let by_prose = nominate(
            &object,
            NewText {
                turn: 9,
                prose: "diet/src/object.rs is where I am.",
                tool_output: "diet/src/object.rs:1",
            },
        );
        assert_eq!(literal(&by_prose[0]).1, Source::Prose);
    }

    /// What a corpus case says this tier owes it. The word is read off disk,
    /// so it is looked up in the vocabulary rather than matched against
    /// literals: a spelling nobody defined is a case nobody checked, and it
    /// must say so instead of falling through.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Expectation {
        Nominate,
        DoNotNominate,
    }

    impl Expectation {
        const ALL: &'static [Self] = &[Self::Nominate, Self::DoNotNominate];

        fn tag(self) -> &'static str {
            match self {
                Self::Nominate => "nominate",
                Self::DoNotNominate => "do_not_nominate",
            }
        }

        fn written(word: &str) -> Option<Self> {
            Self::ALL.iter().copied().find(|kind| kind.tag() == word)
        }
    }

    // The committed corpus, walked. Each case is an entry recorded at an
    // earlier turn, the text of a later one, and the single word this tier
    // owes: `nominate` or `do_not_nominate`. The cases sit on disk rather
    // than in this file because a tier checked only against examples
    // written beside it is checked against its own reflection. The three
    // silent cases are the point of a precision-first tier: an English
    // restatement, a longer name that merely contains the anchor, and a
    // turn on the same topic that never names it.
    #[test]
    fn the_committed_corpus_gets_the_answer_it_records() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("capture/collector/corpus");
        let mut nominating = 0_usize;
        let mut silent = 0_usize;
        for found in std::fs::read_dir(&dir).expect("the corpus is readable") {
            let case = found.expect("a readable entry").path();
            if !case.is_dir() {
                continue;
            }
            let name = case
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .expect("a named case")
                .to_owned();
            let read = |file: &str| {
                std::fs::read_to_string(case.join(file))
                    .unwrap_or_else(|err| panic!("{name}: {file}: {err}"))
            };
            let mut object = WorkingObject::open(regime());
            add(&mut object, "e1", read("entry.txt").trim(), 5);
            // Tool output is optional: most cases are prose, and the one
            // that is not says so by carrying the file.
            let tool_output =
                std::fs::read_to_string(case.join("tool_output.txt")).unwrap_or_default();
            let nominations = nominate(
                &object,
                NewText {
                    turn: 18,
                    prose: read("prose.txt").trim(),
                    tool_output: tool_output.trim(),
                },
            );
            let word = read("expected");
            let expectation = Expectation::written(word.trim())
                .unwrap_or_else(|| panic!("{name}: `expected` reads {:?}", word.trim()));
            match expectation {
                Expectation::Nominate => {
                    nominating += 1;
                    assert!(
                        !nominations.is_empty(),
                        "{name}: the corpus expects a nomination and the tier gave none"
                    );
                }
                Expectation::DoNotNominate => {
                    silent += 1;
                    assert!(
                        nominations.is_empty(),
                        "{name}: a false nomination: {nominations:?}"
                    );
                }
            }
        }
        // A corpus that quietly emptied would pass every assertion above.
        assert!(
            nominating >= 5,
            "the corpus lost nominating cases: {nominating}"
        );
        assert!(
            silent >= 3,
            "a corpus with no silent cases cannot catch over-firing: {silent}"
        );
    }
}
