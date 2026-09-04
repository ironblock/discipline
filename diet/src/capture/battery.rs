//! The dev-loop compliance battery.
//!
//! Iterating on the *machinery* -- routers, reconcilers, formats, seams --
//! does not need a capable substrate. It needs a **protocol-compliant** one.
//! In the research phase every mechanism change was validated against a
//! twenty-seven-billion-parameter substrate on an accelerator box, so finding
//! out whether a reconciler edit was correct cost a ten-minute drive. Ten
//! minutes was not the price of the reconciler; it was the price of paying
//! for capability at a seam where only compliance was being used.
//!
//! So this module asks five yes-or-no questions and nothing else. Does the
//! candidate answer an interview in a register the parser can read; does it
//! call a tool when one is offered rather than describing the call; does it
//! decline when the turn establishes nothing; does an answer survive the
//! working object being rendered back into the prompt at a seam; does a side
//! excursion produce enough distinct facts for a closure to have something to
//! dispose of. Pass or fail per behaviour, with a reason on a fail. **There is
//! no quality grading here and there never will be**: a dev-loop candidate's
//! results are evidence about the machinery and are not capability evidence
//! about the candidate.
//!
//! Two consequences are in the types rather than in a convention:
//!
//! * **The substrate is supplied, never chosen here.** [`Responder::regime`]
//!   is required, and the record the drive writes names it. Which serving
//!   engine, which weights and what time budget a dev-loop regimen pins is an
//!   open ruling; a default here would settle it by accident, and the run
//!   whose result surprises somebody is the run nobody thought to tag.
//! * **A canned responder is a fixture, not a service.** [`CannedResponder`]
//!   replays an archived drive off disk. It opens no socket and binds no port,
//!   so the battery is the same offline, in a sandbox, and on a machine that
//!   already has something listening.
//!
//! **What is not here, and why.** No candidate has been put to this battery,
//! because choosing one is not this module's to make and cannot be made from
//! here. Three things are open and each has to be ruled on before a
//! `dev-loop` regimen can be pinned: which serving engine runs it, whether
//! weights are fetched in a job or cached and how that is tagged, and what
//! wall-clock budget a lane serving a real substrate gets. Until then the
//! only numbers this module produces are the ones a canned script carries,
//! and those are zero because a replay serves nothing.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::formats::decline;
use crate::formats::interview::{self, Completion, FieldKind, Outcome as FieldOutcome};
use crate::formats::record::json::{self, Value};
use crate::formats::record::{self, Count, Event, Record, Regime};
use crate::object::{Applied, EntryId, ObjectError, Patch, Provenance, WorkingObject};

/// The lane a battery exchange is booked under.
///
/// `interview` rather than a lane of its own: the battery asks a fork the same
/// questions an interview fork is asked, and coining a lane name for a
/// harness would put a name in provenance that no reader could trace back to
/// a lane that exists.
const LANE: &str = "interview";

/// How many distinct facts a side excursion has to leave behind.
///
/// Two, because one is what a candidate that repeats the question also
/// produces, and closure disposes of entries by name: an excursion that said
/// the same thing twice leaves one entry and a closure with nothing to choose
/// between.
const EXCURSION_ENTRIES: usize = 2;

/// A behaviour the battery decides.
///
/// Compliance, not capability. Every one of these is something the machinery
/// needs from whatever is on the other end of the seam, and none of them is
/// something a reader should mistake for the candidate being good at anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Behaviour {
    /// The answer reaches the interview parser as fields rather than as prose.
    AnswersParseably,
    /// An offered tool is called, not described.
    UsesOfferedTool,
    /// A turn that establishes nothing says so instead of filling the field.
    DeclinesCorrectly,
    /// An answer still cites the object after the object was rendered back
    /// into the prompt.
    SurvivesSeamRefill,
    /// A side excursion leaves distinct facts a closure can dispose of.
    ProducesClosableTangent,
}

impl Behaviour {
    /// Every behaviour, in the order the battery exercises them.
    pub const ALL: &'static [Self] = &[
        Self::AnswersParseably,
        Self::UsesOfferedTool,
        Self::DeclinesCorrectly,
        Self::SurvivesSeamRefill,
        Self::ProducesClosableTangent,
    ];

    /// The name this behaviour is written under, in reports and fixtures.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::AnswersParseably => "answers_parseably",
            Self::UsesOfferedTool => "uses_offered_tool",
            Self::DeclinesCorrectly => "declines_correctly",
            Self::SurvivesSeamRefill => "survives_seam_refill",
            Self::ProducesClosableTangent => "produces_closable_tangent",
        }
    }

    /// The behaviour `tag` names, if there is one.
    ///
    /// Iterates [`Behaviour::ALL`] rather than matching the text, so a variant
    /// added without a tag cannot be looked up and a tag with no variant
    /// cannot be written.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|behaviour| behaviour.tag() == tag)
    }

    /// The prose this behaviour is asked with.
    ///
    /// # Panics
    ///
    /// If no row of `ASKS` names this behaviour, which
    /// `every_behaviour_has_an_ask_and_every_ask_file_is_named_by_one`
    /// refuses: a behaviour the battery would ask nothing is a behaviour it
    /// would report on without having put it.
    #[must_use]
    pub fn ask(self) -> &'static str {
        let (_, ask) = ASKS
            .iter()
            .find(|(behaviour, _)| *behaviour == self)
            .expect("every behaviour has a row in ASKS; every_behaviour_has_an_ask holds it");
        ask
    }
}

