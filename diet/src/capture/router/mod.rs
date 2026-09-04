//! Interview routing by tool-output class.
//!
//! The naive capture design interviewed the model after every tool call, with
//! the same questions each time. Measured on real drives, that was wasteful in
//! a specific, structured way: **53% of steps were directory listings or
//! similarly inert tool outputs** that never produced a capture-worthy fact,
//! and the interviews that did matter -- decisions, gotchas, plan changes --
//! were asked at the wrong moment, mid-exploration, before the model had
//! concluded anything. The one-size interview cost roughly thirty-six forks
//! per drive; class-aware routing projected fifteen to eighteen for the same
//! coverage.
//!
//! Routing is decided by code that looks at the tool call, never by asking
//! the model whether this step matters. Models are poor at recognising their
//! own knowledge gaps in the moment, which is the whole reason capture exists.
//!
//! Five requirements, from the first adversarial review of this design:
//!
//! * A call matching no pattern routes to a **declared default** -- a fork
//!   with the generic ask -- and never to silence. Silence is a class, not
//!   an absence from the table.
//! * The classification table is **data** (`classes.tsv`), with a conformance
//!   corpus of real tool calls under `diet/capture/router/corpus/`.
//! * A call the table could not place is a **typed event**
//!   ([`Unclassified`]), so misrouting is a number rather than a suspicion.
//! * **Judgment asks fire at turn boundaries only.** A question that needs
//!   the model to have concluded something is asked once, when the canonical
//!   turn ends, not after every call in the middle of it.
//! * The router's **census** -- which classes fired, how many forks, against
//!   how many the naive design would have spent -- is emitted per drive.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use crate::capture::mechanical::{self, Facts};
use crate::formats::record::json::{Decimal, Value};
use crate::formats::record::{Event, Record};
use crate::formats::shell::{self, Command, List, Simple};

// ---------------------------------------------------------------------------
// vocabulary
// ---------------------------------------------------------------------------

/// What a tool call was for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    /// A directory listing. Inert: nothing it returns is worth a fork.
    DirectoryListing,
    /// A probe of the environment -- `pwd`, `wc`, `nproc`. Inert.
    Inert,
    /// A document was read: prose, whose facts a later turn may need.
    DocumentRead,
    /// Source was read: names and their gotchas.
    SourceRead,
    /// Something was fetched from the network.
    WebRead,
    /// A search. Its result is a location, not a fact; the read that follows
    /// is what carries the fact.
    Search,
    /// A file was changed.
    Edit,
    /// A command with side effects the model chose to have.
    SideEffect,
    /// A test run: an outcome the model was waiting for.
    TestRun,
    /// A build, check, lint or format run.
    Build,
    /// Version control.
    VersionControl,
    /// Nothing in the table matched. Routed to the declared default.
    Unknown,
}

impl Class {
    /// Every class, in declaration order.
    pub const ALL: &'static [Self] = &[
        Self::DirectoryListing,
        Self::Inert,
        Self::DocumentRead,
        Self::SourceRead,
        Self::WebRead,
        Self::Search,
        Self::Edit,
        Self::SideEffect,
        Self::TestRun,
        Self::Build,
        Self::VersionControl,
        Self::Unknown,
    ];

    /// The spelling the table and the corpus use.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::DirectoryListing => "directory-listing",
            Self::Inert => "inert",
            Self::DocumentRead => "document-read",
            Self::SourceRead => "source-read",
            Self::WebRead => "web-read",
            Self::Search => "search",
            Self::Edit => "edit",
            Self::SideEffect => "side-effect",
            Self::TestRun => "test-run",
            Self::Build => "build",
            Self::VersionControl => "version-control",
            Self::Unknown => "unknown",
        }
    }

    /// The class a tag names, by iterating [`Self::ALL`]: the one place a
    /// string becomes a class.
    #[must_use]
    pub fn from_tag(text: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|class| class.tag() == text)
    }

    /// What the router does with a call of this class.
    ///
    /// By class alone in v0. An exit-aware refinement -- a failing build
    /// asks, a passing one does not -- is a later version of this table, not
    /// a special case inside it.
    #[must_use]
    pub fn routing(self) -> Routing {
        match self {
            Self::DirectoryListing
            | Self::Inert
            | Self::Search
            | Self::Build
            | Self::VersionControl => Routing::Silent,
            Self::DocumentRead | Self::WebRead => Routing::Fork(AskKind::Finding),
            Self::SourceRead => Routing::Fork(AskKind::ApiSurface),
            Self::Edit => Routing::Fork(AskKind::Change),
            Self::TestRun => Routing::Fork(AskKind::Outcome),
            Self::SideEffect => Routing::Defer,
            // The declared default. Not silence: a pattern nobody wrote a
            // row for is exactly the call nobody has looked at yet.
            Self::Unknown => Routing::Fork(AskKind::Generic),
        }
    }
}

/// What the router decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Routing {
    /// No fork.
    Silent,
    /// Fork now, with this class-tuned ask.
    Fork(AskKind),
    /// Nothing now; the judgment ask at the turn boundary covers it.
    Defer,
}

impl Routing {
    /// The spelling the corpus uses: `silent`, `defer`, `fork:<ask>`.
    #[must_use]
    pub fn tag(self) -> String {
        match self {
            Self::Silent => "silent".to_owned(),
            Self::Defer => "defer".to_owned(),
            Self::Fork(kind) => format!("fork:{}", kind.tag()),
        }
    }
}

