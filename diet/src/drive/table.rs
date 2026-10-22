//! A batch's receipt-level table (#914, #932): what each arm's runs did,
//! read from their logs, and each arm's deltas against the baseline. A table, not a claim: at small N the
//! interval describes the runs and tests nothing.
//!
//! Every row names its source: a line kind of the session log
//! (`diet/formats/log`), or the batch's `runs.jsonl`, the one source of a
//! run's unmet expectations and of whether it was void. No model is asked.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use crate::drive::pair::BATCH;
use crate::drive::scenario::{RUNS, repeat_dir};
use crate::formats::log::{self, Event, ToolOutcome};

/// The bootstrap's draws.
pub const DRAWS: usize = 10_000;

/// The seed unless `--seed` names another.
pub const SEED: u64 = 0;

/// The primary measure's row until the maintainer writes it (#932).
pub const PRIMARY_MEASURE: &str = "primary measure (#932): not declared";

/// One run's receipts, read from its log.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Receipts {
    /// `bash` calls that ran and exited non-zero, foreground or background.
    pub failed_commands: u64,
    /// `bash` calls that ran and no exit came back: a signal ended them.
    pub signalled_commands: u64,
    /// `bash` calls their confinement failed.
    pub confinement_failed: u64,
    /// Prompt tokens, summed over every response.
    pub prompt_tokens: u64,
    /// Output tokens, summed over every response.
    pub output_tokens: u64,
    /// Milliseconds from the session's open to its last line.
    pub wall_ms: u64,
    /// Each settled turn's milliseconds, from its ask to its settle.
    pub turn_ms: Vec<u64>,
    /// Settled turns.
    pub turns: u64,
    /// Forks opened.
    pub forks: u64,
    /// Patches written.
    pub captures: u64,
    /// Meditations that refilled from an empty working memory, carrying
    /// nothing (`meditation` with `empty: true`, #942).
    pub empty_refills: u64,
    /// Commands refused.
    pub refusals: u64,
    /// Settled turns, by reason.
    pub settles: BTreeMap<String, u64>,
    /// Unmet expectations, from `runs.jsonl`.
    pub unmet: u64,
    /// Each graded task (#942): the attempt its first right grade came at,
    /// or none when no grade of it was right.
    pub tasks: BTreeMap<String, Option<u64>>,
    /// Each lane's prefill (#932): cached and new tokens, over the
    /// responses that reported them, and how many did of how many.
    pub lanes: BTreeMap<String, Prefill>,
    /// Whether the run's head carried its own nonce (#932): `session.start`
    /// names one.
    pub nonced: bool,
    /// Each task's derived outcome (#942), from the run's `outcomes.json`
    /// when an analysis wrote one beside its log: `{"outcomes": {task: outcome}}`.
    pub outcomes: BTreeMap<String, String>,
    /// Whether the run went on in a later session its regimen started at
    /// the refill (#948): `refill_session` other than `same`.
    pub carried: bool,
    /// Whether that later session's carry held a file under the run's seal
    /// (#948): its `session.start` names a `carried_source`.
    pub carried_source: bool,
}

/// One lane's prefill in one run (#932).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Prefill {
    /// Prompt tokens a cache supplied.
    pub cached: u64,
    /// Prompt tokens prefilled anew.
    pub new: u64,
    /// Responses that reported both.
    pub reported: u64,
    /// Responses on the lane.
    pub responses: u64,
}

impl Prefill {
    /// `of`, when every response on the lane reported it; none otherwise,
    /// never 0 for a count nobody gave.
    fn whole(&self, of: u64) -> f64 {
        if self.responses > 0 && self.reported == self.responses {
            count(of)
        } else {
            f64::NAN
        }
    }
}

/// A response's cached and new prefill, where its server reported them:
/// llama.cpp's and `TabbyAPI`'s `timings` (`cache_n`, `prompt_n`), else a
/// hosted API's `usage` (`cached_tokens`, and the rest of `prompt_tokens`,
/// cache writes counted new).
fn prefill_of(timings: Option<&log::Timings>, usage: Option<&log::Usage>) -> Option<(u64, u64)> {
    match (timings.and_then(|t| t.cache_n.zip(t.prompt_n)), usage) {
        (Some(both), _) => Some(both),
        (None, Some(usage)) => usage
            .cached_tokens
            .map(|cached| (cached, usage.prompt_tokens.saturating_sub(cached))),
        (None, None) => None,
    }
}

