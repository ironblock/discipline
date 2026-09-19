//! The bakeoff runner: the instrument composed into numbers.
//!
//! [`sense`](super::sense) exposes every piece the bakeoff needs -- the
//! register, the sense sets, the caches, the scorings, the gates, the
//! metrics, the controls, the bootstrap, the null -- and nothing composed
//! them. This does, in one place, because a second composition in a results
//! directory would be the second implementation this repository forbids.
//!
//! WHAT IT IS POINTED AT is a results directory's `run.jsonl`. That choice
//! saves inventing a manifest format: a record already declares its regime on
//! the `start` row, and already names every input it consumed with a digest
//! on its `claim` rows. So the two refusals #24's ruling asks for are
//! inherited rather than re-implemented -- a record whose regime cannot be
//! spelled does not parse at all, and a cache whose bytes are not the bytes
//! the record consumed fails a comparison this module makes against
//! [`digest::sha256_hex`](crate::digest::sha256_hex).
//!
//! THE BUDGET IS NOT ONE NUMBER. It was a `const` here set to eight, and
//! eight was not chosen -- it was the widest budget the precision failure
//! fixture happened to demonstrate. A fixture's shape is not the instrument's
//! parameter space, so the ladder is pre-registered in
//! [`sense::BUDGETS`](super::sense::BUDGETS), the fixtures are built from it,
//! and every cell reports every metric at every budget on it. Ruled
//! 2026-09-10 on #69.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::capture::pairs;
use crate::capture::sense::{
    self, Blocker, Cached, Cell, ControlFailure, DataError, EmbeddedSet, Embedder, Gate, Metric,
    PRE_REGISTRATION, Reported, Row, ScoreError, Scoring, SenseSet, SetError, decimal,
};
use crate::digest::sha256_hex;
use crate::formats::record::json::{self, Value};
use crate::formats::record::{Artifact, Event, Regime, Summary};

/// The seed every resampling in a run is drawn from.
///
/// One seed, written here, so that two runs over the same inputs produce the
/// same p-values -- which is what makes the numbers a recompute rather than a
/// re-fire.
pub const SEED: u64 = 0x5ee_d24;

/// Why a run did not produce numbers.
#[derive(Debug, Clone, PartialEq)]
pub enum RunError {
    /// The run record is not a session record.
    Record(String),
    /// A consumed input is not beside the record.
    Missing {
        /// What the record said it consumed.
        path: String,
    },
    /// A consumed input's bytes are not the bytes the record consumed.
    Digest {
        /// The input.
        path: String,
        /// What the record declares.
        declared: String,
        /// What is on disk.
        found: String,
    },
    /// A consumed input is not text.
    NotUtf8 {
        /// The input.
        path: String,
    },
    /// A consumed input is not the data its name says it is.
    Data {
        /// The input.
        path: String,
        /// Why not.
        reason: DataError,
    },
    /// No embedder cache among the consumed inputs. A bakeoff of nothing is
    /// not a bakeoff.
    NoCaches,
    /// No register rows for a set an embedder was asked about.
    NoRegister {
        /// The set.
        set: SenseSet,
    },
    /// A sense set could not be embedded.
    Set(SetError),
    /// A row the gate admitted could not be scored.
    Score(ScoreError),
    /// A control row could not be scored at all -- a cache miss, which is a
    /// fact about the inputs and not a reading about the embedder.
    ///
    /// The OTHER control failures are not here. A control that lands in the
    /// wrong place is a per-cell result, written into the report as
    /// `control_failed` with its readings and no metrics over that cell, and
    /// the run proceeds to the next cell. Ruled on #24 (2026-09-15): the verb
    /// was written against a fixture embedder whose controls land at their
    /// extremes by construction; real embedders fail individually, and a run
    /// that stopped at the first (embedder, scoring) failure could not report
    /// the reading that stopped it.
    Control {
        /// Which embedder.
        embedder: String,
        /// Which scoring.
        scoring: Scoring,
        /// What went wrong.
        failure: ControlFailure,
    },
    /// A metric could not be reported, in a named cell.
    Metric {
        /// Which embedder.
        embedder: String,
        /// Which set.
        set: SenseSet,
        /// Which scoring.
        scoring: Scoring,
        /// Which gate.
        gate: Gate,
        /// Why.
        error: sense::MetricError,
    },
    /// The paired bootstrap could not run.
    Bootstrap(sense::BootstrapError),
    /// A pairs run could not turn a register into cells.
    Pairs(pairs::CellError),
    /// The record consumed a sense register and a pairs register both, and a
    /// run is one instrument or the other.
    MixedRegisters,
    /// A pairs run consumed no `pairs-intent.jsonl`, which is its primary
    /// register.
    NoPairs,
    /// A pairs run's embedder has a cache for one role and not the other,
    /// and no role-less cache to stand in.
    RoleCacheMissing {
        /// The embedder.
        model: String,
        /// The role with no cache.
        role: &'static str,
    },
    /// The directory to assemble into already holds a report.
    Occupied {
        /// The directory.
        path: String,
    },
    /// `pre-registration.json` was written and its bytes changed before the
    /// scores were.
    ///
    /// The endpoints have to be fixed before the numbers, or they are not a
    /// pre-registration; the digest in the front-matter is what says they
    /// were. If the file moved between being written and being pinned, the
    /// digest would name bytes nobody scored against.
    PreRegistrationMoved {
        /// The digest written into the front-matter.
        declared: String,
        /// What the file hashes to now.
        found: String,
    },
    /// A file the assembly had to write could not be written.
    Write {
        /// The file.
        path: String,
        /// Why not.
        reason: String,
    },
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Occupied { path } => write!(
                f,
                "{path} already holds a README.md: assembling over somebody's results is not \
                 an assembly step, and a directory is cheap to name differently"
            ),
            Self::PreRegistrationMoved { declared, found } => write!(
                f,
                "pre-registration.json was written as {declared} and now hashes to \
                 {found}; the endpoints have to be fixed before the numbers, and a \
                 digest naming bytes nobody scored against pins nothing"
            ),
            Self::Write { path, reason } => write!(
                f,
                "{path} could not be written: {reason}. A directory assembled halfway is worse \
                 than one not assembled, because it looks like a result"
            ),
            Self::Record(reason) => write!(f, "the run record is not a session record: {reason}"),
            Self::Missing { path } => write!(
                f,
                "the record consumes {path}, which is not beside it: a score over an input \
                 nobody can produce is not a recompute"
            ),
            Self::Digest {
                path,
                declared,
                found,
            } => write!(
                f,
                "{path} is {found}, and the record consumed {declared}: these are not the same \
                 bytes, so whatever this scored is not what the record says it scored"
            ),
            Self::NotUtf8 { path } => write!(f, "{path} is not UTF-8"),
            Self::Data { path, reason } => write!(f, "{path}: {reason}"),
            Self::NoCaches => write!(
                f,
                "the record consumes no `*.vectors.jsonl` cache, so there is no embedder to \
                 compare against another"
            ),
            Self::NoRegister { set } => write!(
                f,
                "no register rows for {}, so its cells would be scored over nothing",
                set.tag()
            ),
            Self::Set(err) => write!(f, "{err}"),
            Self::Score(err) => write!(f, "{err}"),
            Self::Control {
                embedder,
                scoring,
                failure,
            } => write!(f, "{embedder} under {}: {failure}", scoring.tag()),
            Self::Metric {
                embedder,
                set,
                scoring,
                gate,
                error,
            } => write!(
                f,
                "{embedder}/{}/{}/{}: {error}",
                set.tag(),
                scoring.tag(),
                gate.tag()
            ),
            Self::Bootstrap(err) => write!(f, "{err}"),
            Self::Pairs(err) => write!(f, "{err}"),
            Self::MixedRegisters => write!(
                f,
                "the record consumes a sense register and a pairs register both; a run is one \
                 instrument or the other"
            ),
            Self::NoPairs => write!(
                f,
                "a pairs run consumed no pairs-intent.jsonl, which is its primary register"
            ),
            Self::RoleCacheMissing { model, role } => write!(
                f,
                "{model}: no {role} cache ({model}.{role}.vectors.jsonl) and no {model}.vectors.jsonl \
                 to stand in for it"
            ),
        }
    }
}

impl Error for RunError {}

/// Every artifact the record's claims consume, in the order they were named.
fn consumed(events: &[Event]) -> Vec<&Artifact> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Claim { consumes, .. } => Some(consumes.iter()),
            _ => None,
        })
        .flatten()
        .collect()
}

/// Read one consumed input, refusing bytes the record did not consume.
fn read_consumed(dir: &Path, artifact: &Artifact) -> Result<String, RunError> {
    let path = dir.join(&artifact.path);
    let bytes = std::fs::read(&path).map_err(|_| RunError::Missing {
        path: artifact.path.clone(),
    })?;
    let found = sha256_hex(&bytes);
    if found != artifact.sha256 {
        return Err(RunError::Digest {
            path: artifact.path.clone(),
            declared: artifact.sha256.clone(),
            found,
        });
    }
    String::from_utf8(bytes).map_err(|_| RunError::NotUtf8 {
        path: artifact.path.clone(),
    })
}

/// Which side of a pair a role cache serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Role {
    Query,
    Document,
}

impl Role {
    fn tag(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Document => "document",
        }
    }
}

/// What a consumed input is, decided by its name.
enum Input {
    /// A `<model>.vectors.jsonl` cache, under the model's id.
    Cache(String),
    /// A `<model>.query.vectors.jsonl` or `<model>.document.vectors.jsonl`
    /// cache: one side of a pairs run's embedder.
    RoleCache(String, Role),
    /// A pairs-run register, by its fixed file name.
    Pairs(pairs::PairRegister),
    /// A register file, under what its name says is in it.
    Register(sense::RegisterName),
    /// Something the bakeoff does not read -- a record consumes what it
    /// consumes, and this reads the parts it knows.
    Other,
}

/// Classify by file name.
///
/// The register's own naming rule does the work: `<source>-<set>.jsonl` is a
/// register file and [`sense::RegisterName::of`] is the one reader of that
/// rule. Anything ending `.vectors.jsonl` is a cache, under the stem before
/// it. Everything else is left alone rather than guessed at.
fn classify(path: &str) -> Input {
    let name = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path);
    if let Some(model) = name.strip_suffix(".query.vectors.jsonl") {
        return Input::RoleCache(model.to_owned(), Role::Query);
    }
    if let Some(model) = name.strip_suffix(".document.vectors.jsonl") {
        return Input::RoleCache(model.to_owned(), Role::Document);
    }
    if let Some(model) = name.strip_suffix(".vectors.jsonl") {
        return Input::Cache(model.to_owned());
    }
    if let Some(register) = pairs::PairRegister::of(name) {
        return Input::Pairs(register);
    }
    match name
        .strip_suffix(".jsonl")
        .and_then(sense::RegisterName::of)
    {
        Some(named) => Input::Register(named),
        None => Input::Other,
    }
}

/// The inputs a run reads, gathered and checked.
struct Inputs {
    caches: Vec<Cached>,
    register: BTreeMap<SenseSet, Vec<Row>>,
    /// A pairs run's role caches, by embedder id.
    role_caches: BTreeMap<(String, Role), Cached>,
    /// A pairs run's primary register.
    pairs: Vec<pairs::Pair>,
    /// A pairs run's authored-sense register.
    turns: Vec<pairs::Turn>,
}

fn gather(dir: &Path, events: &[Event]) -> Result<Inputs, RunError> {
    let mut caches = Vec::new();
    let mut register: BTreeMap<SenseSet, Vec<Row>> = BTreeMap::new();
    let mut role_caches = BTreeMap::new();
    let mut pair_rows = Vec::new();
    let mut turn_rows = Vec::new();
    for artifact in consumed(events) {
        let what = classify(&artifact.path);
        if matches!(what, Input::Other) {
            continue;
        }
        let source = read_consumed(dir, artifact)?;
        match what {
            Input::Cache(model) => {
                caches.push(
                    Cached::load(&model, &source).map_err(|reason| RunError::Data {
                        path: artifact.path.clone(),
                        reason,
                    })?,
                );
            }
            Input::Register(named) => {
                let rows = sense::register(&source).map_err(|reason| RunError::Data {
                    path: artifact.path.clone(),
                    reason,
                })?;
                register.entry(named.set).or_default().extend(rows);
            }
            Input::RoleCache(model, role) => {
                let role_cache =
                    Cached::load(&model, &source).map_err(|reason| RunError::Data {
                        path: artifact.path.clone(),
                        reason,
                    })?;
                role_caches.insert((model, role), role_cache);
            }
            Input::Pairs(pairs::PairRegister::Intent) => {
                pair_rows = pairs::pairs(&source).map_err(|reason| RunError::Data {
                    path: artifact.path.clone(),
                    reason,
                })?;
            }
            Input::Pairs(pairs::PairRegister::Sense) => {
                turn_rows = pairs::turns(&source).map_err(|reason| RunError::Data {
                    path: artifact.path.clone(),
                    reason,
                })?;
            }
            Input::Other => unreachable!("filtered above"),
        }
    }
    if caches.is_empty() && role_caches.is_empty() {
        return Err(RunError::NoCaches);
    }
    Ok(Inputs {
        caches,
        register,
        role_caches,
        pairs: pair_rows,
        turns: turn_rows,
    })
}