/// Which ask a fork carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AskKind {
    /// The one-size ask, and the declared default.
    Generic,
    /// After source was read.
    ApiSurface,
    /// After a run whose outcome was the point.
    Outcome,
    /// After a change.
    Change,
    /// After a document or a page was read.
    Finding,
    /// "Anything you meant to record?" -- the self-capture reminder.
    Reminder,
    /// Decision and plan. Turn boundaries only.
    Judgment,
}

impl AskKind {
    /// Every kind, in declaration order.
    pub const ALL: &'static [Self] = &[
        Self::Generic,
        Self::ApiSurface,
        Self::Outcome,
        Self::Change,
        Self::Finding,
        Self::Reminder,
        Self::Judgment,
    ];

    /// The spelling the corpus uses, and the template's file name.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::ApiSurface => "api_surface",
            Self::Outcome => "outcome",
            Self::Change => "change",
            Self::Finding => "finding",
            Self::Reminder => "reminder",
            Self::Judgment => "judgment",
        }
    }

    /// The kind a tag names.
    #[must_use]
    pub fn from_tag(text: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.tag() == text)
    }

    /// The ask's template, compiled in from `asks/<tag>.txt`.
    ///
    /// One `include_str!` per kind, so a kind without a template does not
    /// compile rather than rendering as nothing at run time.
    #[must_use]
    pub fn template(self) -> &'static str {
        match self {
            Self::Generic => include_str!("asks/generic.txt"),
            Self::ApiSurface => include_str!("asks/api_surface.txt"),
            Self::Outcome => include_str!("asks/outcome.txt"),
            Self::Change => include_str!("asks/change.txt"),
            Self::Finding => include_str!("asks/finding.txt"),
            Self::Reminder => include_str!("asks/reminder.txt"),
            Self::Judgment => include_str!("asks/judgment.txt"),
        }
    }
}

/// The fork-local imperative, version 1. Versioned as data because the
/// wording is load-bearing: one sentence raised engagement by 22 points in a
/// 630-call experiment, and which of its clauses does the work is an open
/// ablation.
pub const IMPERATIVE: &str = include_str!("imperative.txt");

/// A tool family: what kind of thing a tool does, whatever a harness calls it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Family {
    /// Runs a command line.
    Shell,
    /// Reads a file by path.
    Read,
    /// Writes or edits a file by path.
    Edit,
    /// Lists files by pattern.
    Glob,
    /// Searches file contents.
    Grep,
    /// Fetches from the network.
    Web,
    /// Lists a directory.
    List,
}

impl Family {
    /// Every family.
    pub const ALL: &'static [Self] = &[
        Self::Shell,
        Self::Read,
        Self::Edit,
        Self::Glob,
        Self::Grep,
        Self::Web,
        Self::List,
    ];

    /// The spelling the table uses.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Read => "read",
            Self::Edit => "edit",
            Self::Glob => "glob",
            Self::Grep => "grep",
            Self::Web => "web",
            Self::List => "list",
        }
    }

    fn from_tag(text: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|family| family.tag() == text)
    }
}

/// Tool names and their families: the names seen in real drives. A harness
/// that names its shell tool something else adds a row here and a call to
/// the corpus; it does not get a guess.
pub const FAMILIES: &[(&str, Family)] = &[
    ("bash", Family::Shell),
    ("Bash", Family::Shell),
    ("sh", Family::Shell),
    ("shell", Family::Shell),
    ("run_command", Family::Shell),
    ("execute", Family::Shell),
    ("Read", Family::Read),
    ("read_file", Family::Read),
    ("cat_file", Family::Read),
    ("view_file", Family::Read),
    ("Edit", Family::Edit),
    ("Write", Family::Edit),
    ("MultiEdit", Family::Edit),
    ("NotebookEdit", Family::Edit),
    ("edit_file", Family::Edit),
    ("write_file", Family::Edit),
    ("Glob", Family::Glob),
    ("glob", Family::Glob),
    ("Grep", Family::Grep),
    ("grep", Family::Grep),
    ("search", Family::Grep),
    ("WebFetch", Family::Web),
    ("WebSearch", Family::Web),
    ("fetch_url", Family::Web),
    ("web_fetch", Family::Web),
    ("LS", Family::List),
    ("list_directory", Family::List),
    ("list_files", Family::List),
];

/// The family of a tool, by its name.
#[must_use]
pub fn family_of(tool: &str) -> Option<Family> {
    FAMILIES
        .iter()
        .find(|(name, _)| *name == tool)
        .map(|(_, family)| *family)
}

/// The argument keys a path-taking tool has been seen to use.
const PATH_KEYS: &[&str] = &["file_path", "path", "filename", "file", "notebook_path"];

/// The argument keys a shell tool has been seen to use for its command line.
const COMMAND_KEYS: &[&str] = &["command", "cmd", "script"];

// ---------------------------------------------------------------------------
// the table
// ---------------------------------------------------------------------------

/// The classification table, as data.
const TABLE: &str = include_str!("classes.tsv");

/// One term of a rule.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Term {
    /// Matches anything of the family.
    Any,
    /// The command word is one of these.
    Word(Vec<String>),
    /// Some operand equals one of these.
    Arg(Vec<String>),
    /// Some operand is the flag `-x`, alone or bundled, or `--x`.
    Flag(String),
    /// Some operand path, or the tool's path, has one of these extensions.
    Ext(Vec<String>),
}

