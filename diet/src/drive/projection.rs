//! A driven session's record, projected from its own log (#157, ruled).
//!
//! One source: the log `serve` streams and writes is what the record is
//! made from, at the session's end, and nothing else is consulted but the
//! regime it ran under and which engine served it. What the record cannot
//! spell is NAMED, never dropped: each such fact is an [`Unspellable`],
//! with its reason and, for a response, its text.
//!
//! # The counts
//!
//! A record's `response.output_tokens` and `turn.prefill_tokens` are
//! derived from the server's `timings` (`predicted_n`; `prompt_n +
//! cache_n`) ONLY for an engine in [`CITED`]: the engines on which the
//! server's `usage` was measured equal to its `timings`, on every capture
//! ([`MEASUREMENT`]). For any other engine they are unspellable --
//! "equality unmeasured for this engine" -- and never a derived number. A
//! response carrying `usage` (a dialect whose server reports no timings,
//! log v2) gives `output_tokens` as the server's own `completion_tokens`.
//!
//! # Head changes
//!
//! A live record names every change of a lane's head between its requests
//! (`prefix.changed`). The log carries each request's head digest, and the
//! trunk it was built from is in the log too: the session's head, then each
//! answered turn's ask and answer, appended as the session appends them.
//! Each request's shape is rebuilt from that -- `serve`'s trunk has no tools
//! and no template arguments, which is ASSERTED by the check, not assumed:
//! only where `client::head` over the rebuilt shape gives the logged digest
//! does the row carry its diff. Anywhere else it is `unattributed`, with
//! nothing guessed, and the mismatch is named (ruled on #157).

use std::collections::{BTreeMap, BTreeSet};

use crate::client::head::Head;
use crate::client::shape::{
    Limits, Message, RequestShape, Role, SamplerCard, ToolCall, ToolDefinition,
};
use crate::formats::log::{self, Event as Line, Lane};
use crate::formats::record::json::Decimal;
use crate::formats::record::{self, Count, Event, Execution, PrefixReason, Regime, Source};

use super::registry::Identity;

/// Where the equality of `usage` and `timings` was measured (#157).
pub const MEASUREMENT: &str =
    "https://github.com/ironblock/discipline/issues/157#issuecomment-5941593602";

/// The engines the measurement covers: a commit prefix, or an
/// `engine_build_info` literal together with the binary measured. A literal
/// alone names no engine (every prebuilt release of a fork may report
/// `b0-unknown-dirty`), so it cites only beside the `engine_identity` the
/// captures were taken on (#264's review).
pub const CITED: &[Engine] = &[
    Engine::Commit("e7051ef"),
    Engine::Commit("4df29be"),
    Engine::Literal(
        "b0-unknown-dirty",
        "980845d60ae7a820f5e2a8b7081727a242b35d3ca8a4021a6fb1240f4a0aa3d4",
    ),
    // The stream-replay substrate (#411): it plays an `e7051ef` capture,
    // byte for byte, so its `usage` and `timings` are that capture's, which
    // the measurement covers. Its literal and acts digest, as
    // `canned::replay_build_info` and `canned::replay_digest` compute them.
    Engine::Literal(
        "canned-2087aa015ae2a2a05b36adedc25902d605ac2b2c461ef9139747a25ca1e566f8",
        "2087aa015ae2a2a05b36adedc25902d605ac2b2c461ef9139747a25ca1e566f8",
    ),
    // The tool-turn replay: two `e486f80` captures, in each of which
    // `usage` equals `timings` (`canned::tests::the_tool_turn_replays_usage_
    // is_its_timings`), as `canned::replay_tools_build_info` computes it.
    Engine::Literal(
        "canned-7336cc7fed1f2e64c6095c49f73317c9b30be3ba8d22d7cf3bf8fc2bc62c6966",
        "7336cc7fed1f2e64c6095c49f73317c9b30be3ba8d22d7cf3bf8fc2bc62c6966",
    ),
];

/// An engine, as the registry pins it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// A commit, by a prefix of its hash.
    Commit(&'static str),
    /// A `build_info` literal, exactly, and the sha256 of the engine binary
    /// it was measured on (the registry's `engine_identity`).
    Literal(&'static str, &'static str),
}

/// Which cited engine `identity` is, if any: its `engine_build_info`
/// literal and its engine's digest when it declares a literal (checked
/// first, as the engine check does), else its `engine_commit`.
#[must_use]
pub fn cited(identity: &Identity) -> Option<Engine> {
    CITED
        .iter()
        .copied()
        .find(|engine| match (engine, &identity.engine_build_info) {
            (Engine::Literal(literal, binary), Some(declared)) => {
                declared == literal && identity.engine.version_or_digest == *binary
            }
            (Engine::Commit(prefix), None) => identity
                .engine_commit
                .as_deref()
                .is_some_and(|commit| commit.starts_with(prefix)),
            (Engine::Literal(..), None) | (Engine::Commit(_), Some(_)) => false,
        })
}

impl Engine {
    /// How the sidecar names it.
    #[must_use]
    pub fn describes(self) -> String {
        match self {
            Self::Commit(prefix) => format!("commit {prefix}"),
            Self::Literal(literal, binary) => {
                format!("build_info {literal}, engine binary sha256 {binary}")
            }
        }
    }
}

/// The sidecar beside a record (ruled on #157): everything the record could
/// not spell, and -- when counts were derived -- the engine and the
/// measurement they were derived on. Canonical JSON, one line.
#[must_use]
pub fn sidecar(projection: &Projection) -> String {
    let mut out = String::new();
    crate::formats::record::json::render(&sidecar_value(projection), &mut out);
    out
}

/// [`sidecar`], as a value.
#[must_use]
pub fn sidecar_value(projection: &Projection) -> crate::formats::record::json::Value {
    use crate::formats::record::json::Value;
    let mut fields = BTreeMap::new();
    if let Some(engine) = projection.engine {
        fields.insert("engine".to_owned(), Value::String(engine.describes()));
        fields.insert(
            "measurement".to_owned(),
            Value::String(MEASUREMENT.to_owned()),
        );
    }
    let items = projection
        .unspellable
        .iter()
        .map(|item| {
            let mut object = BTreeMap::from([
                (
                    "seq".to_owned(),
                    Value::Integer(i64::try_from(item.seq).unwrap_or(i64::MAX)),
                ),
                ("kind".to_owned(), Value::String(item.kind.to_owned())),
                ("why".to_owned(), Value::String(item.why.clone())),
            ]);
            if let Some(text) = &item.text {
                object.insert("text".to_owned(), Value::String(text.clone()));
            }
            Value::Object(object)
        })
        .collect();
    fields.insert("unspellable".to_owned(), Value::Array(items));
    Value::Object(fields)
}

/// A fact the record could not spell, named rather than dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unspellable {
    /// The log line it came from.
    pub seq: u64,
    /// The log line's kind.
    pub kind: &'static str,
    /// Why the record cannot carry it.
    pub why: String,
    /// The text the line carried, when it carried any: a response's answer,
    /// a cancelled call's partial, a failure's message.
    pub text: Option<String>,
}

/// The record's rows, and everything the record could not spell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Projection {
    /// The rows, in the log's order.
    pub events: Vec<Event>,
    /// What the rows do not carry.
    pub unspellable: Vec<Unspellable>,
    /// The cited engine the counts were derived on, or `None` when none was.
    pub engine: Option<Engine>,
}

/// The client's role for a log's.
fn role_of(role: log::Role) -> Role {
    match role {
        log::Role::System => Role::System,
        log::Role::User => Role::User,
        log::Role::Assistant => Role::Assistant,
    }
}

/// The record id of the request logged at `seq`.
fn request_id(seq: u64) -> String {
    format!("q/{seq}")
}

/// The record's spelling of the log's `timings`, or `None` when a value is
/// past what the record can spell.
fn timings_of(timings: &log::Timings) -> Option<record::Timings> {
    let count = |n: Option<u64>| match n {
        None => Some(None),
        Some(n) => Count::new(n).ok().map(Some),
    };
    let millis = |m: &Option<log::Millis>| match m {
        None => Some(None),
        Some(m) => {
            let text = m.as_str();
            match text.parse::<u64>() {
                Ok(n) if n.to_string() == text => {
                    Count::new(n).ok().map(|n| Some(record::Millis::Whole(n)))
                }
                _ => Decimal::new(text).map(|d| Some(record::Millis::Exact(d))),
            }
        }
    };
    Some(record::Timings {
        prompt_n: count(timings.prompt_n)?,
        cache_n: count(timings.cache_n)?,
        prompt_ms: millis(&timings.prompt_ms)?,
        predicted_n: count(timings.predicted_n)?,
        predicted_ms: millis(&timings.predicted_ms)?,
        draft_n: count(timings.draft_n)?,
        draft_n_accepted: count(timings.draft_n_accepted)?,
    })
}

/// A response's `output_tokens`, or why it has none: the server's own
/// `usage` count when it sent one, else `predicted_n` on a cited engine.
fn output_tokens(
    usage: Option<&log::Usage>,
    timings: Option<&log::Timings>,
    engine: Option<Engine>,
) -> Result<Count, &'static str> {
    if let Some(usage) = usage {
        return Count::new(usage.completion_tokens).map_err(|_| "a count past the record's bound");
    }
    if engine.is_none() {
        return Err("equality unmeasured for this engine");
    }
    let predicted = timings
        .and_then(|timings| timings.predicted_n)
        .ok_or("the server reported no predicted_n")?;
    Count::new(predicted).map_err(|_| "a count past the record's bound")
}

/// A turn's `prefill_tokens` from its trunk response's `timings`, on a
/// cited engine: `prompt_n + cache_n`, both as the server reported them.
fn prefill_tokens(
    timings: Option<&log::Timings>,
    engine: Option<Engine>,
) -> Result<Count, &'static str> {
    if engine.is_none() {
        return Err("equality unmeasured for this engine");
    }
    let timings = timings.ok_or("its trunk response carried no timings")?;
    let (Some(prompt), Some(cache)) = (timings.prompt_n, timings.cache_n) else {
        return Err("the server reported no prompt_n or no cache_n");
    };
    prompt
        .checked_add(cache)
        .and_then(|total| Count::new(total).ok())
        .ok_or("a count past the record's bound")
}

