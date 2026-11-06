//! The operator script (#908): a run's log read back into a scenario that
//! holds every move its operator made -- the asks and their files, the
//! scoping marks, meditations, flags and edits, the phase moves and rulings,
//! the tangents, the operator's gestures (the app audit's page walk, F20),
//! and the end -- so `diet-drive scenario` can play them again.
//!
//! The script fixes the inputs, not what comes back. Each step keeps its
//! place (`place`, which the runner does not read): the turn it belongs to,
//! and for every ask after the first, the response it followed. A scenario's
//! steps run once the session has settled, so every derived move runs
//! between turns; a background job that ended between turns, before the
//! next move, is waited for, as the operator's next move came after it.
//! Only the first ask is fixed: every later one was typed in
//! answer to what the model did, and a scripted run of it is not that
//! answer. The header lists the fixed asks (`fixed_asks`), and a person who
//! knows a later ask was written in advance adds its index.
//!
//! A move the format cannot express is refused, naming its line, and never
//! dropped: a stop, a call moved to the background, an approval answered
//! call by call, a tangent closed with more than one ruling, a meditation
//! declared with a phase move, a refused command that is not an ask, a keep
//! from an ask, a wrap that keeps nothing, a gesture whose line the runner
//! would not find again, and a chip answered out of order.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde_json::{Map, Value, json};

/// A run's operator moves, as a scenario's parts.
#[derive(Debug, Clone, PartialEq)]
pub struct Derived {
    /// The trunk's head as the run's `session.start` holds it, with what
    /// the replay's start adds again taken off: the instruction files'
    /// render (#559), read from the replay's worktree, and the fork ask
    /// set's priming (#598), from its regimen.
    pub head: String,
    /// The instruction files the run's start read, by path in its
    /// worktree: what a replay's worktree must hold for its head to match.
    pub instruction_files: Vec<String>,
    /// One `step` line per move, in the log's order.
    pub steps: Vec<Value>,
    /// The indices, among the asks, of those that are fixed: the first.
    pub fixed_asks: Vec<u64>,
    /// The scope every approval the operator answered was given, when any
    /// was answered (the header's `approvals`).
    pub approvals: Option<String>,
    /// Each file an ask attached: its place in the replay's worktree, and
    /// its path in the recording beside the log.
    pub files: BTreeMap<String, String>,
}

/// A move the scenario format cannot express, or a log that is not one to
/// derive from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The line, by its `seq`, when one line is why.
    pub seq: Option<u64>,
    /// That line's kind.
    pub kind: Option<String>,
    /// Why.
    pub why: String,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.seq, &self.kind) {
            (Some(seq), Some(kind)) => write!(f, "line {seq} (`{kind}`): {}", self.why),
            _ => f.write_str(&self.why),
        }
    }
}

impl std::error::Error for Refusal {}

fn refused(line: &Map<String, Value>, why: impl Into<String>) -> Refusal {
    Refusal {
        seq: line.get("seq").and_then(Value::as_u64),
        kind: line.get("kind").and_then(Value::as_str).map(str::to_owned),
        why: why.into(),
    }
}

fn text<'a>(line: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    line.get(key).and_then(Value::as_str)
}

/// Every line of `log`, read as the log module reads it and then as JSON
/// objects.
fn lines_of(log: &str) -> Result<Vec<Map<String, Value>>, Refusal> {
    crate::formats::log::read(log).map_err(|why| Refusal {
        seq: None,
        kind: None,
        why: format!("the log does not read: {why}"),
    })?;
    log.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| match serde_json::from_str(line) {
            Ok(Value::Object(object)) => Ok(object),
            _ => Err(Refusal {
                seq: None,
                kind: None,
                why: "a log line is not an object".to_owned(),
            }),
        })
        .collect()
}

/// Working memory's live entries as the log's patches leave them, by id:
/// what a `pick` is written from.
#[derive(Default)]
struct Live(BTreeMap<String, String>);

impl Live {
    fn patched(&mut self, line: &Map<String, Value>) {
        let Some(entry) = line.get("entry").and_then(Value::as_object) else {
            return;
        };
        let (Some(id), content) = (text(entry, "id"), text(entry, "text")) else {
            return;
        };
        match text(line, "op") {
            Some("add") => {
                self.0
                    .insert(id.to_owned(), content.unwrap_or_default().to_owned());
            }
            Some("supersede") => {
                if let Some(old) = text(line, "supersedes") {
                    self.0.remove(old);
                }
                self.0
                    .insert(id.to_owned(), content.unwrap_or_default().to_owned());
            }
            Some(_) => {
                self.0.remove(id);
            }
            None => {}
        }
    }

    fn closed(&mut self, line: &Map<String, Value>) {
        for key in ["dropped", "parked"] {
            for id in line
                .get(key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                self.0.remove(id);
            }
        }
    }

    /// The `pick` that names entry `id` alone: its text, when no other live
    /// entry holds it.
    fn pick(&self, id: &str, line: &Map<String, Value>) -> Result<String, Refusal> {
        let content = self
            .0
            .get(id)
            .ok_or_else(|| refused(line, format!("entry `{id}` is not live in working memory")))?;
        let holding = self
            .0
            .values()
            .filter(|other| other.contains(content.as_str()))
            .count();
        if content.is_empty() || holding != 1 {
            return Err(refused(
                line,
                format!("entry `{id}`'s text does not pick it alone among the live entries"),
            ));
        }
        Ok(content.clone())
    }
}

/// The line `seq` names among `before`, the lines logged before a gesture.
fn line_at(before: &[Map<String, Value>], seq: u64) -> Option<&Map<String, Value>> {
    before
        .iter()
        .find(|line| line.get("seq").and_then(Value::as_u64) == Some(seq))
}