/// The ask each behaviour is put with.
///
/// A table of included files rather than string literals in the source: the
/// wording of an ask is data a later version revises, and a revision that has
/// to be made in Rust is a revision that arrives as a diff nobody can read
/// beside the answers it changed.
const ASKS: &[(Behaviour, &str)] = &[
    (
        Behaviour::AnswersParseably,
        include_str!("../../capture/battery/asks/answers_parseably.txt"),
    ),
    (
        Behaviour::UsesOfferedTool,
        include_str!("../../capture/battery/asks/uses_offered_tool.txt"),
    ),
    (
        Behaviour::DeclinesCorrectly,
        include_str!("../../capture/battery/asks/declines_correctly.txt"),
    ),
    (
        Behaviour::SurvivesSeamRefill,
        include_str!("../../capture/battery/asks/survives_seam_refill.txt"),
    ),
    (
        Behaviour::ProducesClosableTangent,
        include_str!("../../capture/battery/asks/produces_closable_tangent.txt"),
    ),
];

/// The follow-up put after the object is rendered back into the prompt.
const REFILL_ASK: &str = include_str!("../../capture/battery/asks/survives_seam_refill.refill.txt");

/// A tool the battery offers, and what a call to it has to carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Offer {
    /// The tool's name.
    pub tool: &'static str,
    /// Arguments a call is not a call without.
    pub required_args: &'static [&'static str],
}

/// The one tool the battery offers.
///
/// The self-capture contract's own writing tool, so that a candidate which
/// passes this behaviour has been seen doing the thing capture will ask of it,
/// rather than the thing a harness invented to be easy.
pub const OFFERED_TOOL: Offer = Offer {
    tool: "update_record",
    required_args: &["field", "content"],
};

/// One thing the battery puts to a responder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// Which behaviour this exchange decides.
    pub behaviour: Behaviour,
    /// Which exchange within that behaviour, from 1. Only
    /// [`Behaviour::SurvivesSeamRefill`] has a second.
    pub exchange: u32,
    /// The prompt as sent, seam render included.
    pub ask: String,
    /// The tool offered with this ask, if one was.
    pub offered: Option<Offer>,
}

/// One tool call a responder made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInvocation {
    /// Which tool.
    pub tool: String,
    /// What it was called with.
    pub args: BTreeMap<String, Value>,
}

/// What came back.
///
/// The token counts come from the responder rather than from a count this
/// module makes: nothing here can tokenize, and a length in bytes written into
/// a field named for tokens is a measurement of the wrong thing that reads
/// like a measurement of the right one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reply {
    /// The prose that came back. May be empty: an answer of nothing at all is
    /// a typed outcome and the collapse this battery exists to catch.
    pub text: String,
    /// The tool calls that came with it, in order.
    pub calls: Vec<ToolInvocation>,
    /// What the prompt cost, as the responder reports it.
    pub prefill_tokens: Count,
    /// What the answer cost, as the responder reports it.
    pub output_tokens: Count,
}

/// Whatever is on the other end of the seam.
///
/// A trait so that the battery is written once and run against a canned
/// fixture here and against a served substrate when one is pinned. A client
/// that speaks to a local server implements this and nothing else changes.
pub trait Responder {
    /// What to call this responder in a report.
    fn id(&self) -> &str;

    /// The regime this responder answers under.
    ///
    /// Required, with no default. Every result carries its full regime, and a
    /// battery that invented one would put a substrate tag on a record that
    /// nothing served.
    fn regime(&self) -> &Regime;

    /// Answer one probe.
    fn reply(&mut self, probe: &Probe) -> Reply;
}

/// What the battery decided about one behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The behaviour held.
    Pass,
    /// It did not, and this is what went wrong.
    Fail(String),
}

impl Outcome {
    /// Whether the behaviour held.
    #[must_use]
    pub fn is_pass(&self) -> bool {
        matches!(self, Self::Pass)
    }

    /// Why it did not, when it did not.
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Pass => None,
            Self::Fail(why) => Some(why),
        }
    }
}

/// What the battery decided, behaviour by behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    responder: String,
    outcomes: Vec<(Behaviour, Outcome)>,
}

impl Report {
    /// What was decided about `behaviour`.
    ///
    /// Every behaviour in [`Behaviour::ALL`] is present in a report the
    /// battery produced, so this is `None` only for a report built by hand.
    #[must_use]
    pub fn outcome(&self, behaviour: Behaviour) -> Option<&Outcome> {
        self.outcomes
            .iter()
            .find(|(which, _)| *which == behaviour)
            .map(|(_, outcome)| outcome)
    }