/// The record `lines` project to, under `regime`, on `engine`.
///
/// # Errors
///
/// When the log does not begin with its session, or a request carries no
/// `head_sha256`: a live record's request names the head it sent, and
/// record validation refuses one that does not.
pub fn project(
    lines: &[log::Line],
    regime: &Regime,
    engine: Option<Engine>,
) -> Result<Projection, String> {
    project_in(lines, regime, engine, None)
}

/// [`project`], reading each `ask`'s attached files back from `recording`,
/// the directory their `path`s are relative to (#372): a head the
/// operator's image rode in is rebuilt with it, through the one
/// [`crate::client::attach`] the loop sent it with. With no `recording`, or
/// a copy that is missing or not its digest, that head is named
/// unattributed with the reason.
///
/// # Errors
///
/// As [`project`].
pub fn project_in(
    lines: &[log::Line],
    regime: &Regime,
    engine: Option<Engine>,
    recording: Option<&std::path::Path>,
) -> Result<Projection, String> {
    let Some(log::Line {
        event:
            Line::SessionStart {
                model,
                head,
                tools,
                template_kwargs,
                fork_delivery,
                tool_output,
                ..
            },
        ..
    }) = lines.first()
    else {
        return Err("the log does not begin with its session".to_owned());
    };
    let substrate = regime
        .substrates
        .first()
        .map(|substrate| substrate.id.clone())
        .ok_or("the regime declares no substrate")?;
    let mut walk = Walk::over(lines, substrate, engine);
    walk.recording = recording.map(std::path::Path::to_path_buf);
    walk.tools = tools_of(tools.as_deref().unwrap_or_default());
    walk.template_kwargs = kwargs_of(template_kwargs.as_ref());
    walk.model.clone_from(model);
    walk.head = head
        .iter()
        .map(|message| Message::new(role_of(message.role), message.content.clone()))
        .collect();
    walk.trunk.clone_from(&walk.head);
    for line in &lines[1..] {
        walk.line(line)?;
    }
    let mut events = vec![Event::Start {
        regime: Box::new(regime.clone()),
        source: Source::Live,
        regimen_sha256: None,
        // The fork delivery lever's state, as `session.start` names it.
        fork_delivery: *fork_delivery,
        // The tool output cap, as `session.start` names it (#554).
        tool_output: *tool_output,
        levers: None,
    }];
    events.extend(walk.events);
    Ok(Projection {
        events,
        unspellable: walk.unspellable,
        engine,
    })
}

/// The walk over a log's lines after its first.
struct Walk<'a> {
    /// What each request (by `seq`) came to: its response, cancel or failure.
    outcome: BTreeMap<u64, &'a Line>,
    /// Each turn's trunk request, by `seq`.
    trunk_of: BTreeMap<u32, u64>,
    substrate: String,
    engine: Option<Engine>,
    events: Vec<Event>,
    unspellable: Vec<Unspellable>,
    /// Whether a turn row could not be spelled: turn rows run from 1
    /// without a gap, so none after it can be either.
    turns_broken: bool,
    /// The kinds with no row at all, named once each.
    named_kinds: BTreeSet<&'static str>,
    /// The model the session's requests name, from its first line.
    model: String,
    /// The trunk as the session holds it: its head, then each answered
    /// turn's ask and answer -- or, after a seam, the head refilled from the
    /// seam's render (v6, #493).
    trunk: Vec<Message>,
    /// The session's head, from `session.start`: what a seam refills from.
    head: Vec<Message>,
    /// Each turn's ask.
    asks: BTreeMap<u32, String>,
    /// Each trunk request's turn, by `seq`.
    turn_of: BTreeMap<u64, u32>,
    /// The trunk's last request: its logged digest, and its head rebuilt
    /// from the log when that gave the same digest.
    last_head: Option<(String, Option<Head>)>,
    /// The forks with a row, by `seq` (v5, #374).
    forks: BTreeSet<u64>,
    /// Each fork's capture row, by the fork's `seq`: its place in `events`.
    captures: BTreeMap<u64, usize>,
    /// The directory an `ask`'s attached files are read back from (#372).
    recording: Option<std::path::PathBuf>,
    /// Each turn's attached files, from its `ask` line.
    ask_files: BTreeMap<u32, Vec<log::RecordedFile>>,
    /// Each turn's delivered note (v7), from its `delivered` line.
    notes: BTreeMap<u32, String>,
    /// The tools the session's requests declared, rebuilt from
    /// `session.start`'s names (#472), or why they cannot be.
    tools: Result<Vec<ToolDefinition>, String>,
    /// The template variables every request sent, from `session.start`
    /// (v7, R1).
    template_kwargs: BTreeMap<String, crate::formats::record::json::Value>,
    /// The trunk requests that were tool steps: a `tool_call` line cites
    /// them.
    stepped: BTreeSet<u64>,
    /// What each turn cancelled while generating had said, from its
    /// `cancelled` line (#575).
    cancelled_text: BTreeMap<u32, String>,
    /// Each turn's tool steps so far, in order: what the session sent after
    /// the ask, and what it put back on the trunk.
    steps: BTreeMap<u32, Vec<Step>>,
    /// Why the trunk stopped being rebuildable, from the first turn whose
    /// attachment could not be read back: every later head names it (#471's
    /// review, NB3).
    trunk_unrebuilt: Option<String>,
    /// Each lane but the trunk's last logged head digest (v5, #374): a live
    /// record names every move of a lane's head, and an interview head moves
    /// with the trunk it is cut from.
    side_heads: BTreeMap<Lane, String>,
}

/// One tool step of a turn (#472): what the model said, its calls, and what
/// each was given back -- the messages the session sent after it.
struct Step {
    /// The step's request, by `seq`.
    request: u64,
    /// The step's response: its text and reasoning.
    said: Message,
    /// Its calls, in order, each with the result message it was shown, when
    /// it was shown anything: `read`'s images on it, read back (#557).
    calls: Vec<(ToolCall, Option<Message>)>,
    /// Why a result's image could not be read back, from the first that
    /// could not: every head that carries the step names it (#557).
    unrebuilt: Option<String>,
}

impl Step {
    /// The messages the session sent after this step: the assistant's call
    /// message, then a result for each call that was shown one.
    fn messages(&self) -> Vec<Message> {
        let mut said = self.said.clone();
        said.tool_calls = self.calls.iter().map(|(call, _)| call.clone()).collect();
        let mut messages = vec![said];
        messages.extend(self.calls.iter().filter_map(|(_, shown)| shown.clone()));
        messages
    }
}

/// The template variables `session.start` says every request sent (R1), as
/// a request carries them.
fn kwargs_of(
    kwargs: Option<&log::TemplateKwargs>,
) -> BTreeMap<String, crate::formats::record::json::Value> {
    use crate::formats::record::json::Value;
    let mut sent = BTreeMap::new();
    if let Some(kwargs) = kwargs {
        if let Some(thinking) = kwargs.enable_thinking {
            sent.insert("enable_thinking".to_owned(), Value::Boolean(thinking));
        }
        if let Some(effort) = &kwargs.reasoning_effort {
            sent.insert("reasoning_effort".to_owned(), Value::String(effort.clone()));
        }
        if let Some(preserve) = kwargs.preserve_thinking {
            sent.insert("preserve_thinking".to_owned(), Value::Boolean(preserve));
        }
    }
    sent
}

/// The tools `session.start` names, as the session declared them (#472):
/// each name to the definition its requests carried. The head's digest is
/// the check: a definition that differs from what was sent leaves the head
/// unverified, never wrongly verified.
fn tools_of(names: &[String]) -> Result<Vec<ToolDefinition>, String> {
    names
        .iter()
        .map(|name| {
            if name == super::tool_loop::BASH {
                Ok(super::tool_loop::bash_tool())
            } else if let Some(tool) = super::standard::definitions()
                .into_iter()
                .find(|tool| tool.name == *name)
            {
                Ok(tool)
            } else {
                Err(format!(
                    "the session declared `{name}`, a tool with no definition here"
                ))
            }
        })
        .collect()
}

/// `tools` with `bash` as it was declared before its description (#558),
/// when `tools` declares `bash`.
fn earlier_bash(tools: &[ToolDefinition]) -> Option<Vec<ToolDefinition>> {
    let at = tools
        .iter()
        .position(|tool| tool.name == super::tool_loop::BASH)?;
    let mut earlier = tools.to_vec();
    earlier[at] = super::tool_loop::bash_tool_before_its_description();
    Some(earlier)
}

impl<'a> Walk<'a> {
    fn over(lines: &'a [log::Line], substrate: String, engine: Option<Engine>) -> Self {
        let mut outcome = BTreeMap::new();
        let mut trunk_of = BTreeMap::new();
        let mut stepped = BTreeSet::new();
        for line in lines {
            match &line.event {
                Line::Request {
                    turn,
                    lane: Lane::Trunk,
                    ..
                } => {
                    trunk_of.entry(*turn).or_insert(line.seq);
                }
                Line::Response { to_request, .. } => {
                    outcome.insert(*to_request, &line.event);
                }
                Line::ToolCall { request, .. } => {
                    stepped.insert(*request);
                }
                Line::Cancelled { request, .. } | Line::RequestFailed { request, .. } => {
                    outcome.insert(*request, &line.event);
                }
                _ => {}
            }
        }
        Self {
            outcome,
            trunk_of,
            substrate,
            engine,
            events: Vec::new(),
            unspellable: Vec::new(),
            turns_broken: false,
            named_kinds: BTreeSet::new(),
            model: String::new(),
            trunk: Vec::new(),
            head: Vec::new(),
            asks: BTreeMap::new(),
            turn_of: BTreeMap::new(),
            last_head: None,
            forks: BTreeSet::new(),
            captures: BTreeMap::new(),
            recording: None,
            ask_files: BTreeMap::new(),
            notes: BTreeMap::new(),
            tools: Ok(Vec::new()),
            template_kwargs: BTreeMap::new(),
            stepped,
            cancelled_text: BTreeMap::new(),
            steps: BTreeMap::new(),
            trunk_unrebuilt: None,
            side_heads: BTreeMap::new(),
        }
    }