impl Receipts {
    /// One run's receipts from its two sessions' (#948): the first's, and the
    /// later one's its regimen started at the refill. Counts and times add,
    /// each turn's time is kept, a task's grades are wherever it was graded,
    /// and a lane's prefill adds.
    #[must_use]
    pub fn merged(self, later: Self) -> Self {
        let mut settles = self.settles;
        for (reason, n) in later.settles {
            *settles.entry(reason).or_default() += n;
        }
        let mut tasks = self.tasks;
        tasks.extend(later.tasks);
        let mut lanes = self.lanes;
        for (lane, prefill) in later.lanes {
            let held = lanes.entry(lane).or_default();
            held.cached += prefill.cached;
            held.new += prefill.new;
            held.reported += prefill.reported;
            held.responses += prefill.responses;
        }
        Self {
            failed_commands: self.failed_commands + later.failed_commands,
            signalled_commands: self.signalled_commands + later.signalled_commands,
            confinement_failed: self.confinement_failed + later.confinement_failed,
            prompt_tokens: self.prompt_tokens + later.prompt_tokens,
            output_tokens: self.output_tokens + later.output_tokens,
            wall_ms: self.wall_ms + later.wall_ms,
            turn_ms: self.turn_ms.into_iter().chain(later.turn_ms).collect(),
            turns: self.turns + later.turns,
            forks: self.forks + later.forks,
            captures: self.captures + later.captures,
            empty_refills: self.empty_refills + later.empty_refills,
            refusals: self.refusals + later.refusals,
            settles,
            unmet: self.unmet,
            tasks,
            lanes,
            nonced: self.nonced && later.nonced,
            outcomes: self.outcomes,
            carried: true,
            carried_source: later.carried_source,
        }
    }

    /// A run's receipts from its log's lines, its unmet expectations given.
    #[must_use]
    pub fn of(lines: &[log::Line], unmet: u64) -> Self {
        let mut receipts = Self {
            unmet,
            ..Self::default()
        };
        let mut asked: BTreeMap<u32, u64> = BTreeMap::new();
        let mut lane_of: BTreeMap<u64, String> = BTreeMap::new();
        for line in lines {
            receipts.wall_ms = receipts.wall_ms.max(line.t);
            match &line.event {
                Event::ToolCall {
                    name,
                    outcome,
                    exit,
                    ..
                } if name == "bash" => match (outcome, exit) {
                    (ToolOutcome::Ran, Some(exit)) if *exit != 0 => receipts.failed_commands += 1,
                    (ToolOutcome::Ran, None) => receipts.signalled_commands += 1,
                    (ToolOutcome::CommandFailed, _) => receipts.confinement_failed += 1,
                    _ => {}
                },
                Event::BackgroundEnded {
                    exit: Some(exit), ..
                } if *exit != 0 => receipts.failed_commands += 1,
                Event::SessionStart { head_nonce, .. } => {
                    receipts.nonced = head_nonce.is_some();
                }
                Event::Request { lane, .. } => {
                    lane_of.insert(line.seq, lane.tag().to_owned());
                }
                Event::Response {
                    to_request,
                    usage,
                    timings,
                    ..
                } => {
                    if let Some(usage) = usage {
                        receipts.prompt_tokens += usage.prompt_tokens;
                        receipts.output_tokens += usage.completion_tokens;
                    }
                    if let Some(lane) = lane_of.get(to_request) {
                        let prefill = receipts.lanes.entry(lane.clone()).or_default();
                        prefill.responses += 1;
                        if let Some((cached, new)) = prefill_of(timings.as_ref(), usage.as_ref()) {
                            prefill.cached += cached;
                            prefill.new += new;
                            prefill.reported += 1;
                        }
                    }
                }
                Event::Ask { turn, .. } => {
                    asked.insert(*turn, line.t);
                }
                Event::TurnSettled { turn, reason } => {
                    receipts.turns += 1;
                    *receipts.settles.entry(reason.tag().to_owned()).or_default() += 1;
                    if let Some(at) = asked.get(turn) {
                        receipts.turn_ms.push(line.t.saturating_sub(*at));
                    }
                }
                Event::Grade {
                    task,
                    attempt,
                    right,
                    ..
                } => {
                    let first = receipts.tasks.entry(task.clone()).or_default();
                    if *right && first.is_none() {
                        *first = Some(*attempt);
                    }
                }
                Event::Fork { .. } => receipts.forks += 1,
                Event::Patch { .. } => receipts.captures += 1,
                Event::Meditation {
                    empty: Some(true), ..
                } => receipts.empty_refills += 1,
                Event::Refused { .. } => receipts.refusals += 1,
                _ => {}
            }
        }
        receipts
    }
}

/// One row: its name, its source, and its value for each run.
struct Row {
    name: String,
    source: String,
    value: Valued,
    /// What a run with no value prints: `—`, or `not reported` (#932).
    missing: &'static str,
    /// Each run's note, when the row keeps one beside its value.
    note: Option<Noted>,
}

/// A row's value for a run.
type Valued = Box<dyn Fn(&Receipts) -> f64>;

/// A row's note for a run.
type Noted = Box<dyn Fn(&Receipts) -> String>;

/// A per-turn value: the run's total over its settled turns, or none.
fn per_turn(total: u64, turns: u64) -> f64 {
    if turns == 0 {
        f64::NAN
    } else {
        count(total) / count(turns)
    }
}

/// A count as a float: exact below 2^53, which no receipt reaches.
#[allow(clippy::cast_precision_loss)]
fn count(n: u64) -> f64 {
    n as f64
}