    /// Which responder this is about.
    #[must_use]
    pub fn responder(&self) -> &str {
        &self.responder
    }

    /// Every behaviour that did not hold, with its reason.
    #[must_use]
    pub fn failures(&self) -> Vec<(Behaviour, &str)> {
        self.outcomes
            .iter()
            .filter_map(|(behaviour, outcome)| outcome.reason().map(|reason| (*behaviour, reason)))
            .collect()
    }

    /// Whether every behaviour held.
    #[must_use]
    pub fn compliant(&self) -> bool {
        self.failures().is_empty()
    }

    /// The report in the record's own value space.
    ///
    /// One renderer, so what a caller reads and what a fixture pins are the
    /// same bytes.
    #[must_use]
    pub fn value(&self) -> Value {
        let mut behaviours = BTreeMap::new();
        for (behaviour, outcome) in &self.outcomes {
            let mut row = BTreeMap::from([("pass".to_owned(), Value::Boolean(outcome.is_pass()))]);
            if let Some(reason) = outcome.reason() {
                row.insert("reason".to_owned(), Value::String(reason.to_owned()));
            }
            behaviours.insert(behaviour.tag().to_owned(), Value::Object(row));
        }
        Value::Object(BTreeMap::from([
            ("report".to_owned(), Value::String("battery".to_owned())),
            (
                "responder".to_owned(),
                Value::String(self.responder.clone()),
            ),
            ("compliant".to_owned(), Value::Boolean(self.compliant())),
            ("behaviours".to_owned(), Value::Object(behaviours)),
        ]))
    }
}

/// One run of the battery: what it decided, and the archive of how.
///
/// The events are the point of the integration lane. A report says the
/// machinery answered; a record says the drive that produced the answers is
/// itself readable by the one authorized reader, which is the half a unit test
/// cannot reach because a unit test never writes the file or runs the binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drive {
    /// What the battery decided.
    pub report: Report,
    /// The archive of the drive, in the order things happened.
    pub events: Vec<Event>,
}

impl Drive {
    /// The drive's archive as a record.
    #[must_use]
    pub fn record(&self) -> Record {
        Record {
            events: self.events.clone(),
        }
    }
}

/// The battery itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battery;

impl Battery {
    /// Put every behaviour to `responder` and report what held.
    #[must_use]
    pub fn run(responder: &mut dyn Responder) -> Report {
        Self::drive(responder).report
    }

    /// Run the battery and keep the archive of the drive.
    #[must_use]
    pub fn drive(responder: &mut dyn Responder) -> Drive {
        let regime = responder.regime().clone();
        let mut events = vec![Event::Start {
            regime: Box::new(regime.clone()),
        }];
        let mut outcomes = Vec::with_capacity(Behaviour::ALL.len());

        for (turn, behaviour) in (1_u32..).zip(Behaviour::ALL.iter().copied()) {
            let mut exchange = Exchange::new(behaviour, turn, &regime);
            let outcome = exchange.put(responder);
            events.push(Event::Turn {
                index: turn,
                prefill_tokens: exchange.prefill,
            });
            events.append(&mut exchange.events);
            outcomes.push((behaviour, outcome));
        }

        Drive {
            report: Report {
                responder: responder.id().to_owned(),
                outcomes,
            },
            events,
        }
    }
}

/// One behaviour's exchanges, and the rows they produced.
///
/// A struct rather than a long function so that the record rows are appended
/// at the one place the reply arrives: a row written anywhere else is a row
/// that can describe an exchange that did not happen.
struct Exchange<'a> {
    behaviour: Behaviour,
    turn: u32,
    regime: &'a Regime,
    events: Vec<Event>,
    prefill: Count,
    next: u32,
}

impl<'a> Exchange<'a> {
    fn new(behaviour: Behaviour, turn: u32, regime: &'a Regime) -> Self {
        Self {
            behaviour,
            turn,
            regime,
            events: Vec::new(),
            prefill: Count::default(),
            next: 1,
        }
    }

    /// The id of the request for exchange `n`, derived from the behaviour it
    /// belongs to rather than minted from a counter of its own.
    fn ask_id(&self, exchange: u32) -> String {
        format!("{}/{exchange}/ask", self.behaviour.tag())
    }

    fn answer_id(&self, exchange: u32) -> String {
        format!("{}/{exchange}/answer", self.behaviour.tag())
    }