/// One row of the table.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Rule {
    class: Class,
    family: Family,
    terms: Vec<Term>,
}

/// Why the table could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableError {
    /// A row with other than three tab-separated fields.
    Shape { line: usize },
    /// A class the vocabulary does not have.
    Class { line: usize, text: String },
    /// A family the vocabulary does not have.
    Family { line: usize, text: String },
    /// A term with a key this module does not know.
    Term { line: usize, text: String },
    /// No rows at all.
    Empty,
}

impl fmt::Display for TableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shape { line } => write!(f, "classes.tsv line {line}: not three fields"),
            Self::Class { line, text } => {
                write!(f, "classes.tsv line {line}: `{text}` is not a class")
            }
            Self::Family { line, text } => {
                write!(f, "classes.tsv line {line}: `{text}` is not a family")
            }
            Self::Term { line, text } => {
                write!(f, "classes.tsv line {line}: `{text}` is not a term")
            }
            Self::Empty => write!(f, "classes.tsv has no rows"),
        }
    }
}

impl Error for TableError {}

fn split_list(text: &str) -> Vec<String> {
    text.split('|').map(str::to_owned).collect()
}

/// A tiny closed vocabulary, matched by iterating it, so the lint that
/// refuses string-literal match arms holds here too.
const TERM_KEYS: &[&str] = &["word", "arg", "flag", "ext"];

fn parse_term(text: &str, line: usize) -> Result<Term, TableError> {
    if text == "*" {
        return Ok(Term::Any);
    }
    let (key, value) = text.split_once('=').ok_or_else(|| TableError::Term {
        line,
        text: text.to_owned(),
    })?;
    match TERM_KEYS.iter().position(|known| *known == key) {
        Some(0) => Ok(Term::Word(split_list(value))),
        Some(1) => Ok(Term::Arg(split_list(value))),
        Some(2) => Ok(Term::Flag(value.to_owned())),
        Some(3) => Ok(Term::Ext(split_list(value))),
        _ => Err(TableError::Term {
            line,
            text: text.to_owned(),
        }),
    }
}

/// The rules, parsed from [`TABLE`].
///
/// # Errors
///
/// Returns the first row that is not a rule. A table that does not parse is
/// an error a caller is handed, never a router that quietly has no rows: a
/// router with no rows classifies every call as unknown, which is the same
/// output a router with a perfect table gives a drive full of novel tools.
/// The two must not be spelled the same way.
fn rules() -> Result<Vec<Rule>, TableError> {
    let mut parsed = Vec::new();
    for (index, raw) in TABLE.lines().enumerate() {
        let line = index + 1;
        let text = raw.trim_end();
        if text.is_empty() || text.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = text.split('\t').collect();
        let [class, family, rule] = fields.as_slice() else {
            return Err(TableError::Shape { line });
        };
        let class = Class::from_tag(class).ok_or_else(|| TableError::Class {
            line,
            text: (*class).to_owned(),
        })?;
        let family = Family::from_tag(family).ok_or_else(|| TableError::Family {
            line,
            text: (*family).to_owned(),
        })?;
        let terms = rule
            .split_whitespace()
            .map(|term| parse_term(term, line))
            .collect::<Result<Vec<_>, _>>()?;
        parsed.push(Rule {
            class,
            family,
            terms,
        });
    }
    if parsed.is_empty() {
        return Err(TableError::Empty);
    }
    Ok(parsed)
}

// ---------------------------------------------------------------------------
// classification
// ---------------------------------------------------------------------------

/// What a rule is matched against: the parts of a call that a term can name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Subject {
    /// The command word, when the family is shell and the word is literal.
    word: Option<String>,
    /// Operand texts.
    operands: Vec<String>,
    /// The tool's own path argument, when the family takes one.
    path: Option<String>,
}

fn extension_of(path: &str) -> Option<&str> {
    let name = path.rsplit('/').next()?;
    let (stem, ext) = name.rsplit_once('.')?;
    if stem.is_empty() { None } else { Some(ext) }
}

impl Term {
    fn matches(&self, subject: &Subject) -> bool {
        match self {
            Self::Any => true,
            Self::Word(names) => subject
                .word
                .as_ref()
                .is_some_and(|word| names.iter().any(|name| name == word)),
            Self::Arg(values) => subject
                .operands
                .iter()
                .any(|operand| values.iter().any(|value| value == operand)),
            Self::Flag(flag) => subject.operands.iter().any(|operand| {
                operand.strip_prefix("--").is_some_and(|long| long == flag)
                    || (operand.starts_with('-')
                        && !operand.starts_with("--")
                        && operand[1..].contains(flag.as_str()))
            }),
            Self::Ext(exts) => {
                let has = |path: &str| {
                    extension_of(path).is_some_and(|ext| exts.iter().any(|e| e == ext))
                };
                subject.path.as_deref().is_some_and(has)
                    || subject.operands.iter().any(|operand| has(operand))
            }
        }
    }
}

impl Rule {
    fn matches(&self, family: Family, subject: &Subject) -> bool {
        self.family == family && self.terms.iter().all(|term| term.matches(subject))
    }
}

