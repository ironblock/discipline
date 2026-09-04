//! Register mimicry: the answer that is its own ask wearing an answer's
//! clothes.
//!
//! A forked model that has nothing to say can still fill a template. It hands
//! the headings back with the slots still in them, writes the field labels
//! into the fields, or matches the ask's register so faithfully that nothing
//! in the reply is new. The research phase's parser saga was partly a mimicry
//! saga: several "parser defects" were the model returning the template with
//! cosmetic changes, and a tolerant parser captured the result as entries.
//! The object then held the ask, attributed to the model, as fact.
//!
//! Two consequences, both in the types:
//!
//! * **Mimicry is a typed outcome, never a content entry.** [`AskOutcome`]
//!   names what an ask came to, and [`AskOutcome::patches`] hands back nothing
//!   for a mimicked one. The decision is taken once, for the whole answer, in
//!   [`classify`]. It is deliberately not taken a second time per field at the
//!   point of writing: two rules that each cover the other cannot both be
//!   proven red, and the one that stops being reached rots.
//! * **Two forks is the budget.** A fork is cheap off a warm prefix and it is
//!   not free, and the object's quality is not improved by a second
//!   non-answer. [`run_ask`] retries a mimicked ask exactly once, with the
//!   fork-local imperative said again and the failure named, and then records
//!   what happened. There is no third call; see [`FORK_BUDGET`].
//!
//! Precision is the endpoint, not recall. A short genuine answer that reuses
//! one of the ask's headings is an answer, and an answer that quotes one line
//! of the ask as evidence is an answer. Mimicry is only claimed when **every**
//! field is inert and at least one of them is the ask coming back -- an
//! all-decline answer is a decline, and an empty answer is silence. The
//! labelled corpus under `diet/capture/mimicry/corpus/` is where that
//! boundary is pinned; it is also the evidence about which asks invite an
//! echo, which is the other reason mimicry is harvested rather than only
//! suppressed.

use std::collections::BTreeMap;

use crate::formats::interview::{self, Answer, Field, Outcome};
use crate::formats::record::json::Value;
use crate::object::{EntryId, ObjectError, Patch, Provenance};

// ---------------------------------------------------------------------------
// placeholders
// ---------------------------------------------------------------------------

/// A shape a template leaves for an answer to fill in.
///
/// A closed vocabulary rather than a regex per call site, for the reason every
/// vocabulary in this crate is one: a shape added here is a shape every
/// exhaustive match has to account for, and the coverage test iterates
/// [`Placeholder::ALL`] rather than a list somebody kept in step by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Placeholder {
    /// `<what you decided>`.
    Angle,
    /// `[one sentence]`.
    Square,
    /// `{one sentence}`.
    Brace,
    /// `…`, or the three dots an emitter types instead.
    Ellipsis,
    /// `TBD`.
    ToBeDecided,
    /// `TODO`.
    ToDo,
    /// `(your answer here)` -- the form-shaped slot, which reads as prose and
    /// is not.
    FillIn,
}

impl Placeholder {
    /// Every shape, in declaration order.
    ///
    /// This is the table a value is tried against, and the coverage test
    /// requires a sample for each entry: a shape with no sample is a shape
    /// nothing exercises, and an empty table would let every template slot
    /// through as content.
    pub const ALL: &'static [Self] = &[
        Self::Angle,
        Self::Square,
        Self::Brace,
        Self::Ellipsis,
        Self::ToBeDecided,
        Self::ToDo,
        Self::FillIn,
    ];

    /// A stable name, for records and fixtures.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Angle => "angle",
            Self::Square => "square",
            Self::Brace => "brace",
            Self::Ellipsis => "ellipsis",
            Self::ToBeDecided => "tbd",
            Self::ToDo => "todo",
            Self::FillIn => "fill-in",
        }
    }

    /// The shape `text` is, if it is one.
    ///
    /// The whole value has to be the placeholder. A sentence that mentions a
    /// slot in passing is content, and reading it as a slot would cost the
    /// precision this detector exists to keep.
    #[must_use]
    pub fn of(text: &str) -> Option<Self> {
        let body = text.trim();
        Self::ALL.iter().copied().find(|shape| shape.is(body))
    }

    /// Whether `body`, already trimmed, is this shape.
    fn is(self, body: &str) -> bool {
        match self {
            Self::Angle => wrapped(body, '<', '>').is_some(),
            Self::Square => wrapped(body, '[', ']').is_some(),
            Self::Brace => wrapped(body, '{', '}').is_some(),
            Self::Ellipsis => {
                let bare = unpunctuated(body);
                bare == "\u{2026}" || bare == "..."
            }
            Self::ToBeDecided => unpunctuated(body).eq_ignore_ascii_case("tbd"),
            Self::ToDo => unpunctuated(body).eq_ignore_ascii_case("todo"),
            Self::FillIn => wrapped(body, '(', ')').is_some_and(is_fill_in),
        }
    }
}