    fn name(&mut self, seq: u64, kind: &'static str, why: String, text: Option<String>) {
        self.unspellable.push(Unspellable {
            seq,
            kind,
            why,
            text,
        });
    }

    fn line(&mut self, line: &log::Line) -> Result<(), String> {
        match &line.event {
            Line::Ask {
                turn, text, files, ..
            } => {
                self.asks.insert(*turn, text.clone());
                self.ask(line.seq, *turn, files.as_ref());
            }
            Line::Request {
                turn,
                lane,
                head_sha256,
                ..
            } => self.request(line.seq, *turn, *lane, head_sha256.as_deref())?,
            // A capped call is not an answer (#290, ruled 5969297103): no
            // row, its text named, and nothing put on the rebuilt trunk.
            Line::Response {
                text,
                capped: Some(true),
                ..
            } => self.name(
                line.seq,
                "response",
                "a capped call: its output cap ended it before an answer, and its turn \
                 settled failed"
                    .to_owned(),
                Some(text.clone()),
            ),
            Line::Response {
                to_request,
                text,
                reasoning,
                timings,
                usage,
                ..
            } => {
                self.answered(*to_request, text, reasoning.as_ref());
                self.response(
                    line.seq,
                    *to_request,
                    text,
                    timings.as_ref(),
                    usage.as_ref(),
                );
            }
            Line::Cancelled {
                request, partial, ..
            } => self.cancelled_call(line.seq, *request, partial),
            Line::RequestFailed {
                reason, message, ..
            } => self.name(
                line.seq,
                "request.failed",
                format!(
                    "a failed call ({}): the record has no row for one",
                    reason.tag()
                ),
                Some(message.clone()),
            ),
            // A fork (v5, #374): its row, and its patches as one capture row.
            Line::Fork { lane, of_turn, .. } => self.fork(line.seq, *lane, *of_turn),
            Line::Patch { fork, .. } => self.patch(line.seq, *fork),
            // The record's fork row carries no outcome: a fork that settled
            // `value` is carried by its capture row, and any other outcome is
            // named with its word.
            Line::ForkSettled { fork, outcome } => self.fork_settled(line.seq, *fork, *outcome),
            // A seam (v6, #493): its row, and the trunk refilled exactly as
            // the session refills it, so the next request's head is rebuilt
            // and checked like any other.
            Line::Seam { .. } => self.seam(line),
            // Forks' patches delivered (v7): the note follows its turn's ask
            // on the rebuilt trunk, as it does on the session's.
            Line::Delivered { turn, text, .. } => {
                self.notes.insert(*turn, text.clone());
            }
            // Facts the record has no row for at all, named once per kind.
            Line::IdleGap { .. } | Line::Refused { .. } | Line::Progress { .. } => {
                let kind = match &line.event {
                    Line::IdleGap { .. } => log::Kind::IdleGap,
                    Line::Refused { .. } => log::Kind::Refused,
                    _ => log::Kind::Progress,
                }
                .tag();
                if self.named_kinds.insert(kind) {
                    self.name(
                        line.seq,
                        kind,
                        format!("the record has no row for a `{kind}` line"),
                        None,
                    );
                }
            }
            // A tool call (v3, #302): its row carries how it ran under the
            // log's own words, so the record says what confined it.
            Line::ToolCall { .. }
            | Line::TurnSettled {
                reason: log::SettleReason::MaxSteps,
                ..
            } => self.stepping(line),
            // A turn that failed after a step keeps the steps that completed,
            // as the session does (`State::keep_ran_steps`).
            Line::TurnSettled {
                turn,
                reason:
                    reason @ (log::SettleReason::Failed
                    | log::SettleReason::Timeout
                    | log::SettleReason::Cancelled),
            } => self.cut_short(*turn, *reason),
            // Carried by the rows above: a delta by its response's text, a
            // settlement and a settled turn by the turn and response rows.
            Line::Delta { .. }
            | Line::Settlement { .. }
            | Line::StopAsked { .. }
            | Line::TurnSettled { .. }
            | Line::SessionStart { .. } => {}
        }
        Ok(())
    }

    /// [`Self::tool_call`], from a `tool_call` line.
    fn tool_call_line(&mut self, seq: u64, line: &Line) {
        let Line::ToolCall {
            turn,
            name,
            arguments,
            outcome,
            argv,
            confined,
            isolation,
            network,
            exit,
            reason,
            policy,
            stdout,
            cwd,
            approval,
            files,
            ..
        } = line
        else {
            return;
        };
        self.tool_call(
            seq,
            *turn,
            name,
            arguments,
            Execution {
                outcome: *outcome,
                reason: *reason,
                argv: argv.clone(),
                confined: confined.clone(),
                isolation: *isolation,
                network: *network,
                policy: policy.clone(),
                cwd: cwd.clone(),
                approval: approval.clone(),
                files: files.clone(),
            },
            *exit,
            stdout.as_ref().map(|out| out.text.clone()),
        );
    }

    /// A tool call's row, `t/<seq>`, under the turn that made it. Named
    /// rather than written when that turn has no row (the record ties a call
    /// to its turn) or its exit does not fit a row's; its arguments are named
    /// beside the row when they are not a JSON object, which `args` holds.
    #[allow(clippy::too_many_arguments)] // one row's fields, as the line holds them
    fn tool_call(
        &mut self,
        seq: u64,
        turn: u32,
        name: &str,
        arguments: &str,
        exec: Execution,
        exit: Option<u64>,
        output: Option<String>,
    ) {
        let turned = self
            .events
            .iter()
            .any(|event| matches!(event, Event::Turn { index, .. } if *index == turn));
        if !turned {
            self.name(
                seq,
                "tool_call",
                format!("a tool call of turn {turn}, which has no row"),
                None,
            );
            return;
        }
        let args = record::json::line(arguments).ok();
        if args.is_none() {
            self.name(
                seq,
                "tool_call",
                "a tool call's arguments that are not a JSON object: its row carries no `args`"
                    .to_owned(),
                Some(arguments.to_owned()),
            );
        }
        let exit = match exit.map(i64::try_from) {
            Some(Err(_)) => {
                self.name(seq, "tool_call", "an exit status past i64".to_owned(), None);
                return;
            }
            Some(Ok(code)) => Some(code),
            None => None,
        };
        self.events.push(Event::ToolCall {
            id: format!("t/{seq}"),
            at_turn: turn,
            tool: name.to_owned(),
            args,
            exit,
            output,
            exec: Some(exec),
        });
    }

    /// A fork's row, `f/<seq>`, off the turn it follows: served on the
    /// regime's substrate, like every row this projection writes. Named
    /// rather than written when that turn has no row.
    fn fork(&mut self, seq: u64, lane: Lane, of_turn: u32) {
        let turned = self
            .events
            .iter()
            .any(|event| matches!(event, Event::Turn { index, .. } if *index == of_turn));
        if !turned {
            self.name(
                seq,
                "fork",
                format!("a fork of turn {of_turn}, which has no row"),
                None,
            );
            return;
        }
        self.forks.insert(seq);
        self.events.push(Event::Fork {
            id: format!("f/{seq}"),
            lane: lane.tag().to_owned(),
            substrate: self.substrate.clone(),
            of_turn,
        });
    }

    /// A seam's row, `s/<seq>`, at the turn it follows, and the trunk
    /// refilled from `render` and the section of tool outputs it carried
    /// through [`crate::seam::render::refill`]. The
    /// row is named rather than written when that turn has no row; the
    /// trunk is refilled either way, since the session's was.
    fn seam(&mut self, line: &log::Line) {
        let Line::Seam {
            at_turn,
            render,
            tail_tokens,
            outputs,
            ..
        } = &line.event
        else {
            return;
        };
        let (seq, at_turn, tail_tokens) = (line.seq, *at_turn, tail_tokens.unwrap_or(0));
        // The tail the session kept after the refill (#552), cut from the
        // rebuilt trunk by the same function.
        let turns = self.trunk.get(self.head.len()..).unwrap_or_default();
        let kept = crate::seam::render::tail(turns, tail_tokens).to_vec();
        let carried_turns = kept
            .iter()
            .filter(|message| message.role == Role::User)
            .count() as u64;
        let carried_tokens = kept
            .iter()
            .map(crate::seam::render::estimated_tokens)
            .sum::<u64>();
        // The section the seam carried of the outputs it compacted away
        // (#553), after the render, as sent.
        let sent = outputs
            .as_ref()
            .map_or_else(|| render.clone(), |section| format!("{render}{section}"));
        self.trunk = crate::seam::render::refill(&self.head, &sent);
        self.trunk.extend(kept);
        // An attachment the old trunk carried and could not be read back is
        // not on the refilled one.
        self.trunk_unrebuilt = None;
        let turned = self
            .events
            .iter()
            .any(|event| matches!(event, Event::Turn { index, .. } if *index == at_turn));
        if !turned {
            self.name(
                seq,
                "seam",
                format!("a seam at turn {at_turn}, which has no row"),
                None,
            );
            return;
        }
        let Ok(rendered_bytes) = Count::new(render.len() as u64) else {
            self.name(
                seq,
                "seam",
                "a render past the record's bound".to_owned(),
                None,
            );
            return;
        };
        let tailed = tail_tokens > 0;
        self.events.push(Event::Seam {
            id: format!("s/{seq}"),
            at_turn,
            rendered_bytes,
            tail_tokens: tailed.then_some(tail_tokens),
            carried_turns: tailed.then_some(carried_turns),
            carried_tokens: tailed.then_some(carried_tokens),
        });
    }

    /// A patch, counted on its fork's capture row, `c/<fork>`: one entry per
    /// patch line, the row written at the fork's first patch.
    fn patch(&mut self, seq: u64, fork: u64) {
        if !self.forks.contains(&fork) {
            self.name(
                seq,
                "patch",
                format!("a patch of fork {fork}, which has no row"),
                None,
            );
            return;
        }
        if let Some(&at) = self.captures.get(&fork) {
            if let Some(Event::Capture { entries, .. }) = self.events.get_mut(at) {
                *entries += 1;
            }
            return;
        }
        self.captures.insert(fork, self.events.len());
        self.events.push(Event::Capture {
            id: format!("c/{fork}"),
            from_fork: format!("f/{fork}"),
            entries: 1,
        });
    }