/// The producer of the last pipeline of the last AND-OR item: what the model
/// wanted from the call. `cd diet && cargo test` is a test run;
/// `cargo test | tail -15` is a test run and not a `tail`; `cat notes.md |
/// head` is a document read. A compound producer is descended into by the
/// same rule.
fn producer(list: &List) -> Option<&Simple> {
    let tail = list.items.last()?.chain.last()?;
    match tail.pipeline.first()? {
        Command::Simple(simple) => Some(simple),
        Command::Subshell(inner) | Command::Group(inner) => producer(inner),
    }
}

fn basename(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

fn string_arg<'a>(args: &'a BTreeMap<String, Value>, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| match args.get(*key) {
        Some(Value::String(text)) => Some(text.as_str()),
        _ => None,
    })
}

/// A call, classified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    /// The class.
    pub class: Class,
    /// The tool's family, if its name was known.
    pub family: Option<Family>,
    /// The command word the rule saw, for a shell call.
    pub word: Option<String>,
}

/// Classify one tool call by its name and arguments, against rows already
/// read.
///
/// Never fails, and cannot: a call no row places is [`Class::Unknown`], which
/// is a class with a routing of its own. The rows come in as an argument
/// precisely so that "no row matched" and "no rows were read" cannot arrive
/// at this function looking alike.
#[must_use]
fn classify(rules: &[Rule], tool: &str, args: Option<&BTreeMap<String, Value>>) -> Classification {
    let Some(family) = family_of(tool) else {
        return Classification {
            class: Class::Unknown,
            family: None,
            word: None,
        };
    };
    let subject = match family {
        Family::Shell => args
            .and_then(|args| string_arg(args, COMMAND_KEYS))
            .and_then(|line| shell::parse(line).ok())
            .as_ref()
            .and_then(producer)
            .map(|simple| Subject {
                word: simple
                    .command()
                    .filter(|word| word.literal)
                    .map(|word| basename(&word.text).to_owned()),
                operands: simple
                    .operands()
                    .iter()
                    .map(|word| word.text.clone())
                    .collect(),
                path: None,
            })
            .unwrap_or_default(),
        Family::Read | Family::Edit | Family::Glob | Family::Grep | Family::Web | Family::List => {
            Subject {
                word: None,
                operands: Vec::new(),
                path: args
                    .and_then(|args| string_arg(args, PATH_KEYS))
                    .map(str::to_owned),
            }
        }
    };
    let class = rules
        .iter()
        .find(|rule| rule.matches(family, &subject))
        .map_or(Class::Unknown, |rule| rule.class);
    Classification {
        class,
        family: Some(family),
        word: subject.word,
    }
}

// ---------------------------------------------------------------------------
// intent
// ---------------------------------------------------------------------------

/// Phrases with which the model says what it is about to do.
const INTENT_MARKERS: &[&str] = &[
    "i'll ",
    "i will ",
    "i am going to ",
    "i'm going to ",
    "going to ",
    "next i",
    "next, i",
    "next step",
    "let me ",
];

/// The last sentence of `text` in which the model states an intent, if any.
///
/// Mechanical: a sentence containing one of [`INTENT_MARKERS`]. The ask
/// quotes it back rather than asking the model to reconstruct what it meant
/// to do -- that is the stated-intent hole.
#[must_use]
pub fn stated_intent(text: &str) -> Option<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        current.push(c);
        if matches!(c, '.' | '!' | '?') && chars.peek().is_none_or(|next| next.is_whitespace()) {
            sentences.push(std::mem::take(&mut current));
        }
    }
    if !current.trim().is_empty() {
        sentences.push(current);
    }
    sentences
        .iter()
        .rev()
        .map(|sentence| sentence.trim())
        .find(|sentence| {
            let lower = sentence.to_lowercase();
            INTENT_MARKERS.iter().any(|marker| lower.contains(marker))
        })
        .map(str::to_owned)
}

// ---------------------------------------------------------------------------
// asks
// ---------------------------------------------------------------------------

/// An ask about to be rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask<'a> {
    /// Which ask.
    pub kind: AskKind,
    /// What the model said it was about to do, if it said.
    pub intent: Option<&'a str>,
}

impl Ask<'_> {
    /// Render the ask: the imperative and the intent filled here, the
    /// mechanical facts by the mechanical lane. A line whose hole has no
    /// value is dropped whole, so an ask never says `You said: ""`.
    #[must_use]
    pub fn render(&self, facts: &Facts) -> String {
        let mut out = String::new();
        for line in self.kind.template().lines() {
            let line = line.replace("{imperative}", IMPERATIVE.trim());
            let line = if line.contains("{intent}") {
                match self.intent {
                    Some(intent) => line.replace("{intent}", intent),
                    None => continue,
                }
            } else {
                line
            };
            out.push_str(&line);
            out.push('\n');
        }
        mechanical::fill(&out, facts)
    }
}

// ---------------------------------------------------------------------------
// the router
// ---------------------------------------------------------------------------

/// What a decision is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trigger {
    /// A tool call, by its record id.
    Call(String),
    /// The end of a turn.
    TurnEnd(u32),
}

/// One routing decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// What it is about.
    pub trigger: Trigger,
    /// The turn it was made in.
    pub turn: u32,
    /// The class, or [`Class::Unknown`] for a turn end.
    pub class: Class,
    /// What was decided.
    pub routing: Routing,
}

/// A call the table could not place. Typed, so that misrouting is counted
/// rather than suspected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unclassified {
    /// The call's record id.
    pub id: String,
    /// The turn.
    pub turn: u32,
    /// The tool as named.
    pub tool: String,
    /// The command word, when there was one.
    pub word: Option<String>,
}