/// What `body` holds, when it opens with `open`, closes with `close` and holds
/// no second `close` in between.
///
/// The last condition is what keeps `<a> and <b>` -- a sentence about two
/// slots -- from reading as one slot.
fn wrapped(body: &str, open: char, close: char) -> Option<&str> {
    let inner = body.strip_prefix(open)?.strip_suffix(close)?;
    if inner.contains(close) {
        return None;
    }
    Some(inner)
}

/// Whether a parenthesised span is the form-shaped slot.
///
/// Narrow on purpose: `(your earlier note was right)` is a parenthetical
/// answer, and only the `your … here` frame is a slot.
fn is_fill_in(inner: &str) -> bool {
    let lowered = inner.trim().to_lowercase();
    lowered.starts_with("your") && lowered.ends_with("here")
}

/// `body` without the punctuation an emitter puts after a bare marker.
fn unpunctuated(body: &str) -> &str {
    body.trim_end_matches(['.', ':', ';', ',', '!', '?'])
        .trim_end()
}

// ---------------------------------------------------------------------------
// the detector
// ---------------------------------------------------------------------------

/// Why a field's value carries no information.
///
/// Named rather than boolean because the cases mean different things about the
/// emitter: a decline is a model saying there is nothing, and an echo is a
/// model saying nothing while looking like it said something. Only the second
/// is mimicry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inert {
    /// No text, or none that carries a letter or a digit.
    Empty,
    /// A decline, as classified by [`crate::formats::decline`] -- the one
    /// authorized implementation of that family.
    Declined,
    /// A template slot handed back unfilled.
    Placeholder(Placeholder),
    /// Text the ask already contained.
    Echo,
}

impl Inert {
    /// Whether this is the ask coming back, rather than silence.
    ///
    /// The distinction decides the verdict: an answer whose every field is
    /// silence has declined, and an answer whose every field is inert with at
    /// least one of them the ask returning is mimicry.
    #[must_use]
    pub fn is_echoed(self) -> bool {
        matches!(self, Self::Placeholder(_) | Self::Echo)
    }
}

/// How much of an answer was its own ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Echo {
    /// How many of the answer's fields came back as the ask: an echo of its
    /// text, or a template slot left unfilled. This is the positive evidence.
    /// An empty field and a decline are inert too, and neither counts here.
    pub echoed: u64,
    /// How many fields the answer carried in all.
    pub of: u64,
}

/// What an answer is, with respect to the ask it answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classification {
    /// The answer is the ask. Never content.
    Mimicry(Echo),
    /// The answer says something. A decline says something -- that there is
    /// nothing to record -- and so does an empty emission, which is silence
    /// and is the ablation's collapse case rather than mimicry.
    Answer,
}

impl Classification {
    /// Whether this answer mimicked its ask.
    #[must_use]
    pub fn is_mimicry(self) -> bool {
        matches!(self, Self::Mimicry(_))
    }

    /// The label a corpus case would carry for this classification.
    #[must_use]
    pub fn label(self) -> Label {
        match self {
            Self::Mimicry(_) => Label::Mimicry,
            Self::Answer => Label::Answer,
        }
    }
}

/// What a harvested case is labelled.
///
/// The harvest is the point of detecting mimicry at all: a mimicked ask is a
/// clean signal that the ask is badly shaped, and the rate per template is a
/// profile cell. A label is written down, so it is a closed vocabulary with a
/// lookup that iterates [`Label::ALL`], never a string compared in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Label {
    /// The answer mimicked the ask.
    Mimicry,
    /// The answer answered, declined, or said nothing at all.
    Answer,
}

impl Label {
    /// Every label, in declaration order.
    pub const ALL: &'static [Self] = &[Self::Mimicry, Self::Answer];

    /// The word this label is written under.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Mimicry => "mimicry",
            Self::Answer => "answer",
        }
    }

    /// The label `written` names, if there is one.
    #[must_use]
    pub fn from_written(written: &str) -> Option<Self> {
        let word = written.trim();
        Self::ALL.iter().copied().find(|label| label.tag() == word)
    }
}