/// How a gesture's step names the line `seq` (the app audit's page walk,
/// F20): a `tool_call` by its id, a `response` by its text -- as the runner
/// finds it again, the last such line among `before`.
fn line_pick(
    before: &[Map<String, Value>],
    seq: u64,
    gesture: &Map<String, Value>,
) -> Result<Value, Refusal> {
    let line = line_at(before, seq).ok_or_else(|| {
        refused(
            gesture,
            format!("it names line {seq}, which is not before it"),
        )
    })?;
    let last = |found: &dyn Fn(&Map<String, Value>) -> bool| {
        before
            .iter()
            .rev()
            .find(|line| found(line))
            .and_then(|line| line.get("seq").and_then(Value::as_u64))
    };
    match text(line, "kind") {
        Some("tool_call") => {
            let id = text(line, "id").unwrap_or_default();
            if id.is_empty()
                || last(&|other| {
                    text(other, "kind") == Some("tool_call") && text(other, "id") == Some(id)
                }) != Some(seq)
            {
                return Err(refused(
                    gesture,
                    format!("call `{id}` at line {seq} is not the last call of its id"),
                ));
            }
            Ok(json!({"call": id}))
        }
        Some("response") => {
            let said = text(line, "text").unwrap_or_default();
            if said.is_empty()
                || last(&|other| {
                    text(other, "kind") == Some("response")
                        && text(other, "text").is_some_and(|text| text.contains(said))
                }) != Some(seq)
            {
                return Err(refused(
                    gesture,
                    format!("the answer at line {seq} is not the last one holding its text"),
                ));
            }
            Ok(json!({"answer": said}))
        }
        _ => Err(refused(
            gesture,
            format!("line {seq} is neither a call nor an answer"),
        )),
    }
}

/// The step of an operator's keep, `entry` the operator entry it wrote: its
/// span from the last call's result or answer holding it, as the runner
/// finds it again.
fn keep_step(
    before: &[Map<String, Value>],
    entry: &Map<String, Value>,
    gesture: &Map<String, Value>,
) -> Result<Value, Refusal> {
    let span = text(entry, "text").unwrap_or_default();
    let from = match text(entry, "kept_from") {
        Some("tool-result") => before
            .iter()
            .rev()
            .find(|line| {
                text(line, "kind") == Some("tool_call")
                    && text(line, "shown").is_some_and(|shown| shown.contains(span))
            })
            .and_then(|line| line.get("seq").and_then(Value::as_u64))
            .map(|seq| line_pick(before, seq, gesture))
            .transpose()?,
        Some("answer") => before
            .iter()
            .any(|line| {
                text(line, "kind") == Some("response")
                    && text(line, "text").is_some_and(|said| said.contains(span))
            })
            .then(|| json!({"answer": span})),
        kept_from => {
            return Err(refused(
                gesture,
                format!(
                    "a keep from `{}` has no step: a step keeps from a call's result or an answer",
                    kept_from.unwrap_or_default()
                ),
            ));
        }
    };
    let from = from.ok_or_else(|| refused(gesture, "no line before it holds its span"))?;
    let mut step = json!({"step": "keep", "from": from, "span": span, "place": {"between": true}});
    if let Some(tag) = text(entry, "category") {
        step["tag"] = json!(tag);
    }
    Ok(step)
}

/// The first chip of `open` unanswered: what a `suggestion` or `correction`
/// step answers. A line answering another is refused.
fn answered_first(
    open: &mut Vec<u64>,
    answers: Option<u64>,
    line: &Map<String, Value>,
) -> Result<(), Refusal> {
    match (open.first().copied(), answers) {
        (Some(first), Some(answers)) if first == answers => {
            open.remove(0);
            Ok(())
        }
        _ => Err(refused(
            line,
            "it answers a chip that is not the first unanswered: its step answers the first",
        )),
    }
}

/// `log`'s operator moves as a scenario's steps, `priming` -- the fork ask
/// set's, as the regimen the replay runs under adds it -- taken off the head.
///
/// # Errors
///
/// A log that does not read, a head that is not one system message, or a
/// move the scenario format cannot express, naming its line.
pub fn derive(log: &str, priming: Option<&str>) -> Result<Derived, Refusal> {
    derived(&lines_of(log)?, priming)
}