/// Per-class counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tally {
    /// Calls of this class.
    pub seen: u64,
    /// Of which forked now.
    pub forked: u64,
    /// Of which deferred to the turn boundary.
    pub deferred: u64,
    /// Of which silent.
    pub silent: u64,
}

/// The coverage census of a drive.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Census {
    /// Counts per class.
    pub per_class: BTreeMap<Class, Tally>,
    /// Calls the table could not place.
    pub unclassified: u64,
    /// Forks the router spent on calls.
    pub call_forks: u64,
    /// Judgment asks, one per turn boundary.
    pub judgment_asks: u64,
    /// Turns seen.
    pub turns: u64,
    /// Tool calls seen.
    pub tool_calls: u64,
}

impl Census {
    /// Forks spent: the call forks and the judgment asks.
    #[must_use]
    pub fn forks(&self) -> u64 {
        self.call_forks + self.judgment_asks
    }

    /// What the naive design would have spent: one interview after every
    /// tool call, and one at every turn.
    #[must_use]
    pub fn naive_forks(&self) -> u64 {
        self.tool_calls + self.turns
    }

    /// The census as the record's value space. The reduction is a decimal
    /// with three digits, computed from the two counts it sits beside.
    ///
    /// # Errors
    ///
    /// Returns [`CensusError::Reduction`] if the reduction this census
    /// computed is not a decimal the record grammar will read back. The
    /// arithmetic below cannot produce one today; the check is here so that
    /// the day it can, the census says so instead of banking a number no
    /// reader accepts.
    pub fn value(&self) -> Result<Value, CensusError> {
        let per_class = self
            .per_class
            .iter()
            .map(|(class, tally)| {
                (
                    class.tag().to_owned(),
                    Value::Object(BTreeMap::from([
                        ("seen".to_owned(), Value::Integer(as_i64(tally.seen))),
                        ("forked".to_owned(), Value::Integer(as_i64(tally.forked))),
                        (
                            "deferred".to_owned(),
                            Value::Integer(as_i64(tally.deferred)),
                        ),
                        ("silent".to_owned(), Value::Integer(as_i64(tally.silent))),
                    ])),
                )
            })
            .collect();
        let naive = self.naive_forks();
        let reduction = if naive == 0 {
            "0.000".to_owned()
        } else {
            // Integer arithmetic to three places, rounded half up, so the
            // number is the same on every machine.
            let saved = naive.saturating_sub(self.forks());
            let thousandths = (saved * 1000 + naive / 2) / naive;
            format!("{}.{:03}", thousandths / 1000, thousandths % 1000)
        };
        Ok(Value::Object(BTreeMap::from([
            ("per_class".to_owned(), Value::Object(per_class)),
            (
                "unclassified".to_owned(),
                Value::Integer(as_i64(self.unclassified)),
            ),
            ("forks".to_owned(), Value::Integer(as_i64(self.forks()))),
            (
                "judgment_asks".to_owned(),
                Value::Integer(as_i64(self.judgment_asks)),
            ),
            ("naive_forks".to_owned(), Value::Integer(as_i64(naive))),
            ("turns".to_owned(), Value::Integer(as_i64(self.turns))),
            (
                "tool_calls".to_owned(),
                Value::Integer(as_i64(self.tool_calls)),
            ),
            // `Decimal::new` asks the record grammar whether these digits
            // are a decimal. The format above always produces one, and if it
            // ever stops the census says so rather than banking a number no
            // reader would accept.
            (
                "reduction".to_owned(),
                Value::Decimal(
                    Decimal::new(&reduction).ok_or(CensusError::Reduction(reduction.clone()))?,
                ),
            ),
        ])))
    }
}

/// Why a census could not be written as the record's value space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CensusError {
    /// The reduction is not a decimal the record grammar reads back. The
    /// census and the format have drifted apart.
    Reduction(String),
}

impl fmt::Display for CensusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reduction(digits) => {
                write!(
                    f,
                    "the reduction {digits} is not a decimal the record reads"
                )
            }
        }
    }
}

impl Error for CensusError {}

fn as_i64(count: u64) -> i64 {
    i64::try_from(count).unwrap_or(i64::MAX)
}

/// The router: feed it a record's events in order.
#[derive(Debug, Clone)]
pub struct Router {
    /// The rows, read once when the router was made.
    rules: Vec<Rule>,
    /// The open turn, if one is.
    turn: Option<u32>,
    /// The lane of every request seen, so a response can be placed.
    lanes: BTreeMap<String, String>,
    /// What the model last said it was about to do.
    intent: Option<String>,
    census: Census,
    unclassified: Vec<Unclassified>,
}

/// The canonical lane's name in the record.
const CANONICAL_LANE: &str = "main";

impl Router {
    /// A router with nothing seen, holding the rows it will route by.
    ///
    /// # Errors
    ///
    /// Returns the first row of the table that is not a rule. There is no
    /// router without a table: a router that failed to read one would
    /// classify a whole drive as unknown and report that as a finding.
    pub fn new() -> Result<Self, TableError> {
        Ok(Self {
            rules: rules()?,
            turn: None,
            lanes: BTreeMap::new(),
            intent: None,
            census: Census::default(),
            unclassified: Vec::new(),
        })
    }

    /// The intent the model last stated, for the asks that quote it.
    #[must_use]
    pub fn intent(&self) -> Option<&str> {
        self.intent.as_deref()
    }