fn rows(
    reasons: &BTreeSet<String>,
    tasks: &BTreeSet<String>,
    (lanes, nonced): (&BTreeSet<String>, bool),
) -> Vec<Row> {
    let row = |name: &str, source: &'static str, value: Box<dyn Fn(&Receipts) -> f64>| Row {
        name: name.to_owned(),
        source: source.to_owned(),
        value,
        missing: "—",
        note: None,
    };
    let mut rows = vec![
        row(
            "failed commands",
            "`tool_call` with `name: bash`, `outcome: ran` and `exit` ≠ 0; `background.ended` with `exit` ≠ 0",
            Box::new(|r| count(r.failed_commands)),
        ),
        row(
            "commands ended by a signal",
            "`tool_call` with `name: bash`, `outcome: ran` and no `exit`",
            Box::new(|r| count(r.signalled_commands)),
        ),
        row(
            "commands their confinement failed",
            "`tool_call` with `name: bash` and `outcome: command_failed`",
            Box::new(|r| count(r.confinement_failed)),
        ),
        row(
            "output tokens per run",
            "`response.usage.completion_tokens`, summed",
            Box::new(|r| count(r.output_tokens)),
        ),
        row(
            "output tokens per turn",
            "`response.usage.completion_tokens`, summed, ÷ `turn.settled` lines",
            Box::new(|r| per_turn(r.output_tokens, r.turns)),
        ),
        row(
            "prompt tokens per run",
            "`response.usage.prompt_tokens`, summed",
            Box::new(|r| count(r.prompt_tokens)),
        ),
        row(
            "prompt tokens per turn",
            "`response.usage.prompt_tokens`, summed, ÷ `turn.settled` lines",
            Box::new(|r| per_turn(r.prompt_tokens, r.turns)),
        ),
        row(
            "wall ms per run",
            "the last line's `t`",
            Box::new(|r| count(r.wall_ms)),
        ),
        row(
            "wall ms per turn",
            "`turn.settled`'s `t` less its turn's `ask`'s `t`, the run's median",
            Box::new(|r| median(&r.turn_ms.iter().copied().map(count).collect::<Vec<_>>())),
        ),
        row(
            "turns",
            "`turn.settled` lines",
            Box::new(|r| count(r.turns)),
        ),
        row("forks", "`fork` lines", Box::new(|r| count(r.forks))),
        row("captures", "`patch` lines", Box::new(|r| count(r.captures))),
        row(
            "empty refills",
            "`meditation` lines with `empty: true`: a refill that carried nothing",
            Box::new(|r| count(r.empty_refills)),
        ),
        row(
            "refusals",
            "`refused` lines",
            Box::new(|r| count(r.refusals)),
        ),
    ];
    for reason in reasons {
        let reason = reason.clone();
        rows.push(Row {
            name: format!("settles: {reason}"),
            source: "`turn.settled` lines, by `reason`".to_owned(),
            missing: "—",
            note: None,
            value: Box::new(move |r| count(r.settles.get(&reason).copied().unwrap_or(0))),
        });
    }
    rows.extend(prefill_rows(lanes, nonced));
    rows.extend(task_rows(tasks));
    rows.push(row(
        "unmet expectations",
        "`runs.jsonl`'s `unmet`, the batch's line for the run",
        Box::new(|r| count(r.unmet)),
    ));
    rows
}

/// Each graded task's rows (#942): attempts to right, and never right.
fn task_rows(tasks: &BTreeSet<String>) -> Vec<Row> {
    let mut rows = Vec::new();
    for task in tasks {
        let (first, never) = (task.clone(), task.clone());
        rows.push(Row {
            name: format!("attempts to right: {task}"),
            source:
                "`grade` lines of the task: the first `right: true`'s `attempt`; none when none is"
                    .to_owned(),
            missing: "—",
            note: None,
            value: Box::new(move |r| match r.tasks.get(&first) {
                Some(Some(attempt)) => count(*attempt),
                _ => f64::NAN,
            }),
        });
        rows.push(Row {
            name: format!("never right: {task}"),
            source: "`grade` lines of the task: 1 when none is `right: true`, 0 when one is; none when it was not graded".to_owned(),
            missing: "—",
            note: None,
            value: Box::new(move |r| match r.tasks.get(&never) {
                Some(Some(_)) => 0.0,
                Some(None) => 1.0,
                None => f64::NAN,
            }),
        });
    }
    rows
}

/// One row per task and outcome a run's `outcomes.json` names (#942): 1 for
/// a run whose task had that outcome, 0 for one whose had another, none for
/// one that has no outcome for the task; an arm's runs sum to its count.
fn outcome_rows<'a>(runs: impl Iterator<Item = &'a Receipts>) -> Vec<Row> {
    let named: BTreeSet<(String, String)> = runs
        .flat_map(|r| {
            r.outcomes
                .iter()
                .map(|(task, outcome)| (task.clone(), outcome.clone()))
        })
        .collect();
    named
        .iter()
        .map(|(task, outcome)| {
            let (task_of, outcome_of) = (task.clone(), outcome.clone());
            Row {
                name: format!("outcome: {task}: {outcome}"),
                source: "`outcomes.json` beside the run's log, written by an analysis of the run \
                         (diet/scenarios/lite/outcomes.py): 1 when the task's outcome is this one"
                    .to_owned(),
                missing: "—",
                note: None,
                value: Box::new(move |r| match r.outcomes.get(&task_of) {
                    Some(had) if *had == outcome_of => 1.0,
                    Some(_) => 0.0,
                    None => f64::NAN,
                }),
            }
        })
        .collect()
}