/// [`derive`] over a log's lines, read.
#[allow(clippy::too_many_lines)]
fn derived(lines: &[Map<String, Value>], priming: Option<&str>) -> Result<Derived, Refusal> {
    let start = lines
        .first()
        .filter(|line| text(line, "kind") == Some("session.start"));
    let Some(start) = start else {
        return Err(Refusal {
            seq: None,
            kind: None,
            why: "the log does not open with `session.start`".to_owned(),
        });
    };
    let head = match start
        .get("head")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
    {
        Some([message]) if message.get("role").and_then(Value::as_str) == Some("system") => message
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        _ => return Err(refused(start, "its head is not one system message")),
    };
    // The run's own nonce (#932), which the replay's start puts again.
    let head = match text(start, "head_nonce") {
        Some(nonce) => head
            .strip_prefix(&crate::drive::regimen::nonce_line(nonce))
            .map(str::to_owned)
            .ok_or_else(|| refused(start, "it names a head nonce its head does not open with"))?,
        None => head,
    };
    let instruction_files: Vec<String> = start
        .get("instruction_files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|file| file.get("path").and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    // `instructions::into_system`'s join, then `session::primed`'s.
    let head = if instruction_files.is_empty() {
        head
    } else {
        match head.rfind("\n\n---\n\n--- Context from: ") {
            Some(at) => head[..at].to_owned(),
            None => {
                return Err(refused(
                    start,
                    "it names instruction files its head does not hold",
                ));
            }
        }
    };
    let head = priming
        .map(|priming| format!("\n\n{}", priming.trim()))
        .and_then(|suffix| head.strip_suffix(&suffix).map(str::to_owned))
        .unwrap_or(head);

    let mut steps: Vec<Value> = Vec::new();
    let mut files = BTreeMap::new();
    let mut live = Live::default();
    let mut lane_of: BTreeMap<u64, String> = BTreeMap::new();
    let mut answered: Option<u64> = None;
    let mut gap: Option<(u64, u64)> = None;
    let mut asks = 0_u64;
    let mut held_open = false;
    let mut approvals: Option<(String, u64)> = None;
    let mut awaiting = false;
    // The open grade step (#942): its task, its index among the steps, and
    // the latest grade of it, which the next ask of its task answers.
    let mut grading: Option<OpenGrade> = None;
    // The operator's gestures (the app audit's page walk, F20): the forks an
    // accepted suggestion fired and the entries a wrap kept, which are that
    // gesture's and no step of their own; the chips not yet answered.
    let fired: BTreeSet<u64> = lines
        .iter()
        .filter(|line| text(line, "kind") == Some("suggestion"))
        .filter_map(|line| line.get("fork").and_then(Value::as_u64))
        .collect();
    let wrapping: BTreeSet<&str> = lines
        .iter()
        .filter(|line| text(line, "kind") == Some("wrap.accepted"))
        .filter_map(|line| text(line, "entry"))
        .collect();
    let mut shadows: Vec<u64> = Vec::new();
    let mut refusals: Vec<u64> = Vec::new();

    for (at, line) in lines.iter().enumerate() {
        let before = &lines[..at];
        let kind = text(line, "kind").unwrap_or_default();
        let seq = line.get("seq").and_then(Value::as_u64).unwrap_or_default();
        let turn = line.get("turn").and_then(Value::as_u64);
        match kind {
            "request" => {
                lane_of.insert(seq, text(line, "lane").unwrap_or_default().to_owned());
            }
            "response" => {
                let to = line.get("to_request").and_then(Value::as_u64);
                if to.and_then(|to| lane_of.get(&to)).map(String::as_str) == Some("trunk") {
                    answered = Some(seq);
                }
            }
            "idle.gap" => {
                gap = Some((
                    line.get("read").and_then(Value::as_u64).unwrap_or_default(),
                    line.get("compose")
                        .and_then(Value::as_u64)
                        .unwrap_or_default(),
                ));
            }
            // A feedback ask (#942): the grade step sends it, so it is no
            // step of its own; its text, filled, stands in for its template.
            "ask" if fed_back(grading.as_ref(), text(line, "task")) => {
                if let Some((_, at, Some((attempt, attempts, right)))) = grading
                    .as_mut()
                    .map(|(task, at, last)| (task, *at, last.take()))
                {
                    let key = if right {
                        "right"
                    } else if attempt < attempts {
                        "wrong"
                    } else {
                        "last"
                    };
                    if steps[at][key].as_str().is_none_or(str::is_empty) {
                        steps[at][key] = json!(text(line, "text").unwrap_or_default());
                    }
                }
            }
            "grade" => {
                let task = text(line, "task").unwrap_or_default().to_owned();
                let attempt = line.get("attempt").and_then(Value::as_u64).unwrap_or(1);
                let attempts = line
                    .get("attempts")
                    .and_then(Value::as_u64)
                    .unwrap_or(attempt);
                let right = line.get("right").and_then(Value::as_bool).unwrap_or(false);
                let open = grading.as_ref().is_some_and(|(open, _, _)| *open == task);
                if attempt == 1 || !open {
                    let sha = text(line, "check_sha256").unwrap_or_default();
                    steps.push(json!({
                        "step": "grade",
                        "task": task,
                        "check": format!(
                            "echo 'the run graded with a check its log keeps only the sha256 of \
                             ({sha}): write the check here' >&2; exit 1"
                        ),
                        "attempts": attempts,
                        "wrong": "",
                        "place": {
                            "turn": turn,
                            "between": true,
                            "note": "the log keeps the check's sha256, not the check, and the feedback \
                                     asks filled, not their templates: `check` fails until it is \
                                     written, and `wrong`, `last` and `right` are the run's texts",
                        },
                    }));
                    grading = Some((task, steps.len() - 1, None));
                }
                if let Some((_, _, last)) = grading.as_mut() {
                    *last = Some((attempt, attempts, right));
                }
            }
            "ask" => {
                let mut step =
                    json!({"step": "ask", "text": text(line, "text").unwrap_or_default()});
                if let Some(task) = text(line, "task") {
                    step["task"] = json!(task);
                }
                if line.get("scoping").and_then(Value::as_bool) == Some(true) {
                    step["scoping"] = json!(true);
                }
                let mut attach = Vec::new();
                for file in line
                    .get("files")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let (Some(sha), Some(path)) = (
                        file.get("sha256").and_then(Value::as_str),
                        file.get("path").and_then(Value::as_str),
                    ) else {
                        return Err(refused(line, "a file it attached names no digest or path"));
                    };
                    let extension = file
                        .get("media_type")
                        .and_then(Value::as_str)
                        .and_then(|media| media.split('/').nth(1))
                        .unwrap_or("bin");
                    let place = format!("attached/{sha}.{extension}");
                    files.insert(place.clone(), path.to_owned());
                    attach.push(Value::String(place));
                }
                if !attach.is_empty() {
                    step["attach"] = Value::Array(attach);
                }
                let mut place = json!({"turn": turn, "between": true});
                if asks > 0
                    && let Some(response) = answered
                {
                    place["answers"] = json!(response);
                }
                if let Some((read, compose)) = gap.take() {
                    place["read_ms"] = json!(read);
                    place["compose_ms"] = json!(compose);
                }
                step["place"] = place;
                steps.push(step);
                asks += 1;
            }
            "refused" => {
                if text(line, "command") != Some("ask") {
                    return Err(refused(
                        line,
                        format!(
                            "a refused `{}` has no step: only a refused ask does",
                            text(line, "command").unwrap_or_default()
                        ),
                    ));
                }
                steps.push(json!({
                    "step": "ask",
                    "text": "",
                    "refused": text(line, "because").unwrap_or_default(),
                    "place": {
                        "between": true,
                        "note": "the log keeps no refused ask's text: the refusal turns on the \
                                 session's state, not on what was typed",
                    },
                }));
            }
            "flag" => {
                let entry = text(line, "entry").unwrap_or_default();
                steps.push(json!({
                    "step": "flag",
                    "pick": live.pick(entry, line)?,
                    "note": text(line, "note").unwrap_or_default(),
                    "place": {"turn": turn, "between": true},
                }));
            }
            "patch" if text(line, "lane") == Some("operator") => {
                let entry = line.get("entry").and_then(Value::as_object);
                match text(line, "op") {
                    Some("add") => {
                        let entry = entry.ok_or_else(|| refused(line, "its entry is missing"))?;
                        if !text(entry, "id").is_some_and(|id| wrapping.contains(id)) {
                            steps.push(keep_step(before, entry, line)?);
                        }
                    }
                    Some("retire") => {
                        let id = entry
                            .and_then(|entry| text(entry, "id"))
                            .unwrap_or_default();
                        steps.push(json!({
                            "step": "retire",
                            "pick": live.pick(id, line)?,
                            "place": {"between": true},
                        }));
                    }
                    Some("supersede") => {
                        let old = text(line, "supersedes").unwrap_or_default();
                        let new = entry
                            .and_then(|entry| text(entry, "text"))
                            .unwrap_or_default();
                        steps.push(json!({
                            "step": "edit",
                            "pick": live.pick(old, line)?,
                            "text": new,
                            "place": {"between": true},
                        }));
                    }
                    op => {
                        return Err(refused(
                            line,
                            format!(
                                "an operator's `{}` has no step: only a keep, an edit and a \
                                 retire do",
                                op.unwrap_or_default()
                            ),
                        ));
                    }
                }
                live.patched(line);
            }
            "patch" => live.patched(line),
            "fork" if text(line, "why") == Some("operator") && !fired.contains(&seq) => {
                let place = if let Some(cut) = line.get("cut").and_then(Value::as_u64) {
                    cut
                } else {
                    let request = line.get("at").and_then(Value::as_u64);
                    before
                        .iter()
                        .find(|other| {
                            text(other, "kind") == Some("response")
                                && other.get("to_request").and_then(Value::as_u64) == request
                        })
                        .and_then(|other| other.get("seq").and_then(Value::as_u64))
                        .ok_or_else(|| refused(line, "its request has no answer before it"))?
                };
                let mut step = json!({
                    "step": "fork",
                    "at": line_pick(before, place, line)?,
                    "place": {"between": true},
                });
                match text(line, "ask") {
                    Some(crate::drive::session::OPERATOR_TEXT_ASK) => {
                        step["text"] = json!(text(line, "question").unwrap_or_default());
                    }
                    ask => step["ask"] = json!(ask.unwrap_or_default()),
                }
                steps.push(step);
            }
            "wrap.accepted" => {
                let entry = text(line, "entry")
                    .ok_or_else(|| refused(line, "a wrap that keeps nothing has no step"))?;
                let span = live
                    .0
                    .get(entry)
                    .ok_or_else(|| refused(line, format!("entry `{entry}` is not live")))?;
                let (Some(call), Some(from)) = (
                    line.get("call").and_then(Value::as_u64),
                    line.get("from").and_then(Value::as_u64),
                ) else {
                    return Err(refused(line, "it names no call or no source"));
                };
                let call = line_pick(before, call, line)?;
                steps.push(json!({
                    "step": "wrap",
                    "call": call["call"],
                    "from": line_pick(before, from, line)?,
                    "span": span,
                    "place": {"between": true},
                }));
            }
            "fork.shadowed" => shadows.push(seq),
            "patch.refused" => refusals.push(seq),
            "suggestion" | "correction" => {
                let (open, key) = if kind == "suggestion" {
                    (&mut shadows, "shadow")
                } else {
                    (&mut refusals, "refused")
                };
                let answers = line.get(key).and_then(Value::as_u64);
                let answer = match text(line, "outcome") {
                    Some("accepted") => Some("accept"),
                    Some("rejected") => Some("reject"),
                    _ => None,
                };
                match answer {
                    Some(answer) => {
                        answered_first(open, answers, line)?;
                        steps.push(json!({
                            "step": kind,
                            "answer": answer,
                            "place": {"between": true},
                        }));
                    }
                    // Unanswered: the chip lapsed, no step's.
                    None => open.retain(|chip| Some(*chip) != answers),
                }
            }
            "meditation.held" => {
                if text(line, "reason") == Some("operator") {
                    steps.push(json!({"step": "declare-meditation", "place": {"between": true}}));
                    held_open = true;
                }
            }
            "meditation" => {
                if line.get("refilled_anyway").and_then(Value::as_bool) == Some(true) {
                    steps.push(json!({"step": "refill-anyway", "place": {"between": true}}));
                    held_open = false;
                } else if text(line, "reason") == Some("operator") {
                    if held_open {
                        held_open = false;
                    } else if line.contains_key("phase") {
                        return Err(refused(
                            line,
                            "a meditation declared with a phase move has no step: \
                             `move-phase` with `meditate` writes a different move",
                        ));
                    } else {
                        steps.push(json!({
                            "step": "declare-meditation",
                            "place": {"between": true},
                        }));
                    }
                }
            }
            "phase.ruled" => match (text(line, "by"), text(line, "call")) {
                (Some("operator"), _) => steps.push(json!({
                    "step": "move-phase",
                    "to": text(line, "to").unwrap_or_default(),
                    "meditate": text(line, "choice") == Some("meditation"),
                    "place": {"between": true},
                })),
                (None, Some(call)) => steps.push(json!({
                    "step": "rule-phase",
                    "call": call,
                    "choice": text(line, "choice").unwrap_or_default(),
                    "place": {"between": true},
                })),
                _ => {}
            },
            "phase.proposed" => {
                if text(line, "by") == Some("operator") {
                    let mut step = json!({
                        "step": "propose-phase",
                        "to": text(line, "to").unwrap_or_default(),
                        "place": {"between": true},
                    });
                    if let Some(meditate) = line.get("meditate").and_then(Value::as_bool) {
                        step["meditate"] = json!(meditate);
                    }
                    steps.push(step);
                }
            }
            "phase.started" => steps.push(json!({
                "step": "start-phase",
                "phase": text(line, "phase").unwrap_or_default(),
                "place": {"between": true},
            })),
            "tangent.open" => steps.push(json!({
                "step": "open-tangent",
                "id": text(line, "id").unwrap_or_default(),
                "place": {"between": true},
            })),
            "tangent.close" => {
                let ruled: Vec<&str> = ["keep", "drop", "park"]
                    .into_iter()
                    .zip(["kept", "dropped", "parked"])
                    .filter(|(_, key)| {
                        line.get(*key)
                            .and_then(Value::as_array)
                            .is_some_and(|ids| !ids.is_empty())
                    })
                    .map(|(word, _)| word)
                    .collect();
                let each = match ruled.as_slice() {
                    [] => "keep",
                    [one] => one,
                    _ => {
                        return Err(refused(
                            line,
                            "a tangent closed with more than one ruling has no step: \
                             `close-tangent` rules one way on every entry",
                        ));
                    }
                };
                steps.push(
                    json!({"step": "close-tangent", "each": each, "place": {"between": true}}),
                );
                live.closed(line);
            }
            "stop.asked" => {
                return Err(refused(
                    line,
                    "a stop has no step: a scenario's steps run once the session has settled",
                ));
            }
            "tool_call" => {
                if let Some(approval) = line.get("approval").and_then(Value::as_object)
                    && approval.contains_key("decided_at")
                {
                    let scope = text(approval, "scope").unwrap_or_default().to_owned();
                    match &approvals {
                        Some((first, _)) if *first != scope => {
                            return Err(refused(
                                line,
                                format!(
                                    "an approval answered `{scope}` where an earlier one was \
                                     answered `{first}`: the header's `approvals` gives every \
                                     prompt one answer"
                                ),
                            ));
                        }
                        Some(_) => {}
                        None => approvals = Some((scope, seq)),
                    }
                }
                if line.contains_key("background") {
                    let asked = text(line, "arguments")
                        .and_then(|arguments| serde_json::from_str::<Value>(arguments).ok())
                        .and_then(|arguments| {
                            arguments.get("is_background").and_then(Value::as_bool)
                        })
                        == Some(true);
                    if !asked {
                        return Err(refused(
                            line,
                            "a call the operator moved to the background has no step",
                        ));
                    }
                }
            }
            "settlement" => {
                awaiting = text(line, "to") == Some("awaiting");
                if text(line, "to") == Some("ended") {
                    steps.push(json!({"step": "end"}));
                }
            }
            // A background job that ended between turns, before the next
            // move: the operator's next move came after it, so the script
            // waits for it (its id is the run's own; its status is not).
            "background.ended" if awaiting => steps.push(json!({
                "step": "wait",
                "until": "background.ended",
                "where": {"status": text(line, "status").unwrap_or_default()},
                "place": {"between": true},
            })),
            _ => {}
        }
    }
    Ok(Derived {
        head,
        instruction_files,
        steps,
        fixed_asks: if asks > 0 { vec![0] } else { Vec::new() },
        approvals: approvals.map(|(scope, _)| scope),
        files,
    })
}