    /// The census so far.
    #[must_use]
    pub fn census(&self) -> &Census {
        &self.census
    }

    /// Every call the table could not place, so far.
    #[must_use]
    pub fn unclassified(&self) -> &[Unclassified] {
        &self.unclassified
    }

    /// Observe one event. Returns the decisions it produced: a call's
    /// decision, and at a turn boundary the judgment ask for the turn that
    /// just closed.
    pub fn observe(&mut self, event: &Event) -> Vec<Decision> {
        match event {
            Event::Turn { index, .. } => {
                let mut out = Vec::new();
                out.extend(self.end_turn());
                self.turn = Some(*index);
                self.census.turns += 1;
                out
            }
            Event::Request { id, lane, .. } => {
                self.lanes.insert(id.clone(), lane.clone());
                Vec::new()
            }
            Event::Response {
                to_request,
                text: Some(text),
                ..
            } => {
                if self
                    .lanes
                    .get(to_request)
                    .is_some_and(|lane| lane == CANONICAL_LANE)
                    && let Some(intent) = stated_intent(text)
                {
                    self.intent = Some(intent);
                }
                Vec::new()
            }
            Event::ToolCall {
                id,
                at_turn,
                tool,
                args,
                ..
            } => vec![self.route_call(id, *at_turn, tool, args.as_ref())],
            Event::Summary { .. } => self.end_turn().into_iter().collect(),
            Event::Start { .. }
            | Event::Response { .. }
            | Event::Fork { .. }
            | Event::Capture { .. }
            | Event::Seam { .. }
            | Event::Rejected { .. }
            | Event::Claim { .. } => Vec::new(),
        }
    }

    /// Close the open turn: the judgment ask fires here and nowhere else.
    pub fn end_turn(&mut self) -> Option<Decision> {
        let turn = self.turn.take()?;
        self.census.judgment_asks += 1;
        Some(Decision {
            trigger: Trigger::TurnEnd(turn),
            turn,
            class: Class::Unknown,
            routing: Routing::Fork(AskKind::Judgment),
        })
    }

    fn route_call(
        &mut self,
        id: &str,
        turn: u32,
        tool: &str,
        args: Option<&BTreeMap<String, Value>>,
    ) -> Decision {
        let classified = classify(&self.rules, tool, args);
        let routing = classified.class.routing();
        self.census.tool_calls += 1;
        let tally = self.census.per_class.entry(classified.class).or_default();
        tally.seen += 1;
        match routing {
            Routing::Silent => tally.silent += 1,
            Routing::Defer => tally.deferred += 1,
            Routing::Fork(_) => {
                tally.forked += 1;
                self.census.call_forks += 1;
            }
        }
        if classified.class == Class::Unknown {
            self.census.unclassified += 1;
            self.unclassified.push(Unclassified {
                id: id.to_owned(),
                turn,
                tool: tool.to_owned(),
                word: classified.word,
            });
        }
        Decision {
            trigger: Trigger::Call(id.to_owned()),
            turn,
            class: classified.class,
            routing,
        }
    }
}

/// A whole record, routed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replay {
    /// Every decision, in order.
    pub decisions: Vec<Decision>,
    /// The census.
    pub census: Census,
    /// The calls the table could not place.
    pub unclassified: Vec<Unclassified>,
}

