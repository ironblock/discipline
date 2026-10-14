//! The step warrants' gate: today's patterns, or a classifier that scores
//! each step against the hypotheses the regimen names.
//!
//! `warrant_gate = "regex"`, the default, is #783's gate, unchanged. With
//! `warrant_gate = "classifier"` a step's `trouble` and `open` forks fire on
//! scores from a Sidekick classifier (`POST /v1/classify`, `format = "nli"`):
//! each probe scores its windows of the step against its hypotheses, and its
//! class fires when the probe's aggregate reaches its threshold. A trouble
//! fork then asks only the classes that fired. The classifier gates; it never
//! writes into working memory.
//!
//! No setting has a product default: the regimen names the endpoint, the
//! model and its revision, the compute units, the truncation side, the
//! timeout, the windowing, and every probe's class, input, hypotheses,
//! aggregation and threshold, or the session refuses to start.
//!
//! No harness this one follows gates its side calls with a classifier, so
//! there is no convention to follow here.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

use crate::formats::regimen::{Regimen, Value};

/// The regimen key choosing the gate: `regex` or `classifier`.
pub const WARRANT_GATE: &str = "warrant_gate";
/// The regimen table the classifier's settings live in.
pub const WARRANT_CLASSIFIER: &str = "warrant_classifier";

/// A class a probe fires: the trouble trio's three, or `open`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GateClass {
    /// `GOTCHAS`.
    Gotcha,
    /// `DECISIONS`.
    Decision,
    /// `CONSTRAINTS`.
    Constraint,
    /// `OPEN_NEXT`.
    Open,
}

impl GateClass {
    /// Every class, in the trio's order, then `open`.
    pub const ALL: [Self; 4] = [Self::Gotcha, Self::Decision, Self::Constraint, Self::Open];

    /// The regimen's and the log's word for it.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Gotcha => "gotcha",
            Self::Decision => "decision",
            Self::Constraint => "constraint",
            Self::Open => "open",
        }
    }

    fn of(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|class| class.word() == word)
    }
}

/// What a probe scores: the step's own prose or its calls' output, both,
/// or the previous step's -- the output a step's prose answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Input {
    /// The step's reasoning, a newline, then its text.
    Prose,
    /// Each of its calls' shown output.
    Output,
    /// Both.
    Both,
    /// The previous step's prose.
    PreviousProse,
    /// The previous step's calls' output.
    PreviousOutput,
}

impl Input {
    /// Every input, in this order.
    pub const ALL: [Self; 5] = [
        Self::Prose,
        Self::Output,
        Self::Both,
        Self::PreviousProse,
        Self::PreviousOutput,
    ];

    /// The regimen's word for it.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Prose => "prose",
            Self::Output => "output",
            Self::Both => "both",
            Self::PreviousProse => "previous_prose",
            Self::PreviousOutput => "previous_output",
        }
    }
}

/// How a probe reduces its hypotheses' scores, each already the max over
/// its windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aggregate {
    /// The highest.
    Max,
    /// Their mean.
    Mean,
}

/// One probe: a class, the windows it reads, its hypotheses and how they
/// combine, and the threshold its aggregate must reach.
#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    /// Its name: its table's key.
    pub name: String,
    /// The class it fires.
    pub class: GateClass,
    /// The windows it reads.
    pub input: Input,
    /// Its hypotheses, each sent whole as a candidate label.
    pub hypotheses: Vec<String>,
    /// How the hypotheses' scores combine.
    pub aggregate: Aggregate,
    /// The aggregate it fires at or above, as written.
    pub threshold: String,
}

/// The classifier's settings, every one named by the regimen.
#[derive(Debug, Clone, PartialEq)]
pub struct Classifier {
    /// Where Sidekick listens: `http://<host>:<port>`.
    pub endpoint: String,
    /// The model id, as Sidekick serves it.
    pub model: String,
    /// The model's revision.
    pub revision: String,
    /// The compute units the daemon serves it on.
    pub compute_units: String,
    /// `left` or `right`: which end of a window Sidekick cuts.
    pub truncation_side: String,
    /// How long a step's call may take before the step falls back.
    pub timeout: Duration,
    /// The sentences in a prose window, at most.
    pub prose_window_sentences: usize,
    /// The characters in a prose window, at most.
    pub prose_window_chars: usize,
    /// The characters of an output's tail window.
    pub output_tail_chars: usize,
    /// The windows one request carries, at most: the model's `max_batch`.
    pub batch: usize,
    /// The probes, by name.
    pub probes: Vec<Probe>,
}

/// The gate a regimen chose.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum WarrantGate {
    /// #783's patterns, unchanged: the default.
    #[default]
    Regex,
    /// A classifier.
    Classifier(Box<Classifier>),
}

/// The gate `regimen` declares.
///
/// # Errors
///
/// A gate word that is neither `regex` nor `classifier`; or, for a
/// classifier, any setting missing or malformed, naming it -- there are no
/// defaults.
pub fn read(regimen: &Regimen) -> Result<WarrantGate, String> {
    match regimen.get(WARRANT_GATE) {
        None => Ok(WarrantGate::Regex),
        Some(Value::String(word)) if word == "regex" => Ok(WarrantGate::Regex),
        Some(Value::String(word)) if word == "classifier" => {
            let Some(Value::Table(table)) = regimen.get(WARRANT_CLASSIFIER) else {
                return Err(format!(
                    "`{WARRANT_GATE} = \"classifier\"` needs a `[{WARRANT_CLASSIFIER}]` table"
                ));
            };
            classifier(table).map(|classifier| WarrantGate::Classifier(Box::new(classifier)))
        }
        Some(_) => Err(format!(
            "`{WARRANT_GATE}` is `regex` or `classifier`, and nothing else"
        )),
    }
}