/// Classify `answer` against the `ask` it answers.
///
/// Mimicry iff every field is inert and at least one of them is the ask coming
/// back. Both halves are load-bearing. Without the first, an answer that
/// quotes one line of its ask as evidence and then says something is condemned
/// for the quote. Without the second, an all-decline answer -- which is a
/// legitimate outcome the decline format exists to keep -- is condemned for
/// having declined.
#[must_use]
pub fn classify(ask: &str, answer: &Answer) -> Classification {
    let asked = normalise(ask);
    let mut echoed: u64 = 0;
    let mut of: u64 = 0;
    let mut every_field_inert = true;
    for field in &answer.fields {
        of += 1;
        match inertness(field, &asked) {
            Some(inert) if inert.is_echoed() => echoed += 1,
            Some(_) => {}
            None => every_field_inert = false,
        }
    }
    if of > 0 && every_field_inert && echoed > 0 {
        Classification::Mimicry(Echo { echoed, of })
    } else {
        Classification::Answer
    }
}

/// Why this field carries no information, or `None` when it does.
///
/// `asked` is the already-normalised ask, so an answer with many fields
/// normalises the ask once.
fn inertness(field: &Field, asked: &str) -> Option<Inert> {
    let text = match &field.outcome {
        Outcome::Decline(_) => return Some(Inert::Declined),
        Outcome::Value(value) => value.as_str(),
        // Prose that named no field, and a tag announced with no value: both
        // are text the emitter produced, and both can be the ask returning.
        Outcome::Unparseable => field.raw.as_str(),
    };
    if let Some(shape) = Placeholder::of(text) {
        return Some(Inert::Placeholder(shape));
    }
    let said = normalise(text);
    if said.is_empty() {
        return Some(Inert::Empty);
    }
    if echoes(asked, &said) {
        return Some(Inert::Echo);
    }
    None
}

/// `text` with case, punctuation and whitespace taken out of the comparison.
///
/// Lower case, single-spaced, and only letters and digits survive. An emitter
/// that hands the template back with the bullets changed and the emphasis
/// removed has still handed the template back; matching the bytes would miss
/// exactly the cosmetic-change case this module is named for.
fn normalise(text: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for character in text.chars() {
        if character.is_alphanumeric() {
            if gap && !out.is_empty() {
                out.push(' ');
            }
            gap = false;
            out.extend(character.to_lowercase());
        } else {
            gap = true;
        }
    }
    out
}

/// Whether `said` appears in `asked` as whole words.
///
/// Both are normalised text: lower-case words joined by single spaces, so
/// padding each with a space is a word boundary. A plain substring test reads
/// `plan` out of `planning` and condemns a genuine answer for a syllable, and
/// precision is what this detector is for.
fn echoes(asked: &str, said: &str) -> bool {
    if said.is_empty() {
        return false;
    }
    format!(" {asked} ").contains(&format!(" {said} "))
}

// ---------------------------------------------------------------------------
// the retry policy
// ---------------------------------------------------------------------------

/// How many forks one ask may spend.
///
/// Two. A fork is cheap off a warm prefix but it is not free, and the object
/// is not improved by a second non-answer, so a mimicked ask is retried once
/// and then recorded. A third call is a different design and this constant is
/// where it would have to be argued for, rather than in a loop condition
/// somebody widened.
pub const FORK_BUDGET: u32 = 2;

/// The sentence a retry adds.
///
/// A rendered ask already carries its fork-local imperative, so the retry says
/// that sentence again and names the failure. A model that returned the
/// question does not need a different question; it needs to be told that
/// returning the question is not an answer.
const STRENGTHENED: &str = concat!(
    "Answer from this turn alone. Do not repeat the question: handing the ",
    "headings back is not an answer, and declining is better than filling ",
    "the slots with the slot names."
);

/// The ask as it is put the second time.
///
/// Public because a caller that renders its own asks needs to be able to see
/// what the retry will say, and because a class-tuned variant substituted here
/// is a change with an owner rather than a string built at a call site.
#[must_use]
pub fn strengthen(ask: &str) -> String {
    format!("{}\n\n{STRENGTHENED}", ask.trim_end())
}