/// Route an archived drive.
///
/// # Errors
///
/// Returns the first row of the routing table that is not a rule.
pub fn replay(record: &Record) -> Result<Replay, TableError> {
    let mut router = Router::new()?;
    let mut decisions = Vec::new();
    for event in &record.events {
        decisions.extend(router.observe(event));
    }
    Ok(Replay {
        decisions,
        census: router.census,
        unclassified: router.unclassified,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};

    use super::{
        AskKind, Class, Family, IMPERATIVE, Router, Routing, Trigger, classify, replay, rules,
        stated_intent,
    };
    use crate::capture::mechanical::Facts;
    use crate::formats::record::json::Value;
    use crate::formats::record::{self, Event};

    fn shell(command: &str) -> BTreeMap<String, Value> {
        BTreeMap::from([("command".to_owned(), Value::String(command.to_owned()))])
    }

    fn path(p: &str) -> BTreeMap<String, Value> {
        BTreeMap::from([("file_path".to_owned(), Value::String(p.to_owned()))])
    }

    #[test]
    fn every_class_has_a_tag_that_reads_back_and_a_routing() {
        for class in Class::ALL {
            assert_eq!(
                Class::from_tag(class.tag()),
                Some(*class),
                "{}",
                class.tag()
            );
            // Routing is total: the match is exhaustive, so this only checks
            // that the default is what the design declares.
            if *class == Class::Unknown {
                assert_eq!(
                    class.routing(),
                    Routing::Fork(AskKind::Generic),
                    "an unknown pattern must route to the declared default, never to silence"
                );
            }
        }
        for kind in AskKind::ALL {
            assert_eq!(AskKind::from_tag(kind.tag()), Some(*kind));
        }
        for family in Family::ALL {
            assert_eq!(Family::from_tag(family.tag()), Some(*family));
        }
    }

    #[test]
    fn every_ask_kind_has_a_template_that_carries_the_imperative() {
        for kind in AskKind::ALL {
            let template = kind.template();
            assert!(
                template.contains("{imperative}"),
                "{}: the ask does not carry the fork-local imperative",
                kind.tag()
            );
            assert!(!template.trim().is_empty(), "{}: an empty ask", kind.tag());
        }
        assert!(!IMPERATIVE.trim().is_empty());
    }

    #[test]
    fn the_table_parses_and_covers_every_class_but_unknown() {
        let rules = rules().expect("the table parses");
        let covered: BTreeSet<Class> = rules.iter().map(|rule| rule.class).collect();
        for class in Class::ALL {
            if *class == Class::Unknown {
                assert!(!covered.contains(class), "unknown is what no row says");
            } else {
                assert!(
                    covered.contains(class),
                    "{}: no row in the table",
                    class.tag()
                );
            }
        }
    }

    #[test]
    fn an_unknown_tool_call_routes_to_the_declared_default_and_says_so() {
        let mut router = Router::new().expect("the table parses");
        router.observe(&Event::Turn {
            index: 1,
            prefill_tokens: record::Count::default(),
        });
        let decisions = router.observe(&Event::ToolCall {
            id: "t1".to_owned(),
            at_turn: 1,
            tool: "bash".to_owned(),
            args: Some(shell("xyzzy --frob")),
            exit: Some(127),
            output: None,
        });
        assert_eq!(decisions.len(), 1);
        assert_eq!(
            decisions[0].routing,
            Routing::Fork(AskKind::Generic),
            "an unknown pattern must route to the declared default, never to silence"
        );
        assert_eq!(
            router.unclassified().len(),
            1,
            "an unknown pattern must be a typed event, or misrouting cannot be measured"
        );
        assert_eq!(router.unclassified()[0].word.as_deref(), Some("xyzzy"));
        // A tool whose NAME is unknown is unknown too, with no word.
        let decisions = router.observe(&Event::ToolCall {
            id: "t2".to_owned(),
            at_turn: 1,
            tool: "Frobnicate".to_owned(),
            args: None,
            exit: None,
            output: None,
        });
        assert_eq!(decisions[0].class, Class::Unknown);
        assert_eq!(router.census().unclassified, 2);
    }

    #[test]
    fn a_judgment_ask_waits_for_the_turn_boundary() {
        let mut router = Router::new().expect("the table parses");
        let mut all = Vec::new();
        all.extend(router.observe(&Event::Turn {
            index: 1,
            prefill_tokens: record::Count::default(),
        }));
        all.extend(router.observe(&Event::ToolCall {
            id: "t1".to_owned(),
            at_turn: 1,
            tool: "bash".to_owned(),
            args: Some(shell("rm -rf build")),
            exit: Some(0),
            output: None,
        }));
        assert!(
            all.iter()
                .all(|d| d.routing != Routing::Fork(AskKind::Judgment)),
            "a judgment ask fired in the middle of a turn"
        );
        assert_eq!(all.last().map(|d| d.routing), Some(Routing::Defer));
        let closing = router.observe(&Event::Turn {
            index: 2,
            prefill_tokens: record::Count::default(),
        });
        assert_eq!(closing.len(), 1);
        assert_eq!(closing[0].routing, Routing::Fork(AskKind::Judgment));
        assert_eq!(closing[0].trigger, Trigger::TurnEnd(1));
        assert_eq!(router.census().judgment_asks, 1);
    }

    #[test]
    fn the_producer_of_the_last_pipeline_decides_the_class() {
        let table = rules().expect("the table parses");
        let cases = [
            ("cargo test -p x 2>&1 | tail -15", Class::TestRun),
            ("cd diet && cargo test", Class::TestRun),
            ("cat notes.md | head -20", Class::DocumentRead),
            ("cat src/lib.rs", Class::SourceRead),
            ("(cd a; cargo build)", Class::Build),
            ("ls -la exercise", Class::DirectoryListing),
            ("echo x; df -h . | tail -1", Class::Inert),
            ("./target/debug/diet check-record run.jsonl", Class::Unknown),
            ("$CMD --flag", Class::Unknown),
            (
                "git add -A && git commit -q -F - <<'EOF'\nmsg\nEOF",
                Class::VersionControl,
            ),
            ("sed -i 's/a/b/' file.rs", Class::Edit),
            ("sed -n 80,160p file.rs", Class::SourceRead),
            ("curl -sS https://example.invalid/x", Class::WebRead),
            ("python3 scripts/check-library.py", Class::SideEffect),
            ("grep -rn 'fn parse' diet/", Class::Search),
        ];
        for (command, want) in cases {
            let got = classify(&table, "bash", Some(&shell(command))).class;
            assert_eq!(got, want, "{command:?} was classified as {}", got.tag());
        }
        assert_eq!(
            classify(&table, "Read", Some(&path("AGENTS.md"))).class,
            Class::DocumentRead
        );
        assert_eq!(
            classify(&table, "Read", Some(&path("src/lib.rs"))).class,
            Class::SourceRead
        );
        assert_eq!(
            classify(&table, "Edit", Some(&path("src/lib.rs"))).class,
            Class::Edit
        );
        assert_eq!(classify(&table, "Grep", None).class, Class::Search);
        assert_eq!(classify(&table, "WebFetch", None).class, Class::WebRead);
    }

    #[test]
    fn stated_intent_takes_the_last_sentence_that_states_one() {
        let text = "The parser keeps every line. I'll fix the caller next. That is all.";
        assert_eq!(
            stated_intent(text).as_deref(),
            Some("I'll fix the caller next.")
        );
        assert_eq!(stated_intent("Nothing here states a plan."), None);
        assert_eq!(
            stated_intent("Next I am going to run the gate").as_deref(),
            Some("Next I am going to run the gate")
        );
    }

    #[test]
    fn a_line_with_an_unfilled_hole_is_dropped_whole() {
        let ask = super::Ask {
            kind: AskKind::Generic,
            intent: None,
        };
        let rendered = ask.render(&Facts::default());
        assert!(
            !rendered.contains("You said"),
            "an ask quoted an intent nobody stated: {rendered}"
        );
        assert!(
            !rendered.contains('{'),
            "a hole survived rendering: {rendered}"
        );
        assert!(rendered.starts_with(IMPERATIVE.trim()));
        let ask = super::Ask {
            kind: AskKind::Generic,
            intent: Some("I'll run the gate."),
        };
        let rendered = ask.render(&Facts::default());
        assert!(rendered.contains("You said: \"I'll run the gate.\""));
    }

    fn corpus_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("capture/router/corpus")
    }

    /// Every corpus case: the record and its expectation.
    fn corpus() -> Vec<(PathBuf, record::Record, serde_json::Value)> {
        let dir = corpus_dir();
        let mut cases: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap_or_else(|err| panic!("{}: {err}", dir.display()))
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
            .collect();
        cases.sort();
        assert!(
            !cases.is_empty(),
            "{}: no cases, so every assertion over the corpus would hold vacuously",
            dir.display()
        );
        cases
            .into_iter()
            .map(|path| {
                let text = std::fs::read_to_string(&path).expect("a readable case");
                let parsed = record::parse(&text)
                    .unwrap_or_else(|err| panic!("{}: not a record: {err}", path.display()));
                let expected = path.with_extension("expected.json");
                let expected: serde_json::Value = serde_json::from_str(
                    &std::fs::read_to_string(&expected)
                        .unwrap_or_else(|err| panic!("{}: {err}", expected.display())),
                )
                .expect("an expectation is JSON");
                (path, parsed, expected)
            })
            .collect()
    }

    #[test]
    fn the_corpus_routes_every_call_as_labelled() {
        for (path, parsed, expected) in corpus() {
            let replayed = replay(&parsed).expect("the table parses");
            let labels = expected["calls"].as_object().expect("calls");
            assert!(!labels.is_empty(), "{}: no labelled calls", path.display());
            let mut misrouted = Vec::new();
            let mut seen = 0;
            for decision in &replayed.decisions {
                let Trigger::Call(id) = &decision.trigger else {
                    continue;
                };
                seen += 1;
                let label = &labels[id.as_str()];
                let want_class = label["class"].as_str().expect("class");
                let want_routing = label["routing"].as_str().expect("routing");
                if decision.class.tag() != want_class || decision.routing.tag() != want_routing {
                    misrouted.push(format!(
                        "{id}: expected {want_class}/{want_routing}, got {}/{}",
                        decision.class.tag(),
                        decision.routing.tag()
                    ));
                }
            }
            assert_eq!(
                seen,
                labels.len(),
                "{}: a labelled call was not routed",
                path.display()
            );
            assert!(
                misrouted.is_empty(),
                "{}: {} misrouted call(s):\n  {}",
                path.display(),
                misrouted.len(),
                misrouted.join("\n  ")
            );
            let unclassified: Vec<&str> = replayed
                .unclassified
                .iter()
                .map(|u| u.id.as_str())
                .collect();
            let want_unclassified: Vec<&str> = expected["unclassified"]
                .as_array()
                .expect("unclassified")
                .iter()
                .map(|v| v.as_str().expect("id"))
                .collect();
            assert_eq!(
                unclassified,
                want_unclassified,
                "{}: unclassified",
                path.display()
            );
            assert_eq!(
                replayed.census.forks(),
                expected["forks"].as_u64().expect("forks"),
                "{}: forks",
                path.display()
            );
            assert_eq!(
                replayed.census.naive_forks(),
                expected["naive_forks"].as_u64().expect("naive_forks"),
                "{}: naive forks",
                path.display()
            );
            assert_eq!(
                replayed.census.judgment_asks,
                expected["judgment_asks"].as_u64().expect("judgment_asks"),
                "{}: judgment asks",
                path.display()
            );
            assert!(
                replayed.census.forks() < replayed.census.naive_forks(),
                "{}: the router spent no fewer forks than the naive design",
                path.display()
            );
        }
    }

    #[test]
    fn the_census_reports_the_reduction_as_a_decimal_of_its_own_counts() {
        let (_, parsed, _) = corpus().into_iter().next().expect("a case");
        let census = replay(&parsed).expect("the table parses").census;
        let value = census.value().expect("the reduction is a decimal");
        let Value::Object(members) = value else {
            panic!("the census is an object");
        };
        let Some(Value::Decimal(reduction)) = members.get("reduction") else {
            panic!("the reduction is a decimal");
        };
        let saved = census.naive_forks() - census.forks();
        let thousandths = (saved * 1000 + census.naive_forks() / 2) / census.naive_forks();
        assert_eq!(
            reduction.as_str(),
            format!("{}.{:03}", thousandths / 1000, thousandths % 1000),
            "the reduction is not the number its own counts give"
        );
        assert!(matches!(
            members.get("naive_forks"),
            Some(Value::Integer(_))
        ));
    }
}