fn classifier(table: &BTreeMap<String, Value>) -> Result<Classifier, String> {
    let at = |key: &str| format!("`{WARRANT_CLASSIFIER}.{key}`");
    let text = |key: &str| match table.get(key) {
        Some(Value::String(word)) if !word.is_empty() => Ok(word.clone()),
        _ => Err(format!("{} is required, a nonempty string", at(key))),
    };
    let count = |key: &str| match table.get(key) {
        Some(Value::Integer(n)) if *n > 0 => {
            usize::try_from(*n).map_err(|_| format!("{} is too large", at(key)))
        }
        _ => Err(format!("{} is required, a whole number above 0", at(key))),
    };
    let endpoint = text("endpoint")?;
    if !loopback(&endpoint) {
        return Err(format!(
            "{} is a loopback address -- `http://127.0.0.1`, `http://localhost` or \
             `http://[::1]`, with any port: the step's reasoning and tool output go to it \
             over plain HTTP, so it must be this machine",
            at("endpoint")
        ));
    }
    let revision = text("revision")?;
    if revision.len() != 40 || !revision.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "{} is the model's full commit, 40 hex digits: a prefix is not provenance",
            at("revision")
        ));
    }
    let truncation_side = text("truncation_side")?;
    if !["left", "right"].contains(&truncation_side.as_str()) {
        return Err(format!("{} is `left` or `right`", at("truncation_side")));
    }
    let mut probes = Vec::new();
    for (name, value) in table {
        let Value::Table(probe) = value else {
            continue;
        };
        probes.push(read_probe(name, probe)?);
    }
    if probes.is_empty() {
        return Err(format!(
            "`[{WARRANT_CLASSIFIER}]` names no probe: a `[{WARRANT_CLASSIFIER}.<name>]` table \
             with a class, an input, its hypotheses, an aggregate and a threshold"
        ));
    }
    Ok(Classifier {
        endpoint,
        model: text("model")?,
        revision,
        compute_units: text("compute_units")?,
        truncation_side,
        timeout: Duration::from_millis(count("timeout_ms")? as u64),
        prose_window_sentences: count("prose_window_sentences")?,
        prose_window_chars: count("prose_window_chars")?,
        output_tail_chars: count("output_tail_chars")?,
        batch: count("batch")?,
        probes,
    })
}

fn read_probe(name: &str, table: &BTreeMap<String, Value>) -> Result<Probe, String> {
    let at = |key: &str| format!("`{WARRANT_CLASSIFIER}.{name}.{key}`");
    let word = |key: &str| match table.get(key) {
        Some(Value::String(word)) => Ok(word.as_str()),
        _ => Err(format!("{} is required, a string", at(key))),
    };
    let class = GateClass::of(word("class")?).ok_or_else(|| {
        format!(
            "{} is `gotcha`, `decision`, `constraint` or `open`",
            at("class")
        )
    })?;
    let input = word("input").and_then(|word| {
        Input::ALL
            .into_iter()
            .find(|input| input.word() == word)
            .ok_or_else(|| {
                format!(
                    "{} is `prose`, `output`, `both`, `previous_prose` or `previous_output`",
                    at("input")
                )
            })
    })?;
    let aggregate = match word("aggregate")? {
        "max" => Aggregate::Max,
        "mean" => Aggregate::Mean,
        _ => return Err(format!("{} is `max` or `mean`", at("aggregate"))),
    };
    let hypotheses = match table.get("hypotheses") {
        Some(Value::Array(items)) if !items.is_empty() => items
            .iter()
            .map(|item| match item {
                Value::String(text) if !text.is_empty() => Ok(text.clone()),
                _ => Err(format!("{} holds nonempty strings", at("hypotheses"))),
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => {
            return Err(format!(
                "{} is required, a list of strings",
                at("hypotheses")
            ));
        }
    };
    let threshold = match table.get("threshold") {
        Some(Value::Float(decimal)) => decimal.as_str().to_owned(),
        Some(Value::Integer(n)) if *n == 0 || *n == 1 => n.to_string(),
        _ => {
            return Err(format!(
                "{} is required, a number from 0 to 1",
                at("threshold")
            ));
        }
    };
    if !threshold
        .parse::<f64>()
        .is_ok_and(|value| (0.0..=1.0).contains(&value))
    {
        return Err(format!("{} is a number from 0 to 1", at("threshold")));
    }
    Ok(Probe {
        name: name.to_owned(),
        class,
        input,
        hypotheses,
        aggregate,
        threshold,
    })
}

/// A step, as the classifier reads it: what it said, and what each of its
/// calls showed the model.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StepText {
    /// Its reasoning, a newline, then its text.
    pub said: String,
    /// Each call that ran: its arguments, as the model wrote them, and its
    /// shown output.
    pub outputs: Vec<(String, String)>,
}

/// A step's windows: its prose's and its calls' output's.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Windows {
    /// Sentence windows of the prose.
    pub prose: Vec<String>,
    /// Each call's output windows.
    pub output: Vec<String>,
}

impl Windows {
    /// The windows of `step` under `classifier`'s windowing.
    #[must_use]
    pub fn of(classifier: &Classifier, step: &StepText) -> Self {
        Self {
            prose: prose_windows(
                &step.said,
                classifier.prose_window_sentences,
                classifier.prose_window_chars,
            ),
            output: step
                .outputs
                .iter()
                .flat_map(|(command, output)| {
                    output_windows(command, output, classifier.output_tail_chars)
                })
                .collect(),
        }
    }
}

/// The windows `input` reads, of this step and the `previous`.
fn windows_for<'a>(input: Input, this: &'a Windows, previous: Option<&'a Windows>) -> Vec<&'a str> {
    let empty: &'a [String] = &[];
    let all = |list: &'a [String]| list.iter().map(String::as_str);
    match input {
        Input::Prose => all(&this.prose).collect(),
        Input::Output => all(&this.output).collect(),
        Input::Both => all(&this.prose).chain(all(&this.output)).collect(),
        Input::PreviousProse => all(previous.map_or(empty, |previous| &previous.prose)).collect(),
        Input::PreviousOutput => all(previous.map_or(empty, |previous| &previous.output)).collect(),
    }
}

/// `text`'s sentence windows, as the trouble-detector register windows
/// them: each line, stripped -- code-fence lines skipped -- is split after
/// `.`, `!`, `?` or `:` at the whitespace before the next word; a sentence
/// longer than `chars` is chopped at its last space before `chars` (at
/// `chars` when that space falls in the first half); a sentence with fewer
/// than three word characters is dropped; and the sentences are packed,
/// greedily and without overlap, into windows of at most `sentences`
/// sentences and `chars` characters, joined by a space.
#[must_use]
pub fn prose_windows(text: &str, sentences: usize, chars: usize) -> Vec<String> {
    let mut split: Vec<String> = Vec::new();
    for line in text.split('\n') {
        let line = line.trim();
        if line.is_empty() || line.starts_with("```") {
            continue;
        }
        for sentence in split_sentences(line) {
            let sentence = sentence.trim();
            if !sentence.is_empty() {
                split.extend(chop(sentence, chars));
            }
        }
    }
    split.retain(|sentence| {
        sentence
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_')
            .count()
            >= 3
    });
    let mut windows: Vec<String> = Vec::new();
    let mut window: Vec<String> = Vec::new();
    for sentence in split {
        // The window with this sentence, joined by spaces, in characters.
        let longer = window
            .iter()
            .chain(std::iter::once(&sentence))
            .map(|part| part.chars().count())
            .sum::<usize>()
            + window.len();
        if !window.is_empty() && (window.len() >= sentences || longer > chars) {
            windows.push(window.join(" "));
            window.clear();
        }
        window.push(sentence);
    }
    if !window.is_empty() {
        windows.push(window.join(" "));
    }
    windows
}

