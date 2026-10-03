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
use crate::client::shape::{Limits, Message, RequestShape, Role, SamplerCard};
use crate::formats::log::{self, Event as Line, Lane};
use crate::formats::record::json::Decimal;
use crate::formats::record::{self, Count, Event, PrefixReason, Regime, Source};

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
    let Some(log::Line {
        event: Line::SessionStart { model, head, .. },
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
    walk.model.clone_from(model);
    walk.trunk = head
        .iter()
        .map(|message| Message::new(role_of(message.role), message.content.clone()))
        .collect();
    for line in &lines[1..] {
        walk.line(line)?;
    }
    let mut events = vec![Event::Start {
        regime: Box::new(regime.clone()),
        source: Source::Live,
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
    /// turn's ask and answer.
    trunk: Vec<Message>,
    /// Each turn's ask.
    asks: BTreeMap<u32, String>,
    /// Each trunk request's turn, by `seq`.
    turn_of: BTreeMap<u64, u32>,
    /// The trunk's last request: its logged digest, and its head rebuilt
    /// from the log when that gave the same digest.
    last_head: Option<(String, Option<Head>)>,
}

impl<'a> Walk<'a> {
    fn over(lines: &'a [log::Line], substrate: String, engine: Option<Engine>) -> Self {
        let mut outcome = BTreeMap::new();
        let mut trunk_of = BTreeMap::new();
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
            asks: BTreeMap::new(),
            turn_of: BTreeMap::new(),
            last_head: None,
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
            Line::Ask { turn, text } => {
                self.asks.insert(*turn, text.clone());
                self.ask(line.seq, *turn);
            }
            Line::Request {
                turn,
                lane,
                head_sha256,
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
            Line::Cancelled { partial, .. } => self.name(
                line.seq,
                "cancelled",
                "a cancelled call: the record has no row for one".to_owned(),
                Some(partial.clone()),
            ),
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

    fn ask(&mut self, seq: u64, turn: u32) {
        let trunk_timings =
            self.trunk_of
                .get(&turn)
                .and_then(|request| match self.outcome.get(request) {
                    Some(Line::Response {
                        timings,
                        capped: None | Some(false),
                        ..
                    }) => timings.as_ref(),
                    _ => None,
                });
        match (
            self.turns_broken,
            prefill_tokens(trunk_timings, self.engine),
        ) {
            (false, Ok(prefill_tokens)) => self.events.push(Event::Turn {
                index: turn,
                prefill_tokens,
            }),
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
        messages.push(Message::new(
            Role::User,
            self.asks.get(&turn).cloned().unwrap_or_default(),
        ));
        let rebuilt = Head::of(&RequestShape {
            model: self.model.clone(),
            messages,
            sampler: SamplerCard::empty(),
            limits: Limits {
                attempt: std::time::Duration::ZERO,
                call: std::time::Duration::ZERO,
                max_output_tokens: 0,
                retries: 0,
            },
            grammar: None,
            // ASSERTED by the digest check below: `serve`'s trunk sends none.
            template_kwargs: BTreeMap::new(),
            tools: Vec::new(),
        });
        let verified = (rebuilt.digest() == logged).then_some(rebuilt);
        if verified.is_none() {
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

    /// The trunk after an answered trunk request: its ask and its answer,
    /// appended as the session appends them.
    fn answered(&mut self, to_request: u64, text: &str, reasoning: Option<&String>) {
        let Some(turn) = self.turn_of.get(&to_request) else {
            return;
        };
        let ask = self.asks.get(turn).cloned().unwrap_or_default();
        self.trunk.push(Message::new(Role::User, ask));
        let mut answer = Message::new(Role::Assistant, text.to_owned());
        answer.reasoning = reasoning.cloned();
        self.trunk.push(answer);
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
            },
            Line::Settlement {
                from: State::Awaiting,
                to: State::Turn,
            },
            Line::Request {
                turn,
                lane: Lane::Trunk,
                head_sha256: Some(head()),
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
                },
                Line::Settlement {
                    from: State::Awaiting,
                    to: State::Turn,
                },
                Line::Request {
                    turn,
                    lane: Lane::Trunk,
                    head_sha256: Some(head()),
                },
                outcome,
            ]);
        }
        events.push(Line::Request {
            turn: 2,
            lane: Lane::Trunk,
            head_sha256: Some(head()),
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
    fn a_trunk_with_a_shape_the_rebuild_does_not_assume_is_unattributed() {
        // The rebuild assumes no kwargs and no tools (ruled on #157); a trunk
        // that sends kwargs is caught by the digest, not rebuilt wrong.
        let log = a_real_session_log_shaped(
            None,
            BTreeMap::from([(
                "enable_thinking".to_owned(),
                crate::formats::record::json::Value::Boolean(false),
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
        // A literal governs: a cited commit beside an uncited literal is not cited.
        identity.engine_commit = Some("e7051efc8002847f7269c5606318431179b5904e".to_owned());
        identity.engine_build_info = Some("b9-somethingelse".to_owned());
        assert_eq!(cited(&identity), None);
    }
}
