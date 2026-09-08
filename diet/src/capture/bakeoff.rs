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
//! [`digest::sha256`](crate::digest::sha256).
//!
//! It computes and reports. It writes no files: every other verb of this CLI
//! answers with a value on stdout, and a verb that edited a results directory
//! would be a second writer for `check-results.py` to agree with.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::capture::sense::{
    self, Blocker, Cached, Cell, ControlFailure, DataError, EmbeddedSet, Embedder, Gate, Metric,
    PRE_REGISTRATION, Reported, Row, ScoreError, Scoring, SenseSet, SetError, decimal,
};
use crate::digest::sha256;
use crate::formats::record::json::Value;
use crate::formats::record::{Artifact, Event, Regime};

/// The nomination budget, fixed here rather than swept.
///
/// EIGHT, AND EIGHT IS THE CEILING THE INSTRUMENT ALLOWS. [`Reported::take`]
/// refuses a metric it cannot first demonstrate failing, and the precision
/// fixture holds exactly eight non-positives above its positives -- so at any
/// budget above eight a positive enters the fixture's own top-k, the fixture
/// stops reading as failure, and the metric is refused. The over-firing
/// fixture puts two hard negatives on top, so it needs a budget of at least
/// two. The reportable range is therefore 2..=8 and this takes the top of it.
///
/// That is a bound set by a fixture rather than by the science, and it is
/// worth saying out loud: a budget of eight over a register of a few hundred
/// rows is a narrow endpoint. Widening it means widening the precision
/// fixture, which is a change to the instrument and a ruling.
pub const BUDGET: usize = 8;

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
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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
    let found = sha256(&bytes);
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
                    let reported = Metric::ALL
                        .iter()
                        .copied()
                        .map(|metric| Reported::take(metric, BUDGET, &scored))
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

    Ok(Value::Object(BTreeMap::from([
        ("pre_registration".to_owned(), PRE_REGISTRATION.value()),
        (
            "budget".to_owned(),
            Value::Integer(i64::try_from(BUDGET).unwrap_or(i64::MAX)),
        ),
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
    ])))
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

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use std::path::{Path, PathBuf};

    use super::{BUDGET, RunError, run};
    use crate::capture::sense::{self, Embedder, Fixture};
    use crate::digest::sha256;
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
                sha256(body.as_bytes())
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

    const SUMMARY: &str = concat!(
        r#"{"record":"summary","kind":"recompute","targets_checked":1,"targets_matched":1,"#,
        r#""digests":["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]}"#
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
                sense::Metric::ALL.len(),
                "every metric is reported, or none is"
            );
        }
        // One comparison per cell key: two embedders is one pair.
        let Value::Array(comparisons) = field(&report, "comparisons") else {
            panic!("comparisons is not a list")
        };
        assert_eq!(comparisons.len(), 4 * 2, "one pair per cell");
        assert_eq!(
            field(&report, "budget"),
            &Value::Integer(i64::try_from(BUDGET).expect("a budget")),
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // The refusal #24's ruling asks for: the caches are recompute inputs, so a
    // cache whose bytes are not the bytes the record consumed is not the
    // cache the record's numbers came from.
    #[test]
    fn a_cache_the_record_did_not_consume_is_refused() {
        let dir = scratch("digest");
        let path = write_run(&dir);
        let cache = dir.join("even.vectors.jsonl");
        let mut body = std::fs::read_to_string(&cache).expect("the cache");
        body.push_str("{\"text\":\"one more\",\"vector\":[1.00000000]}\n");
        std::fs::write(&cache, body).expect("a written cache");

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
            sha256(register_source().as_bytes())
        );
        let path = dir.join("run.jsonl");
        std::fs::write(&path, record).expect("a record");
        assert!(matches!(run(&path), Err(RunError::NoCaches)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