/// `line` split at each run of whitespace that follows `.`, `!`, `?` or `:`
/// and comes before a non-space.
fn split_sentences(line: &str) -> Vec<String> {
    let letters: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut current = String::new();
    let mut at = 0;
    while at < letters.len() {
        let c = letters[at];
        if c.is_whitespace() && at > 0 && matches!(letters[at - 1], '.' | '!' | '?' | ':') {
            let mut end = at;
            while end < letters.len() && letters[end].is_whitespace() {
                end += 1;
            }
            if end < letters.len() {
                out.push(std::mem::take(&mut current));
                at = end;
                continue;
            }
        }
        current.push(c);
        at += 1;
    }
    out.push(current);
    out
}

/// `sentence` chopped into pieces of at most `chars` characters, each cut at
/// its last space before `chars`, or at `chars` when that space falls in the
/// first half; each piece stripped.
fn chop(sentence: &str, chars: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest: Vec<char> = sentence.chars().collect();
    while rest.len() > chars {
        let cut = match rest[..chars].iter().rposition(|c| *c == ' ') {
            Some(at) if at >= chars / 2 => at,
            _ => chars,
        };
        out.push(rest[..cut].iter().collect::<String>().trim().to_owned());
        rest = rest[cut..]
            .iter()
            .collect::<String>()
            .trim()
            .chars()
            .collect();
    }
    if !rest.is_empty() {
        out.push(rest.into_iter().collect());
    }
    out
}

/// A call's output windows, as the register windows them: `$ ` and the
/// first 200 characters of `command`, a line break, then the last `tail`
/// characters of the output (`(no output)` when it has none); and, when a
/// line looks like an error, the same head and that line's first 400
/// characters, unless the tail window already holds it.
#[must_use]
pub fn output_windows(command: &str, output: &str, tail: usize) -> Vec<String> {
    // The arguments' first 200 characters, then trimmed, as the register cuts
    // them.
    let first: String = command.chars().take(200).collect();
    let head = format!("$ {}", first.trim());
    let out = output.trim_end();
    let letters: Vec<char> = out.chars().collect();
    let shown = if letters.is_empty() {
        "(no output)".to_owned()
    } else {
        letters[letters.len().saturating_sub(tail)..]
            .iter()
            .collect()
    };
    let mut windows = vec![format!("{head}\n{shown}")];
    if let Some(line) = out.split('\n').find(|line| looks_like_an_error(line)) {
        let window = format!(
            "{head}\n{}",
            line.trim().chars().take(400).collect::<String>()
        );
        if !windows[0].contains(&window) {
            windows.push(window);
        }
    }
    windows
}

/// Whether `line` looks like an error, by the register's pattern:
/// `error`, `Error`, `ERROR`, `Traceback`, `failed`, `FAILED`, `Cannot find`,
/// `not found`, `EEXIST`, `ENOENT`, or `TS` and four digits.
fn looks_like_an_error(line: &str) -> bool {
    const WORDS: &[&str] = &[
        "error",
        "Error",
        "ERROR",
        "Traceback",
        "failed",
        "FAILED",
        "Cannot find",
        "not found",
        "EEXIST",
        "ENOENT",
    ];
    WORDS.iter().any(|word| line.contains(word))
        || line.match_indices("TS").any(|(at, _)| {
            let digits: Vec<char> = line[at + 2..].chars().take(4).collect();
            digits.len() == 4 && digits.iter().all(char::is_ascii_digit)
        })
}

/// One probe's reading of a step.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    /// The probe.
    pub probe: String,
    /// Its class.
    pub class: GateClass,
    /// Its aggregate score.
    pub score: f64,
    /// Its threshold, as written.
    pub threshold: String,
    /// Whether the score reached the threshold.
    pub fired: bool,
}

/// What the classifier said of one step.
#[derive(Debug, Clone, PartialEq)]
pub enum Gated {
    /// Every probe's decision, with what served them.
    Decided {
        /// Each probe's, in the regimen's order.
        decisions: Vec<Decision>,
        /// The model Sidekick reported serving.
        model: String,
        /// The compute units it reported.
        compute_units: String,
        /// The step's calls, wall time.
        latency_ms: u64,
    },
    /// The step fell back to the patterns, and why.
    Fallback {
        /// Why: unreachable, an error, a timeout, a mismatch.
        reason: String,
        /// The time spent before it fell back.
        latency_ms: u64,
    },
}

impl Gated {
    /// The classes that fired, each once, in [`GateClass::ALL`]'s order;
    /// `None` on a fallback.
    #[must_use]
    pub fn fired(&self) -> Option<Vec<GateClass>> {
        match self {
            Self::Fallback { .. } => None,
            Self::Decided { decisions, .. } => Some(
                GateClass::ALL
                    .into_iter()
                    .filter(|class| {
                        decisions
                            .iter()
                            .any(|decision| decision.class == *class && decision.fired)
                    })
                    .collect(),
            ),
        }
    }
}

