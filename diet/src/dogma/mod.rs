//! The dogma: what this program asks a model, in what words, and at which
//! site.
//!
//! A *template* is the text of one ask, pinned byte for byte. The words are
//! the intellectual content of the program: change one and every result
//! produced before the change stops being comparable with every result
//! produced after it. So the templates are data files under
//! `diet/dogma/templates/`, embedded at compile time, and `diet/dogma/
//! MANIFEST.tsv` pins each by digest and length. The test that checks the
//! manifest fails on a single changed byte, at the developer's `cargo test`,
//! long before CI.
//!
//! Three things the types hold rather than a convention:
//!
//! * **The vocabulary is closed.** [`Template`] is an enum, so capture code
//!   names an ask as a variant and the compiler knows every ask there is. A
//!   template file without a manifest line, a manifest line without a
//!   variant, or a variant absent from [`Template::ALL`] each fail a test.
//! * **A hole is typed.** Where a template reads `{path}` the caller fills
//!   [`Hole::Path`], and [`Template::fill`] refuses a hole left empty, a hole
//!   the template does not have, and a hole filled twice. The prior harness
//!   filled holes by string replacement at each call site, and a template
//!   whose holes changed kept compiling while its asks went out with the
//!   braces still in them. The fill is one pass, so a value that happens to
//!   contain `{intent}` is not substituted a second time.
//! * **A sentence is a claim about its site.** Three fork templates say "the
//!   main session continues in parallel", which is true inside a capture fork
//!   and false on the compaction path. Every template declares its [`Site`],
//!   and a test holds that no compaction-site template carries the fork-local
//!   sentence.
//!
//! Changing a template is a dogma version bump, never an edit: bump
//! [`VERSION`], regenerate the manifest, and say why in the commit message. A
//! record's regime carries the version it ran under as `dogma_version`.
//!
//! This module is the interface between the seat that owns the dogma and the
//! capture code that reads it. Capture code takes a [`Template`] and asks it
//! for text; it never carries ask text of its own.

use std::error::Error;
use std::fmt;

pub mod vocabulary;

/// The version of the dogma this crate carries.
///
/// A record produced under these templates carries this number as its
/// regime's `dogma_version`. Two records with different versions were asked
/// different questions, and their numbers do not compare.
pub const VERSION: u32 = 0;

/// The digest of `diet/dogma/MANIFEST.tsv` that [`VERSION`] was declared
/// against.
///
/// A regenerated manifest is a changed dogma, and this constant is what makes
/// the version site the place that change has to be written down: the test
/// that recomputes it fails until both lines are edited together. Without it
/// the manifest could be regenerated after an edit and the version left at
/// what it was, which is a bump that never happened.
pub const MANIFEST_DIGEST: &str = "e93b1fbf63c31266";

/// The per-model operating points, as TOML text, exactly as pinned.
///
/// Transcribed measurements, with their receipts beside them; the tables are
/// the research program's byte for byte. Exposed as text rather than parsed
/// because this crate has no TOML reader beyond the regimen subset, and the
/// operating points use tables and floats that subset does not admit. The
/// consumer that parses it names its reader; the digest below is what it
/// checks first.
pub const OPERATING_POINTS: &str = include_str!("../../dogma/operating-points.toml");

/// The digest of [`OPERATING_POINTS`] that [`VERSION`] was declared against.
pub const OPERATING_POINTS_DIGEST: &str = "ae5e3496eb1976df";

/// The manifest, exactly as pinned: one line per template, `name`, digest
/// and byte length, tab-separated, after the comment lines.
///
/// Public so that anything reporting which dogma it ran under can print or
/// record the pin itself rather than a copy of it.
pub const MANIFEST: &str = include_str!("../../dogma/MANIFEST.tsv");

/// The sentence that closes a fork-local ask.
///
/// It tells a forked model to answer and stop, and it is true only in a
/// fork: the main session does continue in parallel there. On the compaction
/// path it would be a lie, which is why [`Site`] exists and why the test
/// `no_compaction_template_carries_the_fork_local_sentence` exists.
pub const FORK_LOCAL_SENTENCE: &str = "Answer only this question, then end your turn. \
     Do not run a command — the main session continues in parallel.";