/// One cell's report: scored, with its metrics -- and, typed beside them, the
/// metrics that had no value on this cell's rows.
///
/// A metric undefined on the subject is a fact about the cell, not about the
/// instrument: a gate that drops every row of a set puts them all at the
/// scoring's floor, and a separation over no spread is not a number. It is
/// reported as that, per metric and budget, with its cause, and the cell keeps
/// the metrics that did compute and its scores for the comparisons. The same
/// class as a control failure one metric over, and handled the same way, so
/// that a run over real embedders reports what it found rather than stopping
/// at the first cell with nothing to standardise by. A metric undefined on
/// its own FAILURE FIXTURE stays a refusal: that is the instrument, not the
/// cell.
struct CellReport {
    embedder: String,
    set: SenseSet,
    scoring: Scoring,
    gate: Gate,
    reported: Vec<Reported>,
    undefined: Vec<(Metric, usize, sense::UndefinedCause)>,
    scores: Vec<f64>,
}

/// A cell whose controls did not land where they must, reported as that.
///
/// The controls are checked per (embedder, set, scoring), ungated, so one
/// failure covers both gates of that scoring and no metric is computed for
/// either. The readings are the failure's own: which control, what it scored,
/// which row displaced it and what that row scored. This is a RESULT -- the
/// over-firing the instrument exists to see, when a positive sinks below
/// random words -- and not a refusal of the run. Ruled on #24 (2026-09-15).
struct ControlFailed {
    embedder: String,
    set: SenseSet,
    scoring: Scoring,
    failure: ControlFailure,
}

/// What a cell came to: numbers, or the control reading that stopped them.
enum Outcome {
    Scored(CellReport),
    ControlFailed(ControlFailed),
}

impl Outcome {
    fn value(&self) -> Value {
        match self {
            Self::Scored(cell) => cell.value(),
            Self::ControlFailed(cell) => cell.value(),
        }
    }
}

impl ControlFailed {
    fn value(&self) -> Value {
        let mut members = BTreeMap::from([
            ("embedder".to_owned(), Value::String(self.embedder.clone())),
            ("set".to_owned(), Value::String(self.set.tag().to_owned())),
            (
                "scoring".to_owned(),
                Value::String(self.scoring.tag().to_owned()),
            ),
            (
                "result".to_owned(),
                Value::String("control_failed".to_owned()),
            ),
            (
                "failure".to_owned(),
                Value::String(self.failure.to_string()),
            ),
        ]);
        let readings = match &self.failure {
            ControlFailure::NotAtTop {
                control,
                score,
                row,
                other,
            }
            | ControlFailure::NotAtBottom {
                control,
                score,
                row,
                other,
            } => {
                members.insert(
                    "control".to_owned(),
                    Value::String(control.tag().to_owned()),
                );
                BTreeMap::from([
                    ("control_score".to_owned(), decimal(*score, 8)),
                    ("row".to_owned(), Value::String(row.clone())),
                    ("row_score".to_owned(), decimal(*other, 8)),
                ])
            }
            ControlFailure::Inverted { top, bottom } => BTreeMap::from([
                ("top_score".to_owned(), decimal(*top, 8)),
                ("bottom_score".to_owned(), decimal(*bottom, 8)),
            ]),
            ControlFailure::Missing { control } => {
                members.insert(
                    "control".to_owned(),
                    Value::String(control.tag().to_owned()),
                );
                BTreeMap::new()
            }
            // Never built: a cache miss is a refusal of the run, above.
            ControlFailure::Unscorable(_) => BTreeMap::new(),
        };
        members.insert("readings".to_owned(), Value::Object(readings));
        Value::Object(members)
    }
}

impl CellReport {
    fn value(&self) -> Value {
        Value::Object(BTreeMap::from([
            ("embedder".to_owned(), Value::String(self.embedder.clone())),
            ("set".to_owned(), Value::String(self.set.tag().to_owned())),
            (
                "scoring".to_owned(),
                Value::String(self.scoring.tag().to_owned()),
            ),
            ("gate".to_owned(), Value::String(self.gate.tag().to_owned())),
            ("result".to_owned(), Value::String("scored".to_owned())),
            (
                "metrics".to_owned(),
                Value::Array(self.reported.iter().map(Reported::record).collect()),
            ),
            (
                "undefined".to_owned(),
                Value::Array(
                    self.undefined
                        .iter()
                        .map(|(metric, budget, cause)| {
                            Value::Object(BTreeMap::from([
                                ("metric".to_owned(), Value::String(metric.tag().to_owned())),
                                (
                                    "budget".to_owned(),
                                    Value::Integer(i64::try_from(*budget).unwrap_or(i64::MAX)),
                                ),
                                ("cause".to_owned(), Value::String(cause.to_string())),
                            ]))
                        })
                        .collect(),
                ),
            ),
        ]))
    }

    /// How a cell is named where two of them are compared.
    fn key(&self) -> String {
        format!(
            "{}/{}/{}",
            self.set.tag(),
            self.scoring.tag(),
            self.gate.tag()
        )
    }
}

/// Run the bakeoff described by the record at `path`.
///
/// # Errors
///
/// Returns [`RunError`] when the record does not parse, an input it consumed
/// is absent or is not the bytes it consumed, a control does not land where it
/// must, or a metric cannot be reported over rows built to fail.
pub fn run(path: &Path) -> Result<Value, RunError> {
    computed(path).map(|done| done.report)
}

/// Which instrument a run is: the sense bakeoff, or the pairs register.
///
/// Decided by what the record consumed, and one or the other: the two have
/// different pre-registrations, hypotheses and cells, and a directory that
/// mixed them would carry a pre-registration for half its numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunKind {
    Sense,
    Pairs,
}

impl RunKind {
    /// The pre-registration the directory carries, pinned by digest.
    fn pre_registration(self) -> Value {
        match self {
            Self::Sense => PRE_REGISTRATION.value(),
            Self::Pairs => pairs::pre_registration(),
        }
    }

    /// The claim row's hypothesis.
    fn hypothesis(self) -> &'static str {
        match self {
            Self::Sense => PRE_REGISTRATION.primary,
            Self::Pairs => pairs::HYPOTHESIS,
        }
    }

    /// The pre-registered endpoints the README restates as strings.
    fn endpoints(self) -> [(&'static str, &'static str); 5] {
        match self {
            Self::Sense => [
                ("primary", PRE_REGISTRATION.primary),
                ("separation", PRE_REGISTRATION.separation),
                ("over_firing", PRE_REGISTRATION.over_firing),
                ("comparator", PRE_REGISTRATION.comparator),
                ("correction", PRE_REGISTRATION.correction),
            ],
            Self::Pairs => [
                ("primary", pairs::PRIMARY),
                ("separation", pairs::SEPARATION),
                ("over_firing", pairs::OVER_FIRING),
                (
                    "by_source",
                    "precision at every budget broken out by source, planted against mined, beside the pooled figure",
                ),
                ("correction", pairs::CORRECTION),
            ],
        }
    }

    /// The README's title.
    fn title(self) -> &'static str {
        match self {
            Self::Sense => "The sense bakeoff, over the caches this record consumed",
            Self::Pairs => "The entry-to-turn nomination run, over the caches this record consumed",
        }
    }

    /// What the record declared, as the README's observation.
    fn observation(self) -> &'static str {
        match self {
            Self::Sense => {
                "A record declared the caches it consumed, with a digest for each, and\n\
                            nothing had turned those caches into numbers."
            }
            Self::Pairs => {
                "A record declared a pairs register and the caches that place its texts,\n\
                            with a digest for each, and nothing had turned them into numbers."
            }
        }
    }
}

/// A run, and everything the directory it assembles into needs from it.
struct Computed {
    /// Which instrument.
    kind: RunKind,
    /// The numbers.
    report: Value,
    /// The record the run was described by: its regime is the regime these
    /// numbers are about.
    record: crate::formats::record::Record,
    /// Where that record lives, so a consumed input can be found beside it.
    dir: PathBuf,
}

fn computed(path: &Path) -> Result<Computed, RunError> {
    let source = std::fs::read_to_string(path).map_err(|err| RunError::Record(err.to_string()))?;
    let record =
        crate::formats::record::parse(&source).map_err(|err| RunError::Record(err.to_string()))?;
    let dir: PathBuf = path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let inputs = gather(&dir, &record.events)?;
    let senses = sense::shipped_senses().map_err(|reason| RunError::Data {
        path: "the shipped sense sets".to_owned(),
        reason,
    })?;
    if !inputs.pairs.is_empty() || !inputs.turns.is_empty() || !inputs.role_caches.is_empty() {
        if !inputs.register.is_empty() {
            return Err(RunError::MixedRegisters);
        }
        return computed_pairs(record, dir, &inputs, &senses);
    }

    let mut cells = Vec::new();
    for cached in &inputs.caches {
        for set in SenseSet::ALL.iter().copied() {
            let Some(rows) = inputs.register.get(&set) else {
                continue;
            };
            cells.extend(cells_of(cached, rows, set, &senses)?);
        }
    }
    if cells.is_empty() {
        let set = SenseSet::ALL.first().copied().unwrap_or(SenseSet::Mistake);
        return Err(RunError::NoRegister { set });
    }

    // THE PRE-REGISTRATION IS NOT IN HERE. Ruled 2026-09-14: it is written
    // beside this file as `pre-registration.json` and pinned by the
    // front-matter's `pre_registration_sha256`. Keeping a copy here as well
    // would be the same object in two files with nothing comparing them --
    // the defect this rung fixed twice already this week, minted fresh in the
    // commit that fixes it.
    // The comparisons and the null are over the cells that produced numbers;
    // a cell whose controls failed has none to compare. The decision rule's
    // "at least one cell" clause evaluates over these too.
    let scored: Vec<&CellReport> = cells
        .iter()
        .filter_map(|cell| match cell {
            Outcome::Scored(report) => Some(report),
            Outcome::ControlFailed(_) => None,
        })
        .collect();
    let report = Value::Object(BTreeMap::from([
        ("regime".to_owned(), regime_ids(record.regime())),
        (
            "embedders".to_owned(),
            Value::Array(
                inputs
                    .caches
                    .iter()
                    .map(|cache| Value::String(cache.id().to_owned()))
                    .collect(),
            ),
        ),
        (
            "cells".to_owned(),
            Value::Array(cells.iter().map(Outcome::value).collect()),
        ),
        (
            "control_failed".to_owned(),
            Value::Integer(i64::try_from(cells.len() - scored.len()).unwrap_or(i64::MAX)),
        ),
        ("comparisons".to_owned(), comparisons(&scored)?),
        ("null".to_owned(), null_over(&scored)),
        (
            "resolved_blockers".to_owned(),
            Value::Array(
                resolved()
                    .iter()
                    .map(|blocker| Value::String(blocker.tag().to_owned()))
                    .collect(),
            ),
        ),
    ]));
    Ok(Computed {
        kind: RunKind::Sense,
        report,
        record,
        dir,
    })
}