    fn ask(&mut self, seq: u64, turn: u32, files: Option<&Vec<log::RecordedFile>>) {
        if let Some(files) = files {
            self.ask_files.insert(turn, files.clone());
        }
        let trunk_timings =
            self.trunk_of
                .get(&turn)
                .and_then(|request| match self.outcome.get(request) {
                    // A capped call's prompt was still prefilled: the cap
                    // bounds the output, not the prompt (#313's review).
                    Some(Line::Response { timings, .. }) => timings.as_ref(),
                    _ => None,
                });
        match (
            self.turns_broken,
            prefill_tokens(trunk_timings, self.engine),
        ) {
            (false, Ok(prefill_tokens)) => {
                self.events.push(Event::Turn {
                    index: turn,
                    prefill_tokens,
                    files: files.cloned(),
                });
                return;
            }
            (false, Err(why)) => {
                self.turns_broken = true;
                self.name(
                    seq,
                    "ask",
                    format!("turn {turn}'s prefill_tokens: {why}"),
                    None,
                );
            }
            (true, _) => self.name(
                seq,
                "ask",
                format!(
                    "turn {turn} follows a turn the record could not spell, and turn rows run \
                     from 1 without a gap"
                ),
                None,
            ),
        }
        // Not written on a row: the files the operator attached are named,
        // so a reference the record could not hold is not lost.
        if let Some(files) = files {
            let named: Vec<String> = files
                .iter()
                .map(|file| format!("{} sha256 {}", file.path, file.sha256))
                .collect();
            self.name(
                seq,
                "ask",
                format!(
                    "turn {turn}'s attached files ({}), which have no turn row to ride on",
                    named.join(", ")
                ),
                None,
            );
        }
    }

    fn request(
        &mut self,
        seq: u64,
        turn: u32,
        lane: Lane,
        head_sha256: Option<&str>,
    ) -> Result<(), String> {
        let Some(head_sha256) = head_sha256 else {
            return Err(format!(
                "the request at seq {seq} carries no head_sha256, and a live record's request \
                 names the head it sent"
            ));
        };
        self.events.push(Event::Request {
            id: request_id(seq),
            lane: lane.tag().to_owned(),
            substrate: self.substrate.clone(),
            retry_of: None,
            text: None,
            head_sha256: Some(head_sha256.to_owned()),
        });
        if lane == Lane::Trunk {
            self.turn_of.insert(seq, turn);
            self.head_change(seq, turn, head_sha256);
        } else {
            self.side_head_change(seq, lane, head_sha256);
        }
        if !self.outcome.contains_key(&seq) {
            self.name(
                seq,
                "request",
                "a request with no outcome in the log".to_owned(),
                None,
            );
        }
        Ok(())
    }

    /// The trunk request at `seq`'s head, rebuilt from the log and checked
    /// against its logged digest, and the change row from the trunk's last
    /// request when the head moved.
    fn head_change(&mut self, seq: u64, turn: u32, logged: &str) {
        let mut messages = self.trunk.clone();
        let asked = self.user_message(turn);
        let note = self.notes.get(&turn).cloned();
        let unrebuilt = asked
            .as_ref()
            .err()
            .map(|why| format!("turn {turn}'s attachment: {why}"))
            .or_else(|| self.tools.as_ref().err().cloned())
            .or_else(|| self.trunk_unrebuilt.clone())
            .or_else(|| {
                self.steps
                    .get(&turn)
                    .into_iter()
                    .flatten()
                    .find_map(|step| step.unrebuilt.clone())
            });
        messages.push(asked.unwrap_or_else(|_| Message::new(Role::User, String::new())));
        if let Some(note) = note {
            messages.push(Message::new(Role::User, note));
        }
        for step in self.steps.get(&turn).into_iter().flatten() {
            messages.extend(step.messages());
        }
        let tools = self.tools.clone().unwrap_or_default();
        let head = |tools: Vec<ToolDefinition>| {
            Head::of(&RequestShape {
                model: self.model.clone(),
                messages: messages.clone(),
                sampler: SamplerCard::empty(),
                limits: Limits {
                    attempt: std::time::Duration::ZERO,
                    call: std::time::Duration::ZERO,
                    max_output_tokens: 0,
                    retries: 0,
                    context_window: None,
                },
                grammar: None,
                // From `session.start` (R1), and ASSERTED by the digest check
                // below: a kwarg the log cannot carry leaves the head unverified.
                template_kwargs: self.template_kwargs.clone(),
                // The session's declared tools, from `session.start` (#472).
                tools,
            })
        };
        // A log written before `bash` carried its description (#558) sent
        // the definition I0 captured: its heads rebuild with that one, and
        // the digest still decides.
        let verified = std::iter::once(tools.clone())
            .chain(earlier_bash(&tools))
            .map(head)
            .find(|rebuilt| rebuilt.digest() == logged);
        // A head whose attachment could not be read back is never verified,
        // whatever the digest of what the rebuild could reach.
        let verified = verified.filter(|_| unrebuilt.is_none());
        if let Some(why) = unrebuilt {
            self.name(
                seq,
                "request",
                format!(
                    "its head could not be rebuilt from the log: {why}; a change at it is \
                     unattributed"
                ),
                None,
            );
        } else if verified.is_none() {
            self.name(
                seq,
                "request",
                format!(
                    "its head could not be rebuilt from the log: the logged {logged} is not what \
                     client::head gives over the log's trunk, so a change at it is unattributed"
                ),
                None,
            );
        }
        let previous = self
            .last_head
            .replace((logged.to_owned(), verified.clone()));
        let Some((previous_digest, previous_head)) = previous else {
            return;
        };
        if previous_digest == logged {
            return;
        }
        let id = format!("{}#prefix", request_id(seq));
        let at_request = request_id(seq);
        let row = match (previous_head, verified) {
            (Some(previous), Some(now)) => now
                .change_from(&previous)
                .map(|change| change.event(id.clone(), at_request.clone())),
            _ => None,
        };
        self.events.push(row.unwrap_or(Event::PrefixChanged {
            id,
            at_request,
            reason: PrefixReason::Unattributed,
            diff: Vec::new(),
        }));
    }

    /// A side lane's head moving (v5, #374): named as unattributed, since the
    /// log holds an interview request's digest and not the head it hashed --
    /// the trunk's warm tail and the fork's question -- so the move cannot be
    /// rebuilt and attributed as the trunk's is.
    fn side_head_change(&mut self, seq: u64, lane: Lane, logged: &str) {
        let Some(previous) = self.side_heads.insert(lane, logged.to_owned()) else {
            return;
        };
        if previous == logged {
            return;
        }
        self.events.push(Event::PrefixChanged {
            id: format!("{}#prefix", request_id(seq)),
            at_request: request_id(seq),
            reason: PrefixReason::Unattributed,
            diff: Vec::new(),
        });
    }

    /// Turn `turn`'s user message as the session sent it: the ask's words,
    /// and each attached file read back from the recording and attached
    /// through [`crate::client::attach`], in the line's order (#372).
    fn user_message(&self, turn: u32) -> Result<Message, String> {
        let mut message = Message::new(
            Role::User,
            self.asks.get(&turn).cloned().unwrap_or_default(),
        );
        let Some(files) = self.ask_files.get(&turn) else {
            return Ok(message);
        };
        let Some(recording) = &self.recording else {
            return Err("no recording directory to read it back from".to_owned());
        };
        for file in files {
            let bytes = std::fs::read(recording.join(&file.path))
                .map_err(|why| format!("{}: {why}", file.path))?;
            message = crate::client::attach(message, file, &bytes)
                .map_err(|why| format!("{}: {why}", file.path))?;
        }
        Ok(message)
    }

    /// The trunk after an answered trunk request: its ask and its answer,
    /// appended as the session appends them.
    fn answered(&mut self, to_request: u64, text: &str, reasoning: Option<&String>) {
        let Some(turn) = self.turn_of.get(&to_request) else {
            return;
        };
        let turn = *turn;
        // A tool step: the session goes on with it, and puts it on the
        // trunk only when the turn settles (#472).
        if self.stepped.contains(&to_request) {
            let mut said = Message::new(Role::Assistant, text.to_owned());
            said.reasoning = reasoning.cloned();
            self.steps.entry(turn).or_default().push(Step {
                request: to_request,
                said,
                calls: Vec::new(),
                unrebuilt: None,
            });
            return;
        }
        self.onto_the_trunk(turn, usize::MAX);
        let mut answer = Message::new(Role::Assistant, text.to_owned());
        answer.reasoning = reasoning.cloned();
        self.trunk.push(answer);
    }

    /// Turn `turn`'s exchange onto the trunk, as the session puts it there:
    /// the user message, then its first `steps` tool steps (#472). A final
    /// answer keeps every step; a `max_steps` settle keeps all but the last.
    fn onto_the_trunk(&mut self, turn: u32, steps: usize) {
        let asked = self.user_message(turn).unwrap_or_else(|why| {
            self.trunk_unrebuilt.get_or_insert_with(|| {
                format!("the trunk carries turn {turn}'s attachment: {why}")
            });
            Message::new(
                Role::User,
                self.asks.get(&turn).cloned().unwrap_or_default(),
            )
        });
        self.trunk.push(asked);
        if let Some(note) = self.notes.get(&turn) {
            self.trunk.push(Message::new(Role::User, note.clone()));
        }
        let taken = self.steps.remove(&turn).unwrap_or_default();
        for step in taken.iter().take(steps) {
            if let Some(why) = &step.unrebuilt {
                self.trunk_unrebuilt
                    .get_or_insert_with(|| format!("the trunk carries turn {turn}'s {why}"));
            }
            self.trunk.extend(step.messages());
        }
    }

    /// A fork's settling: the record's fork row carries no outcome, so a
    /// fork settled `value` is carried by its capture row and any other
    /// outcome is named with its word.
    fn fork_settled(&mut self, seq: u64, fork: u64, outcome: log::ForkOutcome) {
        if outcome != log::ForkOutcome::Value {
            self.name(
                seq,
                "fork.settled",
                format!(
                    "fork f/{fork} settled `{}`: the record's fork row carries no outcome, and \
                     only a fork settled `value` captures",
                    outcome.tag()
                ),
                None,
            );
        }
    }