/// The step `windows` (and `previous`, the step before's) scored by
/// `classifier`: one call for each input the probes read -- every window in
/// `input`, every hypothesis as a candidate label, each label scored on its
/// own (`multi_label`) -- each hypothesis's score the max over its windows,
/// each probe's the max or mean over its hypotheses. Any failure, a model or
/// compute units other than declared, or a revision other than declared when
/// Sidekick reports one, falls back.
#[must_use]
pub fn gate(classifier: &Classifier, windows: &Windows, previous: Option<&Windows>) -> Gated {
    let started = Instant::now();
    // One deadline for the whole step, every batch within it.
    let deadline = started + classifier.timeout;
    let elapsed = || u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let fallback = |reason: String| Gated::Fallback {
        reason,
        latency_ms: elapsed(),
    };
    let mut scores: BTreeMap<(String, String), f64> = BTreeMap::new();
    let mut served: Option<(String, String)> = None;
    for input in Input::ALL {
        let probes: Vec<&Probe> = classifier
            .probes
            .iter()
            .filter(|probe| probe.input == input)
            .collect();
        if probes.is_empty() {
            continue;
        }
        let texts: Vec<&str> = windows_for(input, windows, previous);
        let mut labels: Vec<&str> = Vec::new();
        for probe in &probes {
            for hypothesis in &probe.hypotheses {
                if !labels.contains(&hypothesis.as_str()) {
                    labels.push(hypothesis);
                }
            }
        }
        if texts.is_empty() {
            // Nothing to read: every hypothesis scores 0.
            for label in &labels {
                scores.insert((input.word().to_owned(), (*label).to_owned()), 0.0);
            }
            continue;
        }
        // In batches of the model's own `max_batch`, as Sidekick takes them.
        let mut rows: Vec<Vec<f64>> = Vec::new();
        for batch in texts.chunks(classifier.batch) {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return fallback("timeout: step".to_owned());
            }
            let answer = match classify(classifier, batch, &labels, left) {
                Ok(answer) => answer,
                Err(why) if why.starts_with("timeout") => {
                    return fallback("timeout: step".to_owned());
                }
                Err(why) => return fallback(why),
            };
            if let Some(why) = mismatch(classifier, &answer) {
                return fallback(why);
            }
            served = Some((answer.model.clone(), answer.compute_units.clone()));
            rows.extend(answer.probs);
        }
        for (at, label) in labels.iter().enumerate() {
            let best = rows
                .iter()
                .filter_map(|row| row.get(at).copied())
                .fold(0.0_f64, f64::max);
            scores.insert((input.word().to_owned(), (*label).to_owned()), best);
        }
    }
    let decisions = decide(classifier, &scores);
    let (model, compute_units) = served.unwrap_or_else(|| {
        (
            format!("{}@{}", classifier.model, classifier.revision),
            classifier.compute_units.clone(),
        )
    });
    Gated::Decided {
        decisions,
        model,
        compute_units,
        latency_ms: elapsed(),
    }
}

/// Each probe's decision from its hypotheses' `scores`, keyed by input and
/// hypothesis: the max or mean over its hypotheses, against its threshold.
fn decide(classifier: &Classifier, scores: &BTreeMap<(String, String), f64>) -> Vec<Decision> {
    classifier
        .probes
        .iter()
        .map(|probe| {
            let each: Vec<f64> = probe
                .hypotheses
                .iter()
                .map(|hypothesis| {
                    scores
                        .get(&(probe.input.word().to_owned(), hypothesis.clone()))
                        .copied()
                        .unwrap_or(0.0)
                })
                .collect();
            let score = match probe.aggregate {
                Aggregate::Max => each.iter().copied().fold(0.0_f64, f64::max),
                Aggregate::Mean => {
                    each.iter().sum::<f64>()
                        / f64::from(u32::try_from(each.len().max(1)).unwrap_or(u32::MAX))
                }
            };
            let threshold: f64 = probe.threshold.parse().unwrap_or(f64::INFINITY);
            Decision {
                probe: probe.name.clone(),
                class: probe.class,
                score,
                threshold: probe.threshold.clone(),
                fired: score >= threshold,
            }
        })
        .collect()
}

/// What one classify call answered: each input's per-label scores, and the
/// provenance headers.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    /// Per input, per label, P(entailment).
    pub probs: Vec<Vec<f64>>,
    /// `sidekick-model`.
    pub model: String,
    /// `sidekick-compute-units`.
    pub compute_units: String,
}

/// Whether `endpoint` is plain HTTP to this machine: `127.0.0.1`, `localhost`
/// or `[::1]`, with any port and nothing after but a path.
fn loopback(endpoint: &str) -> bool {
    let Some(rest) = endpoint.strip_prefix("http://") else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or_default();
    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        let Some((host, after)) = bracketed.split_once(']') else {
            return false;
        };
        if !(after.is_empty() || port(after)) {
            return false;
        }
        return host == "::1";
    } else {
        match authority.split_once(':') {
            Some((host, after)) if port(&format!(":{after}")) => host,
            Some(_) => return false,
            None => authority,
        }
    };
    host == "127.0.0.1" || host == "localhost"
}

/// Whether `after` is `:` and a port number.
fn port(after: &str) -> bool {
    after
        .strip_prefix(':')
        .is_some_and(|digits| !digits.is_empty() && digits.parse::<u16>().is_ok())
}

/// The way the served model differs from the declared one, when it does:
/// its id, its revision when Sidekick reports one (`id@revision`), or its
/// compute units.
fn mismatch(classifier: &Classifier, answer: &Answer) -> Option<String> {
    let (id, revision) = answer
        .model
        .split_once('@')
        .map_or((answer.model.as_str(), None), |(id, revision)| {
            (id, Some(revision))
        });
    if id != classifier.model {
        return Some(format!("mismatch: model {id}, not {}", classifier.model));
    }
    if let Some(revision) = revision
        && revision != classifier.revision
    {
        return Some(format!(
            "mismatch: revision {revision}, not {}",
            classifier.revision
        ));
    }
    if answer.compute_units != classifier.compute_units {
        return Some(format!(
            "mismatch: compute units {}, not {}",
            answer.compute_units, classifier.compute_units
        ));
    }
    None
}