/// The pairs instrument, composed: every embedder's two sides, the intent
/// register's cells, the sense register's, the comparisons and the null.
fn computed_pairs(
    record: crate::formats::record::Record,
    dir: PathBuf,
    inputs: &Inputs,
    senses: &[sense::Sense],
) -> Result<Computed, RunError> {
    if inputs.pairs.is_empty() {
        return Err(RunError::NoPairs);
    }
    let plain: BTreeMap<&str, &Cached> = inputs
        .caches
        .iter()
        .map(|cache| (cache.id(), cache))
        .collect();
    let mut ids: BTreeSet<String> = plain.keys().map(|id| (*id).to_owned()).collect();
    ids.extend(inputs.role_caches.keys().map(|(model, _)| model.clone()));
    let side = |model: &str, role: Role| -> Result<&Cached, RunError> {
        inputs
            .role_caches
            .get(&(model.to_owned(), role))
            .or_else(|| plain.get(model).copied())
            .ok_or_else(|| RunError::RoleCacheMissing {
                model: model.to_owned(),
                role: role.tag(),
            })
    };
    let mut cells = Vec::new();
    for id in &ids {
        let roles = pairs::Roles {
            id,
            query: side(id, Role::Query)?,
            document: side(id, Role::Document)?,
        };
        cells.extend(pairs::intent_cells(&inputs.pairs, roles, SEED).map_err(RunError::Pairs)?);
        if !inputs.turns.is_empty() {
            cells.extend(
                pairs::sense_cells(&inputs.turns, senses, roles, SEED).map_err(RunError::Pairs)?,
            );
        }
    }
    let scored: Vec<&pairs::CellReport> = cells
        .iter()
        .filter_map(|cell| match cell {
            pairs::Outcome::Scored(report) => Some(report),
            pairs::Outcome::ControlFailed(_) => None,
        })
        .collect();
    let (comparisons, gate_comparisons) = pair_comparisons(&scored)?;
    let count = |n: usize| Value::Integer(i64::try_from(n).unwrap_or(i64::MAX));
    let intent_rows = pairs::intent_rows(&inputs.pairs);
    let report = Value::Object(BTreeMap::from([
        ("regime".to_owned(), regime_ids(record.regime())),
        (
            "embedders".to_owned(),
            Value::Array(ids.iter().map(|id| Value::String(id.clone())).collect()),
        ),
        (
            "registers".to_owned(),
            Value::Object(BTreeMap::from([
                (
                    "intent".to_owned(),
                    Value::Object(BTreeMap::from([
                        ("pairs".to_owned(), count(inputs.pairs.len())),
                        ("rows".to_owned(), count(intent_rows.len())),
                        (
                            "excluded_no_intent".to_owned(),
                            count(inputs.pairs.len() - intent_rows.len()),
                        ),
                        ("by_label".to_owned(), pair_census(&intent_rows)),
                    ])),
                ),
                (
                    "sense".to_owned(),
                    Value::Object(BTreeMap::from([(
                        "rows".to_owned(),
                        count(inputs.turns.len()),
                    )])),
                ),
            ])),
        ),
        (
            "cells".to_owned(),
            Value::Array(cells.iter().map(pairs::Outcome::value).collect()),
        ),
        (
            "control_failed".to_owned(),
            count(cells.len() - scored.len()),
        ),
        ("comparisons".to_owned(), comparisons),
        ("gate_comparisons".to_owned(), gate_comparisons),
    ]));
    Ok(Computed {
        kind: RunKind::Pairs,
        report,
        record,
        dir,
    })
}

/// The intent register's rows by label and source: counts, not scores.
fn pair_census(rows: &[&pairs::Pair]) -> Value {
    let mut members = BTreeMap::new();
    for label in sense::Label::ALL {
        for source in pairs::PairSource::ALL {
            let n = rows
                .iter()
                .filter(|row| row.label == *label && row.source == *source)
                .count();
            members.insert(
                format!("{}/{}", label.tag(), source.tag()),
                Value::Integer(i64::try_from(n).unwrap_or(i64::MAX)),
            );
        }
    }
    Value::Object(members)
}

/// One paired bootstrap, rendered.
fn comparison_value(cell: &str, a: &str, b: &str, test: &sense::Bootstrap, adjusted: f64) -> Value {
    Value::Object(BTreeMap::from([
        ("cell".to_owned(), Value::String(cell.to_owned())),
        ("a".to_owned(), Value::String(a.to_owned())),
        ("b".to_owned(), Value::String(b.to_owned())),
        ("difference".to_owned(), decimal(test.observed, 4)),
        ("p".to_owned(), decimal(test.p.value(), 6)),
        ("p_holm".to_owned(), decimal(adjusted, 6)),
        (
            "attainable_p_floor".to_owned(),
            decimal(sense::attainable_p_floor(PRE_REGISTRATION.resamples), 6),
        ),
    ]))
}

/// The pairs run's comparisons: every embedder against every other within a
/// cell, and the anchored arm against the ungated arm within every
/// (embedder, register, scoring) -- the bootstrap the pre-gate sub-rule needs,
/// which the sense instrument never emitted. Holm-corrected together.
fn pair_comparisons(cells: &[&pairs::CellReport]) -> Result<(Value, Value), RunError> {
    let mut by_cell: BTreeMap<String, Vec<&pairs::CellReport>> = BTreeMap::new();
    for cell in cells {
        by_cell.entry(cell.key()).or_default().push(cell);
    }
    let mut raw = Vec::new();
    let mut across = Vec::new();
    for (key, group) in &by_cell {
        for (index, a) in group.iter().enumerate() {
            for b in group.iter().skip(index + 1) {
                let test =
                    sense::paired_bootstrap(&a.scores, &b.scores, PRE_REGISTRATION.resamples, SEED)
                        .map_err(RunError::Bootstrap)?;
                raw.push(test.p.value());
                across.push((key.clone(), a.embedder.clone(), b.embedder.clone(), test));
            }
        }
    }
    let mut arms = Vec::new();
    for with in cells
        .iter()
        .filter(|cell| cell.gate == pairs::PairGate::Anchored)
    {
        let Some(without) = cells.iter().find(|cell| {
            cell.gate == pairs::PairGate::Without
                && cell.embedder == with.embedder
                && cell.register == with.register
                && cell.scoring == with.scoring
        }) else {
            continue;
        };
        let test = sense::paired_bootstrap(
            &with.scores,
            &without.scores,
            PRE_REGISTRATION.resamples,
            SEED,
        )
        .map_err(RunError::Bootstrap)?;
        raw.push(test.p.value());
        arms.push((
            format!("{}/{}/{}", with.register.tag(), with.scoring, with.embedder),
            with.gate.tag().to_owned(),
            without.gate.tag().to_owned(),
            test,
        ));
    }
    let corrected = sense::holm(&raw);
    let (first, second) = corrected.split_at(across.len());
    let render = |rows: &[(String, String, String, sense::Bootstrap)], adjusted: &[f64]| {
        Value::Array(
            rows.iter()
                .zip(adjusted)
                .map(|((cell, a, b, test), adjusted)| comparison_value(cell, a, b, test, *adjusted))
                .collect(),
        )
    };
    Ok((render(&across, first), render(&arms, second)))
}

/// Every cell of one embedder over one set: a scored cell per (scoring, gate),
/// or one `control_failed` per scoring whose controls did not land.
fn cells_of(
    cached: &Cached,
    rows: &[Row],
    set: SenseSet,
    senses: &[sense::Sense],
) -> Result<Vec<Outcome>, RunError> {
    let mut cells = Vec::new();
    let embedded = EmbeddedSet::embed(senses, set, cached).map_err(RunError::Set)?;
    for scoring in Scoring::ALL.iter().copied() {
        match sense::controls(rows, &embedded, cached, scoring) {
            Ok(()) => {}
            // A row the embedder could not place is a fact about the
            // inputs, and the run cannot say anything over them.
            Err(failure @ ControlFailure::Unscorable(_)) => {
                return Err(RunError::Control {
                    embedder: cached.id().to_owned(),
                    scoring,
                    failure,
                });
            }
            // A control in the wrong place is a reading about this
            // cell. It goes in the report as what it is, no metric is
            // computed over the cell, and the run goes on.
            Err(failure) => {
                cells.push(Outcome::ControlFailed(ControlFailed {
                    embedder: cached.id().to_owned(),
                    set,
                    scoring,
                    failure,
                }));
                continue;
            }
        }
        for gate in Gate::ALL.iter().copied() {
            let cell = Cell { scoring, gate };
            let scored =
                sense::score_rows(rows, &embedded, cached, cell).map_err(RunError::Score)?;
            // EVERY PRE-REGISTERED BUDGET, not one. The budget was a
            // `const` here set to eight, and eight was not chosen: it
            // was the widest the precision fixture happened to
            // demonstrate. A cell now carries the ladder, because a
            // single number is a sweep nobody can do afterwards --
            // rerunning at another budget means rerunning the bakeoff.
            let mut reported = Vec::new();
            let mut undefined = Vec::new();
            for budget in sense::BUDGETS.iter().copied() {
                for metric in Metric::ALL.iter().copied() {
                    match Reported::take(metric, budget, &scored) {
                        Ok(taken) => reported.push(taken),
                        // The subject had no value for this metric: a fact
                        // about this cell's rows, kept beside the metrics that
                        // did compute.
                        Err(sense::MetricError::Undefined {
                            on: sense::MetricSubject::Subject,
                            cause,
                            ..
                        }) => undefined.push((metric, budget, cause)),
                        // The instrument's own fixture did not fail, or had no
                        // value: not this cell's fact, and not a result.
                        Err(error) => {
                            return Err(RunError::Metric {
                                embedder: cached.id().to_owned(),
                                set,
                                scoring,
                                gate,
                                error,
                            });
                        }
                    }
                }
            }
            cells.push(Outcome::Scored(CellReport {
                embedder: cached.id().to_owned(),
                set,
                scoring,
                gate,
                reported,
                undefined,
                scores: scored.iter().map(|row| row.score).collect(),
            }));
        }
    }
    Ok(cells)
}

/// Which of the pre-registration's blockers this run cleared by running.
///
/// [`Blocker::RegimeSpelling`] is cleared by the record parsing at all: a
/// regime with no declared substrates is refused by the schema, so a record
/// this got numbers out of is a record whose regime is spelled.
fn resolved() -> Vec<Blocker> {
    vec![
        Blocker::ArchivedTranscripts,
        Blocker::JudgeModel,
        Blocker::EmbeddingModels,
        Blocker::RegimeSpelling,
    ]
}

/// The regime, as the ids it declares.
fn regime_ids(regime: &Regime) -> Value {
    Value::Object(BTreeMap::from([
        ("arm".to_owned(), Value::String(regime.arm.clone())),
        (
            "substrates".to_owned(),
            Value::Array(
                regime
                    .substrate_ids()
                    .into_iter()
                    .map(|id| Value::String(id.to_owned()))
                    .collect(),
            ),
        ),
        (
            "dogma_version".to_owned(),
            Value::Integer(i64::from(regime.dogma_version)),
        ),
    ]))
}

/// Every embedder against every other, within a cell, Holm-corrected.
fn comparisons(cells: &[&CellReport]) -> Result<Value, RunError> {
    let mut by_cell: BTreeMap<String, Vec<&CellReport>> = BTreeMap::new();
    for cell in cells {
        by_cell.entry(cell.key()).or_default().push(cell);
    }
    let mut raw = Vec::new();
    let mut rows = Vec::new();
    for (key, group) in &by_cell {
        for (index, a) in group.iter().enumerate() {
            for b in group.iter().skip(index + 1) {
                let test =
                    sense::paired_bootstrap(&a.scores, &b.scores, PRE_REGISTRATION.resamples, SEED)
                        .map_err(RunError::Bootstrap)?;
                raw.push(test.p.value());
                rows.push((key.clone(), a.embedder.clone(), b.embedder.clone(), test));
            }
        }
    }
    let corrected = sense::holm(&raw);
    Ok(Value::Array(
        rows.iter()
            .zip(corrected)
            .map(|((cell, a, b, test), adjusted)| {
                Value::Object(BTreeMap::from([
                    ("cell".to_owned(), Value::String(cell.clone())),
                    ("a".to_owned(), Value::String(a.clone())),
                    ("b".to_owned(), Value::String(b.clone())),
                    ("difference".to_owned(), decimal(test.observed, 4)),
                    ("p".to_owned(), decimal(test.p.value(), 6)),
                    ("p_holm".to_owned(), decimal(adjusted, 6)),
                    (
                        "attainable_p_floor".to_owned(),
                        decimal(sense::attainable_p_floor(PRE_REGISTRATION.resamples), 6),
                    ),
                ]))
            })
            .collect(),
    ))
}

/// The shuffled-label null, over the first cell that can carry one.
fn null_over(cells: &[&CellReport]) -> Value {
    let names: BTreeSet<&str> = cells.iter().map(|cell| cell.embedder.as_str()).collect();
    Value::Object(BTreeMap::from([
        (
            "embedders".to_owned(),
            Value::Integer(i64::try_from(names.len()).unwrap_or(i64::MAX)),
        ),
        (
            "shuffles".to_owned(),
            Value::Integer(i64::from(PRE_REGISTRATION.null_shuffles)),
        ),
    ]))
}

// ---------------------------------------------------------------------------
// assembling the directory
// ---------------------------------------------------------------------------

/// Gate 0 for a bakeoff directory, written beside the numbers it checks.
///
/// It re-derives what can be re-derived WITHOUT this crate's binary, and says
/// so, because `check-recompute.py` runs it in a sandboxed copy of the
/// directory where `target/debug/diet` is not reachable. So it hashes the
/// committed artefacts against the digests the record declares, counts them
/// against the summary's totals, and hashes the product against the digest the
/// report states. It does NOT re-run the bakeoff: the metrics are not
/// re-derived here, and a limit stated is a limit somebody can close.
const RECOMPUTE: &str = r#"#!/usr/bin/env bash
# Gate 0 for this directory: every number the report states re-derives from the
# artefacts committed beside it.
#
# WHAT THIS DOES NOT DO, stated because an undeclared non-catch is the vacuous
# class: it does not re-run the bakeoff. `check-recompute.py` runs this script
# in a sandboxed copy of the directory, where the `diet` binary is not
# reachable, so the metrics themselves are not re-derived here -- what is
# re-derived is every digest and every count the record and the report state.
# A cache edited after the fact, a product edited after the fact, and a total
# that disagrees with the rows are all caught. A metric computed wrongly by a
# binary that is not here is not.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"