/// A run's receipts merged with the later session's it went on in (#948),
/// read from its `next/session.log`; as they were when it did not go on.
fn with_later_session(held: Receipts, dir_of_run: &Path) -> Result<Receipts, String> {
    let path = dir_of_run.join("next").join("session.log");
    if !path.exists() {
        return Ok(held);
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|why| format!("{} cannot be read: {why}", path.display()))?;
    let lines = log::parse(&text).map_err(|why| format!("{}: {why}", path.display()))?;
    let carried_source = matches!(
        lines.first().map(|line| &line.event),
        Some(Event::SessionStart {
            carried_source: Some(files),
            ..
        }) if !files.is_empty()
    );
    Ok(held.merged(Receipts {
        carried_source,
        ..Receipts::of(&lines, 0)
    }))
}

/// The carried runs' row (#948), when any run of the batch went on in a
/// later session: 1 for a run whose carry held a file under the run's
/// seal, so memory alone is told apart from memory with restored source.
fn carried_rows(carried: bool) -> Vec<Row> {
    if !carried {
        return Vec::new();
    }
    vec![Row {
        name: "carried sealed source".to_owned(),
        source: "the later session's `session.start` `carried_source`: 1 when its carry held a \
                 file under the run's seal, 0 when it did not"
            .to_owned(),
        missing: "—",
        note: None,
        value: Box::new(|r| {
            if !r.carried {
                f64::NAN
            } else if r.carried_source {
                1.0
            } else {
                0.0
            }
        }),
    }]
}

