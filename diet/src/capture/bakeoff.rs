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
    /// A control did not land where it must.
    Control {
        /// Which embedder.
        embedder: String,
        /// Which scoring.
        scoring: Scoring,
        /// What went wrong.
        failure: ControlFailure,
    },
    /// A metric could not be reported.
    Metric(sense::MetricError),
    /// The paired bootstrap could not run.
    Bootstrap(sense::BootstrapError),
    /// The directory to assemble into already holds a report.
    Occupied {
        /// The directory.
        path: String,
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
            Self::Metric(err) => write!(f, "{err}"),
            Self::Bootstrap(err) => write!(f, "{err}"),
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

/// What a consumed input is, decided by its name.
enum Input {
    /// A `<model>.vectors.jsonl` cache, under the model's id.
    Cache(String),
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
    if let Some(model) = name.strip_suffix(".vectors.jsonl") {
        return Input::Cache(model.to_owned());
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
}

fn gather(dir: &Path, events: &[Event]) -> Result<Inputs, RunError> {
    let mut caches = Vec::new();
    let mut register: BTreeMap<SenseSet, Vec<Row>> = BTreeMap::new();
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
            Input::Other => unreachable!("filtered above"),
        }
    }
    if caches.is_empty() {
        return Err(RunError::NoCaches);
    }
    Ok(Inputs { caches, register })
}

/// One cell's report.
struct CellReport {
    embedder: String,
    set: SenseSet,
    scoring: Scoring,
    gate: Gate,
    reported: Vec<Reported>,
    scores: Vec<f64>,
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
            (
                "metrics".to_owned(),
                Value::Array(self.reported.iter().map(Reported::record).collect()),
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

/// A run, and everything the directory it assembles into needs from it.
struct Computed {
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

    let mut cells = Vec::new();
    for cached in &inputs.caches {
        for set in SenseSet::ALL.iter().copied() {
            let Some(rows) = inputs.register.get(&set) else {
                continue;
            };
            let embedded = EmbeddedSet::embed(&senses, set, cached).map_err(RunError::Set)?;
            for scoring in Scoring::ALL.iter().copied() {
                sense::controls(rows, &embedded, cached, scoring).map_err(|failure| {
                    RunError::Control {
                        embedder: cached.id().to_owned(),
                        scoring,
                        failure,
                    }
                })?;
                for gate in Gate::ALL.iter().copied() {
                    let cell = Cell { scoring, gate };
                    let scored = sense::score_rows(rows, &embedded, cached, cell)
                        .map_err(RunError::Score)?;
                    // EVERY PRE-REGISTERED BUDGET, not one. The budget was a
                    // `const` here set to eight, and eight was not chosen: it
                    // was the widest the precision fixture happened to
                    // demonstrate. A cell now carries the ladder, because a
                    // single number is a sweep nobody can do afterwards --
                    // rerunning at another budget means rerunning the bakeoff.
                    let reported = sense::BUDGETS
                        .iter()
                        .copied()
                        .flat_map(|budget| {
                            Metric::ALL
                                .iter()
                                .copied()
                                .map(move |metric| (metric, budget))
                        })
                        .map(|(metric, budget)| Reported::take(metric, budget, &scored))
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(RunError::Metric)?;
                    cells.push(CellReport {
                        embedder: cached.id().to_owned(),
                        set,
                        scoring,
                        gate,
                        reported,
                        scores: scored.iter().map(|row| row.score).collect(),
                    });
                }
            }
        }
    }
    if cells.is_empty() {
        let set = SenseSet::ALL.first().copied().unwrap_or(SenseSet::Mistake);
        return Err(RunError::NoRegister { set });
    }

    let report = Value::Object(BTreeMap::from([
        ("pre_registration".to_owned(), PRE_REGISTRATION.value()),
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
            Value::Array(cells.iter().map(CellReport::value).collect()),
        ),
        ("comparisons".to_owned(), comparisons(&cells)?),
        ("null".to_owned(), null_over(&cells)),
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
        report,
        record,
        dir,
    })
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
fn comparisons(cells: &[CellReport]) -> Result<Value, RunError> {
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
fn null_over(cells: &[CellReport]) -> Value {
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

    let regime = done.record.regime().clone();
    let record = crate::formats::record::Record {
        events: vec![
            Event::Start {
                regime: Box::new(regime.clone()),
            },
            Event::Claim {
                id: "c1".to_owned(),
                hypothesis: PRE_REGISTRATION.primary.to_owned(),
                // A recompute of an endpoint is not a verdict on it. The verb
                // computes numbers; whether they support the pre-registered
                // hypothesis is read off them by a person, and a program that
                // wrote `supported` here would be making a claim it cannot
                // make. Inconclusive is the honest machine answer, and it is
                // the one a reader may change after reading the numbers.
                result: crate::formats::record::Verdict::Inconclusive,
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
                product_sha256: product_sha256.clone(),
            },
        ],
    };
    write(
        &into.join("run.jsonl"),
        crate::formats::record::render(&record).as_bytes(),
    )?;
    write(&into.join("report.json"), product.as_bytes())?;
    write(&into.join("regimen.toml"), regimen_of(&regime).as_bytes())?;
    write(
        &into.join("README.md"),
        report_of(&regime, &product_sha256, checked).as_bytes(),
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
fn report_of(regime: &Regime, product_sha256: &str, checked: u32) -> String {
    let ids: Vec<String> = regime
        .substrate_ids()
        .into_iter()
        .map(|id| format!("{id:?}"))
        .collect();
    let (kind, caveat) = kind_and_caveat(regime);
    format!(
        "+++\n\
         hypothesis = {:?}\n\
         result = \"unadjudicated\"\n\
         kind = {kind:?}\n\
         product_sha256 = {product_sha256:?}\n\
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
         primary = {:?}\n\
         separation = {:?}\n\
         over_firing = {:?}\n\
         comparator = {:?}\n\
         correction = {:?}\n\
         +++\n\
         \n\
         # The sense bakeoff, over the caches this record consumed\n\
         \n\
         Written by `diet bakeoff --into`. The numbers are in `report.json`;\n\
         this file is what makes them checkable.\n\
         \n\
         ## Observation\n\
         \n\
         A record declared the caches it consumed, with a digest for each, and\n\
         nothing had turned those caches into numbers.\n\
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
        PRE_REGISTRATION.primary,
        regime.arm,
        ids.join(", "),
        regime.dogma_version,
        PRE_REGISTRATION.primary,
        PRE_REGISTRATION.separation,
        PRE_REGISTRATION.over_firing,
        PRE_REGISTRATION.comparator,
        PRE_REGISTRATION.correction,
        PRE_REGISTRATION.primary,
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

        let files = [
            ("authored-mistake.jsonl", register_source()),
            ("even.vectors.jsonl", cache(&texts, &register_texts, false)),
            ("lean.vectors.jsonl", cache(&texts, &register_texts, true)),
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
        path
    }

    const START: &str = concat!(
        r#"{"record":"start","regime":{"arm":"bakeoff","dogma_version":0,"substrates":[{"id":"#,
        r#""processor","engine":{"name":"none","version_or_digest":"0"},"weights":{"kind":"#,
        r#""digest","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"#,
        r#""hardware_fingerprint":"one-cpu","sampler_card":{"seed":0},"reasoning":"off"}]}}"#
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
        let Value::Array(budgets) = field(field(&report, "pre_registration"), "budgets") else {
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
}