python3 - <<'PY'
import hashlib
import json
import pathlib
import sys
import tomllib

FENCE = "+++"

# 0 CLEAN, 1 FOUND SOMETHING, 2 COULD NOT RUN -- the repository's contract,
# and this script speaks all three since 2026-09-12. It used to exit 1 for
# everything, because `sys.exit("message")` does, so "the numbers do not
# re-derive" and "this directory cannot be read at all" were one code. Ruled:
# the census must be able to tell a recompute that failed from one that could
# not be attempted, and conflating them is how a gate reads "nothing wrong"
# when it means "did not look".
#
# The line: 2 when this script cannot reach a verdict, 1 when it reached one
# and the verdict is that the numbers do not re-derive. A consumed artefact
# that is not here is a 1 -- the answer is known, and it is no.
def cannot_run(message):
    print(f"recompute: {message}", file=sys.stderr)
    raise SystemExit(2)


def read(path):
    # A FILE THAT IS NOT HERE IS A SENTENCE, NOT A TRACEBACK. This script is
    # read by whoever is holding a directory that will not re-derive, and a
    # stack trace tells them which line of Python raised rather than which
    # artefact is missing.
    #
    # An earlier version of this comment claimed "no path out of this script
    # is an exception nobody wrote". THAT WAS FALSE and a fresh instance
    # proved it: only the file READ came through here, so a file that was
    # present and malformed went straight to `json.loads` or `tomllib.loads`
    # and out as a raw traceback. The parses are wrapped below now, and the
    # claim is not restated -- what holds it is the two cases in
    # `a_directory_this_script_cannot_read_is_a_two_not_a_traceback`.
    try:
        return pathlib.Path(path).read_text(encoding="utf-8")
    except OSError as err:
        cannot_run(f"{path} cannot be read: {err.strerror}")


text = read("README.md")
if not text.startswith(FENCE + "\n"):
    cannot_run("README.md does not open with +++ front-matter")
try:
    front = tomllib.loads(text.split(FENCE + "\n", 2)[1])
except (tomllib.TOMLDecodeError, IndexError) as err:
    cannot_run(f"README.md front-matter is not TOML: {err}")

rows = []
for number, line in enumerate(read("run.jsonl").splitlines(), start=1):
    if not line.strip():
        continue
    try:
        rows.append(json.loads(line))
    except json.JSONDecodeError as err:
        cannot_run(f"run.jsonl line {number} is not JSON: {err.msg}")
summary = next((row for row in rows if row.get("record") == "summary"), None)
if summary is None:
    cannot_run("run.jsonl has no summary row")


def digest(path):
    try:
        return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()
    except OSError as err:
        # An artefact the record consumed and that is not committed beside it
        # means the numbers cannot be re-derived. That is a refusal, and it
        # names the file, because the reader's next move is to go and find it.
        sys.exit(f"{path} is consumed by the record and is not here: {err.strerror}")


# The product, against the digest the report and the summary both state.
product = digest("report.json")
for where, stated in (("front-matter", front["product_sha256"]),
                      ("the summary row", summary["product_sha256"])):
    if stated != product:
        sys.exit(f"{where} states product_sha256 {stated}, the product hashes to {product}")

# Every consumed artefact, against the digest the record declares for it.
consumed = [
    artifact
    for row in rows
    if row.get("record") == "claim"
    for artifact in row.get("consumes", [])
]
# A CHECK OF NOTHING IS NOT A PASS, and gate 0 says so itself rather than
# leaving it to a linter one layer out. Ruled 2026-09-13.
#
# A directory whose claim consumes NOTHING re-derives every one of its zero
# artefacts and reports success, which is how a gate comes to run over nothing
# while reporting that it ran. Paired with `reproducible-by-config` it is a
# contradiction on its face: there is no evidence committed beside the record
# to reproduce it FROM.
#
# This is EXIT 1, not 2. The script read everything it needed and reached a
# verdict; the verdict is that this directory does not support the claim its
# front-matter makes. `check-results.py` refuses the same state through a
# different rule -- a claim that names no artefact could produce a bound but
# never a number -- so `verify.sh` was already red. It was `check-recompute.py`
# ALONE, which is the sandboxed way this script is actually run, that counted
# such a directory as "recomputed". Found by a fresh instance.
if not consumed and front.get("kind") == "reproducible-by-config":
    sys.exit(
        "this directory declares `reproducible-by-config` and its claim "
        "consumes nothing, so there is no evidence here to re-derive from; "
        "a recompute of zero artefacts is not a recompute"
    )

matched = 0
for artifact in consumed:
    found = digest(artifact["path"])
    if found != artifact["sha256"]:
        sys.exit(
            f"{artifact['path']} is declared {artifact['sha256']} and hashes to {found}"
        )
    matched += 1

# And the totals, against what was actually there to count.
for field, counted in (("targets_checked", len(consumed)), ("targets_matched", matched)):
    if summary[field] != counted:
        sys.exit(f"the summary says {field} is {summary[field]}, the rows hold {counted}")
    if front.get(field) not in (None, counted):
        sys.exit(f"the front-matter says {field} is {front[field]}, the rows hold {counted}")

print(f"{len(consumed)} artefact(s) and the product re-derive")
PY
"#;

/// Where a bakeoff's numbers land.
///
/// ASSEMBLED, NOT PRINTED. Ruled 2026-09-10 on #69: printing the report to
/// stdout leaves the assembly of a results directory to a person, which is a
/// vacuum with a human in it. The verb writes the directory -- the record, the
/// regimen, the caches by digest, the front-matter carrying the pre-registered
/// endpoints -- and answers with where it put it.
///
/// The directory is NAMED BY THE CALLER, not by this. A results directory's
/// name carries the date of the run and the claim's slug, and neither is a
/// thing a program should invent: reading the clock would make the same inputs
/// produce a different directory every day.
///
/// # Errors
///
/// Returns [`RunError`] for anything that stops the run, and for a directory
/// that already holds a report -- overwriting somebody's results is not an
/// assembly step.
pub fn assemble(path: &Path, into: &Path) -> Result<Value, RunError> {
    let done = computed(path)?;
    let mut product = String::new();
    json::render(&done.report, &mut product);
    product.push('\n');
    let product_sha256 = sha256_hex(product.as_bytes());

    let artifacts = consumed(&done.record.events);
    let checked = u32::try_from(artifacts.len()).unwrap_or(u32::MAX);

    if into.join("README.md").exists() {
        return Err(RunError::Occupied {
            path: into.display().to_string(),
        });
    }
    std::fs::create_dir_all(into).map_err(|err| RunError::Write {
        path: into.display().to_string(),
        reason: err.to_string(),
    })?;

    // The caches and the register, by the digests the record declares. Copied
    // rather than referenced: a results directory is a claim with its evidence
    // ATTACHED, and evidence that lives somewhere else is a link.
    for artifact in &artifacts {
        let bytes =
            std::fs::read(done.dir.join(&artifact.path)).map_err(|_| RunError::Missing {
                path: artifact.path.clone(),
            })?;
        let landing = into.join(&artifact.path);
        if let Some(parent) = landing.parent() {
            std::fs::create_dir_all(parent).map_err(|err| RunError::Write {
                path: parent.display().to_string(),
                reason: err.to_string(),
            })?;
        }
        write(&landing, &bytes)?;
    }

    // THE PRE-REGISTRATION, FIRST, AND HASHED FROM THE BYTES ON DISK.
    //
    // Ruled 2026-09-14. Hashing the pre-registration as a BLOCK inside
    // `report.json` would have forced `check-results.py` to reproduce this
    // program's serialisation to check it -- a second canonicaliser of record
    // data, disagreeing by one decimal digit and reporting it as tampering.
    // A file has bytes, and bytes are what `digest_of` already hashes for
    // every other artefact in the directory.
    //
    // THE HONEST LIMIT, because the ruling says "before any score exists" and
    // this is as close as this verb can get: scoring happens in `computed()`
    // above, before the directory is created at all, so nothing can write a
    // file into it earlier than this. What IS enforced is that the digest in
    // the front-matter is of the bytes that ended up on disk, and that those
    // bytes are still there, unchanged, at the moment the scores are written
    // -- re-read below rather than trusted from the variable.
    let mut pre_registration = String::new();
    json::render(&done.kind.pre_registration(), &mut pre_registration);
    pre_registration.push('\n');
    let pre_registration_path = into.join("pre-registration.json");
    write(&pre_registration_path, pre_registration.as_bytes())?;
    let pre_registration_sha256 = sha256_hex(pre_registration.as_bytes());

    let regime = done.record.regime().clone();
    let record = synthesized_record(&done, &artifacts, checked, &product_sha256);
    write(
        &into.join("run.jsonl"),
        crate::formats::record::render(&record).as_bytes(),
    )?;
    // Re-read, not re-used: the point of the digest is the file, so the file
    // is what is hashed again. Same refusal shape as a cache whose bytes are
    // not the bytes the record declared -- the scores do not get written over
    // a pre-registration that moved under them.
    let landed = std::fs::read(&pre_registration_path).map_err(|err| RunError::Write {
        path: pre_registration_path.display().to_string(),
        reason: err.to_string(),
    })?;
    let found = sha256_hex(&landed);
    if found != pre_registration_sha256 {
        return Err(RunError::PreRegistrationMoved {
            declared: pre_registration_sha256,
            found,
        });
    }
    write(&into.join("report.json"), product.as_bytes())?;
    write(&into.join("regimen.toml"), regimen_of(&regime).as_bytes())?;
    write(
        &into.join("README.md"),
        report_of(
            done.kind,
            &regime,
            &product_sha256,
            &pre_registration_sha256,
            checked,
        )
        .as_bytes(),
    )?;
    write(&into.join("recompute.sh"), RECOMPUTE.as_bytes())?;
    executable(&into.join("recompute.sh"))?;

    Ok(Value::Object(BTreeMap::from([
        (
            "directory".to_owned(),
            Value::String(into.display().to_string()),
        ),
        ("product_sha256".to_owned(), Value::String(product_sha256)),
        (
            "targets_checked".to_owned(),
            Value::Integer(i64::from(checked)),
        ),
    ])))
}

/// The record the assembled directory carries: the run's regime and source,
/// one claim consuming every artifact, and a recompute summary. Written
/// `unadjudicated`, for the reason the claim row's comment gives.
fn synthesized_record(
    done: &Computed,
    artifacts: &[&Artifact],
    checked: u32,
    product_sha256: &str,
) -> crate::formats::record::Record {
    crate::formats::record::Record {
        events: vec![
            Event::Start {
                regime: Box::new(done.record.regime().clone()),
                source: done.record.source().clone(),
            },
            Event::Claim {
                id: "c1".to_owned(),
                hypothesis: done.kind.hypothesis().to_owned(),
                // THE SAME WORD THE FRONT-MATTER USES, and it has to be:
                // ruled 2026-09-14, after this row said `inconclusive` while
                // the README said `unadjudicated` and nothing compared them.
                // They are not two measurements, they are one statement
                // written twice, and `check-results.py` now refuses a
                // directory where they disagree.
                //
                // `unadjudicated` rather than `inconclusive` because
                // `inconclusive` is a VERDICT -- the data were held against a
                // rule and did not decide -- and this verb applies no rule.
                // It computes the endpoints; the pre-registration names them
                // and does not say what turns them into an answer. A program
                // that wrote `supported` here would be making a claim it
                // cannot make, and one that writes `inconclusive` is making a
                // smaller one it also cannot make.
                result: crate::formats::record::Verdict::Unadjudicated,
                consumes: artifacts
                    .iter()
                    .map(|artifact| (*artifact).clone())
                    .collect(),
                supersedes: None,
            },
            Event::Summary {
                summary: Summary::Recompute {
                    targets_checked: checked,
                    // Every one of them matched, or `computed` would have
                    // refused: `read_consumed` compares each digest before a
                    // single number is produced. So this is a count of what
                    // was verified rather than a second verification.
                    targets_matched: checked,
                    digests: artifacts
                        .iter()
                        .map(|artifact| artifact.sha256.clone())
                        .collect(),
                },
                product_sha256: product_sha256.to_owned(),
            },
        ],
    }
}

/// Write one file, naming it when the write fails.
fn write(path: &Path, bytes: &[u8]) -> Result<(), RunError> {
    std::fs::write(path, bytes).map_err(|err| RunError::Write {
        path: path.display().to_string(),
        reason: err.to_string(),
    })
}