/// Where a template's text is sent.
///
/// A template's sentences are claims about the context they run in, and a
/// claim true at one site is false at another. The site is declared per
/// template so that a check can hold the claim rather than a reviewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Site {
    /// A disposable, single-turn fork off the warm tail of the canonical
    /// session. The session continues in parallel.
    Fork,
    /// The control's summarise-and-replace turn. Nothing continues in
    /// parallel: the answer replaces the conversation.
    Compaction,
}

impl Site {
    /// Every site.
    pub const ALL: &'static [Self] = &[Self::Fork, Self::Compaction];

    /// A stable name, for records and fixtures.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Fork => "fork",
            Self::Compaction => "compaction",
        }
    }

    /// The site `name` names, if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|site| site.name() == name)
    }
}

impl fmt::Display for Site {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A hole in a template's text that the caller fills.
///
/// A hole is written `{tag}` in the template file, braces included; the
/// braces are part of the pinned bytes. Typed rather than named so that a
/// call site cannot fill a hole that is not there and cannot forget one that
/// is: the template declares its holes in [`Template::holes`], and a test
/// holds that declaration against the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Hole {
    /// The path of the file the model just read.
    Path,
    /// How deeply it read that file, in the harness's own words.
    Depth,
    /// What the model said it was about to do, quoted back.
    Intent,
    /// Which category of working note is being audited.
    Category,
    /// How many lines the audited list has, and so how many the answer must.
    LineCount,
    /// The numbered list of working notes to audit.
    Items,
    /// The material the ask is about: its name in one template, its full
    /// text in another. Which is which is the template's business.
    Source,
    /// The material's size, in characters, as a number.
    Size,
    /// An entry recorded earlier, for a supersession verdict.
    OldEntry,
    /// What the model said latest, for the same verdict.
    NewQuote,
}

impl Hole {
    /// Every hole any template has.
    pub const ALL: &'static [Self] = &[
        Self::Path,
        Self::Depth,
        Self::Intent,
        Self::Category,
        Self::LineCount,
        Self::Items,
        Self::Source,
        Self::Size,
        Self::OldEntry,
        Self::NewQuote,
    ];

    /// The tag written between the braces in a template file.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Depth => "depth",
            Self::Intent => "intent",
            Self::Category => "category",
            Self::LineCount => "n",
            Self::Items => "items",
            Self::Source => "source",
            Self::Size => "size",
            Self::OldEntry => "old_entry",
            Self::NewQuote => "new_quote",
        }
    }

    /// The hole `tag` names, if there is one.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|hole| hole.tag() == tag)
    }
}

/// One ask, pinned.
///
/// The variant is the name capture code uses; [`Template::name`] is the name
/// the manifest and the template file use. The two are paired here, once,
/// and a test holds each pairing against the files on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Template {
    /// After a source read: the exports the model will code against.
    ApiSurface,
    /// After reading project rules or guidance: the constraints it imposes.
    Doctrine,
    /// After reading a document: the load-bearing excerpt, verbatim.
    DocRead,
    /// Once per settled turn: the judgment the turn produced.
    TurnBoundary,
    /// After a large ingestion: what in it was load-bearing.
    Evidence,
    /// Span-grounded facts from material already in the session.
    Extract,
    /// The same extraction, with the material carried in the ask.
    ExtractMinimal,
    /// What behaved differently than expected this step.
    Gotchas,
    /// What was decided this step, and why.
    Decisions,
    /// What constraint was learned or committed to this step.
    Constraints,
    /// The invisible why: context a later session could not reconstruct.
    Rationale,
    /// What is open, and what comes next.
    OpenNext,
    /// Whether a later statement replaces an earlier record.
    Supersede,
    /// Audit the working notes at a phase boundary. Legacy wording, kept for
    /// replaying the archive that was recorded under it.
    AuditQ,
    /// Audit the working notes at a scheduled checkpoint, which says so.
    AuditQCadence,
    /// Audit the working notes at a boundary the operator declared.
    AuditQHuman,
    /// Audit rider: the goal must state the current mission.
    AuditGoalNote,
    /// Audit rider: an answered question is updated, not dropped.
    AuditOpenNote,
    /// The audit's last question: what important thing is missing.
    AddQ,
    /// The control's summarise-and-replace ask, deliberately plain.
    NativeGenerate,
}