/// What one ask came to.
///
/// Every case is named. An outcome that could be read as "no answer, probably"
/// is how a non-answer became an entry the first time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskOutcome {
    /// The ask was answered -- with content, or with a decline, which is an
    /// answer about there being nothing.
    Answered {
        /// How many forks it took: one, or two when the first was mimicry.
        forks: u32,
        /// What came back.
        answer: Box<Answer>,
    },
    /// Mimicry, twice. Recorded, not retried again.
    Mimicked {
        /// Always [`FORK_BUDGET`]: the first fork mimicked and so did the
        /// retry.
        forks: u32,
        /// How much of the second answer was its ask.
        echo: Echo,
        /// The second answer, kept. Mimicry is evidence about the ask, and the
        /// harvest needs the text that came back.
        answer: Box<Answer>,
    },
    /// The emission was not an answer at all -- a NUL byte, which is where a
    /// byte stream ends rather than something an emitter writes. Not retried:
    /// a damaged transport is not a badly shaped ask.
    Unreadable {
        /// How many forks had been spent when it arrived.
        forks: u32,
        /// What came back, kept rather than dropped.
        raw: String,
    },
}

impl AskOutcome {
    /// The answer that came back, when what came back was an answer.
    #[must_use]
    pub fn answer(&self) -> Option<&Answer> {
        match self {
            Self::Answered { answer, .. } | Self::Mimicked { answer, .. } => Some(answer),
            Self::Unreadable { .. } => None,
        }
    }

    /// How many forks this ask spent.
    #[must_use]
    pub fn forks(&self) -> u32 {
        match self {
            Self::Answered { forks, .. }
            | Self::Mimicked { forks, .. }
            | Self::Unreadable { forks, .. } => *forks,
        }
    }

    /// Whether what came back may become content in the working object.
    ///
    /// Only an answer may. This is the single place mimicry is kept out of the
    /// object: [`Self::patches`] asks here and nowhere else, and no field-level
    /// second opinion is taken, so the rule can be seen to fail.
    #[must_use]
    pub fn is_content(&self) -> bool {
        matches!(self, Self::Answered { .. })
    }

    /// The patches this outcome contributes to the working object.
    ///
    /// Ids are derived from `event` -- the identifier of the record event the
    /// ask belongs to -- and the field's own canonical tag, in the shape
    /// `<event id>/<field>`. Nothing is minted from a counter.
    ///
    /// `at` is where this ask's first patch sits in its lane's emission for the
    /// turn; the fields after it take the positions following. A position is
    /// not an identifier -- it is what orders two forks that finished in
    /// whatever order they finished -- and the object refuses a turn in which
    /// one lane emitted twice at the same one.
    ///
    /// The inertness of a single field is deliberately not consulted here.
    /// Mimicry is decided once, for the whole answer, by [`classify`], and
    /// [`Self::is_content`] is the one place that decision reaches the object.
    /// A second, field-level rule would cover the first, and two rules that
    /// each cover the other cannot both be proven red.
    ///
    /// This is not the groundedness gate and does not stand in for one. That
    /// gate binds per lane against the lane's contract input, and an ask put to
    /// a fork carries no contract input here; a lane driver that holds one runs
    /// [`crate::capture::grounded::check`] over what this hands back. Keeping
    /// the ask out of the object and checking that an answer's content was in
    /// what the model saw are different failures with different repairs, and
    /// collapsing them would leave neither able to fail on its own.
    ///
    /// # Errors
    ///
    /// Returns [`ObjectError::EmptyId`] when `event` is blank, because an
    /// entry whose provenance cannot be named is worse than no entry.
    pub fn patches(&self, event: &str, at: &Provenance) -> Result<Vec<Patch>, ObjectError> {
        if !self.is_content() {
            return Ok(Vec::new());
        }
        let Some(answer) = self.answer() else {
            return Ok(Vec::new());
        };
        let mut patches: Vec<Patch> = Vec::new();
        for field in &answer.fields {
            let (Some(tag), Outcome::Value(content)) = (field.tag.as_ref(), &field.outcome) else {
                continue;
            };
            let mut provenance = at.clone();
            provenance.index = at
                .index
                .saturating_add(u32::try_from(patches.len()).unwrap_or(u32::MAX));
            patches.push(Patch::Add {
                id: EntryId::new(&format!("{event}/{}", tag.kind.canonical_tag()))?,
                content: content.clone(),
                provenance,
            });
        }
        Ok(patches)
    }
}

/// Put `ask` to `respond`, retrying once if the answer mimics the ask.
///
/// `respond` is called with the exact text sent, so the second call receives
/// the strengthened ask rather than the original -- and the retry is judged
/// against what it was actually asked. It is called at most [`FORK_BUDGET`]
/// times, whatever comes back.
pub fn run_ask(ask: &str, respond: &mut dyn FnMut(&str) -> String) -> AskOutcome {
    let mut sent = ask.to_owned();
    let mut forks = 0;
    loop {
        forks += 1;
        let reply = respond(&sent);
        let Ok(answer) = interview::parse(&reply) else {
            return AskOutcome::Unreadable { forks, raw: reply };
        };
        let Classification::Mimicry(echo) = classify(&sent, &answer) else {
            return AskOutcome::Answered {
                forks,
                answer: Box::new(answer),
            };
        };
        if forks >= FORK_BUDGET {
            return AskOutcome::Mimicked {
                forks,
                echo,
                answer: Box::new(answer),
            };
        }
        sent = strengthen(ask);
    }
}