/// The open grade step (#942): its task, its index among the steps, and
/// the latest grade of it -- attempt, attempts, right -- until an ask
/// answers it.
type OpenGrade = (String, usize, Option<(u64, u64, bool)>);

/// Whether an ask of `task` answers the open grade step's latest grade:
/// the feedback the step itself sends (#942).
fn fed_back(grading: Option<&OpenGrade>, task: Option<&str>) -> bool {
    grading.is_some_and(|(open, _, last)| last.is_some() && Some(open.as_str()) == task)
}

/// `log`'s operator lines, each with what the operator put in and what it
/// did, and nothing a run decides on its own -- no `seq`, no `t`, an entry
/// named by its text rather than its id: what a derived script's replay
/// must log again, line for line (#908).
///
/// # Errors
///
/// A log that does not read.
#[allow(clippy::too_many_lines)]
pub fn operator_lines(log: &str) -> Result<Vec<Value>, Refusal> {
    let lines = lines_of(log)?;
    let mut texts: BTreeMap<String, String> = BTreeMap::new();
    let mut out = Vec::new();
    let named = |texts: &BTreeMap<String, String>, ids: Option<&Value>| -> Vec<String> {
        ids.and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|id| texts.get(id).cloned().unwrap_or_else(|| id.to_owned()))
            .collect()
    };
    for line in &lines {
        let kind = text(line, "kind").unwrap_or_default();
        let pick = |keys: &[&str]| -> Value {
            let mut object = Map::new();
            object.insert("kind".to_owned(), json!(kind));
            for key in keys {
                if let Some(value) = line.get(*key) {
                    object.insert((*key).to_owned(), value.clone());
                }
            }
            Value::Object(object)
        };
        match kind {
            "grade" => out.push(pick(&[
                "turn",
                "task",
                "attempt",
                "attempts",
                "right",
                "timed_out",
            ])),
            "ask" => {
                let mut ask = pick(&["turn", "text", "scoping", "task"]);
                let digests: Vec<Value> = line
                    .get("files")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|file| file.get("sha256").cloned())
                    .collect();
                if !digests.is_empty() {
                    ask["files"] = Value::Array(digests);
                }
                out.push(ask);
            }
            "refused" if text(line, "command") == Some("ask") => {
                out.push(pick(&["command", "because", "during"]));
            }
            "flag" => {
                let mut flag = pick(&["turn", "note"]);
                let entry = text(line, "entry").unwrap_or_default();
                flag["entry"] = json!(
                    texts
                        .get(entry)
                        .cloned()
                        .unwrap_or_else(|| entry.to_owned())
                );
                out.push(flag);
            }
            "patch" => {
                let entry = line.get("entry").and_then(Value::as_object);
                let content = entry
                    .and_then(|entry| text(entry, "text"))
                    .unwrap_or_default();
                if text(line, "lane") == Some("operator") {
                    let old = text(line, "supersedes").unwrap_or_default();
                    let mut patch = json!({
                        "kind": kind,
                        "op": text(line, "op"),
                        "text": content,
                        "supersedes": texts.get(old).cloned().unwrap_or_else(|| old.to_owned()),
                    });
                    // A keep's tag and where it kept from (the app audit's page walk, F20).
                    for key in ["category", "kept_from"] {
                        if let Some(value) = entry.and_then(|entry| entry.get(key)) {
                            patch[key] = value.clone();
                        }
                    }
                    out.push(patch);
                }
                if let Some(id) = entry.and_then(|entry| text(entry, "id")) {
                    texts.insert(id.to_owned(), content.to_owned());
                }
            }
            // The operator's gestures (the app audit's page walk, F20).
            "fork" if text(line, "why") == Some("operator") => {
                let mut fork = pick(&["why", "ask"]);
                if text(line, "ask") == Some(crate::drive::session::OPERATOR_TEXT_ASK) {
                    fork["question"] = json!(text(line, "question"));
                }
                out.push(fork);
            }
            "wrap.accepted" => {
                let entry = text(line, "entry").unwrap_or_default();
                out.push(json!({
                    "kind": kind,
                    "entry": texts.get(entry).cloned().unwrap_or_else(|| entry.to_owned()),
                }));
            }
            "suggestion" | "correction"
                if matches!(text(line, "outcome"), Some("accepted" | "rejected")) =>
            {
                out.push(pick(&["outcome"]));
            }
            "meditation.held" if text(line, "reason") == Some("operator") => {
                out.push(pick(&["reason"]));
            }
            "meditation"
                if text(line, "reason") == Some("operator")
                    || line.get("refilled_anyway").and_then(Value::as_bool) == Some(true) =>
            {
                out.push(pick(&["reason", "refilled_anyway"]));
            }
            "phase.ruled" => out.push(pick(&["by", "call", "choice", "from", "to"])),
            "phase.proposed" if text(line, "by") == Some("operator") => {
                out.push(pick(&["by", "to", "meditate"]));
            }
            "phase.started" => out.push(pick(&["phase"])),
            "tangent.open" => out.push(pick(&["id", "at_turn"])),
            "tangent.close" => out.push(json!({
                "kind": kind,
                "id": text(line, "id"),
                "kept": named(&texts, line.get("kept")),
                "dropped": named(&texts, line.get("dropped")),
                "parked": named(&texts, line.get("parked")),
            })),
            "settlement" if text(line, "to") == Some("ended") => out.push(pick(&["to"])),
            _ => {}
        }
    }
    Ok(out)
}