impl Template {
    /// Every template, in declaration order.
    pub const ALL: &'static [Self] = &[
        Self::ApiSurface,
        Self::Doctrine,
        Self::DocRead,
        Self::TurnBoundary,
        Self::Evidence,
        Self::Extract,
        Self::ExtractMinimal,
        Self::Gotchas,
        Self::Decisions,
        Self::Constraints,
        Self::Rationale,
        Self::OpenNext,
        Self::Supersede,
        Self::AuditQ,
        Self::AuditQCadence,
        Self::AuditQHuman,
        Self::AuditGoalNote,
        Self::AuditOpenNote,
        Self::AddQ,
        Self::NativeGenerate,
    ];

    /// The name the manifest pins this template under, and the stem of its
    /// file under `diet/dogma/templates/`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::ApiSurface => "API_SURFACE",
            Self::Doctrine => "DOCTRINE",
            Self::DocRead => "DOC_READ",
            Self::TurnBoundary => "TURN_BOUNDARY",
            Self::Evidence => "EVIDENCE",
            Self::Extract => "EXTRACT",
            Self::ExtractMinimal => "EXTRACT_MINIMAL",
            Self::Gotchas => "GOTCHAS",
            Self::Decisions => "DECISIONS",
            Self::Constraints => "CONSTRAINTS",
            Self::Rationale => "RATIONALE",
            Self::OpenNext => "OPEN_NEXT",
            Self::Supersede => "SUPERSEDE",
            Self::AuditQ => "AUDIT_Q",
            Self::AuditQCadence => "AUDIT_Q_CADENCE",
            Self::AuditQHuman => "AUDIT_Q_HUMAN",
            Self::AuditGoalNote => "AUDIT_GOAL_NOTE",
            Self::AuditOpenNote => "AUDIT_OPEN_NOTE",
            Self::AddQ => "ADD_Q",
            Self::NativeGenerate => "NATIVE_GENERATE_PROMPT",
        }
    }

    /// The template `name` pins, if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|template| template.name() == name)
    }

    /// The text, exactly as pinned, holes unfilled.
    #[must_use]
    pub fn text(self) -> &'static str {
        match self {
            Self::ApiSurface => include_str!("../../dogma/templates/API_SURFACE.txt"),
            Self::Doctrine => include_str!("../../dogma/templates/DOCTRINE.txt"),
            Self::DocRead => include_str!("../../dogma/templates/DOC_READ.txt"),
            Self::TurnBoundary => include_str!("../../dogma/templates/TURN_BOUNDARY.txt"),
            Self::Evidence => include_str!("../../dogma/templates/EVIDENCE.txt"),
            Self::Extract => include_str!("../../dogma/templates/EXTRACT.txt"),
            Self::ExtractMinimal => include_str!("../../dogma/templates/EXTRACT_MINIMAL.txt"),
            Self::Gotchas => include_str!("../../dogma/templates/GOTCHAS.txt"),
            Self::Decisions => include_str!("../../dogma/templates/DECISIONS.txt"),
            Self::Constraints => include_str!("../../dogma/templates/CONSTRAINTS.txt"),
            Self::Rationale => include_str!("../../dogma/templates/RATIONALE.txt"),
            Self::OpenNext => include_str!("../../dogma/templates/OPEN_NEXT.txt"),
            Self::Supersede => include_str!("../../dogma/templates/SUPERSEDE.txt"),
            Self::AuditQ => include_str!("../../dogma/templates/AUDIT_Q.txt"),
            Self::AuditQCadence => include_str!("../../dogma/templates/AUDIT_Q_CADENCE.txt"),
            Self::AuditQHuman => include_str!("../../dogma/templates/AUDIT_Q_HUMAN.txt"),
            Self::AuditGoalNote => include_str!("../../dogma/templates/AUDIT_GOAL_NOTE.txt"),
            Self::AuditOpenNote => include_str!("../../dogma/templates/AUDIT_OPEN_NOTE.txt"),
            Self::AddQ => include_str!("../../dogma/templates/ADD_Q.txt"),
            Self::NativeGenerate => {
                include_str!("../../dogma/templates/NATIVE_GENERATE_PROMPT.txt")
            }
        }
    }

    /// Where this template's text is sent.
    #[must_use]
    pub fn site(self) -> Site {
        match self {
            Self::NativeGenerate => Site::Compaction,
            Self::ApiSurface
            | Self::Doctrine
            | Self::DocRead
            | Self::TurnBoundary
            | Self::Evidence
            | Self::Extract
            | Self::ExtractMinimal
            | Self::Gotchas
            | Self::Decisions
            | Self::Constraints
            | Self::Rationale
            | Self::OpenNext
            | Self::Supersede
            | Self::AuditQ
            | Self::AuditQCadence
            | Self::AuditQHuman
            | Self::AuditGoalNote
            | Self::AuditOpenNote
            | Self::AddQ => Site::Fork,
        }
    }

    /// The holes this template's text has, in the order they first appear.
    ///
    /// Declared here and held against the text by a test, so that a hole
    /// added to a file fails `cargo test` rather than reaching a caller; and
    /// [`Template::fill`] refuses such a hole on its own, so it does not reach
    /// the wire even when the tests were not run.
    #[must_use]
    pub fn holes(self) -> &'static [Hole] {
        match self {
            Self::ApiSurface => &[Hole::Path, Hole::Depth, Hole::Intent],
            Self::Doctrine | Self::DocRead => &[Hole::Path],
            Self::Evidence => &[Hole::Source, Hole::Size],
            Self::ExtractMinimal => &[Hole::Source],
            Self::Supersede => &[Hole::OldEntry, Hole::NewQuote],
            Self::AuditQ | Self::AuditQCadence | Self::AuditQHuman => {
                &[Hole::Category, Hole::LineCount, Hole::Items]
            }
            Self::TurnBoundary
            | Self::Extract
            | Self::Gotchas
            | Self::Decisions
            | Self::Constraints
            | Self::Rationale
            | Self::OpenNext
            | Self::AuditGoalNote
            | Self::AuditOpenNote
            | Self::AddQ
            | Self::NativeGenerate => &[],
        }
    }

    /// The text with every hole filled.
    ///
    /// One value per hole, every hole filled, no hole the template lacks.
    /// The substitution is a single left-to-right pass over the pinned text,
    /// so a value containing `{path}` stays as written rather than being
    /// filled in turn.
    ///
    /// # Errors
    ///
    /// Returns [`FillError::NotAHole`] for a value whose hole this template
    /// does not have, [`FillError::FilledTwice`] for a hole given two values,
    /// and [`FillError::Unfilled`] for a hole given none.
    pub fn fill(self, values: &[(Hole, &str)]) -> Result<String, FillError> {
        let declared = self.holes();
        let mut seen: Vec<Hole> = Vec::with_capacity(values.len());
        for (hole, _) in values {
            if !declared.contains(hole) {
                return Err(FillError::NotAHole {
                    template: self,
                    hole: *hole,
                });
            }
            if seen.contains(hole) {
                return Err(FillError::FilledTwice {
                    template: self,
                    hole: *hole,
                });
            }
            seen.push(*hole);
        }
        if let Some(hole) = declared.iter().find(|hole| !seen.contains(hole)) {
            return Err(FillError::Unfilled {
                template: self,
                hole: *hole,
            });
        }

        fill_text(self, self.text(), values)
    }
}