    /// Send one probe, record the rows it produced, and hand back the reply.
    fn send(
        &mut self,
        responder: &mut dyn Responder,
        ask: String,
        offered: Option<Offer>,
    ) -> Reply {
        let exchange = self.next;
        self.next += 1;
        let probe = Probe {
            behaviour: self.behaviour,
            exchange,
            ask,
            offered,
        };
        let reply = responder.reply(&probe);
        self.prefill = self.prefill.saturating_add(reply.prefill_tokens);
        self.events.push(Event::Request {
            id: self.ask_id(exchange),
            lane: LANE.to_owned(),
            retry_of: None,
            text: Some(probe.ask.clone()),
        });
        for (index, call) in reply.calls.iter().enumerate() {
            self.events.push(Event::ToolCall {
                id: format!("{}/call/{index}", self.answer_id(exchange)),
                at_turn: self.turn,
                tool: call.tool.clone(),
                args: Some(call.args.clone()),
                exit: None,
                output: None,
            });
        }
        self.events.push(Event::Response {
            id: self.answer_id(exchange),
            to_request: self.ask_id(exchange),
            output_tokens: reply.output_tokens,
            text: Some(reply.text.clone()),
        });
        reply
    }

    /// Put this behaviour and decide it.
    fn put(&mut self, responder: &mut dyn Responder) -> Outcome {
        match self.behaviour {
            Behaviour::AnswersParseably => {
                let reply = self.send(responder, self.behaviour.ask().to_owned(), None);
                judge_answers_parseably(&reply)
            }
            Behaviour::UsesOfferedTool => {
                let reply = self.send(
                    responder,
                    self.behaviour.ask().to_owned(),
                    Some(OFFERED_TOOL),
                );
                judge_uses_offered_tool(&reply)
            }
            Behaviour::DeclinesCorrectly => {
                let reply = self.send(responder, self.behaviour.ask().to_owned(), None);
                judge_declines_correctly(&reply)
            }
            Behaviour::SurvivesSeamRefill => self.put_seam_refill(responder),
            Behaviour::ProducesClosableTangent => {
                let reply = self.send(responder, self.behaviour.ask().to_owned(), None);
                judge_produces_closable_tangent(&reply, self.regime, self.turn)
            }
        }
    }

    /// Two exchanges with a seam between them.
    ///
    /// The second exchange happens whatever the first produced. Skipping it
    /// when the first answered nothing would take a canned script out of step
    /// with the probes for every behaviour after it, and the battery would
    /// then report failures belonging to the wrong question.
    fn put_seam_refill(&mut self, responder: &mut dyn Responder) -> Outcome {
        let first = self.send(responder, self.behaviour.ask().to_owned(), None);
        let carried = self.refill(&first);
        let render = carried
            .as_ref()
            .map_or_else(String::new, |(_, render)| render.clone());
        if let Some((_, render)) = carried.as_ref() {
            self.events.push(Event::Seam {
                id: format!("{}/seam", self.behaviour.tag()),
                at_turn: self.turn,
                rendered_bytes: Count::new(render.len() as u64).unwrap_or_default(),
            });
        }
        let ask = format!("{REFILL_ASK}\n{render}");
        let second = self.send(responder, ask, None);

        let Some((entry, _)) = carried else {
            return Outcome::Fail(
                "the first exchange left the seam nothing to carry: no field held content"
                    .to_owned(),
            );
        };
        if second.text.contains(entry.as_str()) {
            Outcome::Pass
        } else {
            Outcome::Fail(format!(
                "the answer after the seam refill did not name `{entry}`, the one entry \
                 the render carried"
            ))
        }
    }

    /// Capture the first answer into an object and render it at the seam.
    ///
    /// The entry's id is the id of the response it came from, so the render a
    /// candidate is asked to cite carries an identifier the record already
    /// holds rather than one this module made up.
    fn refill(&self, first: &Reply) -> Option<(EntryId, String)> {
        let answer = interview::parse(&first.text).ok()?;
        let (_, content) = tagged_values(&answer).into_iter().next()?;
        let id = EntryId::new(&self.answer_id(1)).ok()?;
        let mut object = WorkingObject::open(self.regime.clone());
        object
            .apply_turn(&[Patch::Add {
                id: id.clone(),
                content,
                provenance: Provenance {
                    turn: self.turn,
                    lane: LANE.to_owned(),
                    fork: None,
                    index: 0,
                },
            }])
            .ok()?;
        Some((id, object.dump()))
    }
}

/// Every field that carried content, with the kind it was tagged as.
fn tagged_values(answer: &interview::Answer) -> Vec<(FieldKind, String)> {
    answer
        .fields
        .iter()
        .filter_map(|field| match (&field.tag, &field.outcome) {
            (Some(tag), FieldOutcome::Value(text)) => Some((tag.kind, text.clone())),
            _ => None,
        })
        .collect()
}

/// Read `text` as an interview answer, or say why it is not one.
fn read_answer(text: &str) -> Result<interview::Answer, Outcome> {
    interview::parse(text).map_err(|err| {
        Outcome::Fail(format!(
            "the answer did not reach the interview parser: {err}"
        ))
    })
}