// ---------------------------------------------------------------------------
// the census hook
// ---------------------------------------------------------------------------

/// What a run of asks came to, counted.
///
/// One of these per ask kind is what a drive census carries: the rate at which
/// a template is mimicked is the evidence for reshaping it, and a rate nobody
/// counted is an opinion. The counters are outcomes rather than forks, because
/// two forks on one ask is one non-answer and not two.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    /// How many asks were put.
    pub asked: u64,
    /// How many came back with an answer, first fork or second.
    pub answered: u64,
    /// How many spent a second fork, for any reason.
    pub retried: u64,
    /// How many mimicked twice and were recorded.
    pub mimicked: u64,
    /// How many came back as something that was not an answer at all.
    pub unreadable: u64,
}

impl Tally {
    /// Count one outcome.
    pub fn observe(&mut self, outcome: &AskOutcome) {
        self.asked += 1;
        if outcome.forks() > 1 {
            self.retried += 1;
        }
        match outcome {
            AskOutcome::Answered { .. } => self.answered += 1,
            AskOutcome::Mimicked { .. } => self.mimicked += 1,
            AskOutcome::Unreadable { .. } => self.unreadable += 1,
        }
    }

    /// The tally as a record value, for the drive census.
    #[must_use]
    pub fn value(&self) -> Value {
        let mut members = BTreeMap::new();
        members.insert("asked".to_owned(), count(self.asked));
        members.insert("answered".to_owned(), count(self.answered));
        members.insert("retried".to_owned(), count(self.retried));
        members.insert("mimicked".to_owned(), count(self.mimicked));
        members.insert("unreadable".to_owned(), count(self.unreadable));
        Value::Object(members)
    }
}