/// The single-pass substitution behind [`Template::fill`], over `text`.
///
/// Separate from the method so that the refusal of an undeclared hole can be
/// exercised on a text no pinned template is allowed to have.
fn fill_text(template: Template, text: &str, values: &[(Hole, &str)]) -> Result<String, FillError> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let hole = after
            .find('}')
            .and_then(|close| Hole::from_tag(&after[..close]).map(|hole| (hole, close)));
        if let Some((hole, close)) = hole {
            // A hole in the text the caller was given no value for is a hole
            // the declaration does not know about. Refused here as well as in
            // the holes test, so that a template edited without a run of the
            // tests cannot send its braces out on the wire.
            let Some((_, value)) = values.iter().find(|(it, _)| *it == hole) else {
                return Err(FillError::Unfilled { template, hole });
            };
            out.push_str(value);
            rest = &after[close + 1..];
        } else {
            // A brace that opens no hole is text. The holes test keeps this
            // branch off the pinned templates; it is here so the fill is
            // total rather than a panic on a brace.
            out.push('{');
            rest = after;
        }
    }
    out.push_str(rest);
    Ok(out)
}

impl fmt::Display for Template {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Why a template could not be filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillError {
    /// A value was given for a hole the template does not have.
    NotAHole {
        /// The template.
        template: Template,
        /// The hole it does not have.
        hole: Hole,
    },
    /// A hole was given two values.
    FilledTwice {
        /// The template.
        template: Template,
        /// The hole.
        hole: Hole,
    },
    /// A hole was given no value, or the text has a hole the declaration
    /// lacks. Refused rather than sent with the braces in it: an ask with
    /// `{intent}` still in it is an ask nobody wrote.
    Unfilled {
        /// The template.
        template: Template,
        /// The hole.
        hole: Hole,
    },
}

impl fmt::Display for FillError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAHole { template, hole } => {
                write!(f, "{template} has no `{{{}}}` hole", hole.tag())
            }
            Self::FilledTwice { template, hole } => {
                write!(f, "{template}'s `{{{}}}` was filled twice", hole.tag())
            }
            Self::Unfilled { template, hole } => {
                write!(f, "{template}'s `{{{}}}` was left unfilled", hole.tag())
            }
        }
    }
}