/// The scenario `derived` makes, as JSON Lines: its header -- `name`, the
/// regimen and head beside it, the files it seeds, the approvals scope and
/// the fixed asks -- then the `reply` and `classify` lines of another
/// scenario (`canned`) when a canned replay needs them, then its steps. A
/// canned source's own `seed` joins the files seeded, each under `seed/`.
#[must_use]
pub fn scenario_text(name: &str, derived: &Derived, canned: &Canned) -> String {
    let mut header = json!({
        "scenario": name,
        "regimen": "regimen.toml",
        "head": "head.md",
        "fixed_asks": derived.fixed_asks,
    });
    let mut seed: Map<String, Value> = derived
        .files
        .keys()
        .map(|place| (place.clone(), json!(format!("files/{}", file_name(place)))))
        .collect();
    for place in canned.seed.keys() {
        seed.insert(place.clone(), json!(format!("seed/{place}")));
    }
    if !seed.is_empty() {
        header["seed"] = Value::Object(seed);
    }
    if let Some(scope) = &derived.approvals {
        header["approvals"] = json!(scope);
    }
    let mut out = format!("{header}\n");
    for line in &canned.lines {
        out.push_str(line);
        out.push('\n');
    }
    for step in &derived.steps {
        out.push_str(&step.to_string());
        out.push('\n');
    }
    out
}