fn judge_answers_parseably(reply: &Reply) -> Outcome {
    let answer = match read_answer(&reply.text) {
        Ok(answer) => answer,
        Err(outcome) => return outcome,
    };
    match answer.completion {
        Completion::Empty => {
            return Outcome::Fail("the answer was empty: nothing came back at all".to_owned());
        }
        Completion::Truncated(signal) => {
            return Outcome::Fail(format!("the answer was cut off: {}", signal.name()));
        }
        Completion::Complete => {}
    }
    if tagged_values(&answer).is_empty() {
        return Outcome::Fail(
            "no field carried content: every region of the answer was untagged prose".to_owned(),
        );
    }
    Outcome::Pass
}

fn judge_uses_offered_tool(reply: &Reply) -> Outcome {
    let offered = OFFERED_TOOL;
    let named: Vec<&ToolInvocation> = reply
        .calls
        .iter()
        .filter(|call| call.tool == offered.tool)
        .collect();
    let [call] = named.as_slice() else {
        if named.len() > 1 {
            return Outcome::Fail(format!(
                "called `{}` {} times where one call was asked for",
                offered.tool,
                named.len()
            ));
        }
        return match reply.calls.first() {
            None => Outcome::Fail(format!(
                "answered in prose where the offered tool `{}` was required",
                offered.tool
            )),
            Some(other) => Outcome::Fail(format!(
                "called `{}` where `{}` was the offered tool",
                other.tool, offered.tool
            )),
        };
    };
    for required in offered.required_args {
        if !call.args.contains_key(*required) {
            return Outcome::Fail(format!(
                "the call to `{}` carried no `{required}` argument",
                offered.tool
            ));
        }
    }
    Outcome::Pass
}

fn judge_declines_correctly(reply: &Reply) -> Outcome {
    if decline::classify(reply.text.trim()).is_decline() {
        return Outcome::Pass;
    }
    let answer = match read_answer(&reply.text) {
        Ok(answer) => answer,
        Err(outcome) => return outcome,
    };
    let tagged: Vec<&interview::Field> = answer
        .fields
        .iter()
        .filter(|field| field.tag.is_some())
        .collect();
    if tagged.is_empty() {
        return Outcome::Fail(
            "the answer neither declined nor tagged a field: a turn with nothing to report \
             says so"
                .to_owned(),
        );
    }
    for field in tagged {
        if !matches!(field.outcome, FieldOutcome::Decline(_)) {
            let written = field
                .tag
                .as_ref()
                .map_or_else(String::new, |tag| tag.as_written.clone());
            return Outcome::Fail(format!(
                "filled `{written}` with content where the turn established nothing"
            ));
        }
    }
    Outcome::Pass
}

fn judge_produces_closable_tangent(reply: &Reply, regime: &Regime, turn: u32) -> Outcome {
    let answer = match read_answer(&reply.text) {
        Ok(answer) => answer,
        Err(outcome) => return outcome,
    };
    let mut patches = Vec::new();
    for (index, (kind, content)) in tagged_values(&answer).into_iter().enumerate() {
        let id = match EntryId::new(&format!("excursion/{}/{index}", kind.canonical_tag())) {
            Ok(id) => id,
            Err(err) => return Outcome::Fail(refused(&err)),
        };
        patches.push(Patch::Add {
            id,
            content,
            provenance: Provenance {
                turn,
                lane: LANE.to_owned(),
                fork: None,
                index: u32::try_from(index).unwrap_or(u32::MAX),
            },
        });
    }
    let mut object = WorkingObject::open(regime.clone());
    let applied = match object.apply_turn(&patches) {
        Ok(applied) => applied,
        Err(err) => return Outcome::Fail(refused(&err)),
    };
    let born = applied
        .iter()
        .filter(|outcome| matches!(outcome, Applied::Created(_)))
        .count();
    if born == EXCURSION_ENTRIES {
        return Outcome::Pass;
    }
    Outcome::Fail(format!(
        "the excursion left {born} distinct entr{} where a closure needs {EXCURSION_ENTRIES} \
         to dispose of",
        if born == 1 { "y" } else { "ies" }
    ))
}

fn refused(err: &ObjectError) -> String {
    format!("the object refused what the excursion produced: {err}")
}

// ---------------------------------------------------------------------------
// the canned responder
// ---------------------------------------------------------------------------

/// Where the canned servers live.
///
/// Resolved from the crate's own directory at compile time. A corpus found
/// through the working directory is a corpus that silently walks zero files
/// when the test runner starts somewhere else.
#[must_use]
pub fn servers_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("capture/battery/servers")
}

/// A canned server: an archived drive, replayed.
///
/// It is a fixture and not a service on purpose. A compliance harness that
/// needs a process listening somewhere cannot run in a sandbox, cannot run
/// twice at once, and collides with whatever a developer already has on that
/// port -- and none of that has anything to do with the behaviours under test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CannedResponder {
    id: String,
    regime: Regime,
    script: Vec<Reply>,
    next: usize,
}