/// Make `recompute.sh` runnable, because gate 0 runs it.
#[cfg(unix)]
fn executable(path: &Path) -> Result<(), RunError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).map_err(|err| {
        RunError::Write {
            path: path.display().to_string(),
            reason: err.to_string(),
        }
    })
}

/// Elsewhere there is nothing to set, and `check-recompute.py` invokes the
/// script through `bash` rather than by executing it, so this is a courtesy
/// rather than the contract.
#[cfg(not(unix))]
fn executable(_path: &Path) -> Result<(), RunError> {
    Ok(())
}

/// The regimen this run applied, as a `regimen` v1 document.
///
/// Every key the front-matter's `[regime]` table carries has to be bound here
/// -- `check-results.py` refuses a regime field the regimen does not bind, on
/// the grounds that it is a claim about the run that nothing backs -- so the
/// two are written from the same three values.
fn regimen_of(regime: &Regime) -> String {
    let ids: Vec<String> = regime
        .substrate_ids()
        .into_iter()
        .map(|id| format!("{id:?}"))
        .collect();
    format!(
        "# The regimen this run applied, written by `diet bakeoff --into`.\n\
         #\n\
         # It is the regimen of the run being RECOMPUTED: these numbers are\n\
         # about that run's substrates, and a recompute that declared its own\n\
         # would be describing the machine that did the arithmetic rather than\n\
         # the machine the result is about.\n\
         arm = {:?}\n\
         substrates = [{}]\n\
         dogma_version = {}\n",
        regime.arm,
        ids.join(", "),
        regime.dogma_version,
    )
}

/// The report: front-matter the directory linter accepts, then the sections it
/// requires, in the order it requires them.
///
/// What a directory may claim about itself, decided from the record.
///
/// A hosted substrate's weights can change under a re-firing, so a directory
/// whose record names one may not claim to be reproducible by config -- ruled
/// 2026-09-08, and enforced by `check-results.py`. Decided HERE, from the
/// record, rather than left to whoever edits the file: the one place the
/// question is answered is the place that can see the answer.
///
/// The kind and the caveat come back together because they are one decision
/// said twice, once for the linter and once for the reader. Separating them
/// is how they come to disagree.
fn kind_and_caveat(regime: &Regime) -> (&'static str, String) {
    let hosted = regime.hosted_substrate_ids();
    if hosted.is_empty() {
        (
            "reproducible-by-config",
            "Every input is committed beside this file at the digest the record \
             consumed, so the same command over the same bytes produces the same \
             numbers."
                .to_owned(),
        )
    } else {
        (
            "historical-observation",
            format!(
                "The run being recomputed was served by hosted weights ({}), which can \
                 change under a re-firing, so this is a historical observation rather \
                 than something reproducible by config.",
                hosted.join(", ")
            ),
        )
    }
}