/// One `POST /v1/classify` call: `texts` in `input`, `labels` as candidate
/// labels, each scored on its own.
///
/// # Errors
///
/// Unreachable, a timeout, a status other than 200, or an answer this does
/// not read, as the reason a step falls back.
pub fn classify(
    classifier: &Classifier,
    texts: &[&str],
    labels: &[&str],
    within: Duration,
) -> Result<Answer, String> {
    let body = serde_json::json!({
        "model": classifier.model,
        "input": texts,
        "candidate_labels": labels,
        "hypothesis_template": "{}",
        "multi_label": true,
        "truncation_side": classifier.truncation_side,
    })
    .to_string();
    let (status, headers, reply) = post(&classifier.endpoint, "/v1/classify", &body, within)?;
    if status != 200 {
        return Err(format!(
            "error: HTTP {status}: {}",
            reply.chars().take(200).collect::<String>()
        ));
    }
    let value: serde_json::Value =
        serde_json::from_str(&reply).map_err(|why| format!("error: unreadable answer: {why}"))?;
    let data = value
        .get("data")
        .and_then(serde_json::Value::as_array)
        .ok_or("error: no `data` in the answer")?;
    let mut probs = vec![Vec::new(); texts.len()];
    for item in data {
        let index = item
            .get("index")
            .and_then(serde_json::Value::as_u64)
            .and_then(|index| usize::try_from(index).ok())
            .filter(|index| *index < texts.len())
            .ok_or("error: an answer item with no index in range")?;
        probs[index] = item
            .get("probs")
            .and_then(serde_json::Value::as_array)
            .ok_or("error: an answer item with no `probs`")?
            .iter()
            .map(|p| p.as_f64().ok_or("error: a score that is not a number"))
            .collect::<Result<Vec<_>, _>>()?;
        if probs[index].len() != labels.len() {
            return Err("error: an answer item scores another number of labels".to_owned());
        }
        // `probs` follows `candidate_labels` in request order (measured on
        // sidekickd 0.10.0); `label` is the argmax. An answer whose `label`
        // is not the label at its highest score has been reordered, and is
        // not read.
        let row = &probs[index];
        let top = row.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let at_top: Vec<&str> = row
            .iter()
            .zip(labels)
            .filter(|(p, _)| (**p - top).abs() <= f64::EPSILON)
            .map(|(_, label)| *label)
            .collect();
        if let Some(label) = item.get("label").and_then(serde_json::Value::as_str)
            && !at_top.contains(&label)
        {
            return Err(format!(
                "error: an answer item's `label` is `{label}`, not the label at its highest \
                 score: its `probs` are not in `candidate_labels`' order"
            ));
        }
    }
    let header = |name: &str| headers.get(name).cloned().unwrap_or_default();
    Ok(Answer {
        probs,
        model: header("sidekick-model"),
        compute_units: header("sidekick-compute-units"),
    })
}

type Reply = (u16, BTreeMap<String, String>, String);

/// A JSON `POST` to `path` at `endpoint` over plain HTTP/1.1, the connection
/// closed after it, within `timeout` in all.
fn post(endpoint: &str, path: &str, body: &str, timeout: Duration) -> Result<Reply, String> {
    let deadline = Instant::now() + timeout;
    let left = || deadline.saturating_duration_since(Instant::now());
    let host = endpoint
        .strip_prefix("http://")
        .unwrap_or(endpoint)
        .trim_end_matches('/');
    let address = std::net::ToSocketAddrs::to_socket_addrs(host)
        .map_err(|why| format!("unreachable: {host}: {why}"))?
        .next()
        .ok_or_else(|| format!("unreachable: {host} resolves to nothing"))?;
    let mut stream =
        std::net::TcpStream::connect_timeout(&address, left().max(Duration::from_millis(1)))
            .map_err(|why| {
                if why.kind() == std::io::ErrorKind::TimedOut {
                    "timeout: connecting".to_owned()
                } else {
                    format!("unreachable: {why}")
                }
            })?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .set_write_timeout(Some(left().max(Duration::from_millis(1))))
        .map_err(|why| format!("error: {why}"))?;
    stream
        .write_all(request.as_bytes())
        .map_err(|why| format!("error: writing: {why}"))?;
    let mut raw = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let remaining = left();
        if remaining.is_zero() {
            return Err("timeout: reading".to_owned());
        }
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|why| format!("error: {why}"))?;
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&chunk[..n]),
            Err(why)
                if matches!(
                    why.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Err("timeout: reading".to_owned());
            }
            Err(why) => return Err(format!("error: reading: {why}")),
        }
    }
    parse_reply(&raw)
}

fn parse_reply(raw: &[u8]) -> Result<Reply, String> {
    let text = String::from_utf8_lossy(raw);
    let (head, rest) = text
        .split_once("\r\n\r\n")
        .ok_or("error: a reply with no headers")?;
    let mut lines = head.lines();
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or("error: a reply with no status")?;
    let headers: BTreeMap<String, String> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let body = if headers
        .get("transfer-encoding")
        .is_some_and(|value| value.eq_ignore_ascii_case("chunked"))
    {
        unchunked(rest)?
    } else {
        rest.to_owned()
    };
    Ok((status, headers, body))
}