impl CannedResponder {
    /// Load the canned server in `dir`, reading `script.jsonl`.
    ///
    /// # Errors
    ///
    /// [`BatteryError::Unreadable`] if the script is not on disk,
    /// [`BatteryError::NotARecord`] if it is not a record, and
    /// [`BatteryError::TurnWithoutAnswer`] if one of its turns has no response.
    pub fn load(dir: &Path) -> Result<Self, BatteryError> {
        let path = dir.join("script.jsonl");
        let id = dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        let source = std::fs::read_to_string(&path).map_err(|err| BatteryError::Unreadable {
            path: path.clone(),
            why: err.to_string(),
        })?;
        let record = record::parse(&source).map_err(|err| BatteryError::NotARecord {
            path: path.clone(),
            why: err.to_string(),
        })?;
        Self::from_record(&id, &record)
    }

    /// Replay `record` as a responder: one exchange per turn, in order.
    ///
    /// # Errors
    ///
    /// [`BatteryError::TurnWithoutAnswer`] for a turn with no response row,
    /// and [`BatteryError::NoExchanges`] for a record with no turns at all --
    /// a script that answers nothing would report every behaviour failed for
    /// a reason that is about the fixture rather than about a candidate.
    pub fn from_record(id: &str, record: &Record) -> Result<Self, BatteryError> {
        let mut script: Vec<Reply> = Vec::new();
        let mut answered: Vec<bool> = Vec::new();
        for event in &record.events {
            match event {
                Event::Turn { prefill_tokens, .. } => {
                    script.push(Reply {
                        prefill_tokens: *prefill_tokens,
                        ..Reply::default()
                    });
                    answered.push(false);
                }
                Event::ToolCall { tool, args, .. } => {
                    if let Some(current) = script.last_mut() {
                        current.calls.push(ToolInvocation {
                            tool: tool.clone(),
                            args: args.clone().unwrap_or_default(),
                        });
                    }
                }
                Event::Response {
                    output_tokens,
                    text,
                    ..
                } => {
                    if let (Some(current), Some(seen)) = (script.last_mut(), answered.last_mut()) {
                        current.text = text.clone().unwrap_or_default();
                        current.output_tokens = *output_tokens;
                        *seen = true;
                    }
                }
                _ => {}
            }
        }
        if script.is_empty() {
            return Err(BatteryError::NoExchanges);
        }
        if let Some(position) = answered.iter().position(|seen| !seen) {
            return Err(BatteryError::TurnWithoutAnswer {
                turn: u32::try_from(position + 1).unwrap_or(u32::MAX),
            });
        }
        Ok(Self {
            id: id.to_owned(),
            regime: record.regime().clone(),
            script,
            next: 0,
        })
    }

    /// How many scripted exchanges are left unused.
    ///
    /// A script longer than the battery is a script whose later rows answer
    /// nothing, which is how a fixture drifts out of step with the probes and
    /// keeps passing.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.script.len().saturating_sub(self.next)
    }
}

impl Responder for CannedResponder {
    fn id(&self) -> &str {
        &self.id
    }

    fn regime(&self) -> &Regime {
        &self.regime
    }

    /// The next scripted reply, or an empty one once the script runs out.
    ///
    /// Empty rather than a panic: a script that stops early is a candidate
    /// that stopped answering, and the battery already has a verdict for that.
    fn reply(&mut self, _probe: &Probe) -> Reply {
        let reply = self.script.get(self.next).cloned().unwrap_or_default();
        self.next += 1;
        reply
    }
}

/// Why a canned server could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatteryError {
    /// The script is not on disk.
    Unreadable {
        /// Where it was looked for.
        path: PathBuf,
        /// What the filesystem said.
        why: String,
    },
    /// The script is not a record.
    NotARecord {
        /// Which file.
        path: PathBuf,
        /// What the record reader said.
        why: String,
    },
    /// A turn in the script has no response row.
    TurnWithoutAnswer {
        /// Which turn.
        turn: u32,
    },
    /// The script holds no turns.
    NoExchanges,
}

impl fmt::Display for BatteryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { path, why } => {
                write!(
                    f,
                    "{}: cannot read the canned script: {why}",
                    path.display()
                )
            }
            Self::NotARecord { path, why } => {
                write!(f, "{}: not a record: {why}", path.display())
            }
            Self::TurnWithoutAnswer { turn } => {
                write!(f, "turn {turn} of the canned script has no response")
            }
            Self::NoExchanges => {
                f.write_str("the canned script holds no turns, so it answers nothing")
            }
        }
    }
}

impl Error for BatteryError {}