    /// A tool step's lines (#472): a `tool_call` -- onto its step, and its
    /// row -- or a `max_steps` settle, which puts the steps that ran on the
    /// trunk but not the last.
    fn stepping(&mut self, line: &log::Line) {
        if let Line::TurnSettled { turn, .. } = &line.event {
            let ran = self.steps.get(turn).map_or(0, Vec::len).saturating_sub(1);
            self.onto_the_trunk(*turn, ran);
            return;
        }
        self.step_call(&line.event);
        self.tool_call_line(line.seq, &line.event);
    }

    /// A turn settled `failed` or `timeout`: the steps the turn went on from
    /// -- every step but one whose request was the turn's last -- join the
    /// trunk with its ask, as the session puts them there. None completed,
    /// and nothing joins it.
    fn failed_after_steps(&mut self, turn: u32) {
        let last_request = self
            .turn_of
            .iter()
            .filter(|(_, of)| **of == turn)
            .map(|(request, _)| *request)
            .max();
        let steps = self.steps.get(&turn).map_or(0, Vec::len);
        let unfinished = self
            .steps
            .get(&turn)
            .and_then(|steps| steps.last())
            .is_some_and(|step| Some(step.request) == last_request);
        let completed = steps - usize::from(unfinished);
        if completed == 0 {
            self.steps.remove(&turn);
            return;
        }
        self.onto_the_trunk(turn, completed);
    }

    /// A turn that settled short of an answer: what of it joins the trunk,
    /// as the session puts it there (#541, #575).
    fn cut_short(&mut self, turn: u32, reason: log::SettleReason) {
        if reason == log::SettleReason::Cancelled {
            self.cancelled_turn(turn);
        } else {
            self.failed_after_steps(turn);
        }
    }

    /// A cancelled call: named, since the record has no row for one, and
    /// its text kept for its turn's settling (#575).
    fn cancelled_call(&mut self, seq: u64, request: u64, partial: &str) {
        if let Some(turn) = self.turn_of.get(&request) {
            self.cancelled_text.insert(*turn, partial.to_owned());
        }
        self.name(
            seq,
            "cancelled",
            "a cancelled call: the record has no row for one".to_owned(),
            Some(partial.to_owned()),
        );
    }

    /// A turn settled `cancelled` (#575): every step it took joins the trunk
    /// with its ask -- the one a cancel cut short has each call answered on
    /// its line -- and then, when the cancel came while it was generating,
    /// the text it had said. Nothing taken and nothing said, and nothing
    /// joins it.
    fn cancelled_turn(&mut self, turn: u32) {
        let steps = self.steps.get(&turn).map_or(0, Vec::len);
        let said = self
            .cancelled_text
            .remove(&turn)
            .filter(|text| !text.is_empty());
        if steps == 0 && said.is_none() {
            return;
        }
        self.onto_the_trunk(turn, usize::MAX);
        if let Some(text) = said {
            self.trunk.push(Message::new(Role::Assistant, text));
        }
    }