fn unchunked(mut rest: &str) -> Result<String, String> {
    let mut body = String::new();
    loop {
        let (size, after) = rest
            .split_once("\r\n")
            .ok_or("error: a chunk with no size")?;
        let size = usize::from_str_radix(size.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| "error: a chunk size that is not hex")?;
        if size == 0 {
            return Ok(body);
        }
        body.push_str(after.get(..size).ok_or("error: a short chunk")?);
        rest = after.get(size + 2..).ok_or("error: a short chunk")?;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A full commit, as a regimen must declare the model's revision.
    pub(crate) const REVISION: &str = "0123456789abcdef0123456789abcdef01234567";
    /// What Sidekick reports serving: the model and that revision.
    pub(crate) const MODEL_AT: &str = "nli-test@0123456789abcdef0123456789abcdef01234567";

    /// A fake classify server on loopback: it answers each connection with
    /// `answer`'s status, headers and body for the labels and inputs it is
    /// sent, and counts the calls. The scores: `scored(label)`.
    pub(crate) struct FakeSidekick {
        pub endpoint: String,
        pub calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl FakeSidekick {
        /// Serving `model` on `units`, scoring each label `scored(label,
        /// text)`.
        pub(crate) fn serving(model: &str, units: &str, scored: fn(&str, &str) -> f64) -> Self {
            Self::answering(model, units, scored, false)
        }

        /// [`Self::serving`], each answer held back `delay` first.
        pub(crate) fn slow(
            model: &str,
            units: &str,
            scored: fn(&str, &str) -> f64,
            delay: Duration,
        ) -> Self {
            Self::answering_after(model, units, scored, (false, delay))
        }

        /// [`Self::answering`], each answer held back `delay` first.
        pub(crate) fn answering(
            model: &str,
            units: &str,
            scored: fn(&str, &str) -> f64,
            sorted: bool,
        ) -> Self {
            Self::answering_after(model, units, scored, (sorted, Duration::ZERO))
        }

        /// [`Self::serving`], its `probs` sorted highest first when `sorted`,
        /// as Hugging Face's pipeline returns them, not in label order; each
        /// answer held back `delay`.
        fn answering_after(
            model: &str,
            units: &str,
            scored: fn(&str, &str) -> f64,
            (sorted, delay): (bool, Duration),
        ) -> Self {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
            let address = std::net::TcpListener::local_addr(&listener).expect("an address");
            let endpoint = format!("http://{address}");
            let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let counted = std::sync::Arc::clone(&calls);
            let (model, units) = (model.to_owned(), units.to_owned());
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { continue };
                    counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let mut raw = Vec::new();
                    let mut buffer = vec![0_u8; 65536];
                    let body = loop {
                        let n = stream.read(&mut buffer).unwrap_or(0);
                        if n == 0 {
                            break String::new();
                        }
                        raw.extend_from_slice(&buffer[..n]);
                        let text = String::from_utf8_lossy(&raw).to_string();
                        if let Some((head, rest)) = text.split_once("\r\n\r\n") {
                            let length = head
                                .lines()
                                .find_map(|line| {
                                    line.to_ascii_lowercase()
                                        .strip_prefix("content-length:")
                                        .map(|n| n.trim().parse::<usize>().unwrap_or(0))
                                })
                                .unwrap_or(0);
                            if rest.len() >= length {
                                break rest.to_owned();
                            }
                        }
                    };
                    let request: serde_json::Value =
                        serde_json::from_str(&body).unwrap_or_default();
                    let texts: Vec<String> = request["input"]
                        .as_array()
                        .map(|items| {
                            items
                                .iter()
                                .map(|item| item.as_str().unwrap_or_default().to_owned())
                                .collect()
                        })
                        .unwrap_or_default();
                    let labels: Vec<String> = request["candidate_labels"]
                        .as_array()
                        .map(|items| {
                            items
                                .iter()
                                .map(|item| item.as_str().unwrap_or_default().to_owned())
                                .collect()
                        })
                        .unwrap_or_default();
                    let data: Vec<serde_json::Value> = texts
                        .iter()
                        .enumerate()
                        .map(|(index, text)| {
                            let mut probs: Vec<f64> =
                                labels.iter().map(|label| scored(label, text)).collect();
                            let top = probs
                                .iter()
                                .enumerate()
                                .fold(0, |best, (at, p)| if *p > probs[best] { at } else { best });
                            if sorted {
                                probs.sort_by(|a, b| b.total_cmp(a));
                            }
                            serde_json::json!({
                                "index": index,
                                "label": labels.get(top),
                                "probs": probs,
                                "num_classes": labels.len(),
                            })
                        })
                        .collect();
                    let reply = serde_json::json!({"object": "list", "data": data}).to_string();
                    std::thread::sleep(delay);
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                         sidekick-model: {model}\r\nsidekick-compute-units: {units}\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                        reply.len()
                    );
                }
            });
            Self { endpoint, calls }
        }
    }

    /// A classifier at `endpoint`, its probes `probes`.
    pub(crate) fn classifier_at(endpoint: &str, probes: Vec<Probe>) -> Classifier {
        Classifier {
            endpoint: endpoint.to_owned(),
            model: "nli-test".to_owned(),
            revision: REVISION.to_owned(),
            compute_units: "cpu_and_gpu".to_owned(),
            truncation_side: "left".to_owned(),
            timeout: Duration::from_millis(2_000),
            prose_window_sentences: 3,
            prose_window_chars: 400,
            output_tail_chars: 800,
            batch: 8,
            probes,
        }
    }

    /// A probe of `class` on `input`, `hypotheses`, the max, at `threshold`.
    pub(crate) fn probe(
        class: GateClass,
        input: Input,
        hypotheses: &[&str],
        threshold: &str,
    ) -> Probe {
        Probe {
            name: format!("{}-{}", class.word(), input.word()),
            class,
            input,
            hypotheses: hypotheses.iter().map(|h| (*h).to_owned()).collect(),
            aggregate: Aggregate::Max,
            threshold: threshold.to_owned(),
        }
    }

    const REGIMEN: &str = r#"warrant_gate = "classifier"

[warrant_classifier]
endpoint = "http://127.0.0.1:8790"
model = "nli-test"
revision = "0123456789abcdef0123456789abcdef01234567"
compute_units = "cpu_and_gpu"
truncation_side = "left"
timeout_ms = 2000
prose_window_sentences = 3
prose_window_chars = 400
output_tail_chars = 800
batch = 8