/// Render a value the way the record renders one.
///
/// Here rather than at every caller so that a report printed by the drive and
/// a report pinned by a test are the same bytes.
#[must_use]
pub fn render_value(value: &Value) -> String {
    let mut out = String::new();
    json::render(value, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::{
        ASKS, Battery, Behaviour, CannedResponder, EXCURSION_ENTRIES, OFFERED_TOOL, Outcome, Probe,
        REFILL_ASK, Reply, Responder, ToolInvocation, servers_dir,
    };
    use crate::formats::record::Regime;
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};

    /// What a canned server's `expected.tsv` says about one behaviour.
    ///
    /// An enum with a lookup rather than a comparison against the word on
    /// disk, so that a typo in a fixture is a refusal instead of a silent
    /// `fail`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Expectation {
        Pass,
        Fail,
    }

    impl Expectation {
        const ALL: &'static [Self] = &[Self::Pass, Self::Fail];

        fn tag(self) -> &'static str {
            match self {
                Self::Pass => "pass",
                Self::Fail => "fail",
            }
        }

        fn from_tag(tag: &str) -> Option<Self> {
            Self::ALL.iter().copied().find(|want| want.tag() == tag)
        }
    }

    fn asks_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("capture/battery/asks")
    }

    /// Every canned server directory, refusing an empty corpus.
    fn servers() -> Vec<PathBuf> {
        let dir = servers_dir();
        let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap_or_else(|err| panic!("{}: {err}", dir.display()))
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        found.sort();
        assert!(
            !found.is_empty(),
            "{}: no canned servers, so every assertion over this corpus would hold vacuously",
            dir.display()
        );
        found
    }

    /// What `expected.tsv` in `dir` says, one row per behaviour.
    fn expectations(dir: &Path) -> BTreeMap<Behaviour, Expectation> {
        let path = dir.join("expected.tsv");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
        let mut table = BTreeMap::new();
        for (number, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let mut columns = line.split('\t');
            let (Some(name), Some(want), None) = (columns.next(), columns.next(), columns.next())
            else {
                panic!(
                    "{}:{}: not `behaviour<TAB>pass|fail`",
                    path.display(),
                    number + 1
                );
            };
            let behaviour = Behaviour::from_tag(name.trim()).unwrap_or_else(|| {
                panic!(
                    "{}:{}: `{name}` is not a behaviour",
                    path.display(),
                    number + 1
                )
            });
            let expectation = Expectation::from_tag(want.trim()).unwrap_or_else(|| {
                panic!(
                    "{}:{}: `{want}` is not `pass` or `fail`",
                    path.display(),
                    number + 1
                )
            });
            table.insert(behaviour, expectation);
        }
        table
    }

    /// A responder that answers every probe with the same text and calls.
    #[derive(Debug)]
    struct Flat {
        id: String,
        regime: Regime,
        text: String,
        calls: Vec<ToolInvocation>,
    }

    impl Flat {
        fn new(text: &str) -> Self {
            Self {
                id: "flat".to_owned(),
                regime: canned().regime().clone(),
                text: text.to_owned(),
                calls: Vec::new(),
            }
        }
    }

    impl Responder for Flat {
        fn id(&self) -> &str {
            &self.id
        }

        fn regime(&self) -> &Regime {
            &self.regime
        }

        fn reply(&mut self, _probe: &Probe) -> Reply {
            Reply {
                text: self.text.clone(),
                calls: self.calls.clone(),
                ..Reply::default()
            }
        }
    }

    fn canned() -> CannedResponder {
        let dir = servers_dir().join("compliant");
        CannedResponder::load(&dir).unwrap_or_else(|err| panic!("{}: {err}", dir.display()))
    }

    #[test]
    fn every_behaviour_has_an_ask_and_every_ask_file_is_named_by_one() {
        let mut wanted: BTreeSet<String> = Behaviour::ALL
            .iter()
            .map(|behaviour| format!("{}.txt", behaviour.tag()))
            .collect();
        wanted.insert("survives_seam_refill.refill.txt".to_owned());

        for behaviour in Behaviour::ALL.iter().copied() {
            assert!(
                ASKS.iter().any(|(which, _)| *which == behaviour),
                "`{}` has no row in ASKS, so the battery would ask it nothing",
                behaviour.tag()
            );
            assert!(
                !behaviour.ask().trim().is_empty(),
                "`{}` is asked with an empty prompt",
                behaviour.tag()
            );
        }
        assert!(
            !REFILL_ASK.trim().is_empty(),
            "the refill follow-up is empty"
        );

        let dir = asks_dir();
        let found: BTreeSet<String> = std::fs::read_dir(&dir)
            .unwrap_or_else(|err| panic!("{}: {err}", dir.display()))
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();
        assert_eq!(
            found,
            wanted,
            "{}: the ask files on disk and the ones the battery includes disagree",
            dir.display()
        );
    }

    #[test]
    fn the_corpus_decides_every_canned_server_as_it_is_labelled() {
        let mut checked = 0;
        for dir in servers() {
            let want = expectations(&dir);
            let missing: Vec<&'static str> = Behaviour::ALL
                .iter()
                .copied()
                .filter(|behaviour| !want.contains_key(behaviour))
                .map(Behaviour::tag)
                .collect();
            assert!(
                missing.is_empty(),
                "{}: expected.tsv says nothing about {missing:?}",
                dir.display()
            );

            let mut responder = CannedResponder::load(&dir)
                .unwrap_or_else(|err| panic!("{}: {err}", dir.display()));
            let report = Battery::run(&mut responder);
            assert_eq!(
                responder.remaining(),
                0,
                "{}: the script has rows the battery never asked for, so it has drifted \
                 out of step with the probes",
                dir.display()
            );
            for behaviour in Behaviour::ALL.iter().copied() {
                let outcome = report.outcome(behaviour).unwrap_or_else(|| {
                    panic!("the report says nothing about `{}`", behaviour.tag())
                });
                let got = if outcome.is_pass() {
                    Expectation::Pass
                } else {
                    Expectation::Fail
                };
                assert_eq!(
                    got,
                    want[&behaviour],
                    "{}: `{}` was decided {} -- {}",
                    dir.display(),
                    behaviour.tag(),
                    got.tag(),
                    outcome.reason().unwrap_or("no reason, it passed")
                );
                checked += 1;
            }
        }
        assert!(checked > 0, "the corpus decided nothing");
    }

    #[test]
    fn prose_where_a_tool_was_offered_fails_only_the_tool_behaviour() {
        let dir = servers_dir().join("prose-where-a-tool-was-offered");
        let mut responder =
            CannedResponder::load(&dir).unwrap_or_else(|err| panic!("{}: {err}", dir.display()));
        let report = Battery::run(&mut responder);
        let failed: Vec<&'static str> = report
            .failures()
            .into_iter()
            .map(|(behaviour, _)| behaviour.tag())
            .collect();
        assert_eq!(
            failed,
            vec![Behaviour::UsesOfferedTool.tag()],
            "prose where a tool call was required must fail that behaviour and only that one"
        );
        let reason = report
            .outcome(Behaviour::UsesOfferedTool)
            .and_then(Outcome::reason)
            .unwrap_or_default();
        assert!(
            reason.contains(OFFERED_TOOL.tool),
            "the reason must name the tool that was offered, and says: {reason}"
        );
    }

    #[test]
    fn an_answer_that_never_names_the_refilled_entry_fails_the_seam_behaviour() {
        let mut responder = Flat::new("LEARNED: the object was rendered into the prompt");
        let report = Battery::run(&mut responder);
        let reason = report
            .outcome(Behaviour::SurvivesSeamRefill)
            .and_then(Outcome::reason)
            .unwrap_or_else(|| {
                panic!("an answer that repeats itself after the seam cites no entry id")
            });
        assert!(
            reason.contains("survives_seam_refill/1/answer"),
            "the reason must name the entry the render carried, and says: {reason}"
        );
    }

    #[test]
    fn an_excursion_that_says_one_thing_twice_leaves_a_closure_nothing_to_choose() {
        let mut responder = Flat::new(
            "DECISION: the excursion established one thing\n\
             LEARNED: the excursion established one thing",
        );
        let report = Battery::run(&mut responder);
        let reason = report
            .outcome(Behaviour::ProducesClosableTangent)
            .and_then(Outcome::reason)
            .unwrap_or_else(|| {
                panic!("two fields holding one fact are one entry, and closure needs two")
            });
        assert!(
            reason.contains(&format!("{EXCURSION_ENTRIES}")) && reason.contains("1 distinct"),
            "the reason must say how many entries the excursion left, and says: {reason}"
        );
    }

    #[test]
    fn a_call_to_the_offered_tool_that_carries_no_content_is_not_a_call() {
        let mut responder = Flat::new("DECISION: recorded");
        responder.calls.push(ToolInvocation {
            tool: OFFERED_TOOL.tool.to_owned(),
            args: BTreeMap::new(),
        });
        let report = Battery::run(&mut responder);
        let reason = report
            .outcome(Behaviour::UsesOfferedTool)
            .and_then(Outcome::reason)
            .unwrap_or_else(|| panic!("a call with no arguments does not record anything"));
        assert!(
            reason.contains("argument"),
            "the reason must name the argument that was missing, and says: {reason}"
        );
    }

    #[test]
    fn a_report_decides_every_behaviour_the_vocabulary_declares() {
        let mut responder = canned();
        let report = Battery::run(&mut responder);
        for behaviour in Behaviour::ALL.iter().copied() {
            assert!(
                report.outcome(behaviour).is_some(),
                "the report says nothing about `{}`, so the battery did not put it",
                behaviour.tag()
            );
        }
        assert!(
            report.compliant(),
            "the compliant canned server must pass every behaviour: {:?}",
            report.failures()
        );
    }

    #[test]
    fn a_tag_and_the_behaviour_it_names_are_the_same_round_trip() {
        for behaviour in Behaviour::ALL.iter().copied() {
            assert_eq!(
                Behaviour::from_tag(behaviour.tag()),
                Some(behaviour),
                "`{}` does not read back as itself",
                behaviour.tag()
            );
        }
        assert_eq!(Behaviour::from_tag("answers_beautifully"), None);
    }
}