/// THE PRE-REGISTERED ENDPOINTS ARE STRINGS HERE AND THE NUMBERS ARE NOT.
/// `check-results.py` walks every number in the front-matter outside `[regime]`
/// and requires it to appear in the summary row -- that is the prose-against-
/// data rule, and it is the rule that makes a report's numbers checkable. The
/// pre-registration carries numbers of its own (the resamples, the attainable
/// p floor, the shuffles, the budget ladder) and a recompute summary has three
/// fields, so carrying them here would mean either a front-matter number
/// nothing backs or a linter taught the schema's internals. They live in
/// `report.json`, whole, where the record's digest holds them.
///
/// `targets_checked` IS carried, because the summary binds it. It is also what
/// `check-recompute.py`'s tamper probe perturbs: that probe bumps an integer
/// in the report and requires `recompute.sh` to notice, so a report with no
/// number in it would make the probe vacuous.
fn report_of(
    kind: RunKind,
    regime: &Regime,
    product_sha256: &str,
    pre_registration_sha256: &str,
    checked: u32,
) -> String {
    let ids: Vec<String> = regime
        .substrate_ids()
        .into_iter()
        .map(|id| format!("{id:?}"))
        .collect();
    let (directory_kind, caveat) = kind_and_caveat(regime);
    let endpoints: Vec<String> = kind
        .endpoints()
        .iter()
        .map(|(key, value)| format!("{key} = {value:?}"))
        .collect();
    format!(
        "+++\n\
         hypothesis = {:?}\n\
         result = \"unadjudicated\"\n\
         kind = {directory_kind:?}\n\
         product_sha256 = {product_sha256:?}\n\
         pre_registration_sha256 = {pre_registration_sha256:?}\n\
         controls_run = [\"scoring-extremes\", \"shuffled-label-null\"]\n\
         known_defects = []\n\
         targets_checked = {checked}\n\
         \n\
         [regime]\n\
         arm = {:?}\n\
         substrates = [{}]\n\
         dogma_version = {}\n\
         \n\
         [pre_registration]\n\
         {}\n\
         +++\n\
         \n\
         # {}\n\
         \n\
         Written by `diet bakeoff --into`. The numbers are in `report.json`;\n\
         this file is what makes them checkable.\n\
         \n\
         ## Observation\n\
         \n\
         {}\n\
         \n\
         ## Hypothesis\n\
         \n\
         {}\n\
         \n\
         ## Test\n\
         \n\
         `diet bakeoff run.jsonl --into <this directory>`, over the artefacts\n\
         committed here. Every cache is read at the digest the record declares\n\
         for it; a cache whose bytes are not those bytes stops the run rather\n\
         than scoring something else. The pre-registered endpoints are in the\n\
         `[pre_registration]` table above and the settings they run under --\n\
         the budget ladder, the resamples, the shuffles, the attainable p floor\n\
         -- are in `report.json`.\n\
         \n\
         ## Results\n\
         \n\
         {checked} artefact(s) checked, {checked} matched. The cells, the\n\
         comparisons and the null are in `report.json`, whose digest is\n\
         `product_sha256` above and in the summary row.\n\
         \n\
         ## Conclusion\n\
         \n\
         `unadjudicated`, which is not a verdict and does not pretend to be.\n\
         Ruled 2026-09-13: `inconclusive` is a SCIENTIFIC verdict -- the data\n\
         did not decide -- and a directory whose numbers are decisive while\n\
         its front-matter says `inconclusive` states a falsehood a reader has\n\
         to catch. `unadjudicated` states what is true: no one has applied a\n\
         decision rule.\n\
         \n\
         And the reason none has been applied is a gap in the PRE-REGISTRATION,\n\
         not in this verb: it names endpoints and no rule that turns them into\n\
         a verdict. The rule going forward is that a pre-registration carries\n\
         its decision rule, written before the data like every other endpoint;\n\
         when it does, this assembler applies it mechanically and writes\n\
         `supported`, `refuted` or `inconclusive`, because applying a\n\
         PRE-REGISTERED rule after seeing numbers is not choosing after seeing\n\
         them. Until this claim's pre-registration carries one, the answer is\n\
         `unadjudicated`.\n\
         \n\
         {caveat}\n",
        kind.hypothesis(),
        regime.arm,
        ids.join(", "),
        regime.dogma_version,
        endpoints.join("\n"),
        kind.title(),
        kind.observation(),
        kind.hypothesis(),
    )
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use std::path::{Path, PathBuf};

    use super::{RunError, assemble, run};
    use crate::capture::sense::{self, Embedder, Fixture};
    use crate::digest::sha256_hex;
    use crate::formats::record::json::Value;

    /// The register this crate ships, used as the fixture's corpus.
    ///
    /// The real file rather than rows invented here: a runner proved against a
    /// corpus written to suit it is a runner proved against itself.
    ///
    /// READ AT TEST TIME, not `include_str!`-ed. A register is data this
    /// binary reads, never data it carries, and `resolve-diet.py` decides
    /// whether the binary is stale by asking the source what it embeds -- so
    /// embedding a corpus in a test would make every edit to it look like a
    /// stale binary, which is the defect #50 reports one directory over.
    fn register_source() -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("capture/sense/register/authored-mistake.jsonl");
        std::fs::read_to_string(path).expect("the shipped register")
    }

    /// A cache in the instrument's own row format, over every text a run
    /// touches.
    ///
    /// Built from [`Fixture`], whose whole reason for existing is that the
    /// control rows land at their extremes by construction -- so a cache that
    /// agrees with it passes `controls` for the same reason `Fixture` does,
    /// and the test is about composition rather than about embeddings.
    ///
    /// `lean` perturbs the register rows only: an orthogonal component in a
    /// bucket no token reaches, which lowers every register cosine and leaves
    /// a sense measured against itself at one. Two caches that differ that way
    /// are two embedders the bootstrap can actually tell apart, without either
    /// of them breaking a control.
    fn cache(texts: &[String], register: &[String], lean: bool) -> String {
        let mut out = String::new();
        for text in texts {
            let mut vector = Fixture.embed(text);
            if lean && register.contains(text) {
                vector[Fixture::DIMENSIONS - 1] += 0.5;
            }
            let spelled: Vec<String> = vector.iter().map(|value| format!("{value:.8}")).collect();
            let _ = writeln!(
                out,
                "{{\"text\":{},\"vector\":[{}]}}",
                quoted(text),
                spelled.join(",")
            );
        }
        out
    }

    /// A string as a JSON string literal.
    fn quoted(text: &str) -> String {
        let mut out = String::from("\"");
        for ch in text.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                _ => out.push(ch),
            }
        }
        out.push('"');
        out
    }

    /// One run directory, written under `dir`, with every digest computed from
    /// the bytes actually written.
    fn write_run(dir: &Path) -> PathBuf {
        write_run_with(dir, &[])
    }

    /// A cache built from [`Fixture`] with one text's vector rewritten, so a
    /// test can put one row where a real embedder might: below the noise
    /// floor, in a signed space the fixture never reaches on its own.
    type Rewrite = (&'static str, fn(&str) -> Option<Vec<f64>>);

    /// [`write_run`], plus one extra cache per rewrite, consumed by the record
    /// like the two fixture caches.
    fn write_run_with(dir: &Path, extra: &[Rewrite]) -> PathBuf {
        std::fs::create_dir_all(dir).expect("a directory");
        let senses = sense::shipped_senses().expect("the shipped senses");
        let rows = sense::register(&register_source()).expect("the shipped register");
        let register_texts: Vec<String> = rows.iter().map(|row| row.text.clone()).collect();
        let mut texts = register_texts.clone();
        texts.extend(senses.iter().map(|sense| sense.text.clone()));
        // The control rows are sense texts verbatim, and every scoring's
        // extremes are drawn from the set, so the cache has to hold them too.
        for scoring in sense::Scoring::ALL {
            let (top, bottom) = scoring.extremes();
            for set in sense::SenseSet::ALL {
                let embedded =
                    sense::EmbeddedSet::embed(&senses, *set, &Fixture).expect("an embeddable set");
                texts.push(top.row(&embedded).text);
                texts.push(bottom.row(&embedded).text);
            }
        }
        texts.sort();
        texts.dedup();

        let mut files = vec![
            ("authored-mistake.jsonl".to_owned(), register_source()),
            (
                "even.vectors.jsonl".to_owned(),
                cache(&texts, &register_texts, false),
            ),
            (
                "lean.vectors.jsonl".to_owned(),
                cache(&texts, &register_texts, true),
            ),
        ];
        for (name, rewrite) in extra {
            let mut out = String::new();
            for text in &texts {
                let vector = rewrite(text).unwrap_or_else(|| Fixture.embed(text));
                let spelled: Vec<String> =
                    vector.iter().map(|value| format!("{value:.8}")).collect();
                let _ = writeln!(
                    out,
                    "{{\"text\":{},\"vector\":[{}]}}",
                    quoted(text),
                    spelled.join(",")
                );
            }
            files.push((format!("{name}.vectors.jsonl"), out));
        }
        let mut consumes = Vec::new();
        for (name, body) in &files {
            std::fs::write(dir.join(name), body).expect("a written input");
            consumes.push(format!(
                "{{\"path\":\"{name}\",\"sha256\":\"{}\"}}",
                sha256_hex(body.as_bytes())
            ));
        }

        let record = format!(
            "{}\n{}\n{}\n",
            START,
            format_args!(
                "{{\"record\":\"claim\",\"id\":\"c1\",\"hypothesis\":\"the cells are \
                 comparable\",\"result\":\"supported\",\"consumes\":[{}]}}",
                consumes.join(",")
            ),
            SUMMARY
        );
        let path = dir.join("run.jsonl");
        std::fs::write(&path, record).expect("a written record");
        path
    }

    const START: &str = concat!(
        r#"{"source":{"kind":"live"},"record":"start","regime":{"arm":"bakeoff","dogma_version":0,"substrates":[{"id":"#,
        r#""processor","engine":{"name":"none","version_or_digest":"0"},"weights":{"kind":"#,
        r#""digest","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"#,
        r#""hardware_fingerprint":"152e2fc3bef0c4a186e04612d86d9e90cacf26c5c71da926630cee1f53031f01","sampler_card":{"seed":0},"reasoning":"off"}]}}"#
    );

    // The product digest is on the ROW and every kind carries it, including a
    // recompute's -- ruled (a) on #68, 2026-09-11. A summary row without one
    // does not parse, so this fixture would stop being a record.
    const SUMMARY: &str = concat!(
        r#"{"record":"summary","kind":"recompute","targets_checked":1,"targets_matched":1,"#,
        r#""digests":["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"],"#,
        r#""product_sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"}"#
    );

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bakeoff-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
        match value {
            Value::Object(members) => members.get(key).expect("the key"),
            _ => panic!("not an object"),
        }
    }

    /// THE WHOLE POINT OF "ASSEMBLE, DON'T PRINT": the directory the verb
    /// writes is one the gates accept. Anything less is a verb that produces
    /// a shape nobody can land.
    ///
    /// Both linters are run against it, as themselves, from this test --
    /// because the two things that could be wrong are different. The
    /// directory linter says the report agrees with the record; gate 0 says
    /// the numbers re-derive from the artefacts. A directory that passed one
    /// and failed the other would be exactly the half-assembled shape the
    /// ruling was about.
    #[test]
    fn the_assembled_directory_is_one_the_gates_accept() {
        // UNDER THE REPOSITORY, not in the system temp directory, because
        // gate 0 asks git whether `recompute.sh` modified the tree and
        // refuses when git cannot answer. Under `target/` it is ignored, so
        // the answer is "nothing changed" -- which is the answer the check
        // wants and the one a scratch directory outside any repository
        // cannot give.
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the workspace root")
            .to_path_buf();
        let dir = root.join("target/bakeoff-assembled");
        let _ = std::fs::remove_dir_all(&dir);
        let path = write_run(&dir);
        let into = dir.join("2026-01-01-a-sense-bakeoff");
        let answer =
            assemble(&path, &into).unwrap_or_else(|err| panic!("the assembly failed: {err}"));

        // Every file the ruling named, and the two the gates require.
        for name in [
            "README.md",
            "run.jsonl",
            "regimen.toml",
            "report.json",
            "recompute.sh",
        ] {
            assert!(into.join(name).is_file(), "{name} was not written");
        }
        // The caches, by digest, beside the record that consumed them.
        assert!(
            into.join("even.vectors.jsonl").is_file(),
            "a cache the record consumed was not committed beside it"
        );

        // The answer names where it put it, and the digest of what it put
        // there -- so a caller has something to check without opening the
        // directory.
        let Value::String(reported) = field(&answer, "product_sha256") else {
            panic!("the answer names the product's digest")
        };
        let product = std::fs::read(into.join("report.json")).expect("the product");
        assert_eq!(
            *reported,
            sha256_hex(&product),
            "the answer's digest is not the product's"
        );

        // AND THE GATES. `check-results.py` dispatches the record verdict to
        // the built binary, and refuses when that binary does not reflect the
        // source -- which `cargo test` alone does not guarantee, because
        // nothing rebuilds it. So the same resolver the linter uses is asked
        // first, and a binary it will not vouch for SKIPS the two gates
        // loudly rather than failing them: the defect would be in the build,
        // not in the directory, and a test that says "refused" about the
        // wrong thing sends the next reader to the wrong file.
        //
        // Loudly, because a test that quietly passes when it could not run is
        // the thing this repository refuses. `verify.sh` builds the binary
        // before it runs the suite, so in the gate this branch is not taken.
        let resolved = std::process::Command::new("python3")
            .arg(root.join("scripts/resolve-diet.py"))
            .current_dir(&root)
            .output();
        let usable = matches!(&resolved, Ok(out) if out.status.success());
        if !usable {
            let why = resolved.map_or_else(
                |err| err.to_string(),
                |out| String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            );
            eprintln!(
                "the assembled directory was NOT linted, and this test proved nothing about \
                 the gates: {why}"
            );
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        for (script, what) in [
            ("scripts/check-results.py", "the directory linter"),
            ("scripts/check-recompute.py", "gate 0"),
        ] {
            let out = std::process::Command::new("python3")
                .arg(root.join(script))
                .arg("--root")
                .arg(&dir)
                .current_dir(&root)
                .output()
                .unwrap_or_else(|err| panic!("{what} could not be run: {err}"));
            assert!(
                out.status.success(),
                "{what} refused the assembled directory:\n{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr),
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A cache the record consumed and that is not beside it is a REFUSAL
    /// THAT NAMES THE FILE, not a traceback. The first cut of `recompute.sh`
    /// let `pathlib` raise: the exit code was right and the message was a
    /// Python stack, which sends the reader to the script instead of to the
    /// missing artefact. Both halves are asserted, because the exit code
    /// alone was already correct and would have passed a test that checked
    /// only that.
    #[test]
    fn a_consumed_cache_that_is_absent_is_named_rather_than_traced() {
        let dir = scratch("absent-cache");
        let path = write_run(&dir);
        let into = dir.join("2026-01-01-a-sense-bakeoff");
        assemble(&path, &into).expect("the assembly");

        let gone = "even.vectors.jsonl";
        std::fs::remove_file(into.join(gone)).expect("the cache to remove");
        let run = std::process::Command::new("bash")
            .arg("recompute.sh")
            .current_dir(&into)
            .output()
            .expect("recompute.sh runs");

        assert!(
            !run.status.success(),
            "a directory missing a cache re-derived"
        );
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
        assert!(
            said.contains(gone),
            "the refusal does not name the file: {said}"
        );
        assert!(
            !said.contains("Traceback"),
            "the refusal is a stack trace rather than a sentence: {said}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 0 clean, 1 found something, 2 could not run -- and this script has to
    /// tell the last two apart.
    ///
    /// Ruled 2026-09-12: `sys.exit("message")` exits 1, so every refusal came
    /// out as "found something", including the ones that mean "I could not
    /// read this directory at all". The census cannot distinguish a recompute
    /// that FAILED from one that could not be ATTEMPTED if they share a code,
    /// and that is how a gate reads "nothing wrong" when it means "did not
    /// look".
    ///
    /// BOTH DIRECTIONS ARE ASSERTED. A test that only checked the 2s would
    /// pass a script that exited 2 for everything, which loses exactly as
    /// much as exiting 1 for everything did. The 1 row is the control.
    ///
    /// The malformed cases are also the ones a fresh instance found raising a
    /// BARE TRACEBACK: only the file read was wrapped, so a file that was
    /// present and unparseable went out through `json.loads` or
    /// `tomllib.loads` with a Python stack trace, under a comment claiming no
    /// path out of the script was an exception nobody wrote.
    #[test]
    fn a_directory_this_script_cannot_read_is_a_two_not_a_traceback() {
        let dir = scratch("exit-codes");
        let path = write_run(&dir);
        let into = dir.join("2026-01-01-a-sense-bakeoff");
        assemble(&path, &into).expect("the assembly");

        let run_it = |dir: &std::path::Path| {
            let run = std::process::Command::new("bash")
                .arg("recompute.sh")
                .current_dir(dir)
                .output()
                .expect("recompute.sh runs");
            let said = format!(
                "{}{}",
                String::from_utf8_lossy(&run.stdout),
                String::from_utf8_lossy(&run.stderr)
            );
            (run.status.code(), said)
        };

        // Clean, first, so the rest are a change from a known state.
        let (code, said) = run_it(&into);
        assert_eq!(
            code,
            Some(0),
            "an untouched assembly did not re-derive: {said}"
        );

        let readme = into.join("README.md");
        let record = into.join("run.jsonl");
        let good_readme = std::fs::read_to_string(&readme).expect("the report");
        let good_record = std::fs::read_to_string(&record).expect("the record");

        // COULD NOT RUN -- three ways, each present-but-unreadable rather
        // than absent, because absent was the only case the old helper knew.
        for (what, file, text) in [
            (
                "front-matter that is not TOML",
                &readme,
                good_readme.replacen("+++\n", "+++\nhypothesis = \"unterminated\n", 1),
            ),
            (
                "a README with no front-matter",
                &readme,
                "no fence here\n".to_owned(),
            ),
            (
                "a record line that is not JSON",
                &record,
                "not json at all {{{\n".to_owned(),
            ),
        ] {
            std::fs::write(file, &text).expect("the damaged file");
            let (code, said) = run_it(&into);
            assert_eq!(code, Some(2), "{what}: expected 2, got {code:?}: {said}");
            assert!(
                !said.contains("Traceback"),
                "{what}: refused with a stack trace rather than a sentence: {said}"
            );
            std::fs::write(&readme, &good_readme).expect("the report back");
            std::fs::write(&record, &good_record).expect("the record back");
        }

        // FOUND SOMETHING -- the control. The script read everything it
        // needed, reached a verdict, and the verdict is no.
        let product = into.join("report.json");
        let good_product = std::fs::read(&product).expect("the product");
        let mut tampered = good_product.clone();
        tampered.push(b' ');
        std::fs::write(&product, &tampered).expect("the tampered product");
        let (code, said) = run_it(&into);
        assert_eq!(
            code,
            Some(1),
            "a product that does not match its digest is a finding, not an \
             inability: {said}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A recompute of zero artefacts is not a recompute, and gate 0 says so.
    ///
    /// Ruled 2026-09-13 with the front-matter choices. A fresh instance built
    /// a directory whose claim consumes `[]` with its counts consistently
    /// zero, and `check-recompute.py` reported `1 recomputed`, exit 0 —
    /// against its own docstring's warning that a directory declaring nothing
    /// "is neither checked nor counted as skipped, which is how a gate comes
    /// to run over nothing while reporting success".
    ///
    /// `check-results.py` refused the same directory, but through a DIFFERENT
    /// rule in the record format — a claim naming no artefact could produce a
    /// bound and never a number — so `verify.sh` was already red and the hole
    /// was invisible there. It is `check-recompute.py` alone, which is the
    /// sandboxed way this script actually runs, that miscounted. Gate 0 now
    /// refuses it itself.
    ///
    /// THE CONTROL IS THE SECOND HALF. A refusal that fired on every
    /// directory would satisfy the first assertion and destroy the verb, so
    /// the untouched assembly must still re-derive.
    #[test]
    fn a_recompute_of_no_evidence_is_refused_by_gate_zero() {
        let dir = scratch("no-evidence");
        let path = write_run(&dir);
        let into = dir.join("2026-01-01-a-sense-bakeoff");
        assemble(&path, &into).expect("the assembly");

        let run_it = || {
            let run = std::process::Command::new("bash")
                .arg("recompute.sh")
                .current_dir(&into)
                .output()
                .expect("recompute.sh runs");
            let said = format!(
                "{}{}",
                String::from_utf8_lossy(&run.stdout),
                String::from_utf8_lossy(&run.stderr)
            );
            (run.status.code(), said)
        };

        // THE CONTROL, first: evidence present, and it re-derives.
        let (code, said) = run_it();
        assert_eq!(
            code,
            Some(0),
            "an untouched assembly did not re-derive: {said}"
        );

        // Now strip the claim's evidence, and take EVERY count that describes
        // it down with it -- the summary row's and the front-matter's alike.
        // That second half is not decoration. Leave either count where it was
        // and the script still exits 1, but on the count/rows disagreement
        // instead, and this test would pass with gate 0 deleted: a fixture
        // graded RED for the wrong reason. What is left is the honest shape
        // the gate is for -- a directory that says, consistently throughout,
        // that it consumes nothing.
        //
        // The counts are run out digit by digit rather than written in here,
        // so a fixture that grows a third target cannot quietly re-open that
        // hole; `zero_after` panics when the key it is given is not there.
        let zero_after = |line: &str, key: &str| -> String {
            let at = line
                .find(key)
                .unwrap_or_else(|| panic!("no `{key}` in: {line}"));
            let start = at + key.len();
            // The count runs to the first non-digit, or to end of line: TOML
            // ends the line where JSON puts a comma.
            let end = line[start..]
                .find(|c: char| !c.is_ascii_digit())
                .map_or(line.len(), |over| start + over);
            format!("{}0{}", &line[..start], &line[end..])
        };

        let readme = into.join("README.md");
        let front = std::fs::read_to_string(&readme).expect("the front-matter");
        let front: String = front
            .lines()
            .map(|line| {
                if line.starts_with("targets_checked = ") {
                    zero_after(line, "targets_checked = ")
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&readme, front + "\n").expect("the zeroed front-matter");

        let record = into.join("run.jsonl");
        let text = std::fs::read_to_string(&record).expect("the record");
        let emptied: String = text
            .lines()
            .map(|line| {
                if line.contains(r#""record":"claim""#) {
                    let open = line.find(r#""consumes":["#).expect("the consumes list");
                    let start = open + r#""consumes":["#.len();
                    let close = line[start..].find(']').expect("its close") + start;
                    format!("{}{}", &line[..start], &line[close..])
                } else if line.contains(r#""record":"summary""#) {
                    zero_after(
                        &zero_after(line, r#""targets_checked":"#),
                        r#""targets_matched":"#,
                    )
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&record, emptied + "\n").expect("the emptied record");

        let (code, said) = run_it();
        assert_eq!(
            code,
            Some(1),
            "a directory consuming nothing re-derived and reported success: {said}"
        );
        assert!(
            said.contains("not a recompute"),
            "the refusal does not name what is wrong: {said}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Assembling over somebody's results is not an assembly step.
    #[test]
    fn a_directory_that_already_holds_a_report_is_refused() {
        let dir = scratch("occupied");
        let path = write_run(&dir);
        let into = dir.join("2026-01-01-a-sense-bakeoff");
        assemble(&path, &into).expect("the first assembly");
        let before = std::fs::read(into.join("README.md")).expect("the report");
        let err = assemble(&path, &into).expect_err("the second assembly");
        assert!(matches!(err, RunError::Occupied { .. }), "{err}");
        assert_eq!(
            std::fs::read(into.join("README.md")).expect("the report"),
            before,
            "a refused assembly rewrote the report anyway"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_run_reports_every_cell_of_every_embedder() {
        let dir = scratch("whole");
        let path = write_run(&dir);
        // `expect` would print the error's Debug, and every one of these
        // errors says what it means in its Display. A test that fails with
        // a struct dump is a test whose failure has to be decoded.
        let report = run(&path).unwrap_or_else(|err| panic!("the run did not report: {err}"));

        let Value::Array(cells) = field(&report, "cells") else {
            panic!("cells is not a list")
        };
        // Two embedders, one set with rows, four scorings, both gates.
        assert_eq!(
            cells.len(),
            2 * 4 * 2,
            "a cell per embedder, scoring and gate"
        );
        for cell in cells {
            let Value::Array(metrics) = field(cell, "metrics") else {
                panic!("metrics is not a list")
            };
            assert_eq!(
                metrics.len(),
                sense::Metric::ALL.len() * sense::BUDGETS.len(),
                "every metric at every pre-registered budget, or none"
            );
            // AND THE LADDER IS THE PRE-REGISTERED ONE, not a count that
            // happens to match. A runner that reported one metric five times
            // at one budget would satisfy the length above and would be
            // reporting a sweep it never did.
            let mut taken: Vec<(String, i64)> = metrics
                .iter()
                .map(|reported| {
                    let (Value::String(metric), Value::Integer(budget)) =
                        (field(reported, "metric"), field(reported, "budget"))
                    else {
                        panic!("a reported metric names itself and its budget")
                    };
                    (metric.clone(), *budget)
                })
                .collect();
            taken.sort();
            let mut want: Vec<(String, i64)> = sense::BUDGETS
                .iter()
                .flat_map(|budget| {
                    sense::Metric::ALL.iter().map(move |metric| {
                        (
                            metric.tag().to_owned(),
                            i64::try_from(*budget).expect("a budget"),
                        )
                    })
                })
                .collect();
            want.sort();
            assert_eq!(taken, want, "the cell's budgets are not the ladder's");
        }
        // One comparison per cell key: two embedders is one pair.
        let Value::Array(comparisons) = field(&report, "comparisons") else {
            panic!("comparisons is not a list")
        };
        assert_eq!(comparisons.len(), 4 * 2, "one pair per cell");
        // The ladder is in the pre-registration and nowhere else. A `budget`
        // key beside it would be a second place to read the same fact, and the
        // one that went stale would be the one somebody believed.
        let Value::Object(report_keys) = &report else {
            panic!("a report is an object")
        };
        assert!(
            !report_keys.contains_key("budget"),
            "the report still carries a single budget beside the pre-registered ladder"
        );
        // AND THE LADDER LIVES IN `pre-registration.json`, which is the file
        // the front-matter pins by digest since 2026-09-14. It used to be a
        // block inside the report; read from the constant here it would prove
        // only that the constant is itself, so it is read back off disk out
        // of an assembled directory — the same bytes a reader gets.
        let assembled = dir.join("2026-01-01-a-sense-bakeoff-ladder");
        assemble(&path, &assembled).expect("the assembly");
        let pinned = std::fs::read_to_string(assembled.join("pre-registration.json"))
            .expect("the pre-registration is written beside the report");
        let pinned = crate::formats::record::json::line(pinned.trim_end())
            .expect("the pre-registration is JSON this crate can read");
        let Some(Value::Array(budgets)) = pinned.get("budgets") else {
            panic!("the pre-registration names its budgets")
        };
        assert_eq!(
            budgets,
            &sense::BUDGETS
                .iter()
                .map(|budget| Value::Integer(i64::try_from(*budget).expect("a budget")))
                .collect::<Vec<_>>(),
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // The refusal #24's ruling asks for: the caches are recompute inputs, so a
    // cache whose bytes are not the bytes the record consumed is not the
    // cache the record's numbers came from.
    //
    // THE TAMPER IS WELL-FORMED, and that is the whole point of it. It used to
    // append a row carrying one dimension where the set declares five hundred
    // and twelve, so the ROW PARSER refused it before the digest was ever
    // compared -- the test passed, and passed for a reason it had not checked.
    // A review found that and could not rule out that a tampered cache which
    // still parses would sail through. Ruled 2026-09-11: cut it well-formed,
    // so nothing but the digest can fire.
    //
    // One value inside an existing row, same width, same text, same shape. If
    // this ever goes green WITH the digest check present, the digest check has
    // a hole, and that is worth learning here rather than after a bakeoff
    // banks a number over a cache nobody verified.
    /// A cell whose control fails is a RESULT, typed, and the run goes on.
    ///
    /// Ruled on #24 (2026-09-15) after the first invocation on four real
    /// embedders: every one failed a control, the run stopped at the first,
    /// and the reading that stopped it -- a mined positive below random
    /// words, which is the over-firing the instrument exists to see -- could
    /// not appear in any report. Here a third cache puts one positive
    /// register row at the negation of the positive sense, so under raw
    /// cosine it sits at -1, under the unrelated-words control at 0.
    #[test]
    fn a_cell_whose_control_fails_is_a_typed_result_and_the_run_proceeds() {
        fn sink_one_positive(text: &str) -> Option<Vec<f64>> {
            let rows = sense::register(&register_source()).expect("the shipped register");
            let sunk = rows
                .iter()
                .find(|row| row.label.is_positive())
                .expect("a positive row");
            if sunk.text != text {
                return None;
            }
            let senses = sense::shipped_senses().expect("the shipped senses");
            let set = sense::EmbeddedSet::embed(&senses, sense::SenseSet::Mistake, &Fixture)
                .expect("an embeddable set");
            let positive = &set.literal(sense::Polarity::Positive).vector;
            // Negated, without minting a negative zero: the cache format
            // refuses `-0.00000000`, and the fixture's vectors are mostly
            // zeros.
            Some(
                positive
                    .iter()
                    .map(|v| if *v == 0.0 { 0.0 } else { -v })
                    .collect(),
            )
        }
        let dir = scratch("control-failed");
        let path = write_run_with(&dir, &[("sunk", sink_one_positive)]);
        let report = run(&path).unwrap_or_else(|err| panic!("the run did not report: {err}"));

        let Value::Array(cells) = field(&report, "cells") else {
            panic!("cells is not a list")
        };
        let mut failed = 0;
        let mut scored_sunk = 0;
        for cell in cells {
            let (Value::String(embedder), Value::String(result)) =
                (field(cell, "embedder"), field(cell, "result"))
            else {
                panic!("a cell names its embedder and its result")
            };
            // Compared, not matched: the library rule refuses a match arm on
            // a string literal, and these are the report's own tags.
            if result == "scored" {
                assert!(matches!(field(cell, "metrics"), Value::Array(_)));
                if embedder == "sunk" {
                    scored_sunk += 1;
                }
            } else {
                assert_eq!(
                    result, "control_failed",
                    "a cell result this test does not know"
                );
                {
                    failed += 1;
                    assert_eq!(embedder, "sunk", "only the sunk cache fails a control");
                    assert_eq!(
                        field(cell, "control"),
                        &Value::String("unrelated_words".to_owned()),
                        "the bottom control under a scoring that ignores the negative"
                    );
                    let Value::Object(readings) = field(cell, "readings") else {
                        panic!("a failed cell carries its readings")
                    };
                    assert!(
                        readings.contains_key("row") && readings.contains_key("row_score"),
                        "the readings name the row that sank and where it sat"
                    );
                    assert!(
                        !matches!(cell, Value::Object(members) if members.contains_key("metrics")),
                        "no metric is computed over a cell whose control failed"
                    );
                }
            }
        }
        assert!(failed >= 1, "the sunk positive tripped no control");
        assert_eq!(
            field(&report, "control_failed"),
            &Value::Integer(failed),
            "the report counts its failed cells"
        );
        // The two fixture caches still score every cell, and comparisons are
        // over scored cells only: sixteen cells for those two, and a pair per
        // cell key where two or more embedders scored.
        let scored_total = i64::try_from(cells.len()).expect("a count") - failed;
        assert_eq!(scored_total, 2 * 4 * 2 + scored_sunk);
        let Value::Array(comparisons) = field(&report, "comparisons") else {
            panic!("comparisons is not a list")
        };
        for comparison in comparisons {
            let (Value::String(a), Value::String(b)) =
                (field(comparison, "a"), field(comparison, "b"))
            else {
                panic!("a comparison names two embedders")
            };
            assert!(a != b);
        }
        // Every pair of the two fixture caches is there; a failed sunk cell
        // never is.
        assert!(comparisons.len() >= 4 * 2);
    }

    /// A row the embedder never placed at all refuses the RUN, not the cell.
    ///
    /// A fresh-instance review of #83 found `cells_of`'s two refusal
    /// branches -- `ControlFailure::Unscorable` becoming `RunError::Control`,
    /// and a `MetricError` other than `Undefined{on: Subject}` becoming
    /// `RunError::Metric` -- had no coverage: inverting either into a cell
    /// fact left 609 of 609 tests green. This closes the reachable half.
    /// [`MetricSubject::FailureFixture`] and `InstrumentNeverFailed` stay
    /// undemonstrated here; both require a metric's own hardcoded
    /// self-check to be broken, which no cache built from real rows can
    /// provoke through this crate's public entry point -- a declared gap,
    /// not a silent one.
    ///
    /// One cache missing a register row's line entirely -- `Cached::load`
    /// admits a file with any subset of rows, so this differs from the
    /// "sunk" cache above, which places the row somewhere scoreable. Missing
    /// is what a real embedder does to a row it could not place at all.
    #[test]
    fn a_row_no_cache_ever_placed_refuses_the_run() {
        let dir = scratch("gappy");
        std::fs::create_dir_all(&dir).expect("a directory");
        let senses = sense::shipped_senses().expect("the shipped senses");
        let rows = sense::register(&register_source()).expect("the shipped register");
        let register_texts: Vec<String> = rows.iter().map(|row| row.text.clone()).collect();
        let mut texts = register_texts.clone();
        texts.extend(senses.iter().map(|sense| sense.text.clone()));
        for scoring in sense::Scoring::ALL {
            let (top, bottom) = scoring.extremes();
            for set in sense::SenseSet::ALL {
                let embedded =
                    sense::EmbeddedSet::embed(&senses, *set, &Fixture).expect("an embeddable set");
                texts.push(top.row(&embedded).text);
                texts.push(bottom.row(&embedded).text);
            }
        }
        texts.sort();
        texts.dedup();
        let missing = register_texts.first().expect("a register row").clone();

        let mut gappy = String::new();
        for text in &texts {
            if *text == missing {
                continue;
            }
            let vector = Fixture.embed(text);
            let spelled: Vec<String> = vector.iter().map(|value| format!("{value:.8}")).collect();
            let _ = writeln!(
                gappy,
                "{{\"text\":{},\"vector\":[{}]}}",
                quoted(text),
                spelled.join(",")
            );
        }

        let files = [
            ("authored-mistake.jsonl".to_owned(), register_source()),
            (
                "even.vectors.jsonl".to_owned(),
                cache(&texts, &register_texts, false),
            ),
            ("gappy.vectors.jsonl".to_owned(), gappy),
        ];
        let mut consumes = Vec::new();
        for (name, body) in &files {
            std::fs::write(dir.join(name), body).expect("a written input");
            consumes.push(format!(
                "{{\"path\":\"{name}\",\"sha256\":\"{}\"}}",
                sha256_hex(body.as_bytes())
            ));
        }
        let record = format!(
            "{}\n{}\n{}\n",
            START,
            format_args!(
                "{{\"record\":\"claim\",\"id\":\"c1\",\"hypothesis\":\"the cells are \
                 comparable\",\"result\":\"supported\",\"consumes\":[{}]}}",
                consumes.join(",")
            ),
            SUMMARY
        );
        let path = dir.join("run.jsonl");
        std::fs::write(&path, record).expect("a written record");

        match run(&path) {
            Err(RunError::Control { embedder, .. }) => {
                assert_eq!(
                    embedder, "gappy",
                    "the wrong embedder was blamed for a row it did place"
                );
            }
            other => panic!("a cache missing a row entirely did not refuse the run: {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A metric undefined on a cell's rows is reported on that cell, typed,
    /// beside the metrics that did compute; the run goes on.
    ///
    /// A cache that places every register row at one vector makes every
    /// scoring flat over the register: precision and over-firing still count
    /// (ties are ties), the AUC is one half, and d' has no spread to divide
    /// by. Before this, the first such cell refused the whole run with
    /// `d_prime is undefined on the subject` and nothing else in it.
    #[test]
    fn a_metric_undefined_on_a_cell_is_reported_on_that_cell_and_the_run_proceeds() {
        fn flatten_register(text: &str) -> Option<Vec<f64>> {
            let rows = sense::register(&register_source()).expect("the shipped register");
            if !rows.iter().any(|row| row.text == text) {
                return None;
            }
            // One vector for every register row: a token no sense uses, so
            // every row sits at the same cosine to every sense.
            Some(Fixture.embed("zebra"))
        }
        let dir = scratch("flat");
        let path = write_run_with(&dir, &[("flat", flatten_register)]);
        let report = run(&path).unwrap_or_else(|err| panic!("the run did not report: {err}"));
        let Value::Array(cells) = field(&report, "cells") else {
            panic!("cells is not a list")
        };
        let mut flat_with_undefined = 0;
        for cell in cells {
            let Value::String(embedder) = field(cell, "embedder") else {
                panic!("a cell names its embedder")
            };
            let Value::Array(undefined) = field(cell, "undefined") else {
                panic!("a scored cell lists what was undefined on it")
            };
            if embedder != "flat" {
                assert!(
                    undefined.is_empty(),
                    "a fixture cell had an undefined metric"
                );
                continue;
            }
            if undefined.is_empty() {
                continue;
            }
            flat_with_undefined += 1;
            for entry in undefined {
                assert_eq!(
                    field(entry, "metric"),
                    &Value::String("d_prime".to_owned()),
                    "only d' has a spread to lack"
                );
                let Value::String(cause) = field(entry, "cause") else {
                    panic!("an undefined metric says why")
                };
                assert!(cause.contains("no spread"), "{cause}");
            }
            let Value::Array(metrics) = field(cell, "metrics") else {
                panic!("metrics is not a list")
            };
            assert!(
                !metrics.is_empty(),
                "the metrics that did compute are kept beside the undefined ones"
            );
        }
        assert!(
            flat_with_undefined >= 1,
            "a flat register left d' defined somewhere it cannot be"
        );
    }

    #[test]
    fn a_cache_the_record_did_not_consume_is_refused() {
        let dir = scratch("digest");
        let path = write_run(&dir);
        let cache = dir.join("even.vectors.jsonl");
        let body = std::fs::read_to_string(&cache).expect("the cache");
        let mut lines: Vec<String> = body.lines().map(ToOwned::to_owned).collect();
        assert!(
            !lines.is_empty(),
            "a cache with no rows tampers with nothing"
        );

        let opened = lines[0].find("\"vector\":[").expect("a vector") + "\"vector\":[".len();
        let comma = lines[0][opened..]
            .find(',')
            .expect("more than one dimension")
            + opened;
        let was: f64 = lines[0][opened..comma].parse().expect("a number");
        lines[0] = format!(
            "{}{:.8}{}",
            &lines[0][..opened],
            was + 0.5,
            &lines[0][comma..]
        );
        std::fs::write(&cache, lines.join("\n") + "\n").expect("a written cache");

        // It still parses, which is what makes the refusal below about the
        // digest. Asserted rather than assumed: a tamper that broke the row
        // would put this test back where it started.
        let rows = std::fs::read_to_string(&cache).expect("the tampered cache");
        assert!(
            sense::Cached::load("even", &rows).is_ok(),
            "the tampered cache must still be a cache, or the parser answers first"
        );

        match run(&path) {
            Err(RunError::Digest { path, .. }) => {
                assert_eq!(path, "even.vectors.jsonl");
            }
            other => panic!("an edited cache was not refused: {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cache_that_is_not_there_is_refused() {
        let dir = scratch("missing");
        let path = write_run(&dir);
        std::fs::remove_file(dir.join("lean.vectors.jsonl")).expect("removable");
        match run(&path) {
            Err(RunError::Missing { path }) => assert_eq!(path, "lean.vectors.jsonl"),
            other => panic!("a missing cache was not refused: {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    // A record that consumes a register and no cache has nothing to compare
    // against anything, and saying so is better than reporting an empty table.
    #[test]
    fn a_run_with_no_cache_is_refused() {
        let dir = scratch("nocache");
        std::fs::create_dir_all(&dir).expect("a directory");
        std::fs::write(dir.join("authored-mistake.jsonl"), register_source()).expect("a register");
        let record = format!(
            "{START}\n{{\"record\":\"claim\",\"id\":\"c1\",\"hypothesis\":\"nothing to \
             compare\",\"result\":\"supported\",\"consumes\":[{{\"path\":\"authored-mistake.\
             jsonl\",\"sha256\":\"{}\"}}]}}\n{SUMMARY}\n",
            sha256_hex(register_source().as_bytes())
        );
        let path = dir.join("run.jsonl");
        std::fs::write(&path, record).expect("a record");
        assert!(matches!(run(&path), Err(RunError::NoCaches)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A pairs run's inputs, written under `dir`: three drives so the
    /// per-drive pooling has something to pool, every label in every drive
    /// so no metric is undefined for want of a class, and one role-less
    /// cache built from [`Fixture`] over every text the run places.
    fn write_pairs_run(dir: &Path) -> PathBuf {
        // Three drives, so the per-drive pooling has something to pool; every
        // label in every drive, so no metric is undefined for want of a class.
        let mut pairs_src = String::new();
        let mut turns_src = String::new();
        let mut texts: Vec<String> = Vec::new();
        for drive in ["d1", "d2", "d3"] {
            for i in 0..3 {
                let rows = [
                    (
                        "positive",
                        format!("the resolver reads flag {i} from `config_{i}.toml`"),
                        format!(
                            "Actually flag {i} in `config_{i}.toml` is never read by the resolver. Let me check the loader for {drive}."
                        ),
                        format!("Let me check the loader for {drive}."),
                    ),
                    (
                        "negative",
                        format!("note {i} about the parser for {drive}"),
                        format!("Reading the docs for module {i}. Let me read on."),
                        "Let me read on.".to_owned(),
                    ),
                    (
                        "hard_negative",
                        format!("the `lexer_{i}` is slow in {drive}"),
                        format!("Using `lexer_{i}` as before in {drive}. Let me run it."),
                        "Let me run it.".to_owned(),
                    ),
                ];
                for (n, (label, entry, prose, intent)) in rows.iter().enumerate() {
                    let id = format!("{drive}-{i}-{n}");
                    let _ = writeln!(
                        pairs_src,
                        "{{\"id\":{},\"drive\":{},\"turn\":1,\"step\":{i},\"entry\":{},\"turn_intent\":{},\"turn_prose\":{},\"turn_tools\":[],\"label\":{},\"source\":{}}}",
                        quoted(&id),
                        quoted(drive),
                        quoted(entry),
                        quoted(intent),
                        quoted(prose),
                        quoted(label),
                        quoted(if n == 0 { "planted" } else { "mined" })
                    );
                    let _ = writeln!(
                        turns_src,
                        "{{\"id\":{},\"drive\":{},\"turn\":1,\"step\":{i},\"text\":{},\"anchored\":{},\"label\":{},\"source\":\"mined\",\"pairs\":[{}]}}",
                        quoted(&format!("t-{id}")),
                        quoted(drive),
                        quoted(prose),
                        n == 0,
                        quoted(label),
                        quoted(&id)
                    );
                    texts.push(entry.clone());
                    texts.push(intent.clone());
                    texts.push(prose.clone());
                }
            }
        }
        for sense in sense::shipped_senses().expect("senses") {
            texts.push(sense.text);
        }
        texts.push(sense::UNRELATED.to_owned());
        texts.sort();
        texts.dedup();
        let files = [
            ("pairs-intent.jsonl".to_owned(), pairs_src),
            ("turns-sense.jsonl".to_owned(), turns_src),
            ("fx.vectors.jsonl".to_owned(), cache(&texts, &[], false)),
        ];
        let mut consumes = Vec::new();
        for (name, body) in &files {
            std::fs::write(dir.join(name), body).expect("a written input");
            consumes.push(format!(
                "{{\"path\":\"{name}\",\"sha256\":\"{}\"}}",
                sha256_hex(body.as_bytes())
            ));
        }
        let record = format!(
            "{}\n{}\n{}\n",
            START,
            format_args!(
                "{{\"record\":\"claim\",\"id\":\"c1\",\"hypothesis\":\"the pairs are \
                 comparable\",\"result\":\"supported\",\"consumes\":[{}]}}",
                consumes.join(",")
            ),
            SUMMARY
        );
        let path = dir.join("run.jsonl");
        std::fs::write(&path, record).expect("a written record");
        path
    }

    /// Both linters over an assembled directory, as themselves; skipped
    /// loudly when the built binary is not one the resolver vouches for.
    fn lint_assembled(root: &Path, dir: &Path) {
        let resolved = std::process::Command::new("python3")
            .arg(root.join("scripts/resolve-diet.py"))
            .current_dir(root)
            .output();
        let usable = matches!(&resolved, Ok(out) if out.status.success());
        if !usable {
            eprintln!(
                "the assembled pairs directory was NOT linted, and this test proved nothing about \
                 the gates"
            );
            return;
        }
        for (script, what) in [
            ("scripts/check-results.py", "the directory linter"),
            ("scripts/check-recompute.py", "gate 0"),
        ] {
            let out = std::process::Command::new("python3")
                .arg(root.join(script))
                .arg("--root")
                .arg(dir)
                .current_dir(root)
                .output()
                .unwrap_or_else(|err| panic!("{what} could not be run: {err}"));
            assert!(
                out.status.success(),
                "{what} refused the assembled pairs directory:\n{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr),
            );
        }
    }

    /// A PAIRS run assembles a directory the gates accept, by the same two
    /// linters: the instrument the courier series adds is held to the shape
    /// the sense bakeoff is held to, or it is a second shape.
    #[test]
    fn a_pairs_run_assembles_a_directory_the_gates_accept() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the workspace root")
            .to_path_buf();
        let dir = root.join("target/bakeoff-pairs-assembled");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory");

        let path = write_pairs_run(&dir);

        let into = dir.join("2026-01-01-a-pairs-run");
        let answer =
            assemble(&path, &into).unwrap_or_else(|err| panic!("the assembly failed: {err}"));
        let Value::String(reported) = field(&answer, "product_sha256") else {
            panic!("the answer names the product's digest")
        };
        let product = std::fs::read_to_string(into.join("report.json")).expect("the product");
        assert_eq!(*reported, sha256_hex(product.as_bytes()));
        assert!(
            product.contains("\"gate_comparisons\""),
            "the pairs run emits the gate-arm bootstrap the sub-rule needs"
        );
        assert!(
            product.contains("\"register\":\"intent\"")
                && product.contains("\"register\":\"sense\""),
            "both registers produced cells"
        );
        let readme = std::fs::read_to_string(into.join("README.md")).expect("the README");
        assert!(
            readme.contains("entry-to-turn"),
            "the README is the pairs run's, not the sense bakeoff's"
        );
        let pre = std::fs::read_to_string(into.join("pre-registration.json"))
            .expect("the pre-registration");
        assert!(
            pre.contains("anchors_required"),
            "the pairs pre-registration was written"
        );

        lint_assembled(&root, &dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A record that consumes both instruments' registers is refused by name.
    #[test]
    fn a_record_consuming_both_instruments_registers_is_refused() {
        let dir = scratch("mixed");
        let path = write_run(&dir);
        let pairs_src = "{\"id\":\"p\",\"drive\":\"d\",\"turn\":1,\"step\":1,\"entry\":\"an entry\",\"turn_intent\":\"Let me.\",\"turn_prose\":\"prose\",\"turn_tools\":[],\"label\":\"positive\",\"source\":\"mined\"}\n";
        std::fs::write(dir.join("pairs-intent.jsonl"), pairs_src).expect("written");
        let record = std::fs::read_to_string(&path).expect("the record");
        let patched = record.replace(
            "\"consumes\":[",
            &format!(
                "\"consumes\":[{{\"path\":\"pairs-intent.jsonl\",\"sha256\":\"{}\"}},",
                sha256_hex(pairs_src.as_bytes())
            ),
        );
        std::fs::write(&path, patched).expect("written");
        assert_eq!(run(&path).unwrap_err(), RunError::MixedRegisters);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