impl Error for FillError {}

/// The digest the manifest pins a template by: FNV-1a, 64-bit, as sixteen
/// lowercase hex digits.
///
/// Dependency-free and stable across builds, which `std`'s default hasher is
/// not. The same function the research archive's manifest used, so a line
/// here and a line there pin the same bytes by the same number.
#[must_use]
pub fn digest(text: &str) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::{
        FORK_LOCAL_SENTENCE, FillError, Hole, MANIFEST, MANIFEST_DIGEST, OPERATING_POINTS,
        OPERATING_POINTS_DIGEST, Site, Template, digest, fill_text,
    };
    use std::collections::BTreeSet;
    use std::path::Path;

    fn manifest() -> Vec<(String, String, usize)> {
        MANIFEST
            .lines()
            .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
            .map(|line| {
                let mut fields = line.split('\t');
                let name = fields.next().expect("a manifest line starts with a name");
                let hash = fields.next().expect("a manifest line carries a digest");
                let bytes = fields
                    .next()
                    .expect("a manifest line ends with a length")
                    .parse()
                    .expect("the length is a number");
                (name.to_owned(), hash.to_owned(), bytes)
            })
            .collect()
    }

    /// Every `{tag}` in `text`, in order, as the hole it names.
    fn holes_in(text: &str) -> Vec<Hole> {
        let mut found = Vec::new();
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            let after = &rest[open + 1..];
            let close = after
                .find('}')
                .unwrap_or_else(|| panic!("an unclosed brace in a template: {after:?}"));
            let tag = &after[..close];
            let hole = Hole::from_tag(tag)
                .unwrap_or_else(|| panic!("`{{{tag}}}` names no hole this crate knows"));
            if !found.contains(&hole) {
                found.push(hole);
            }
            rest = &after[close + 1..];
        }
        found
    }

    // The byte guard. A single changed byte in any template fails here, with
    // the remedy in the message.
    #[test]
    fn every_template_matches_the_manifest_and_nothing_else_does() {
        let pinned = manifest();
        assert_eq!(
            pinned.len(),
            Template::ALL.len(),
            "the manifest pins {} templates but Template::ALL has {}: a template was \
             added or removed without updating both",
            pinned.len(),
            Template::ALL.len()
        );
        // Both directions, as sets: the length check alone let a manifest
        // drop one template's line and duplicate another's, leaving the first
        // template's bytes pinned by nothing.
        let pinned_names: BTreeSet<&str> =
            pinned.iter().map(|(name, _, _)| name.as_str()).collect();
        let declared: BTreeSet<&str> = Template::ALL
            .iter()
            .map(|template| template.name())
            .collect();
        assert_eq!(
            pinned_names, declared,
            "the manifest and Template::ALL do not name the same templates"
        );
        for (name, hash, bytes) in &pinned {
            let template = Template::from_name(name).unwrap_or_else(|| {
                panic!("the manifest pins {name}, and no Template has that name")
            });
            let text = template.text();
            assert_eq!(
                (digest(text), text.len()),
                (hash.clone(), *bytes),
                "\n\ndiet/dogma/templates/{name}.txt has changed.\n\
                 This is the dogma. Changing it changes what every future fire asks the \
                 model, and makes results before and after non-comparable.\n\
                 If the edit was deliberate: bump dogma::VERSION, regenerate \
                 diet/dogma/MANIFEST.tsv, and say why in the commit message.\n\
                 If it was not: `git checkout diet/dogma/templates/{name}.txt`.\n"
            );
        }
    }

    // A file on disk that no variant embeds is dogma nobody can ask; a
    // variant whose file is missing does not compile, so only one direction
    // needs a test.
    #[test]
    fn every_template_file_on_disk_is_a_variant() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("dogma/templates");
        let mut on_disk = BTreeSet::new();
        for entry in std::fs::read_dir(&dir).expect("the templates directory exists") {
            let path = entry.expect("a readable entry").path();
            assert_eq!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("txt"),
                "{} is not a template file",
                path.display()
            );
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .expect("a UTF-8 file stem")
                .to_owned();
            on_disk.insert(stem);
        }
        let declared: BTreeSet<String> = Template::ALL
            .iter()
            .map(|template| template.name().to_owned())
            .collect();
        assert_eq!(
            on_disk, declared,
            "the templates on disk and Template::ALL disagree"
        );
    }

    #[test]
    fn names_round_trip_and_are_distinct() {
        let mut seen = BTreeSet::new();
        for template in Template::ALL {
            assert_eq!(Template::from_name(template.name()), Some(*template));
            assert!(seen.insert(template.name()), "{template} shares its name");
        }
        assert_eq!(Template::from_name("NOT_A_TEMPLATE"), None);
    }

    #[test]
    fn hole_tags_round_trip_and_are_distinct() {
        let mut seen = BTreeSet::new();
        for hole in Hole::ALL {
            assert_eq!(Hole::from_tag(hole.tag()), Some(*hole));
            assert!(seen.insert(hole.tag()), "{hole:?} shares its tag");
        }
        assert_eq!(Hole::from_tag("nope"), None);
    }

    // The declaration and the text agree, both ways: a hole in the text that
    // is not declared would go out as braces; a declared hole not in the text
    // would make fill demand a value nothing consumes.
    #[test]
    fn declared_holes_are_exactly_the_holes_in_the_text() {
        for template in Template::ALL {
            assert_eq!(
                holes_in(template.text()),
                template.holes().to_vec(),
                "{template}'s declared holes disagree with its text"
            );
        }
    }

    #[test]
    fn every_hole_is_used_by_some_template() {
        let used: BTreeSet<Hole> = Template::ALL
            .iter()
            .flat_map(|template| template.holes().iter().copied())
            .collect();
        for hole in Hole::ALL {
            assert!(used.contains(hole), "{hole:?} is a hole no template has");
        }
    }

    // A regenerated manifest is a changed dogma, and the change has to be
    // written down at the version site.
    #[test]
    fn the_version_was_declared_against_this_manifest() {
        assert_eq!(
            digest(MANIFEST),
            MANIFEST_DIGEST,
            "diet/dogma/MANIFEST.tsv changed and dogma::VERSION / MANIFEST_DIGEST did not: \
             a dogma version bump edits both, in one commit, with the reason"
        );
    }

    #[test]
    fn the_operating_points_are_pinned() {
        assert_eq!(
            digest(OPERATING_POINTS),
            OPERATING_POINTS_DIGEST,
            "diet/dogma/operating-points.toml changed and dogma::VERSION / \
             OPERATING_POINTS_DIGEST did not: a dogma version bump edits both"
        );
        // Every model table carries a sampler and a gate, and every listed
        // no-think operation is one of the three the program runs. Read
        // line by line, so it assumes the arrays stay on one line, which the
        // pinned file's do; the digest is the guard, this is the description.
        let mut models = 0;
        for line in OPERATING_POINTS.lines() {
            let line = line.trim();
            if line.starts_with('[') && !line.contains('.') {
                models += 1;
            }
            if let Some(rest) = line.strip_prefix("nothink_ops") {
                for op in rest.split(['[', ']', ',', '"', '=', ' ']) {
                    let op = op.trim();
                    assert!(
                        op.is_empty() || ["extraction", "judgment", "audit"].contains(&op),
                        "`{op}` is not an operation this program runs"
                    );
                }
            }
        }
        assert_eq!(models, 4, "four model families are pinned");
        for table in ["sampler", "gate"] {
            assert_eq!(
                OPERATING_POINTS.matches(&format!(".{table}]")).count(),
                4,
                "every model table carries a `{table}` table"
            );
        }
    }

    // The lane-scoped-sentence rule, as a check. The sentence is a claim that
    // the main session continues in parallel, which the compaction path
    // cannot make.
    #[test]
    fn no_compaction_template_carries_the_fork_local_sentence() {
        let carriers: Vec<Template> = Template::ALL
            .iter()
            .copied()
            .filter(|template| template.text().contains(FORK_LOCAL_SENTENCE))
            .collect();
        assert_eq!(
            carriers,
            vec![Template::ApiSurface, Template::Doctrine, Template::DocRead],
            "the fork-local sentence moved"
        );
        for template in Template::ALL {
            if template.site() == Site::Compaction {
                assert!(
                    !template.text().contains(FORK_LOCAL_SENTENCE),
                    "{template} is sent on the compaction path and claims the main \
                     session continues in parallel"
                );
            }
        }
    }

    #[test]
    fn every_site_is_used_and_named_distinctly() {
        let used: BTreeSet<Site> = Template::ALL.iter().map(|t| t.site()).collect();
        for site in Site::ALL {
            assert!(used.contains(site), "{site} is a site no template uses");
        }
        // And the other direction: a site a template names is in ALL, or the
        // loop above never examines it.
        for template in Template::ALL {
            assert!(
                Site::ALL.contains(&template.site()),
                "{template} is sent to {}, which Site::ALL does not list",
                template.site()
            );
        }
        let names: BTreeSet<&str> = Site::ALL.iter().map(|site| site.name()).collect();
        assert_eq!(names.len(), Site::ALL.len());
        for site in Site::ALL {
            assert_eq!(Site::from_name(site.name()), Some(*site));
        }
        assert_eq!(Site::from_name("seam"), None);
    }

    #[test]
    fn fill_substitutes_every_hole_once() {
        let filled = Template::ApiSurface
            .fill(&[
                (Hole::Path, "src/lib.rs"),
                (Hole::Depth, "whole file"),
                (Hole::Intent, "read the exports"),
            ])
            .expect("every hole given");
        assert!(filled.starts_with(
            "Pause. You just read src/lib.rs (whole file). Before it you said: 'read the exports'."
        ));
        assert!(!filled.contains('{'), "a brace survived the fill: {filled}");
    }

    #[test]
    fn fill_of_a_template_with_no_holes_is_its_text() {
        assert_eq!(
            Template::TurnBoundary.fill(&[]).expect("nothing to fill"),
            Template::TurnBoundary.text()
        );
    }

    // A value is text, not a template. The prior harness chained one
    // `replace` per hole, so a value carrying `{intent}` was filled by the
    // next replace in the chain.
    #[test]
    fn a_value_containing_a_hole_shape_is_not_filled_in_turn() {
        // Each value carries the OTHER hole's tag, so a chained replace in
        // either order fills one of them a second time; only a single pass
        // leaves both as written.
        let filled = Template::Supersede
            .fill(&[
                (Hole::OldEntry, "see {new_quote}"),
                (Hole::NewQuote, "see {old_entry}"),
            ])
            .expect("every hole given");
        assert!(filled.contains("RECORDED: see {new_quote}\n"));
        assert!(filled.contains("LATEST: \"see {old_entry}\"\n"));
        assert_eq!(filled.matches("{new_quote}").count(), 1);
        assert_eq!(filled.matches("{old_entry}").count(), 1);
    }

    // The wire-side half of the holes rule: a hole the text has and the
    // declaration lacks is refused by fill itself, not only by the test.
    #[test]
    fn fill_refuses_a_hole_the_text_has_but_the_declaration_lacks() {
        // No pinned template has this shape (the holes test forbids it), so
        // the scan is driven on a text of the test's own.
        let result = fill_text(
            Template::Doctrine,
            "read {path} at {depth}",
            &[(Hole::Path, "a")],
        );
        assert_eq!(
            result,
            Err(FillError::Unfilled {
                template: Template::Doctrine,
                hole: Hole::Depth
            })
        );
        assert_eq!(
            fill_text(
                Template::Doctrine,
                "read {path} {not_a_hole}",
                &[(Hole::Path, "a")]
            ),
            Ok("read a {not_a_hole}".to_owned())
        );
    }

    #[test]
    fn fill_refuses_a_hole_left_unfilled() {
        assert_eq!(
            Template::ApiSurface.fill(&[(Hole::Path, "a"), (Hole::Depth, "b")]),
            Err(FillError::Unfilled {
                template: Template::ApiSurface,
                hole: Hole::Intent
            })
        );
    }

    #[test]
    fn fill_refuses_a_hole_the_template_lacks() {
        assert_eq!(
            Template::Doctrine.fill(&[(Hole::Path, "a"), (Hole::Size, "12")]),
            Err(FillError::NotAHole {
                template: Template::Doctrine,
                hole: Hole::Size
            })
        );
        assert_eq!(
            Template::NativeGenerate.fill(&[(Hole::Path, "a")]),
            Err(FillError::NotAHole {
                template: Template::NativeGenerate,
                hole: Hole::Path
            })
        );
    }

    #[test]
    fn fill_refuses_a_hole_filled_twice() {
        assert_eq!(
            Template::Doctrine.fill(&[(Hole::Path, "a"), (Hole::Path, "b")]),
            Err(FillError::FilledTwice {
                template: Template::Doctrine,
                hole: Hole::Path
            })
        );
    }

    #[test]
    fn fill_errors_name_the_template_and_the_hole() {
        let err = Template::Evidence
            .fill(&[(Hole::Source, "notes")])
            .expect_err("size unfilled");
        assert_eq!(err.to_string(), "EVIDENCE's `{size}` was left unfilled");
    }

    #[test]
    fn the_digest_is_fnv1a_64() {
        // The published test vectors for FNV-1a 64.
        assert_eq!(digest(""), "cbf29ce484222325");
        assert_eq!(digest("a"), "af63dc4c8601ec8c");
        assert_eq!(digest("foobar"), "85944171f73967e8");
    }
}