/// A run's outcomes (#942): its `outcomes.json`'s `outcomes`, or none.
fn outcomes_in(dir: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(dir.join("outcomes.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|read| {
            read["outcomes"].as_object().map(|outcomes| {
                outcomes
                    .iter()
                    .filter_map(|(task, outcome)| {
                        Some((task.clone(), outcome.as_str()?.to_owned()))
                    })
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// Each lane's prefill rows (#932), cached and new.
fn prefill_rows(lanes: &BTreeSet<String>, nonced: bool) -> Vec<Row> {
    let mut rows = Vec::new();
    // Each lane's prefill (#932), cached and new, never 0 for a count no
    // response gave; carryover-free only when every run's head was its own.
    let carryover = if nonced {
        "carryover-free: every run's head carried its own nonce"
    } else {
        "may include carryover from the run before: no head nonce"
    };
    let source = format!(
        "each `response`'s `timings` `cache_n` and `prompt_n` (llama.cpp, TabbyAPI), else its \
         `usage` `cached_tokens` and the rest of `prompt_tokens` (a hosted API), by its \
         `request`'s lane; not reported unless every response on the lane reported; {carryover}"
    );
    for lane in lanes {
        for (kind, cached) in [("cached", true), ("new", false)] {
            let (of, noted) = (lane.clone(), lane.clone());
            rows.push(Row {
                name: format!("{kind} prefill: {lane}"),
                source: source.clone(),
                value: Box::new(move |r| {
                    r.lanes.get(&of).map_or(f64::NAN, |prefill| {
                        prefill.whole(if cached { prefill.cached } else { prefill.new })
                    })
                }),
                missing: "not reported",
                note: Some(Box::new(move |r| {
                    let prefill = r.lanes.get(&noted).copied().unwrap_or_default();
                    format!("reported {} of {}", prefill.reported, prefill.responses)
                })),
            });
        }
    }
    rows
}

/// The median of `values`; none when there are none.
#[must_use]
pub fn median(values: &[f64]) -> f64 {
    let mut sorted: Vec<f64> = values.iter().copied().filter(|v| !v.is_nan()).collect();
    if sorted.is_empty() {
        return f64::NAN;
    }
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[middle]
    } else {
        f64::midpoint(sorted[middle - 1], sorted[middle])
    }
}

/// The mean of `values`; none when there are none.
#[must_use]
pub fn mean(values: &[f64]) -> f64 {
    let kept: Vec<f64> = values.iter().copied().filter(|v| !v.is_nan()).collect();
    if kept.is_empty() {
        return f64::NAN;
    }
    kept.iter().sum::<f64>() / count(kept.len() as u64)
}

/// `SplitMix64`: a seeded sequence, the same on every machine.
struct Draws(u64);

impl Draws {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// `values` resampled with replacement, as many as there are.
    fn resample(&mut self, values: &[f64]) -> Vec<f64> {
        let n = values.len() as u64;
        (0..n)
            .map(|_| values[usize::try_from(self.next() % n).unwrap_or(0)])
            .collect()
    }
}

/// A B − A delta and its 95% percentile interval: the 250th and 9,750th of
/// the [`DRAWS`] sorted draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Delta {
    /// B's statistic less A's.
    pub value: f64,
    /// The interval's lower end.
    pub low: f64,
    /// Its upper end.
    pub high: f64,
}

/// A 95% percentile interval's ends over `drawn`, its none left out: of
/// the draws sorted, ranks `(n − 1) × 25 / 1000` and `(n − 1) × 975 / 1000`
/// from 0, so the 250th and 9,750th of [`DRAWS`].
#[must_use]
pub fn ends(mut drawn: Vec<f64>) -> (f64, f64) {
    drawn.retain(|v| !v.is_nan());
    drawn.sort_by(f64::total_cmp);
    let Some(last) = drawn.len().checked_sub(1) else {
        return (f64::NAN, f64::NAN);
    };
    (drawn[last * 25 / 1000], drawn[last * 975 / 1000])
}

/// The B − A deltas of the medians and of the means, each with its
/// interval from the same draws: each arm's runs resampled independently.
#[must_use]
pub fn deltas(
    a: &[f64],
    b: &[f64],
    draws: &mut impl FnMut() -> (Vec<f64>, Vec<f64>),
) -> (Delta, Delta) {
    let (mut medians, mut means) = (Vec::with_capacity(DRAWS), Vec::with_capacity(DRAWS));
    for _ in 0..DRAWS {
        let (a, b) = draws();
        medians.push(median(&b) - median(&a));
        means.push(mean(&b) - mean(&a));
    }
    let interval = |value: f64, drawn: Vec<f64>| {
        let (low, high) = ends(drawn);
        Delta { value, low, high }
    };
    (
        interval(median(b) - median(a), medians),
        interval(mean(b) - mean(a), means),
    )
}

/// A number as the table prints it: whole numbers whole, else to two
/// places; `—` for none.
#[must_use]
pub fn shown(value: f64) -> String {
    if value.is_nan() {
        "—".to_owned()
    } else if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}

/// An arm's runs, summarised: their median, range and mean.
struct Spread {
    median: f64,
    min: f64,
    max: f64,
    mean: f64,
}

impl Spread {
    fn of(values: &[f64]) -> Self {
        let kept: Vec<f64> = values.iter().copied().filter(|v| !v.is_nan()).collect();
        Self {
            median: median(values),
            min: kept.iter().copied().reduce(f64::min).unwrap_or(f64::NAN),
            max: kept.iter().copied().reduce(f64::max).unwrap_or(f64::NAN),
            mean: mean(values),
        }
    }
}

/// A number as the JSON holds it: `null` for none.
fn number(value: f64) -> serde_json::Value {
    if value.is_nan() {
        serde_json::Value::Null
    } else {
        serde_json::json!(value)
    }
}

/// An arm's runs: its name and each run's receipts, in `runs.jsonl` order.
type ArmRuns = (String, Vec<Receipts>);

/// One row, as the JSON holds it and as a Markdown line: each arm's
/// spread, in name order, then each other arm's deltas against the
/// baseline's, `base` its index. Each pair of arms draws in turn, the
/// baseline's runs then the other's, so a pair's draws are as they were.
fn laid_out(
    row: &Row,
    (arms, base): (&[ArmRuns], usize),
    draws: &mut Draws,
) -> (serde_json::Value, String) {
    let values = |runs: &[Receipts]| runs.iter().map(|r| (row.value)(r)).collect::<Vec<_>>();
    let under: Vec<Vec<f64>> = arms.iter().map(|(_, runs)| values(runs)).collect();
    let mut against = Vec::new();
    for (at, (name, _)) in arms.iter().enumerate().filter(|(at, _)| *at != base) {
        let (from, to) = (&under[base], &under[at]);
        let found = deltas(from, to, &mut || (draws.resample(from), draws.resample(to)));
        against.push((name.clone(), found));
    }
    let spreads: Vec<Spread> = under.iter().map(|values| Spread::of(values)).collect();
    let or_missing = |value: f64| {
        if value.is_nan() {
            row.missing.to_owned()
        } else {
            shown(value)
        }
    };
    let arm = |spread: &Spread| {
        format!(
            "{} [{}, {}]",
            or_missing(spread.median),
            or_missing(spread.min),
            or_missing(spread.max)
        )
    };
    let interval = |delta: &Delta| {
        format!(
            "{} [{}, {}]",
            shown(delta.value),
            shown(delta.low),
            shown(delta.high)
        )
    };
    let mut line = format!("| {} |", row.name);
    for spread in &spreads {
        let _ = write!(line, " {} |", arm(spread));
    }
    for (_, (by_median, by_mean)) in &against {
        let _ = write!(line, " {} | {} |", interval(by_median), interval(by_mean));
    }
    let _ = writeln!(line, " {} |", row.source);
    let held = |runs: &[f64], spread: &Spread| {
        serde_json::json!({
            "runs": runs.iter().copied().map(number).collect::<Vec<_>>(),
            "median": number(spread.median),
            "min": number(spread.min),
            "max": number(spread.max),
            "mean": number(spread.mean),
        })
    };
    let delta = |delta: &Delta| serde_json::json!({"value": number(delta.value), "low": number(delta.low), "high": number(delta.high)});
    let mut by_arm = serde_json::Map::new();
    for ((name, runs), (values, spread)) in arms.iter().zip(under.iter().zip(&spreads)) {
        let mut held = held(values, spread);
        if let Some(note) = &row.note {
            held["notes"] = serde_json::json!(runs.iter().map(note).collect::<Vec<_>>());
        }
        by_arm.insert(name.clone(), held);
    }
    let deltas: serde_json::Map<String, serde_json::Value> = against
        .iter()
        .map(|(name, (by_median, by_mean))| {
            (
                name.clone(),
                serde_json::json!({"delta_median": delta(by_median), "delta_mean": delta(by_mean)}),
            )
        })
        .collect();
    let json = serde_json::json!({
        "row": row.name,
        "source": row.source,
        "by_arm": by_arm,
        "deltas": deltas,
    });
    (json, line)
}

/// A run named in `runs.jsonl`: its arm, its repeat and its unmet count.
struct Ran {
    arm: String,
    repeat: u64,
    unmet: u64,
}

/// The non-void runs `runs.jsonl` names, in its order.
fn ran(dir: &Path) -> Result<Vec<Ran>, String> {
    let path = dir.join(RUNS);
    let text = std::fs::read_to_string(&path)
        .map_err(|why| format!("{} cannot be read: {why}", path.display()))?;
    let mut runs = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let read: serde_json::Value = serde_json::from_str(line)
            .map_err(|why| format!("{} line {}: {why}", path.display(), index + 1))?;
        if !read["void"].is_null() {
            continue;
        }
        let arm = read["arm"].as_str().ok_or_else(|| {
            format!(
                "{} line {} names no arm: the table reads a `pair` or `arms` batch",
                path.display(),
                index + 1
            )
        })?;
        runs.push(Ran {
            arm: arm.to_owned(),
            repeat: read["repeat"].as_u64().unwrap_or(0),
            unmet: read["unmet"]
                .as_array()
                .map_or(0, |unmet| unmet.len() as u64),
        });
    }
    Ok(runs)
}

/// A batch's baseline (#932): what its `batch.json` names, or, for a pair
/// batch that kept none, `a`.
fn baseline_of(dir: &Path, names: &BTreeSet<&str>) -> Result<String, String> {
    let path = dir.join(BATCH);
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let read: serde_json::Value =
                serde_json::from_str(&text).map_err(|why| format!("{}: {why}", path.display()))?;
            read["baseline"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{} names no baseline", path.display()))
        }
        Err(_) if *names == BTreeSet::from(["a", "b"]) => Ok("a".to_owned()),
        Err(why) => Err(format!(
            "{} cannot be read, and a batch whose arms are not `a` and `b` names its baseline \
             there: {why}",
            path.display()
        )),
    }
}

/// The Markdown table's head: a column per arm's spread, in name order,
/// two per other arm's deltas against `baseline`, then the primary
/// measure's row.
fn header(arms: &[ArmRuns], baseline: &str) -> String {
    let mut markdown = String::from("| row |");
    let others: Vec<&str> = arms
        .iter()
        .filter(|(name, _)| *name != baseline)
        .map(|(name, _)| name.as_str())
        .collect();
    for (name, _) in arms {
        let _ = write!(markdown, " {name} median [min, max] |");
    }
    for name in &others {
        let _ = write!(
            markdown,
            " {name} − {baseline} median [95%] | {name} − {baseline} mean [95%] |"
        );
    }
    markdown.push_str(" source |\n|");
    let columns = 2 + arms.len() + 2 * others.len();
    markdown.push_str(&" --- |".repeat(columns));
    let _ = writeln!(
        markdown,
        "\n| {PRIMARY_MEASURE} |{}",
        " |".repeat(columns - 1)
    );
    markdown
}

/// A batch's table (#914, #932), from `dir`'s `runs.jsonl` and each run's
/// log at `dir/<arm>/repeat-NN/session.log`, its draws seeded by `seed`:
/// each arm's receipts, and each other arm's deltas against the baseline,
/// as JSON and as Markdown.
///
/// # Errors
///
/// When a file cannot be read, a log does not parse, a log's `run` is not
/// the run `runs.jsonl` names there, the runs name more than one batch, or
/// the baseline has no runs.
pub fn table(dir: &Path, seed: u64) -> Result<(serde_json::Value, String), String> {
    let mut receipts: BTreeMap<String, Vec<Receipts>> = BTreeMap::new();
    let mut batches = BTreeSet::new();
    for run in ran(dir)? {
        let path = repeat_dir(&dir.join(&run.arm), run.repeat).join("session.log");
        let text = std::fs::read_to_string(&path)
            .map_err(|why| format!("{} cannot be read: {why}", path.display()))?;
        let lines = log::parse(&text).map_err(|why| format!("{}: {why}", path.display()))?;
        let Some(Event::SessionStart {
            run: Some(named), ..
        }) = lines.first().map(|line| &line.event)
        else {
            return Err(format!("{} names no run on its start", path.display()));
        };
        match &named.paired {
            Some((arm, batch)) if *arm == run.arm && named.repeat == run.repeat => {
                batches.insert(batch.clone());
            }
            _ => {
                return Err(format!(
                    "{}'s run is not arm {} repeat {}",
                    path.display(),
                    run.arm,
                    run.repeat
                ));
            }
        }
        let dir_of_run = path.parent().unwrap_or_else(|| Path::new("."));
        let held = Receipts {
            outcomes: outcomes_in(dir_of_run),
            ..Receipts::of(&lines, run.unmet)
        };
        receipts
            .entry(run.arm)
            .or_default()
            .push(with_later_session(held, dir_of_run)?);
    }
    if batches.len() > 1 {
        return Err(format!(
            "the runs name {} batches: {batches:?}",
            batches.len()
        ));
    }
    let names: BTreeSet<&str> = receipts.keys().map(String::as_str).collect();
    let baseline = baseline_of(dir, &names)?;
    if !receipts.contains_key(&baseline) || receipts.len() < 2 {
        return Err(format!(
            "a table needs runs of the baseline `{baseline}` and of at least one other arm; \
             the runs are of {names:?}"
        ));
    }
    let arms: Vec<ArmRuns> = receipts.into_iter().collect();
    let base = arms
        .iter()
        .position(|(name, _)| *name == baseline)
        .unwrap_or(0);
    let all = || arms.iter().flat_map(|(_, runs)| runs);
    let reasons: BTreeSet<String> = all().flat_map(|r| r.settles.keys().cloned()).collect();
    let tasks: BTreeSet<String> = all().flat_map(|r| r.tasks.keys().cloned()).collect();
    let lanes: BTreeSet<String> = all().flat_map(|r| r.lanes.keys().cloned()).collect();
    let nonced = all().all(|r| r.nonced);
    let mut draws = Draws(seed);
    let mut json_rows = Vec::new();
    let mut markdown = header(&arms, &baseline);
    for row in rows(&reasons, &tasks, (&lanes, nonced))
        .into_iter()
        .chain(carried_rows(all().any(|r| r.carried)))
        .chain(outcome_rows(all()))
    {
        let (json, line) = laid_out(&row, (&arms, base), &mut draws);
        json_rows.push(json);
        markdown.push_str(&line);
    }
    let digest = batches.into_iter().next();
    let ran: BTreeMap<&str, usize> = arms
        .iter()
        .map(|(name, runs)| (name.as_str(), runs.len()))
        .collect();
    let mut head = String::new();
    let _ = writeln!(
        head,
        "# Arms table\n\nBatch `{}`, baseline `{baseline}`. Runs per arm: {}. Each delta is an \
         arm's minus the baseline's, with a 95% percentile interval over {DRAWS} draws, each \
         arm's runs resampled independently, seed {seed}. At small N the interval describes the \
         runs; it is not a test.\n",
        digest.as_deref().unwrap_or("?"),
        ran.iter()
            .map(|(name, n)| format!("{name} {n}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let json = serde_json::json!({
        "arms": digest,
        "baseline": baseline,
        "seed": seed,
        "draws": DRAWS,
        "runs": ran,
        "primary_measure": PRIMARY_MEASURE,
        "rows": json_rows,
    });
    Ok((json, head + &markdown))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #948: a run that went on in a second session is one run: its turns
    /// and times are both sessions', a turn's time kept from each, and its
    /// carry is flagged, with the sealed source the second start names. A
    /// run with no `next/` is as its one log says, and the carried row is
    /// shown only when some run carried.
    #[test]
    fn a_carried_run_is_both_its_sessions_and_says_what_its_carry_held() {
        const FIRST: &str = include_str!(
            "../../formats/log/fixtures/valid/a-v16-meditation-carrying-a-native-summary.jsonl"
        );
        const LATER: &str = include_str!(
            "../../formats/log/fixtures/valid/a-v24-carried-session-whose-meditation-kept-its-trunk.jsonl"
        );
        let dir = std::env::temp_dir().join(format!("diet-table-carried-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("next")).expect("the run's directories");
        let first = Receipts::of(&log::parse(FIRST).expect("a log"), 0);
        let alone = with_later_session(first.clone(), &dir).expect("no later session");
        assert!(!alone.carried && !alone.carried_source);
        assert_eq!(alone.turns, first.turns);
        std::fs::write(dir.join("next").join("session.log"), LATER).expect("the later log");
        let later = Receipts::of(&log::parse(LATER).expect("a log"), 0);
        let both = with_later_session(first.clone(), &dir).expect("both sessions");
        assert!(both.carried && both.carried_source);
        assert_eq!(both.turns, first.turns + later.turns);
        assert_eq!(both.wall_ms, first.wall_ms + later.wall_ms);
        assert_eq!(
            both.turn_ms.len(),
            first.turn_ms.len() + later.turn_ms.len()
        );
        let unsealed = LATER.replace(
            r#""carried_source":["packages/lite/src/scene-core.ts"],"#,
            "",
        );
        assert_ne!(unsealed, LATER, "the later start names its sealed source");
        std::fs::write(dir.join("next").join("session.log"), &unsealed).expect("the later log");
        let memory_alone = with_later_session(first, &dir).expect("both sessions");
        assert!(memory_alone.carried && !memory_alone.carried_source);
        let row = |carried| carried_rows(carried).len();
        assert_eq!((row(false), row(true)), (0, 1));
        let value = &carried_rows(true)[0].value;
        assert!(
            value(&alone).is_nan(),
            "a run that did not carry has no value"
        );
        assert!((value(&both) - 1.0).abs() < f64::EPSILON);
        assert!(value(&memory_alone).abs() < f64::EPSILON);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #942: an empty refill is counted from the meditation line that says
    /// so, and a refill that carried something is not.
    #[test]
    fn an_empty_refill_is_counted_from_its_line() {
        let read = |text: &str| Receipts::of(&log::parse(text).expect("the fixture reads"), 0);
        let empty = read(include_str!(
            "../../formats/log/fixtures/valid/a-v23-meditation-refilled-from-an-empty-working-memory.jsonl"
        ));
        assert_eq!(empty.empty_refills, 1);
        let carried = read(include_str!(
            "../../formats/log/fixtures/valid/a-v16-meditation-carrying-a-native-summary.jsonl"
        ));
        assert_eq!(carried.empty_refills, 0);
    }

    /// #932: a lane's prefill comes from the responses that report it --
    /// `timings` first, then a hosted `usage` -- and a lane where any
    /// response did not report is none, never 0, its note saying how many
    /// did of how many.
    #[test]
    fn a_lanes_prefill_is_what_its_responses_reported_and_none_otherwise() {
        const AUDITED: &str = include_str!(
            "../../formats/log/fixtures/valid/a-v8-meditation-audited-on-its-lane-and-warmed.jsonl"
        );
        let text = AUDITED
            .replace(
                r#""text":"a tracker for the team's bugs","to_request":3"#,
                r#""text":"a tracker for the team's bugs","timings":{"cache_n":100,"prompt_n":20},"to_request":3"#,
            );
        assert_ne!(text, AUDITED, "the trunk's response reports its prefill");
        let receipts = Receipts::of(&log::parse(&text).expect("a log"), 0);
        assert_eq!(
            receipts.lanes["trunk"],
            Prefill {
                cached: 100,
                new: 20,
                reported: 1,
                responses: 1
            }
        );
        assert!((receipts.lanes["trunk"].whole(100) - 100.0).abs() < f64::EPSILON);
        let audit = receipts.lanes["audit"];
        assert_eq!((audit.reported, audit.responses), (0, 1));
        assert!(audit.whole(audit.cached).is_nan(), "not reported, never 0");
        assert!(!receipts.nonced);
        // A hosted API's usage: the rest of the prompt is new.
        let hosted = log::Usage {
            prompt_tokens: 1000,
            completion_tokens: 7,
            cached_tokens: Some(900),
            cache_creation_tokens: Some(60),
            cache_creation_5m_tokens: None,
            cache_creation_1h_tokens: None,
        };
        assert_eq!(prefill_of(None, Some(&hosted)), Some((900, 100)));
        let unreported = log::Usage {
            cached_tokens: None,
            ..hosted
        };
        assert_eq!(prefill_of(None, Some(&unreported)), None);
    }

    /// The median and the mean, by hand.
    #[test]
    fn the_median_and_the_mean_are_as_written() {
        assert!((median(&[3.0, 1.0, 2.0]) - 2.0).abs() < f64::EPSILON);
        assert!((median(&[4.0, 1.0, 2.0, 3.0]) - 2.5).abs() < f64::EPSILON);
        assert!((mean(&[1.0, 2.0, 6.0]) - 3.0).abs() < f64::EPSILON);
        assert!(median(&[]).is_nan() && mean(&[f64::NAN]).is_nan());
    }

    /// Arms whose runs agree within each arm have no spread to resample:
    /// both intervals collapse onto their delta, whatever the seed.
    #[test]
    fn runs_alike_within_each_arm_give_a_point_interval() {
        for seed in [0, 7] {
            let mut draws = Draws(seed);
            let (a, b) = (vec![1000.0, 1000.0], vec![2500.0, 2500.0]);
            let (by_median, by_mean) =
                deltas(&a, &b, &mut || (draws.resample(&a), draws.resample(&b)));
            for delta in [by_median, by_mean] {
                assert_eq!(
                    (delta.value, delta.low, delta.high),
                    (1500.0, 1500.0, 1500.0)
                );
            }
        }
    }

    /// The interval's ends are the 250th and 9,750th of 10,000 sorted draws.
    #[test]
    fn the_interval_ends_are_the_250th_and_9750th_draws() {
        let drawn: Vec<f64> = (1..=10_000).rev().map(f64::from).collect();
        assert_eq!(ends(drawn), (250.0, 9750.0));
        assert!(ends(Vec::new()).0.is_nan());
    }

    /// One seed draws the same interval every time; the interval holds its
    /// delta and lies within what any resample could give.
    #[test]
    fn a_seed_draws_the_same_interval_every_time() {
        let (a, b) = (vec![1.0, 2.0, 9.0], vec![4.0, 5.0, 6.0]);
        let run = |seed| {
            let mut draws = Draws(seed);
            deltas(&a, &b, &mut || (draws.resample(&a), draws.resample(&b)))
        };
        assert_eq!(run(3), run(3));
        let (by_median, by_mean) = run(3);
        assert!((by_median.value - 3.0).abs() < f64::EPSILON);
        assert!((by_mean.value - 1.0).abs() < f64::EPSILON);
        for delta in [by_median, by_mean] {
            assert!(
                delta.low <= delta.value && delta.value <= delta.high,
                "{delta:?}"
            );
            assert!(
                delta.low >= 4.0 - 9.0 && delta.high <= 6.0 - 1.0,
                "{delta:?}"
            );
        }
    }
}