/// A count as a record integer.
///
/// A record holds no floats, and a count that does not fit an `i64` is a count
/// from a run that went wrong in a larger way than this saturation.
fn count(value: u64) -> Value {
    Value::Integer(i64::try_from(value).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use super::{
        AskOutcome, Classification, Echo, FORK_BUDGET, Inert, Label, Placeholder, Tally, classify,
        run_ask, strengthen,
    };
    use crate::formats::interview::{self, Answer};
    use crate::formats::record;
    use crate::object::{Provenance, WorkingObject};

    /// One sample per placeholder shape.
    ///
    /// The coverage test requires an entry for every member of
    /// [`Placeholder::ALL`] and requires each entry to be recognised, so an
    /// emptied table fails on the samples rather than passing over nothing.
    const PLACEHOLDER_SAMPLES: &[(Placeholder, &str)] = &[
        (Placeholder::Angle, "<what you decided>"),
        (Placeholder::Square, "[one sentence]"),
        (Placeholder::Brace, "{one sentence}"),
        (Placeholder::Ellipsis, "\u{2026}"),
        (Placeholder::ToBeDecided, "TBD"),
        (Placeholder::ToDo, "TODO"),
        (Placeholder::FillIn, "(your answer here)"),
    ];

    fn corpus_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("capture")
            .join("mimicry")
            .join("corpus")
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
    }

    /// Every case in the corpus, by its stem.
    ///
    /// Non-empty and fully paired: a corpus that walks zero files, or one file
    /// of a pair, makes every assertion over it hold for nothing.
    fn cases() -> Vec<String> {
        let dir = corpus_dir();
        let mut stems: Vec<String> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap_or_else(|err| panic!("{}: {err}", dir.display()))
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        for path in &entries {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_else(|| panic!("{}: a name that is not text", path.display()))
                .to_owned();
            seen.insert(name.clone());
            if let Some(stem) = name.strip_suffix(".ask.txt") {
                stems.push(stem.to_owned());
            }
        }
        assert!(
            !stems.is_empty(),
            "{}: no cases, so every assertion over this corpus would hold vacuously",
            dir.display()
        );
        for stem in &stems {
            for suffix in [".ask.txt", ".answer.txt", ".verdict"] {
                let wanted = format!("{stem}{suffix}");
                assert!(
                    seen.contains(&wanted),
                    "{}: the case `{stem}` has no `{wanted}`, so it is graded against nothing",
                    dir.display()
                );
            }
        }
        let paired: BTreeSet<String> = stems
            .iter()
            .flat_map(|stem| {
                [".ask.txt", ".answer.txt", ".verdict"].map(|suffix| format!("{stem}{suffix}"))
            })
            .collect();
        for name in &seen {
            assert!(
                paired.contains(name),
                "{}: `{name}` belongs to no case, so nothing reads it",
                dir.display()
            );
        }
        stems
    }

    /// The ask, the answer as parsed, and the recorded label for one case.
    fn case(stem: &str) -> (String, Answer, Label) {
        let dir = corpus_dir();
        let ask = read(&dir.join(format!("{stem}.ask.txt")));
        let source = read(&dir.join(format!("{stem}.answer.txt")));
        let answer = interview::parse(&source)
            .unwrap_or_else(|err| panic!("{stem}: the answer does not parse: {err}"));
        let written = read(&dir.join(format!("{stem}.verdict")));
        let label = Label::from_written(&written)
            .unwrap_or_else(|| panic!("{stem}: `{}` names no verdict", written.trim()));
        (ask, answer, label)
    }

    fn regime() -> record::Regime {
        let source = r#"{"record":"start","regime":{"arm":"baseline","dogma_version":0,"substrate":{"name":"local","model":"m","quantization":"q","sampler":{"seed":0},"reasoning":"on","hardware":"h"}}}"#;
        record::parse(source).expect("a record").regime().clone()
    }

    fn provenance() -> Provenance {
        Provenance {
            turn: 7,
            lane: "interview".to_owned(),
            fork: Some("ask".to_owned()),
            index: 0,
        }
    }

    #[test]
    fn the_corpus_holds_a_case_for_every_label() {
        let labels: BTreeSet<Label> = cases().iter().map(|stem| case(stem).2).collect();
        for label in Label::ALL {
            assert!(
                labels.contains(label),
                "no corpus case is labelled `{}`, so that verdict is never exercised",
                label.tag()
            );
        }
    }

    #[test]
    fn every_corpus_case_classifies_as_its_recorded_label() {
        let mut wrong = Vec::new();
        for stem in cases() {
            let (ask, answer, label) = case(&stem);
            let got = classify(&ask, &answer).label();
            if got != label {
                wrong.push(format!(
                    "{stem}: recorded `{}`, classified `{}`",
                    label.tag(),
                    got.tag()
                ));
            }
        }
        assert!(
            wrong.is_empty(),
            "the corpus disagrees with the detector on {} case(s): {}",
            wrong.len(),
            wrong.join("; ")
        );
    }

    #[test]
    fn every_placeholder_shape_is_recognised() {
        for (shape, sample) in PLACEHOLDER_SAMPLES {
            assert_eq!(
                Placeholder::of(sample),
                Some(*shape),
                "`{sample}` is not read as a placeholder, so a template slot \
                 handed back reads as content"
            );
        }
        for shape in Placeholder::ALL {
            assert!(
                PLACEHOLDER_SAMPLES
                    .iter()
                    .any(|(sampled, _)| sampled == shape),
                "the `{}` placeholder shape has no sample, so nothing exercises it",
                shape.tag()
            );
        }
    }

    #[test]
    fn an_answer_that_is_the_asks_headings_with_placeholders_is_mimicry() {
        let (ask, answer, _) = case("headings-with-placeholders");
        assert_eq!(
            classify(&ask, &answer),
            Classification::Mimicry(Echo { echoed: 3, of: 3 }),
            "an answer that is three of the ask's headings with their slots \
             still in them was not read as mimicry"
        );
    }

    #[test]
    fn a_short_genuine_answer_that_reuses_one_heading_is_not_mimicry() {
        let (ask, answer, _) = case("a-short-genuine-answer-reusing-one-heading");
        assert!(
            !classify(&ask, &answer).is_mimicry(),
            "a one-line answer under one of the ask's own headings was read as \
             mimicry, and a detector that condemns a heading condemns every \
             short answer"
        );
    }

    #[test]
    fn a_genuine_answer_that_quotes_one_line_of_its_ask_is_not_mimicry() {
        let (ask, answer, _) = case("a-genuine-answer-quoting-one-line-of-the-ask");
        assert!(
            !classify(&ask, &answer).is_mimicry(),
            "an answer with one echoed field and one field of its own was read \
             as mimicry: every field must be inert before the verdict is \
             mimicry, and the detector is precision-first"
        );
    }

    #[test]
    fn an_all_decline_answer_is_a_decline_and_not_mimicry() {
        let (ask, answer, _) = case("an-all-decline-answer");
        assert_eq!(
            classify(&ask, &answer),
            Classification::Answer,
            "an answer that declined every field was read as mimicry, which \
             makes declining indistinguishable from parroting"
        );
    }

    #[test]
    fn an_empty_answer_is_not_mimicry_because_nothing_was_echoed() {
        let (ask, answer, _) = case("an-empty-answer");
        assert!(answer.fields.is_empty(), "the empty case carries no field");
        assert_eq!(
            classify(&ask, &answer),
            Classification::Answer,
            "an emission of no characters was read as mimicry: silence echoes \
             nothing, and the two failures need different repairs"
        );
    }

    #[test]
    fn an_echo_is_matched_on_word_boundaries() {
        let answer = interview::parse("DECISION: plan\n").expect("an answer");
        assert!(
            !classify("What is your planning horizon for the resolver?", &answer).is_mimicry(),
            "a value matched inside a word of the ask, so a genuine answer is \
             condemned for a syllable"
        );
        assert!(
            classify("What is your plan for the resolver?", &answer).is_mimicry(),
            "a value that is a whole word of the ask was not read as an echo"
        );
    }

    #[test]
    fn a_decline_is_inert_without_being_an_echo() {
        assert!(!Inert::Declined.is_echoed());
        assert!(!Inert::Empty.is_echoed());
        assert!(Inert::Echo.is_echoed());
        assert!(Inert::Placeholder(Placeholder::Angle).is_echoed());
    }

    #[test]
    fn a_mimicked_ask_is_put_again_with_the_imperative_strengthened() {
        let (ask, _, _) = case("headings-with-placeholders");
        let mimicry = read(&corpus_dir().join("headings-with-placeholders.answer.txt"));
        let mut put = Vec::new();
        let mut respond = |sent: &str| {
            put.push(sent.to_owned());
            mimicry.clone()
        };
        let outcome = run_ask(&ask, &mut respond);
        assert!(
            matches!(outcome, AskOutcome::Mimicked { .. }),
            "two mimicked answers did not record a mimicked ask"
        );
        assert_eq!(
            put.len(),
            2,
            "the mimicked ask was put {} time(s) rather than twice",
            put.len()
        );
        assert_eq!(put[0], ask, "the first fork was not the ask as rendered");
        // Not only `put[1] == strengthen(&ask)`: that holds for a `strengthen`
        // that hands the ask straight back, and a retry that puts the same
        // question again spends the second fork on the text that already
        // produced mimicry.
        assert_ne!(
            put[1], put[0],
            "the retry put the same question a second time"
        );
        assert_eq!(
            put[1],
            strengthen(&ask),
            "the retry was not the strengthened ask"
        );
    }

    #[test]
    fn the_retry_is_judged_against_the_text_it_was_actually_sent() {
        let (ask, _, _) = case("field-labels-as-content");
        let mimicry = read(&corpus_dir().join("field-labels-as-content.answer.txt"));
        let mut calls = 0_u32;
        let mut respond = |sent: &str| {
            calls += 1;
            if calls == 1 {
                mimicry.clone()
            } else {
                // The strengthening sentence, handed back as the answer. It is
                // absent from the original ask and present in what was sent, so
                // an implementation that judges the retry against the first ask
                // reads this as content.
                format!("DECISION: {}\n", sent.lines().last().unwrap_or_default())
            }
        };
        let outcome = run_ask(&ask, &mut respond);
        assert!(
            matches!(outcome, AskOutcome::Mimicked { .. }),
            "an answer that handed the strengthened imperative back was read as \
             content: a retry is judged against the text it was sent"
        );
    }

    #[test]
    fn a_second_mimicry_ends_the_ask_and_never_spends_a_third_fork() {
        let (ask, _, _) = case("field-labels-as-content");
        let mimicry = read(&corpus_dir().join("field-labels-as-content.answer.txt"));
        let mut calls = 0_u32;
        let mut respond = |_: &str| {
            calls += 1;
            mimicry.clone()
        };
        let outcome = run_ask(&ask, &mut respond);
        // The literal two, not `FORK_BUDGET`: a test that reads the budget it
        // is checking agrees with any budget, including the one a later hand
        // widened. Two is the number this design licenses, and it is written
        // down twice on purpose.
        assert!(
            calls <= 2,
            "the ask was put {calls} times: a third fork fired on an ask \
             already mimicked twice, and two forks spent on one non-answer is \
             the budget"
        );
        assert_eq!(
            calls, 2,
            "the ask was put {calls} time(s) before it was recorded as mimicked"
        );
        assert_eq!(
            outcome.forks(),
            FORK_BUDGET,
            "the outcome does not report the forks it spent"
        );
    }

    #[test]
    fn an_answer_on_the_retry_is_an_answer() {
        let (ask, _, _) = case("headings-with-placeholders");
        let mimicry = read(&corpus_dir().join("headings-with-placeholders.answer.txt"));
        let genuine =
            read(&corpus_dir().join("a-short-genuine-answer-reusing-one-heading.answer.txt"));
        let mut calls = 0_u32;
        let mut respond = |_: &str| {
            calls += 1;
            if calls == 1 {
                mimicry.clone()
            } else {
                genuine.clone()
            }
        };
        let outcome = run_ask(&ask, &mut respond);
        assert!(
            matches!(outcome, AskOutcome::Answered { forks: 2, .. }),
            "an ask answered on its retry was not recorded as answered"
        );
    }

    #[test]
    fn an_emission_that_is_not_an_answer_is_neither_mimicry_nor_content() {
        let mut respond = |_: &str| "DECISION: \0".to_owned();
        let outcome = run_ask("What did this turn establish?", &mut respond);
        assert!(
            matches!(outcome, AskOutcome::Unreadable { forks: 1, .. }),
            "an emission carrying a NUL was not recorded as unreadable"
        );
        assert!(!outcome.is_content());
    }

    #[test]
    fn a_mimicked_ask_contributes_nothing_to_the_working_object() {
        let (ask, _, _) = case("headings-with-placeholders");
        let mimicry = read(&corpus_dir().join("headings-with-placeholders.answer.txt"));
        let mut respond = |_: &str| mimicry.clone();
        let outcome = run_ask(&ask, &mut respond);
        assert!(matches!(outcome, AskOutcome::Mimicked { .. }));

        let patches = outcome
            .patches("t7/ask", &provenance())
            .expect("an event id");
        let mut object = WorkingObject::open(regime());
        object.apply_turn(&patches).expect("a turn");
        let held: Vec<String> = object
            .entries()
            .map(|entry| format!("{}={}", entry.id, entry.content))
            .collect();
        assert!(
            held.is_empty(),
            "a mimicked ask reached the working object as content: {}",
            held.join(", ")
        );
    }

    #[test]
    fn an_answered_ask_reaches_the_object_under_ids_derived_from_its_event() {
        let stem = "a-genuine-answer-quoting-one-line-of-the-ask";
        let (ask, _, _) = case(stem);
        let genuine = read(&corpus_dir().join(format!("{stem}.answer.txt")));
        let mut respond = |_: &str| genuine.clone();
        let outcome = run_ask(&ask, &mut respond);

        let patches = outcome
            .patches("t7/ask", &provenance())
            .expect("an event id");
        let mut object = WorkingObject::open(regime());
        object.apply_turn(&patches).expect("a turn");
        let mut ids: Vec<String> = object.entries().map(|entry| entry.id.to_string()).collect();
        ids.sort();
        assert_eq!(
            ids,
            vec!["t7/ask/decision".to_owned(), "t7/ask/evidence".to_owned()],
            "an answered ask did not land every field under `<event id>/<field>`"
        );
    }

    #[test]
    fn a_tally_counts_outcomes_rather_than_forks() {
        let (mimicked_ask, _, _) = case("headings-with-placeholders");
        let mimicry = read(&corpus_dir().join("headings-with-placeholders.answer.txt"));
        let genuine =
            read(&corpus_dir().join("a-short-genuine-answer-reusing-one-heading.answer.txt"));

        let mut tally = Tally::default();
        let mut always = |_: &str| mimicry.clone();
        tally.observe(&run_ask(&mimicked_ask, &mut always));
        let mut once = |_: &str| genuine.clone();
        tally.observe(&run_ask(&mimicked_ask, &mut once));

        assert_eq!(tally.asked, 2);
        assert_eq!(tally.mimicked, 1, "one ask mimicked twice is one mimicry");
        assert_eq!(tally.answered, 1);
        assert_eq!(
            tally.retried, 1,
            "only the mimicked ask spent a second fork"
        );

        let mut rendered = String::new();
        crate::formats::record::json::render(&tally.value(), &mut rendered);
        assert_eq!(
            rendered, "{\"answered\":1,\"asked\":2,\"mimicked\":1,\"retried\":1,\"unreadable\":0}",
            "the census value is not the tally"
        );
    }
}