/// A seeded file's name beside the script: its place's last part.
#[must_use]
pub fn file_name(place: &str) -> &str {
    place.rsplit('/').next().unwrap_or(place)
}

/// What a derived script takes from another scenario to replay on the
/// canned substrate: its `reply` and `classify` lines, as written, and its
/// worktree `seed` -- each path in the worktree, and its source relative to
/// that scenario.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Canned {
    /// The `reply` and `classify` lines.
    pub lines: Vec<String>,
    /// The seed.
    pub seed: BTreeMap<String, String>,
}

/// What `scenario` lends a canned replay ([`Canned`]).
///
/// # Errors
///
/// A scenario that does not read.
pub fn canned_of(scenario: &str) -> Result<Canned, crate::drive::scenario::ScenarioError> {
    let read = crate::drive::scenario::parse(scenario)?;
    Ok(Canned {
        lines: scenario
            .lines()
            .filter(|line| {
                serde_json::from_str::<Value>(line).is_ok_and(|value| {
                    value.get("reply").is_some() || value.get("classify").is_some()
                })
            })
            .map(str::to_owned)
            .collect(),
        seed: read.seed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A log's lines from `rest`, after a start whose head is `head`.
    fn log(head: &str, rest: &[Value]) -> Vec<Map<String, Value>> {
        std::iter::once(json!({
            "seq": 0, "kind": "session.start",
            "head": [{"role": "system", "content": head}],
        }))
        .chain(rest.iter().cloned())
        .enumerate()
        .map(|(seq, mut line)| {
            line["seq"] = json!(seq);
            match line {
                Value::Object(object) => object,
                _ => unreachable!(),
            }
        })
        .collect()
    }

    fn refusal(lines: &[Value]) -> Refusal {
        derived(&log("h", lines), None).expect_err("refused")
    }

    /// #908: every move the format cannot express is refused, naming its
    /// line by `seq` and kind -- never dropped.
    #[test]
    fn a_move_the_format_cannot_express_is_refused_naming_its_line() {
        let stop = refusal(&[json!({"kind": "stop.asked", "turn": 1})]);
        assert_eq!(
            (stop.seq, stop.kind.as_deref()),
            (Some(1), Some("stop.asked"))
        );
        assert!(
            stop.to_string().starts_with("line 1 (`stop.asked`): "),
            "{stop}"
        );
        let moved = refusal(&[json!({
            "kind": "tool_call", "background": "bg_1", "arguments": "{\"command\": \"sleep 9\"}",
        })]);
        assert_eq!(moved.kind.as_deref(), Some("tool_call"), "{moved}");
        let approvals = refusal(&[
            json!({"kind": "tool_call", "approval": {"scope": "once", "decided_at": 5}}),
            json!({"kind": "tool_call", "approval": {"scope": "always", "decided_at": 9}}),
        ]);
        assert_eq!(
            approvals.seq,
            Some(2),
            "the second answer is the one apart: {approvals}"
        );
        let mixed = refusal(&[json!({
            "kind": "tangent.close", "id": "t", "kept": ["a"], "dropped": ["b"], "parked": [],
        })]);
        assert_eq!(mixed.kind.as_deref(), Some("tangent.close"));
        let phased =
            refusal(&[json!({"kind": "meditation", "reason": "operator", "phase": {"to": "b"}})]);
        assert_eq!(phased.kind.as_deref(), Some("meditation"));
        let command =
            refusal(&[json!({"kind": "refused", "command": "flag-entry", "because": "x"})]);
        assert_eq!(command.kind.as_deref(), Some("refused"));
        let unpicked = refusal(&[json!({"kind": "flag", "turn": 1, "entry": "gone", "note": "n"})]);
        assert_eq!(unpicked.kind.as_deref(), Some("flag"));
        // The operator's gestures (the app audit's page walk, F20).
        let asked = refusal(&[
            json!({"kind": "patch", "lane": "operator", "op": "add", "entry": {
                "id": "operator-1", "text": "x", "kept_from": "ask",
            }}),
        ]);
        assert!(asked.to_string().contains("a keep from `ask`"), "{asked}");
        let bare = refusal(&[json!({"kind": "wrap.accepted", "call": 1, "from": 1})]);
        assert_eq!(bare.kind.as_deref(), Some("wrap.accepted"), "{bare}");
        let out_of_order = refusal(&[
            json!({"kind": "fork.shadowed", "why": "open"}),
            json!({"kind": "fork.shadowed", "why": "open"}),
            json!({"kind": "suggestion", "shadow": 2, "outcome": "rejected"}),
        ]);
        assert_eq!(
            (out_of_order.seq, out_of_order.kind.as_deref()),
            (Some(3), Some("suggestion")),
            "{out_of_order}"
        );
        let not_last = refusal(&[
            json!({"kind": "tool_call", "id": "c", "shown": "x"}),
            json!({"kind": "tool_call", "id": "c", "shown": "y"}),
            json!({"kind": "fork", "why": "operator", "ask": "open_next", "at": 0, "cut": 1}),
        ]);
        assert!(
            not_last.to_string().contains("not the last call of its id"),
            "{not_last}"
        );
    }

    /// #908: the moves it writes keep their place -- the first ask fixed,
    /// a later one marked with the response it followed -- and the head is
    /// what the replay's start begins from: the instruction files' render
    /// and the priming taken off.
    #[test]
    fn the_moves_keep_their_place_and_the_head_what_the_start_adds_comes_off() {
        let lines = log(
            "you are the trunk\n\nPRIMING\n\n---\n\n--- Context from: AGENTS.md ---\nx\n--- End of Context from: AGENTS.md ---",
            &[
                json!({"kind": "ask", "turn": 1, "text": "first"}),
                json!({"kind": "request", "lane": "trunk", "turn": 1}),
                json!({"kind": "response", "to_request": 2, "text": "ok"}),
                json!({"kind": "settlement", "from": "turn", "to": "awaiting"}),
                json!({"kind": "idle.gap", "read": 40, "compose": 900}),
                json!({"kind": "ask", "turn": 2, "text": "second", "scoping": true}),
                json!({"kind": "settlement", "from": "awaiting", "to": "ended"}),
            ],
        );
        let mut lines = lines;
        lines[0].insert(
            "instruction_files".to_owned(),
            json!([{"path": "AGENTS.md", "sha256": "0"}]),
        );
        let derived = derived(&lines, Some("PRIMING")).expect("derived");
        assert_eq!(derived.head, "you are the trunk");
        assert_eq!(derived.instruction_files, ["AGENTS.md"]);
        assert_eq!(derived.fixed_asks, [0]);
        assert_eq!(derived.steps.len(), 3);
        assert!(
            derived.steps[0]["place"].get("answers").is_none(),
            "the first is fixed"
        );
        assert_eq!(
            derived.steps[1]["place"]["answers"],
            json!(3),
            "{}",
            derived.steps[1]
        );
        assert_eq!(derived.steps[1]["place"]["compose_ms"], json!(900));
        assert_eq!(derived.steps[1]["scoping"], json!(true));
        assert_eq!(derived.steps[2]["step"], json!("end"));
        // And the scenario it writes reads as one.
        let text = scenario_text("s", &derived, &Canned::default());
        let read = crate::drive::scenario::parse(&text).expect("a scenario");
        assert_eq!(read.fixed_asks, [0]);
        assert_eq!(read.steps.len(), 3);
    }

    /// The operator's gestures (the app audit's page walk, F20): each one a
    /// log holds derives as its step -- an entry picked by its text, a line
    /// by its call's id or its answer's text, never by a seq -- and the steps
    /// read as a scenario again. The replay's operator lines are the round
    /// trip's, in `drive_scenario_cli`.
    #[test]
    #[allow(clippy::too_many_lines)]
    fn every_gesture_derives_as_its_step() {
        let answer = "It holds a note.\n\nAlso: ports.";
        // Seqs 1 to 5: a trunk request that called `call-1`, its result,
        // and the request after it, answered.
        let prefix = [
            json!({"kind": "request", "lane": "trunk", "turn": 1}),
            json!({"kind": "response", "to_request": 1, "text": "Reading it."}),
            json!({"kind": "tool_call", "id": "call-1", "request": 1, "turn": 1, "shown": "a note\n"}),
            json!({"kind": "request", "lane": "trunk", "turn": 1}),
            json!({"kind": "response", "to_request": 4, "text": answer}),
        ];
        let kept = |text: &str, from: &str| {
            json!({"kind": "patch", "lane": "operator", "op": "add", "entry": {
                "id": "operator-6", "text": text, "authority": "operator", "kept_from": from,
            }})
        };
        let keep = json!({"step": "keep", "from": {"call": "call-1"}, "span": "a note"});
        let refused = json!({
            "kind": "patch.refused", "lane": "self-capture", "op": "resolve",
            "entry": "operator-6", "reason": "operator-entry",
        });
        let shadowed = json!({"kind": "fork.shadowed", "why": "open", "at": 4});
        let cases: Vec<(&str, Vec<Value>, Vec<Value>)> = vec![
            (
                "a keep from a tool result",
                vec![
                    json!({"kind": "patch", "lane": "operator", "op": "add", "entry": {
                        "id": "operator-6", "text": "a note", "category": "notes",
                        "authority": "operator", "kept_from": "tool-result",
                    }}),
                ],
                vec![
                    json!({"step": "keep", "from": {"call": "call-1"}, "span": "a note", "tag": "notes"}),
                ],
            ),
            (
                "a keep from an answer",
                vec![kept("Also: ports.", "answer")],
                vec![
                    json!({"step": "keep", "from": {"answer": "Also: ports."}, "span": "Also: ports."}),
                ],
            ),
            (
                "a fork at a call, by a pinned ask",
                vec![json!({
                    "kind": "fork", "lane": "interview", "why": "operator", "trigger": "operator",
                    "ask": "open_next", "at": 1, "cut": 3,
                })],
                vec![json!({"step": "fork", "at": {"call": "call-1"}, "ask": "open_next"})],
            ),
            (
                "a fork at an answer, in the operator's words",
                vec![json!({
                    "kind": "fork", "lane": "interview", "why": "operator", "trigger": "operator",
                    "ask": "operator-text", "question": "What is in it?", "at": 4,
                })],
                vec![json!({"step": "fork", "at": {"answer": answer}, "text": "What is in it?"})],
            ),
            (
                "a wrap",
                vec![
                    kept("It holds a note.", "answer"),
                    json!({"kind": "wrap.accepted", "call": 3, "from": 5, "entry": "operator-6"}),
                ],
                vec![json!({
                    "step": "wrap", "call": "call-1", "from": {"answer": answer},
                    "span": "It holds a note.",
                })],
            ),
            (
                "a suggestion rejected, after one that lapsed",
                vec![
                    shadowed.clone(),
                    json!({"kind": "suggestion", "shadow": 6, "outcome": "unanswered"}),
                    shadowed.clone(),
                    json!({"kind": "suggestion", "shadow": 8, "outcome": "rejected"}),
                ],
                vec![json!({"step": "suggestion", "answer": "reject"})],
            ),
            (
                "a suggestion accepted, its fork no step of its own",
                vec![
                    shadowed.clone(),
                    json!({
                        "kind": "fork", "lane": "interview", "why": "operator",
                        "trigger": "operator", "ask": "open_next", "at": 4,
                    }),
                    json!({"kind": "suggestion", "shadow": 6, "outcome": "accepted", "fork": 7}),
                ],
                vec![json!({"step": "suggestion", "answer": "accept"})],
            ),
            (
                "a correction rejected",
                vec![
                    kept("a note", "tool-result"),
                    refused.clone(),
                    json!({"kind": "correction", "refused": 7, "outcome": "rejected"}),
                ],
                vec![
                    keep.clone(),
                    json!({"step": "correction", "answer": "reject"}),
                ],
            ),
            (
                "a correction accepted",
                vec![
                    kept("a note", "tool-result"),
                    refused,
                    json!({"kind": "patch", "lane": "self-capture", "op": "resolve",
                           "entry": {"id": "operator-6", "text": "a note"}}),
                    json!({"kind": "correction", "refused": 7, "outcome": "accepted", "entry": "operator-6"}),
                ],
                vec![
                    keep.clone(),
                    json!({"step": "correction", "answer": "accept"}),
                ],
            ),
            (
                "a retire",
                vec![
                    kept("a note", "tool-result"),
                    json!({"kind": "patch", "lane": "operator", "op": "retire",
                           "entry": {"id": "operator-6", "text": "a note"}}),
                ],
                vec![keep, json!({"step": "retire", "pick": "a note"})],
            ),
        ];
        let mut wrong = Vec::new();
        for (name, rest, expected) in cases {
            let lines: Vec<Value> = prefix.iter().cloned().chain(rest).collect();
            let read = log("h", &lines);
            let steps = match derived(&read, None) {
                Ok(derived) => {
                    let text = scenario_text("s", &derived, &Canned::default());
                    if let Err(error) = crate::drive::scenario::parse(&text) {
                        wrong.push(format!("{name}: its steps do not read: {error}"));
                    }
                    derived
                        .steps
                        .into_iter()
                        .map(|mut step| {
                            if let Some(step) = step.as_object_mut() {
                                step.remove("place");
                            }
                            step
                        })
                        .collect::<Vec<_>>()
                }
                Err(refusal) => {
                    wrong.push(format!("{name}: refused: {refusal}"));
                    continue;
                }
            };
            if steps != expected {
                wrong.push(format!(
                    "{name}: derived {}, not {}",
                    json!(steps),
                    json!(expected)
                ));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }
}