    /// A `tool_call` line's call, onto its step (#472).
    fn step_call(&mut self, line: &Line) {
        let Line::ToolCall {
            request,
            turn,
            id,
            name,
            arguments,
            shown,
            files,
            recovered_from,
            ..
        } = line
        else {
            return;
        };
        let result = shown
            .as_ref()
            .map(|shown| self.tool_result(id, shown, files.as_deref().unwrap_or_default()));
        if let Some(step) = self
            .steps
            .get_mut(turn)
            .and_then(|steps| steps.iter_mut().rfind(|step| step.request == *request))
        {
            let (result, why) = match result {
                Some((result, why)) => (Some(result), why),
                None => (None, None),
            };
            if step.unrebuilt.is_none() {
                step.unrebuilt = why.map(|why| format!("call {id}'s image: {why}"));
            }
            // A call recovered from the answer's text (#560): the session put
            // what was left of the answer on the trunk, and so does this --
            // the same recovery, once, at the step's first call.
            if recovered_from.is_some()
                && step.calls.is_empty()
                && let Some(recovery) = crate::client::xml_fallback::try_recover(&step.said.content)
            {
                step.said.content = recovery.remaining;
            }
            step.calls.push((
                ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: arguments.clone(),
                },
                result,
            ));
        }
    }

    /// Call `id`'s result message as the session sent it: what it was shown,
    /// and each image among the line's `files` -- `read`'s (#557), never a
    /// capped output's whole -- read back from the recording and attached
    /// through [`crate::client::attach`]; with why, when one could not be.
    fn tool_result(
        &self,
        id: &str,
        shown: &str,
        files: &[log::RecordedFile],
    ) -> (Message, Option<String>) {
        let mut message = Message::tool_result(id.to_owned(), shown.to_owned());
        for file in files
            .iter()
            .filter(|file| file.media_type.starts_with("image/"))
        {
            let Some(recording) = &self.recording else {
                return (
                    message,
                    Some("no recording directory to read it back from".to_owned()),
                );
            };
            let attached = std::fs::read(recording.join(&file.path))
                .map_err(|why| format!("{}: {why}", file.path))
                .and_then(|bytes| {
                    crate::client::attach(message.clone(), file, &bytes)
                        .map_err(|why| format!("{}: {why}", file.path))
                });
            match attached {
                Ok(attached) => message = attached,
                Err(why) => return (message, Some(why)),
            }
        }
        (message, None)
    }

    fn response(
        &mut self,
        seq: u64,
        to_request: u64,
        text: &str,
        timings: Option<&log::Timings>,
        usage: Option<&log::Usage>,
    ) {
        match output_tokens(usage, timings, self.engine) {
            Ok(output_tokens) => self.events.push(Event::Response {
                id: format!("{}#response", request_id(to_request)),
                to_request: request_id(to_request),
                output_tokens,
                text: Some(text.to_owned()),
                timings: timings.and_then(timings_of),
            }),
            Err(why) => self.name(
                seq,
                "response",
                format!("output_tokens: {why}"),
                Some(text.to_owned()),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::log::{
        FailReason, HeadMessage, Lane, Role, SettleReason, State, Timings, Usage, VERSION,
    };
    use crate::formats::record::Record;

    fn regime() -> Regime {
        let regimen =
            crate::formats::regimen::parse(crate::drive::canned::DEV_LOOP).expect("a regimen");
        crate::drive::regimen::regime_of(&regimen, false).expect("a regime")
    }

    fn head() -> String {
        "a".repeat(64)
    }

    fn numbered(events: Vec<Line>) -> Vec<log::Line> {
        events
            .into_iter()
            .enumerate()
            .map(|(seq, event)| log::Line {
                seq: seq as u64,
                t: seq as u64 * 5,
                event,
            })
            .collect()
    }

    fn start() -> Line {
        Line::SessionStart {
            version: VERSION,
            opened: 1_790_000_000_000,
            model: "a-model".to_owned(),
            head: vec![HeadMessage {
                role: Role::System,
                content: "you are the trunk".to_owned(),
            }],
            serving: None,
            claim: None,
            provenance: None,
            tools: None,
            template_kwargs: None,
            unsent: None,
            approvals_off: None,
            fork_delivery: None,
            reasoning_effort_default: None,
            instruction_files: None,
            tool_output: None,
        }
    }

    /// The warm turn-2 capture's counts (`e7051ef`): 18 prefilled, 160 from
    /// the cache, 66 generated.
    fn warm() -> Timings {
        Timings {
            prompt_n: Some(18),
            cache_n: Some(160),
            predicted_n: Some(66),
            ..Timings::default()
        }
    }

    /// One answered turn, its request at `request`.
    fn answered(
        turn: u32,
        request: u64,
        timings: Option<Timings>,
        usage: Option<Usage>,
    ) -> Vec<Line> {
        vec![
            Line::Ask {
                turn,
                text: "say hello".to_owned(),
                scoping: None,
                files: None,
            },
            Line::Settlement {
                from: State::Awaiting,
                to: State::Turn,
            },
            Line::Request {
                turn,
                lane: Lane::Trunk,
                head_sha256: Some(head()),
                fork: None,
                max_tokens: None,
            },
            Line::Delta {
                request,
                piece: log::Piece::Text("Hello".to_owned()),
            },
            Line::Response {
                to_request: request,
                text: "Hello".to_owned(),
                finish_reason: Some("stop".to_owned()),
                reasoning: None,
                timings,
                usage,
                capped: None,
            },
            Line::TurnSettled {
                turn,
                reason: SettleReason::Final,
            },
            Line::Settlement {
                from: State::Turn,
                to: State::Awaiting,
            },
        ]
    }

    /// [`warm`], as the client's transport reports it.
    fn warm_client() -> crate::client::stream::Timings {
        crate::client::stream::Timings {
            prompt_n: Some(18),
            cache_n: Some(160),
            predicted_n: Some(66),
            ..crate::client::stream::Timings::default()
        }
    }

    fn validates(projection: &Projection) {
        let rendered = record::render(&Record {
            events: projection.events.clone(),
        });
        record::parse(&rendered).unwrap_or_else(|why| panic!("{why:?}\n{rendered}"));
    }

    #[test]
    fn on_a_cited_engine_the_counts_derive_from_timings() {
        let projection = project(
            &a_real_session_log_timed(Some(warm_client())),
            &regime(),
            Some(Engine::Commit("e7051ef")),
        )
        .expect("projected");
        assert!(
            projection.unspellable.is_empty(),
            "{:?}",
            projection.unspellable
        );
        let turns: Vec<(u32, u64)> = projection
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Turn {
                    index,
                    prefill_tokens,
                    ..
                } => Some((*index, prefill_tokens.get())),
                _ => None,
            })
            .collect();
        assert_eq!(turns, [(1, 178), (2, 178)], "prompt_n + cache_n");
        let outputs: Vec<(String, u64)> = projection
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Response {
                    to_request,
                    output_tokens,
                    ..
                } => Some((to_request.clone(), output_tokens.get())),
                _ => None,
            })
            .collect();
        assert_eq!(
            outputs
                .iter()
                .map(|(_, output)| *output)
                .collect::<Vec<_>>(),
            [66, 66],
            "predicted_n: {outputs:?}"
        );
        validates(&projection);
    }

    #[test]
    fn on_an_uncited_engine_no_count_is_derived_and_every_one_is_named() {
        let projection = project(
            &a_real_session_log_timed(Some(warm_client())),
            &regime(),
            None,
        )
        .expect("projected");
        assert!(
            !projection
                .events
                .iter()
                .any(|event| matches!(event, Event::Turn { .. } | Event::Response { .. })),
            "never a derived number: {:?}",
            projection.events
        );
        let named: Vec<(&str, bool)> = projection
            .unspellable
            .iter()
            .map(|item| (item.kind, item.text.is_some()))
            .collect();
        assert_eq!(
            named,
            [
                ("ask", false),
                ("response", true),
                ("ask", false),
                ("response", true)
            ],
            "each turn and each response, the answer's text kept"
        );
        assert!(
            projection.unspellable[1]
                .why
                .contains("equality unmeasured for this engine")
        );
        validates(&projection);
    }

    /// A seam (log v6, #493) is the record's `seam` row at its turn, its
    /// `rendered_bytes` the render's length.
    #[test]
    fn a_seam_is_a_seam_row_at_its_turn() {
        let render = "# working set\nd1\ta tracker\n";
        let mut events = vec![start()];
        events.extend(answered(1, 3, Some(warm()), None));
        events.push(Line::Seam {
            at_turn: 1,
            reason: log::SeamReason::Operator,
            prefix_hash_before: "a".repeat(64),
            prefix_hash_after: "b".repeat(64),
            frame: crate::seam::render::FRAME_VERSION.to_owned(),
            render: render.to_owned(),
            carried_entries: 1,
            carried_turns: 0,
            tail_tokens: None,
            carried_tokens: None,
            tool_outputs: None,
            outputs: None,
            carried_outputs: None,
            carried_output_bytes: None,
        });
        let projection = project(
            &numbered(events),
            &regime(),
            Some(Engine::Commit("e7051ef")),
        )
        .expect("projected");
        assert!(
            projection.events.iter().any(|event| matches!(
                event,
                Event::Seam { at_turn: 1, rendered_bytes, .. }
                    if rendered_bytes.get() == render.len() as u64
            )),
            "{:#?}",
            projection.events
        );
        validates(&projection);
    }

    /// A seam that kept a tail (#552): its row names the depth and what
    /// the projection's own cut of the rebuilt trunk kept -- the turn before
    /// it, whole -- and the record validates.
    #[test]
    fn a_seam_that_kept_a_tail_names_its_depth_and_what_it_kept() {
        let mut events = vec![start()];
        events.extend(answered(1, 3, Some(warm()), None));
        events.push(Line::Seam {
            at_turn: 1,
            reason: log::SeamReason::Operator,
            prefix_hash_before: "a".repeat(64),
            prefix_hash_after: "b".repeat(64),
            frame: crate::seam::render::FRAME_VERSION.to_owned(),
            render: "# working set\n".to_owned(),
            carried_entries: 1,
            carried_turns: 1,
            tail_tokens: Some(10_000),
            carried_tokens: Some(1),
            tool_outputs: None,
            outputs: None,
            carried_outputs: None,
            carried_output_bytes: None,
        });
        let projection = project(
            &numbered(events),
            &regime(),
            Some(Engine::Commit("e7051ef")),
        )
        .expect("projected");
        let row = projection
            .events
            .iter()
            .find_map(|event| match event {
                Event::Seam {
                    tail_tokens,
                    carried_turns,
                    carried_tokens,
                    ..
                } => Some((*tail_tokens, *carried_turns, carried_tokens.is_some())),
                _ => None,
            })
            .expect("a seam row");
        assert_eq!(row, (Some(10_000), Some(1), true));
        validates(&projection);
    }

    #[test]
    fn a_response_with_the_servers_usage_counts_by_it() {
        let mut events = vec![start()];
        events.extend(answered(
            1,
            3,
            None,
            Some(Usage {
                prompt_tokens: 12,
                completion_tokens: 2,
                cached_tokens: None,
            }),
        ));
        let projection = project(&numbered(events), &regime(), None).expect("projected");
        assert!(projection.events.iter().any(|event| matches!(
            event,
            Event::Response { output_tokens, .. } if output_tokens.get() == 2
        )));
        validates(&projection);
    }

    #[test]
    fn a_cancelled_a_failed_and_an_unanswered_call_are_named_not_dropped() {
        let mut events = vec![start()];
        for (turn, outcome) in [
            (
                1,
                Line::Cancelled {
                    request: 3,
                    partial: "Hel".to_owned(),
                    reasoning: None,
                },
            ),
            (
                2,
                Line::RequestFailed {
                    request: 7,
                    reason: FailReason::Server,
                    message: "busy".to_owned(),
                    status: Some(503),
                    partial: None,
                },
            ),
        ] {
            events.extend([
                Line::Ask {
                    turn,
                    text: "say hello".to_owned(),
                    scoping: None,
                    files: None,
                },
                Line::Settlement {
                    from: State::Awaiting,
                    to: State::Turn,
                },
                Line::Request {
                    turn,
                    lane: Lane::Trunk,
                    head_sha256: Some(head()),
                    fork: None,
                    max_tokens: None,
                },
                outcome,
            ]);
        }
        events.push(Line::Request {
            turn: 2,
            lane: Lane::Trunk,
            head_sha256: Some(head()),
            fork: None,
            max_tokens: None,
        });
        let events_len = events.len();
        let projection = project(
            &numbered(events),
            &regime(),
            Some(Engine::Commit("e7051ef")),
        )
        .expect("projected");
        let kinds: Vec<&str> = projection
            .unspellable
            .iter()
            .map(|item| item.kind)
            .collect();
        assert!(
            kinds.contains(&"cancelled") && kinds.contains(&"request.failed"),
            "{kinds:?}"
        );
        // The unanswered request by its own seq and reason: every request
        // here is also named for its unrebuildable head, so a bare "request"
        // proves nothing (#264's review).
        let unanswered = u64::try_from(events_len - 1).expect("a seq");
        assert!(
            projection
                .unspellable
                .iter()
                .any(|item| item.seq == unanswered
                    && item.kind == "request"
                    && item.why == "a request with no outcome in the log"),
            "{:?}",
            projection.unspellable
        );
        assert!(
            projection
                .unspellable
                .iter()
                .any(|item| item.text.as_deref() == Some("Hel"))
        );
        validates(&projection);
    }

    #[test]
    fn a_request_without_its_head_is_refused_rather_than_written() {
        let mut events = vec![start()];
        let mut turn = answered(1, 3, Some(warm()), None);
        turn[2] = Line::Request {
            turn: 1,
            lane: Lane::Trunk,
            head_sha256: None,
            fork: None,
            max_tokens: None,
        };
        events.extend(turn);
        let refused = project(
            &numbered(events),
            &regime(),
            Some(Engine::Commit("e7051ef")),
        )
        .expect_err("no head");
        assert!(refused.contains("head_sha256"), "{refused}");
    }

    /// A real session's log: two turns on a canned transport, ended.
    fn a_real_session_log() -> Vec<log::Line> {
        a_real_session_log_timed(None)
    }

    /// The same, each answer reporting `timings` when given.
    fn a_real_session_log_timed(timings: Option<crate::client::stream::Timings>) -> Vec<log::Line> {
        a_real_session_log_shaped(timings, BTreeMap::new())
    }

    /// The same, the trunk sending `template_kwargs`.
    fn a_real_session_log_shaped(
        timings: Option<crate::client::stream::Timings>,
        template_kwargs: BTreeMap<String, crate::formats::record::json::Value>,
    ) -> Vec<log::Line> {
        a_real_session_log_of(timings, template_kwargs, false)
    }

    /// The same, the first turn ended by its output cap when `capped`.
    fn a_real_session_log_of(
        timings: Option<crate::client::stream::Timings>,
        template_kwargs: BTreeMap<String, crate::formats::record::json::Value>,
        capped: bool,
    ) -> Vec<log::Line> {
        use crate::client::stream::{Canned, Step};
        use crate::drive::session::{Session, Settlement, line_of};
        let shape = RequestShape {
            model: "a-model".to_owned(),
            messages: vec![Message::new(
                crate::client::shape::Role::System,
                "you are the trunk",
            )],
            sampler: SamplerCard::empty(),
            limits: Limits {
                attempt: std::time::Duration::from_secs(5),
                call: std::time::Duration::from_secs(5),
                max_output_tokens: 64,
                retries: 0,
                context_window: None,
            },
            grammar: None,
            template_kwargs,
            tools: Vec::new(),
        };
        let session = Session::open(
            Canned::new([
                [
                    Step::Reasoning("thinking\n".to_owned()),
                    Step::Delta("Hello".to_owned()),
                ]
                .into_iter()
                .chain(timings.clone().map(Step::Timings))
                .chain(capped.then(|| Step::FinishReason("length".to_owned())))
                .collect::<Vec<_>>(),
                [Step::Delta("Again".to_owned())]
                    .into_iter()
                    .chain(timings.map(Step::Timings))
                    .collect::<Vec<_>>(),
            ]),
            shape,
        );
        for ask in ["say hello", "say it again"] {
            session.ask(ask, None).expect("accepted");
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while session.settlement() != Settlement::Awaiting
                && std::time::Instant::now() < deadline
            {
                let _ = session.wait_from(0, std::time::Duration::from_millis(20));
            }
        }
        assert_eq!(session.end(None), Ok(()));
        session.events_from(0).iter().map(line_of).collect()
    }

    #[test]
    fn a_real_sessions_head_changes_are_rebuilt_and_named_by_client_head() {
        let projection = project(&a_real_session_log(), &regime(), None).expect("projected");
        let changes: Vec<&Event> = projection
            .events
            .iter()
            .filter(|event| matches!(event, Event::PrefixChanged { .. }))
            .collect();
        assert_eq!(changes.len(), 1, "{:?}", projection.events);
        let Event::PrefixChanged { reason, diff, .. } = changes[0] else {
            unreachable!()
        };
        assert_ne!(
            *reason,
            PrefixReason::Unattributed,
            "the log's trunk rebuilt to the logged digest: {:?}",
            projection.unspellable
        );
        assert!(!diff.is_empty());
        assert!(
            !projection
                .unspellable
                .iter()
                .any(|item| item.why.contains("could not be rebuilt")),
            "{:?}",
            projection.unspellable
        );
        validates(&projection);
    }

    #[test]
    fn a_capped_call_is_named_not_answered_and_the_next_head_still_rebuilds() {
        // #290, ruled 5969297103: a capped call settles `failed`, stays off
        // the trunk, and is no answer -- so the record has no row for it, and
        // the next request's head, rebuilt without it, matches the log.
        let projection = project(
            &a_real_session_log_of(None, BTreeMap::new(), true),
            &regime(),
            None,
        )
        .expect("projected");
        assert!(
            projection
                .unspellable
                .iter()
                .any(|item| item.kind == "response"
                    && item.why.starts_with("a capped call")
                    && item.text.as_deref() == Some("Hello")),
            "{:?}",
            projection.unspellable
        );
        assert!(
            projection.events.iter().all(|event| !matches!(
                event,
                Event::PrefixChanged {
                    reason: PrefixReason::Unattributed,
                    ..
                }
            )),
            "{:?}",
            projection.events
        );
        validates(&projection);
    }

    #[test]
    fn on_a_cited_engine_a_capped_turn_keeps_its_prefill_and_the_next_turn_is_rowed() {
        // #313's review: the cap bounds the output, not the prompt, so a
        // capped call's reported prefill is a turn row's, and the turns after
        // it are not broken by it.
        let projection = project(
            &a_real_session_log_of(Some(warm_client()), BTreeMap::new(), true),
            &regime(),
            Some(Engine::Commit("e7051ef")),
        )
        .expect("projected");
        let turns: Vec<(u32, u64)> = projection
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Turn {
                    index,
                    prefill_tokens,
                    ..
                } => Some((*index, prefill_tokens.get())),
                _ => None,
            })
            .collect();
        assert_eq!(turns, [(1, 178), (2, 178)], "{:?}", projection.unspellable);
        validates(&projection);
    }

    #[test]
    fn a_trunk_with_a_shape_the_rebuild_does_not_assume_is_unattributed() {
        // The rebuild carries only the kwargs `session.start` names (R1); a
        // trunk that sends one the log cannot carry is caught by the digest,
        // not rebuilt wrong.
        let log = a_real_session_log_shaped(
            None,
            BTreeMap::from([(
                "thinking_budget".to_owned(),
                crate::formats::record::json::Value::Integer(512),
            )]),
        );
        let projection = project(&log, &regime(), None).expect("projected");
        let reasons: Vec<&PrefixReason> = projection
            .events
            .iter()
            .filter_map(|event| match event {
                Event::PrefixChanged { reason, .. } => Some(reason),
                _ => None,
            })
            .collect();
        assert_eq!(reasons, [&PrefixReason::Unattributed]);
        assert!(
            projection
                .unspellable
                .iter()
                .any(|item| item.why.starts_with("its head could not be rebuilt")),
            "{:?}",
            projection.unspellable
        );
    }

    #[test]
    fn a_trunk_sending_the_reasoning_state_is_rebuilt_from_its_start() {
        // R1: `enable_thinking` and `reasoning_effort` are logged on
        // `session.start`, so the rebuilt head carries them and verifies.
        let log = a_real_session_log_shaped(
            None,
            BTreeMap::from([
                (
                    "enable_thinking".to_owned(),
                    crate::formats::record::json::Value::Boolean(true),
                ),
                (
                    "reasoning_effort".to_owned(),
                    crate::formats::record::json::Value::String("medium".to_owned()),
                ),
            ]),
        );
        let projection = project(&log, &regime(), None).expect("projected");
        assert!(
            !projection.events.iter().any(|event| matches!(
                event,
                Event::PrefixChanged {
                    reason: PrefixReason::Unattributed,
                    ..
                }
            )),
            "{:?}",
            projection.events
        );
        assert!(
            !projection
                .unspellable
                .iter()
                .any(|item| item.why.starts_with("its head could not be rebuilt")),
            "{:?}",
            projection.unspellable
        );
    }

    #[test]
    fn a_head_the_log_cannot_rebuild_is_unattributed_and_named() {
        // The second request's logged head is not what its trunk rebuilds to:
        // nothing is guessed.
        let mut lines = a_real_session_log();
        let second = lines
            .iter_mut()
            .filter(|line| matches!(line.event, Line::Request { .. }))
            .nth(1)
            .expect("two requests");
        if let Line::Request { head_sha256, .. } = &mut second.event {
            *head_sha256 = Some("c".repeat(64));
        }
        let projection = project(&lines, &regime(), None).expect("projected");
        assert!(projection.events.iter().any(|event| matches!(
            event,
            Event::PrefixChanged { reason: PrefixReason::Unattributed, diff, .. } if diff.is_empty()
        )));
        assert!(
            projection
                .unspellable
                .iter()
                .any(|item| item.why.contains("could not be rebuilt"))
        );
        validates(&projection);
    }

    #[test]
    fn the_cited_engines_are_matched_as_the_engine_check_matches_them() {
        let mut identity = crate::drive::registry::identity(
            crate::drive::registry::REGISTRY,
            "accel24-llamacpp-qwen38-27b-iq3s",
        )
        .expect("registered");
        identity.engine_commit = Some("e7051efc8002847f7269c5606318431179b5904e".to_owned());
        identity.engine_build_info = None;
        assert_eq!(cited(&identity), Some(Engine::Commit("e7051ef")));
        identity.engine_commit = Some("e486f802d3b46d9aa98b38d162e952b150cb5082".to_owned());
        assert_eq!(cited(&identity), None, "e486f80 is unmeasured");
        identity.engine_build_info = Some("b0-unknown-dirty".to_owned());
        assert_eq!(
            cited(&identity),
            None,
            "the literal on another engine's binary is unmeasured"
        );
        let beellama = crate::drive::registry::identity(
            crate::drive::registry::REGISTRY,
            "cpu-beellama-qwen3-1p7b-q4km",
        )
        .expect("registered");
        assert_eq!(cited(&beellama), Some(CITED[2]), "the binary measured");
        let replay =
            crate::drive::registry::identity(crate::drive::registry::REGISTRY, "canned-replay")
                .expect("registered");
        let Engine::Literal(literal, binary) = CITED[3] else {
            panic!("the replay is cited by its literal");
        };
        assert_eq!(
            (literal.to_owned(), binary.to_owned()),
            (
                crate::drive::canned::replay_build_info(),
                crate::drive::canned::replay_digest()
            ),
            "the cited literal is the one the replay computes"
        );
        assert_eq!(cited(&replay), Some(CITED[3]), "the stream replay");
        let tools = crate::drive::registry::identity(
            crate::drive::registry::REGISTRY,
            "canned-replay-tools",
        )
        .expect("registered");
        assert_eq!(cited(&tools), Some(CITED[4]), "the tool-turn replay");
        assert_eq!(
            CITED[4],
            Engine::Literal(
                "canned-7336cc7fed1f2e64c6095c49f73317c9b30be3ba8d22d7cf3bf8fc2bc62c6966",
                "7336cc7fed1f2e64c6095c49f73317c9b30be3ba8d22d7cf3bf8fc2bc62c6966",
            )
        );
        assert_eq!(
            crate::drive::canned::replay_tools_build_info(),
            "canned-7336cc7fed1f2e64c6095c49f73317c9b30be3ba8d22d7cf3bf8fc2bc62c6966"
        );
        let canned =
            crate::drive::registry::identity(crate::drive::registry::REGISTRY, "canned-cache-n")
                .expect("registered");
        assert_eq!(cited(&canned), None, "the canned acts are not a capture");
        // A literal governs: a cited commit beside an uncited literal is not cited.
        identity.engine_commit = Some("e7051efc8002847f7269c5606318431179b5904e".to_owned());
        identity.engine_build_info = Some("b9-somethingelse".to_owned());
        assert_eq!(cited(&identity), None);
    }

    /// A tool call's row carries how it ran from its own line (#302): the
    /// isolation, network, argv and policy each line says, never one value
    /// for every row -- the projection's mirror of the isolation lane's
    /// `the-record-is-a-constant`.
    #[test]
    fn a_tool_calls_row_carries_its_confinement_from_its_line() {
        type Row = (
            Option<log::Isolation>,
            Option<log::ToolOutcome>,
            Option<i64>,
            Option<String>,
        );
        let call = |id: &str, outcome: log::ToolOutcome, isolation: Option<log::Isolation>| {
            Line::ToolCall {
                request: 3,
                turn: 1,
                id: id.to_owned(),
                name: "bash".to_owned(),
                arguments: r#"{"command":"ls"}"#.to_owned(),
                outcome,
                argv: Some(vec!["sh".to_owned(), "-c".to_owned(), "ls".to_owned()]),
                confined: isolation
                    .map(|_| vec!["sh".to_owned(), "-c".to_owned(), "ls".to_owned()]),
                isolation,
                network: isolation.map(|_| log::Network::None),
                exit: (outcome == log::ToolOutcome::Ran).then_some(0),
                reason: (outcome == log::ToolOutcome::Refused)
                    .then_some(log::ToolRefusal::NotAllowed),
                policy: None,
                stdout: (outcome == log::ToolOutcome::Ran).then(|| log::Output {
                    text: "a.txt\n".to_owned(),
                    bytes: 6,
                }),
                stderr: (outcome == log::ToolOutcome::Ran).then(|| log::Output {
                    text: String::new(),
                    bytes: 0,
                }),
                cwd: isolation.map(|_| "/work".to_owned()),
                approval: None,
                files: None,
                shown: None,
                recovered_from: None,
            }
        };
        let mut events = vec![start()];
        let mut turn = answered(1, 3, Some(warm()), None);
        let settled = turn.len() - 2;
        turn.splice(
            settled..settled,
            [
                call("c1", log::ToolOutcome::Ran, Some(log::Isolation::Sandbox)),
                call("c2", log::ToolOutcome::Ran, Some(log::Isolation::None)),
                call("c3", log::ToolOutcome::Refused, None),
            ],
        );
        events.extend(turn);
        let lines = numbered(events);
        let projection =
            project(&lines, &regime(), Some(Engine::Commit("e7051ef"))).expect("projected");
        validates(&projection);
        let rows: Vec<Row> = projection
            .events
            .iter()
            .filter_map(|event| match event {
                Event::ToolCall {
                    exec, exit, output, ..
                } => Some((
                    exec.as_ref().and_then(|e| e.isolation),
                    exec.as_ref().map(|e| e.outcome),
                    *exit,
                    output.clone(),
                )),
                _ => None,
            })
            .collect();
        assert_eq!(
            rows,
            vec![
                (
                    Some(log::Isolation::Sandbox),
                    Some(log::ToolOutcome::Ran),
                    Some(0),
                    Some("a.txt\n".to_owned())
                ),
                (
                    Some(log::Isolation::None),
                    Some(log::ToolOutcome::Ran),
                    Some(0),
                    Some("a.txt\n".to_owned())
                ),
                (None, Some(log::ToolOutcome::Refused), None, None),
            ]
        );
        assert!(
            projection.unspellable.iter().all(|u| u.kind != "tool_call"),
            "{:?}",
            projection.unspellable
        );
    }

    #[test]
    fn a_tool_calls_arguments_that_are_not_json_are_named_beside_its_row() {
        let mut events = vec![start()];
        let mut turn = answered(1, 3, Some(warm()), None);
        let settled = turn.len() - 2;
        turn.insert(
            settled,
            Line::ToolCall {
                request: 3,
                turn: 1,
                id: "c1".to_owned(),
                name: "bash".to_owned(),
                arguments: "ls -la (".to_owned(),
                outcome: log::ToolOutcome::Refused,
                argv: None,
                confined: None,
                isolation: None,
                network: None,
                exit: None,
                reason: Some(log::ToolRefusal::Unparsable),
                policy: None,
                stdout: None,
                stderr: None,
                cwd: None,
                approval: None,
                files: None,
                shown: None,
                recovered_from: None,
            },
        );
        events.extend(turn);
        let lines = numbered(events);
        let projection =
            project(&lines, &regime(), Some(Engine::Commit("e7051ef"))).expect("projected");
        validates(&projection);
        let args: Vec<bool> = projection
            .events
            .iter()
            .filter_map(|event| match event {
                Event::ToolCall { args, .. } => Some(args.is_some()),
                _ => None,
            })
            .collect();
        assert_eq!(args, vec![false]);
        let named: Vec<Option<&str>> = projection
            .unspellable
            .iter()
            .filter(|u| u.kind == "tool_call")
            .map(|u| u.text.as_deref())
            .collect();
        assert_eq!(named, vec![Some("ls -la (")]);
    }

    /// A v5 session with two forks, projected and validated: turn 1's
    /// scoping fork settled `value` with three patches, a plain turn 2, and
    /// turn 3's read fork, declined.
    fn two_forks() -> Projection {
        let fork = |at: u64, of_turn: u32, why: log::Warrant| Line::Fork {
            lane: Lane::Interview,
            of_turn,
            at,
            why,
            question: "what did the operator decide".to_owned(),
        };
        let call = |turn: u32, fork: u64| {
            [
                // Each fork's head is the trunk's tail at its gap plus its
                // question, so two forks never share one.
                Line::Request {
                    turn,
                    lane: Lane::Interview,
                    head_sha256: Some(format!("{fork:064x}")),
                    fork: Some(fork),
                    max_tokens: None,
                },
                Line::Response {
                    to_request: fork + 1,
                    text: "{}".to_owned(),
                    finish_reason: Some("stop".to_owned()),
                    reasoning: None,
                    timings: Some(warm()),
                    usage: None,
                    capped: None,
                },
            ]
        };
        let patch = |fork: u64, id: &str| Line::Patch {
            fork,
            op: log::PatchOp::Add,
            entry: log::PatchEntry {
                id: id.to_owned(),
                text: format!("decision {id}"),
                category: Some("scope".to_owned()),
            },
            supersedes: None,
        };
        let mut events = vec![start()];
        events.extend(answered(1, 3, Some(warm()), None));
        events.push(fork(3, 1, log::Warrant::Scoping));
        events.extend(call(1, 8));
        events.push(Line::ForkSettled {
            fork: 8,
            outcome: log::ForkOutcome::Value,
        });
        events.extend(["d1", "d2", "d3"].map(|id| patch(8, id)));
        events.extend(answered(2, 17, Some(warm()), None));
        events.extend(answered(3, 24, Some(warm()), None));
        events.push(fork(24, 3, log::Warrant::Read));
        events.extend(call(3, 29));
        events.push(Line::ForkSettled {
            fork: 29,
            outcome: log::ForkOutcome::Decline,
        });
        let lines = numbered(events);
        let document: String = lines.iter().map(|line| log::render(line) + "\n").collect();
        log::parse(&document).expect("a v5 log the reader accepts");
        let projection =
            project(&lines, &regime(), Some(Engine::Commit("e7051ef"))).expect("projected");
        validates(&projection);
        projection
    }

    /// A fork and what it captured (v5, #374): the scope-boundary gap's fork
    /// is a row with its three patches as one capture of three entries; a
    /// plain gap has no fork and so no row; a fork that declined is a row,
    /// captures nothing, and its outcome is named.
    #[test]
    fn a_forks_row_and_its_patches_capture_and_a_declined_fork_is_named() {
        let projection = two_forks();
        let forks: Vec<(&str, &str, u32)> = projection
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Fork {
                    id, lane, of_turn, ..
                } => Some((id.as_str(), lane.as_str(), *of_turn)),
                _ => None,
            })
            .collect();
        assert_eq!(
            forks,
            vec![("f/8", "interview", 1), ("f/29", "interview", 3)]
        );
        let captures: Vec<(&str, &str, u32)> = projection
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Capture {
                    id,
                    from_fork,
                    entries,
                } => Some((id.as_str(), from_fork.as_str(), *entries)),
                _ => None,
            })
            .collect();
        assert_eq!(captures, vec![("c/8", "f/8", 3)]);
        let named: Vec<(u64, &str)> = projection
            .unspellable
            .iter()
            .filter(|u| u.kind.starts_with("fork") || u.kind == "patch")
            .map(|u| (u.seq, u.kind))
            .collect();
        assert_eq!(named, vec![(32, "fork.settled")]);
    }

    /// Two forks never share a head -- each is the trunk's tail at its gap
    /// plus its question -- so the second's moved from the first's, and is
    /// named: the live record refuses an unnamed move (#442's review, B1).
    #[test]
    fn a_second_forks_moved_head_is_named_and_the_record_validates() {
        let projection = two_forks();
        let moves: Vec<(&str, PrefixReason)> = projection
            .events
            .iter()
            .filter_map(|event| match event {
                Event::PrefixChanged {
                    at_request, reason, ..
                } if at_request.as_str() == "q/30" => Some((at_request.as_str(), *reason)),
                _ => None,
            })
            .collect();
        assert_eq!(moves, vec![("q/30", PrefixReason::Unattributed)]);
    }

    /// A call's files (#372): the line's `files` on the row, by reference,
    /// as the log wrote them.
    #[test]
    fn a_tool_calls_files_are_carried_onto_its_row() {
        let file = log::RecordedFile {
            path: "files/shot.png".to_owned(),
            sha256: "3f1a8e0c5b2d4f6a7e9c1b3d5f7a9c2e4b6d8f0a1c3e5b7d9f1a3c5e7b9d2f4a".to_owned(),
            media_type: "image/png".to_owned(),
            bytes: 48_213,
        };
        let mut events = vec![start()];
        let mut turn = answered(1, 3, Some(warm()), None);
        let settled = turn.len() - 2;
        turn.insert(
            settled,
            Line::ToolCall {
                request: 3,
                turn: 1,
                id: "c1".to_owned(),
                name: "bash".to_owned(),
                arguments: r#"{"command":"screenshot shot.png"}"#.to_owned(),
                outcome: log::ToolOutcome::Ran,
                argv: Some(vec![
                    "sh".to_owned(),
                    "-c".to_owned(),
                    "screenshot shot.png".to_owned(),
                ]),
                confined: Some(vec!["sh".to_owned()]),
                isolation: Some(log::Isolation::None),
                network: Some(log::Network::Host),
                exit: Some(0),
                reason: None,
                policy: None,
                stdout: Some(log::Output {
                    text: String::new(),
                    bytes: 0,
                }),
                stderr: Some(log::Output {
                    text: String::new(),
                    bytes: 0,
                }),
                cwd: Some("/work".to_owned()),
                approval: None,
                files: Some(vec![file.clone()]),
                shown: None,
                recovered_from: None,
            },
        );
        events.extend(turn);
        let lines = numbered(events);
        let projection =
            project(&lines, &regime(), Some(Engine::Commit("e7051ef"))).expect("projected");
        validates(&projection);
        let files: Vec<Option<Vec<log::RecordedFile>>> = projection
            .events
            .iter()
            .filter_map(|event| match event {
                Event::ToolCall { exec, .. } => Some(exec.as_ref().and_then(|e| e.files.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(files, vec![Some(vec![file])]);
    }

    /// The operator's attachment (#372, 5989411005): the ask's `files` on its
    /// turn's row, by reference, as the log wrote them.
    #[test]
    fn an_asks_attached_files_are_carried_onto_its_turn_row() {
        let file = log::RecordedFile {
            path: "files/screenshot.png".to_owned(),
            sha256: "3f1a8e0c5b2d4f6a7e9c1b3d5f7a9c2e4b6d8f0a1c3e5b7d9f1a3c5e7b9d2f4a".to_owned(),
            media_type: "image/png".to_owned(),
            bytes: 48_213,
        };
        let mut turn = answered(1, 3, Some(warm()), None);
        if let Line::Ask { files, .. } = &mut turn[0] {
            *files = Some(vec![file.clone()]);
        }
        let mut events = vec![start()];
        events.extend(turn);
        let lines = numbered(events);
        let projection =
            project(&lines, &regime(), Some(Engine::Commit("e7051ef"))).expect("projected");
        validates(&projection);
        let files: Vec<Option<Vec<log::RecordedFile>>> = projection
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Turn { files, .. } => Some(files.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(files, vec![Some(vec![file])]);
    }
}