[warrant_classifier.gotcha-prose]
class = "gotcha"
input = "prose"
hypotheses = ["A tool behaved unexpectedly.", "An assumption was wrong."]
aggregate = "mean"
threshold = 0.8
"#;

    #[test]
    fn the_gate_is_regex_unless_the_regimen_names_the_classifier_and_every_setting() {
        let parse = |text: &str| read(&crate::formats::regimen::parse(text).expect("a regimen"));
        assert_eq!(parse(""), Ok(WarrantGate::Regex));
        assert_eq!(parse("warrant_gate = \"regex\"\n"), Ok(WarrantGate::Regex));
        let Ok(WarrantGate::Classifier(classifier)) = parse(REGIMEN) else {
            panic!("a classifier: {:?}", parse(REGIMEN));
        };
        assert_eq!(classifier.probes.len(), 1);
        assert_eq!(classifier.probes[0].aggregate, Aggregate::Mean);
        assert_eq!(classifier.probes[0].threshold, "0.8");
        assert!(parse("warrant_gate = \"maybe\"\n").is_err());
        assert!(parse("warrant_gate = \"classifier\"\n").is_err());
        // No defaults: every setting is refused when it is missing.
        for key in [
            "endpoint",
            "model",
            "revision",
            "compute_units",
            "truncation_side",
            "timeout_ms",
            "prose_window_sentences",
            "prose_window_chars",
            "output_tail_chars",
            "batch",
            "class",
            "input",
            "hypotheses",
            "aggregate",
            "threshold",
        ] {
            let without: String = REGIMEN
                .lines()
                .filter(|line| !line.starts_with(&format!("{key} =")))
                .fold(String::new(), |mut out, line| {
                    out.push_str(line);
                    out.push('\n');
                    out
                });
            let refused = parse(&without);
            assert!(
                refused.as_ref().is_err_and(|why| why.contains(key)),
                "{key}: {refused:?}"
            );
        }
        let no_probe: String = REGIMEN
            .split("[warrant_classifier.gotcha-prose]")
            .next()
            .expect("the head")
            .to_owned();
        assert!(parse(&no_probe).is_err_and(|why| why.contains("no probe")));
    }

    /// The windows are the trouble-detector register's: its sentence split,
    /// chop and packing for prose, and its tail-and-error-line windows for
    /// output, each case here checked against its own implementation.
    /// The step's reasoning and tool output go to the classifier over plain
    /// HTTP, so it must be on this machine: `127.0.0.1`, `localhost` or
    /// `[::1]`, any port. And the revision is provenance: a full commit.
    #[test]
    fn the_endpoint_is_loopback_and_the_revision_a_full_commit() {
        let parse = |text: &str| read(&crate::formats::regimen::parse(text).expect("a regimen"));
        let with = |key: &str, value: &str| {
            REGIMEN
                .lines()
                .map(|line| {
                    if line.starts_with(&format!("{key} =")) {
                        format!("{key} = \"{value}\"\n")
                    } else {
                        format!("{line}\n")
                    }
                })
                .collect::<String>()
        };
        for endpoint in [
            "http://127.0.0.1:8790",
            "http://localhost:8790",
            "http://[::1]:8790",
            "http://127.0.0.1:8790/",
        ] {
            assert!(parse(&with("endpoint", endpoint)).is_ok(), "{endpoint}");
        }
        for endpoint in [
            "http://192.0.2.5:8790",
            "http://sidekick.example.com:8790",
            "http://127.0.0.1.example.com:8790",
            "http://localhost.evil:8790",
        ] {
            assert!(
                parse(&with("endpoint", endpoint)).is_err_and(|why| why.contains("loopback")),
                "{endpoint}"
            );
        }
        for revision in ["6c", "abc1234", &"g".repeat(40), &"a".repeat(39)] {
            assert!(
                parse(&with("revision", revision)).is_err_and(|why| why.contains("40")),
                "{revision}"
            );
        }
    }

    /// A reported revision must be the declared one exactly: a prefix of
    /// it is not provenance.
    #[test]
    fn a_reported_revision_is_the_declared_one_exactly() {
        let windows = Windows {
            prose: vec!["It failed.".to_owned()],
            output: Vec::new(),
        };
        let probes = || {
            vec![probe(
                GateClass::Gotcha,
                Input::Prose,
                &["A tool failed."],
                "0.5",
            )]
        };
        let prefix = FakeSidekick::serving("nli-test@0123456", "cpu_and_gpu", |_, _| 0.9);
        assert!(matches!(
            gate(&classifier_at(&prefix.endpoint, probes()), &windows, None),
            Gated::Fallback { reason, .. } if reason.contains("mismatch: revision")
        ));
        let exact = FakeSidekick::serving(MODEL_AT, "cpu_and_gpu", |_, _| 0.9);
        assert!(matches!(
            gate(&classifier_at(&exact.endpoint, probes()), &windows, None),
            Gated::Decided { .. }
        ));
    }

    /// One deadline for the whole step: with each batch taking 60% of the
    /// timeout, the second cannot finish within it, and the step falls back
    /// -- it never blocks the trunk for a timeout a batch.
    #[test]
    fn a_step_has_one_deadline_across_its_batches() {
        let sidekick = FakeSidekick::slow(
            MODEL_AT,
            "cpu_and_gpu",
            |_, _| 0.9,
            Duration::from_millis(300),
        );
        let mut classifier = classifier_at(
            &sidekick.endpoint,
            vec![probe(GateClass::Gotcha, Input::Prose, &["A"], "0.5")],
        );
        classifier.timeout = Duration::from_millis(500);
        classifier.batch = 1;
        let windows = Windows {
            prose: vec!["One window.".to_owned(), "Another window.".to_owned()],
            output: Vec::new(),
        };
        let started = Instant::now();
        let gated = gate(&classifier, &windows, None);
        assert!(
            matches!(&gated, Gated::Fallback { reason, .. } if reason == "timeout: step"),
            "{gated:?}"
        );
        assert!(
            started.elapsed() < Duration::from_millis(700),
            "it waited past the step's deadline: {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn steps_are_windowed_as_the_register_windows_them() {
        assert_eq!(
            prose_windows(
                "One. Two! Three? Four: five six.\nSeven eight nine.\n```\ncode line here\n```\nok.",
                3,
                400
            ),
            [
                "One. Two! Three?",
                "Four: five six. Seven eight nine.",
                "code line here"
            ]
        );
        // Fewer than three word characters: dropped.
        assert_eq!(
            prose_windows("Hi. a. The parser drops blank lines.", 3, 400),
            ["The parser drops blank lines."]
        );
        // Chopped at the last space before 400.
        let long = format!("{}end of it.", "x ".repeat(300));
        let chopped = prose_windows(&long, 3, 400);
        assert_eq!(chopped.len(), 2);
        assert_eq!(chopped[0].chars().count(), 399);
        assert!(chopped[1].ends_with("end of it."));
        assert!(prose_windows("  \n ", 3, 400).is_empty());
        let output = format!(
            "{}\nerror: no such file\n{}",
            "x".repeat(50),
            "y".repeat(900)
        );
        let windows = output_windows("cat notes.md", &output, 800);
        assert_eq!(
            windows,
            [
                format!("$ cat notes.md\n{}", "y".repeat(800)),
                "$ cat notes.md\nerror: no such file".to_owned()
            ]
        );
        assert_eq!(output_windows("ls", "", 800), ["$ ls\n(no output)"]);
        // The error line already in the tail: not repeated.
        assert_eq!(
            output_windows("t", "Traceback (most recent call last):\n  boom", 800),
            ["$ t\nTraceback (most recent call last):\n  boom"]
        );
        assert!(looks_like_an_error("see TS2345 here"));
        assert!(!looks_like_an_error("see TS23 here"));
    }

    #[test]
    fn a_probe_fires_at_its_threshold_on_the_max_over_windows_then_its_aggregate() {
        // "wrong" scores 0.9 on the window that says "wrong", else 0.1.
        let sidekick = FakeSidekick::serving(MODEL_AT, "cpu_and_gpu", |label, text| {
            if label.contains("wrong") && text.contains("wrong") {
                0.9
            } else {
                0.1
            }
        });
        let at = |threshold: &str, aggregate: Aggregate| {
            let mut probe = probe(
                GateClass::Gotcha,
                Input::Prose,
                &["Something was wrong.", "A tool failed."],
                threshold,
            );
            probe.aggregate = aggregate;
            classifier_at(&sidekick.endpoint, vec![probe])
        };
        let windows = Windows {
            prose: vec!["All good.".to_owned(), "That was wrong.".to_owned()],
            output: Vec::new(),
        };
        let fired = |classifier: &Classifier| gate(classifier, &windows, None).fired();
        assert_eq!(
            fired(&at("0.9", Aggregate::Max)),
            Some(vec![GateClass::Gotcha])
        );
        assert_eq!(fired(&at("0.91", Aggregate::Max)), Some(vec![]));
        // The mean of 0.9 and 0.1.
        assert_eq!(
            fired(&at("0.5", Aggregate::Mean)),
            Some(vec![GateClass::Gotcha])
        );
        assert_eq!(fired(&at("0.51", Aggregate::Mean)), Some(vec![]));
        let Gated::Decided {
            model,
            compute_units,
            ..
        } = gate(&at("0.9", Aggregate::Max), &windows, None)
        else {
            panic!("decided");
        };
        assert_eq!(
            (model.as_str(), compute_units.as_str()),
            (MODEL_AT, "cpu_and_gpu")
        );
    }

    #[test]
    fn an_unreachable_or_mismatched_classifier_falls_back_with_its_reason() {
        let windows = Windows {
            prose: vec!["It failed.".to_owned()],
            output: Vec::new(),
        };
        let probes = || {
            vec![probe(
                GateClass::Gotcha,
                Input::Prose,
                &["A tool failed."],
                "0.5",
            )]
        };
        // Nothing listens: bind, take the port, and let it go.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("a port")
            .local_addr()
            .expect("an address")
            .port();
        let gone = classifier_at(&format!("http://127.0.0.1:{port}"), probes());
        assert!(matches!(
            gate(&gone, &windows, None),
            Gated::Fallback { reason, .. } if reason.starts_with("unreachable")
        ));
        let other = FakeSidekick::serving("nli-other@abc1234", "cpu_and_gpu", |_, _| 0.9);
        assert!(matches!(
            gate(&classifier_at(&other.endpoint, probes()), &windows, None),
            Gated::Fallback { reason, .. } if reason.contains("mismatch: model")
        ));
        let units = FakeSidekick::serving(MODEL_AT, "cpu_only", |_, _| 0.9);
        assert!(matches!(
            gate(&classifier_at(&units.endpoint, probes()), &windows, None),
            Gated::Fallback { reason, .. } if reason.contains("mismatch: compute units")
        ));
        let revision = FakeSidekick::serving("nli-test@fffffff", "cpu_and_gpu", |_, _| 0.9);
        assert!(matches!(
            gate(&classifier_at(&revision.endpoint, probes()), &windows, None),
            Gated::Fallback { reason, .. } if reason.contains("mismatch: revision")
        ));
    }

    /// `probs` are read by index into `candidate_labels`; an answer whose
    /// scores come back reordered -- sorted, as the pipeline sorts them -- is
    /// caught by its `label`, the argmax, and the step falls back.
    #[test]
    fn an_answer_whose_scores_are_reordered_is_not_read() {
        let windows = Windows {
            prose: vec!["It was wrong.".to_owned()],
            output: Vec::new(),
        };
        // "B" scores 0.9 and is second in the request.
        let scored: fn(&str, &str) -> f64 = |label, _| if label == "B" { 0.9 } else { 0.1 };
        let probes = || {
            vec![
                probe(GateClass::Gotcha, Input::Prose, &["A"], "0.5"),
                probe(GateClass::Decision, Input::Prose, &["B"], "0.5"),
            ]
        };
        let ordered = FakeSidekick::answering(MODEL_AT, "cpu_and_gpu", scored, false);
        assert_eq!(
            gate(&classifier_at(&ordered.endpoint, probes()), &windows, None).fired(),
            Some(vec![GateClass::Decision])
        );
        let sorted = FakeSidekick::answering(MODEL_AT, "cpu_and_gpu", scored, true);
        assert!(matches!(
            gate(&classifier_at(&sorted.endpoint, probes()), &windows, None),
            Gated::Fallback { reason, .. } if reason.contains("not in `candidate_labels`' order")
        ));
    }

    #[test]
    fn one_call_carries_every_window_and_every_hypothesis_of_an_input() {
        let sidekick = FakeSidekick::serving(MODEL_AT, "cpu_and_gpu", |_, _| 0.2);
        let classifier = classifier_at(
            &sidekick.endpoint,
            vec![
                probe(GateClass::Gotcha, Input::Prose, &["A", "B"], "0.5"),
                probe(GateClass::Decision, Input::Prose, &["C"], "0.5"),
                probe(GateClass::Gotcha, Input::Output, &["D"], "0.5"),
            ],
        );
        let windows = Windows {
            prose: vec!["one".to_owned(), "two".to_owned()],
            output: vec!["three".to_owned()],
        };
        assert!(matches!(
            gate(&classifier, &windows, None),
            Gated::Decided { .. }
        ));
        // One call for the prose probes, one for the output's.
        assert_eq!(sidekick.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }
    /// The windows of recorded steps, for a parity check against another
    /// windowing run locally: reads JSON lines of `reasoning`, `text` and
    /// `calls` (each `arguments` and `shown`) from the path in
    /// `WARRANT_GATE_PARITY_IN`, writes each step's `prose` and `tool`
    /// windows to `WARRANT_GATE_PARITY_OUT`. Run by hand; nothing private
    /// is in the tree.
    #[test]
    #[ignore = "a local parity check, run by hand on recorded steps"]
    fn windows_of_recorded_steps() {
        let (Ok(input), Ok(output)) = (
            std::env::var("WARRANT_GATE_PARITY_IN"),
            std::env::var("WARRANT_GATE_PARITY_OUT"),
        ) else {
            return;
        };
        let text = std::fs::read_to_string(input).expect("the steps");
        let mut out = String::new();
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let step: serde_json::Value = serde_json::from_str(line).expect("a step");
            let field = |key: &str| step[key].as_str().unwrap_or_default().to_owned();
            let prose = prose_windows(
                &format!("{}\n{}", field("reasoning"), field("text")),
                3,
                400,
            );
            let tool: Vec<String> = step["calls"]
                .as_array()
                .map(|calls| {
                    calls
                        .iter()
                        .flat_map(|call| {
                            output_windows(
                                call["arguments"].as_str().unwrap_or_default(),
                                call["shown"].as_str().unwrap_or_default(),
                                800,
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            out.push_str(&serde_json::json!({"prose": prose, "tool": tool}).to_string());
            out.push('\n');
        }
        std::fs::write(output, out).expect("written");
    }
}
