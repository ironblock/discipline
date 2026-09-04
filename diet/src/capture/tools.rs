//! Tool-mediated self-capture: the model records the fact at the moment it
//! knows the fact matters.
//!
//! Two capture modalities were fought for before this one. Prose interview and
//! parse: ask, then read the answer. Schema-constrained emission: ask, then
//! spend a second constrained turn reformatting the answer into typed JSON.
//! Both were built against a belief that the model could not emit JSON --
//! which was false, then true at eight out of eight, then true so
//! enthusiastically that one structuring pass emitted 454 entries for a
//! 4,222-character answer and 248 of them existed nowhere.
//!
//! An observation sat in plain sight through all of it: models emit JSON
//! reliably, constantly, for **tool calls**. The struggles and the successes
//! were never the same problem, and four differences all point the same way.
//!
//! 1. A tool call is in-distribution as an *act*, not as a transcription task.
//! 2. It **leaves the content channel**. Every mimicry, momentum and
//!    fence-bleed defect in the parser saga was a content-channel pathology;
//!    a tool call rides the chat template's own parsing instead.
//! 3. Servers constrain tool arguments to a schema natively, so the constraint
//!    is not a sentence in a prompt that the model may decline to honour.
//! 4. "You may use this tool" is advisory **by construction**, and the
//!    injection experiments measured advisory framing as the correct authority
//!    calibration. "Emit only JSON" is an imperative with conflict dynamics.
//!
//! The deepest alignment is not any of the four: a model electing to call
//! [`CaptureTool::UpdateRecord`] the moment something matters is this
//! architecture's founding idea -- capture each fact at the moment it is known
//! to matter -- implemented natively, self-timed, by the only seat holding the
//! salience judgment.
//!
//! [`CaptureTool::ProposePhaseTransition`] returns here from the research
//! phase's early era, where it was the most successful capture-adjacent
//! mechanism the program ever ran. It was shelved on a misattribution: the
//! slowness blamed on it had a diagnosed infrastructure cause, single-slot
//! fork eviction forcing a full re-prefill every turn. The acquittal is why it
//! is a first-class arm here rather than a footnote.
//!
//! **Modality does not exempt a lane from grounding.** A tool call writes with
//! capture authority, so [`apply`] puts everything `update_record` emits
//! through [`super::grounded::check`] against what the model actually saw. The
//! founding fabrication went through a lane that was also confident, also
//! well-formed, and also structurally incapable of being wrong about its own
//! output.
//!
//! # Pre-registration: the three-modality bakeoff
//!
//! The comparison this module exists to make possible cannot be run here.
//! There is no model in this environment, hosted or local, so the endpoints
//! are registered now -- before any number exists to argue with -- and the run
//! is reported as blocked rather than faked.
//!
//! * **Modalities**, on one corpus, paired: prose-and-parse; schema-
//!   constrained emission; tool-mediated (this module).
//! * **Endpoints**: capture coverage; fabrication rate, taken as the
//!   groundedness gate's own counts and not as a judge's opinion; token cost;
//!   and the failure profile per modality -- mimicry rate, and forget rate
//!   over session depth.
//! * **Portability is a criterion and it is nearly decisive.** Prose-and-parse
//!   needs full orchestration control. Schema-constrained emission needs
//!   sampler-level grammar access, which is an *engine* capability. Tool
//!   mediation needs only "register a custom tool", the one universal harness
//!   affordance. For a project whose adoption path is *point it at the harness
//!   you already use*, that ranking holds before any measurement runs. The
//!   JSON path stays as the in-house floor and the measurement baseline.
//! * **Per-model propensity is a profile cell**: willingness to self-record,
//!   and forget rate over session depth. Every model eventually stops and has
//!   to be reminded, which is what [`Reminder`] is for; how soon differs, and
//!   that axis appears to be unmeasured anywhere.
//!
//! The cadence's own value is pre-registered too: [`Cadence::DEFAULT`] is
//! every three silent turns. It is a starting point chosen before data, not a
//! tuned number, and it is data so that measuring it does not mean editing
//! code.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::ops::Range;
use std::slice;

use super::grounded::{self, ContractInput, Floor, LaneReport};
use crate::formats::interview::FieldKind;
use crate::formats::record::json::Value;
use crate::formats::record::{Event, objects};
use crate::object::{EntryId, ObjectError, Patch, Provenance};

/// The lane name every patch from this module carries.
///
/// Self-capture is its own lane rather than a flavour of the canonical one:
/// the bakeoff above compares modalities, and a modality whose patches are
/// indistinguishable in provenance from another's cannot be one of the arms.
pub const LANE: &str = "self-capture";

/// The lane whose prose is the model's own.
///
/// An interview fork's answer is also the model speaking, and it is also in
/// the record -- but it is an answer to a question this system asked, and
/// grounding a self-capture in it would let one lane's output certify
/// another's. [`Seen`] takes prose from this lane only.
pub const CANONICAL_LANE: &str = "main";

/// The contract, as data.
///
/// Shipped beside this file rather than built in Rust, because it is
/// registered with a foreign harness: the adapters are a later issue, and what
/// they will register is this file's bytes, not a rendering of a struct that
/// happens to agree with it today.
const CONTRACT: &str = include_str!("tools/contract.jsonl");

// ---------------------------------------------------------------------------
// the contract
// ---------------------------------------------------------------------------

/// A tool this lane offers the model.
///
/// Closed, and an enum rather than a name: a tool call arriving from a harness
/// carries whatever name the harness had, and the one thing this module must
/// never do is treat somebody else's tool as a source of facts about the
/// session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CaptureTool {
    /// Add or amend an entry in a named field.
    UpdateRecord,
    /// Say what became of an entry.
    ResolveEntry,
    /// Ask to advance the session to the next phase. Advisory.
    ProposePhaseTransition,
}

impl CaptureTool {
    /// Every tool the contract may offer.
    pub const ALL: &'static [Self] = &[
        Self::UpdateRecord,
        Self::ResolveEntry,
        Self::ProposePhaseTransition,
    ];

    /// The name the tool is registered under.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::UpdateRecord => "update_record",
            Self::ResolveEntry => "resolve_entry",
            Self::ProposePhaseTransition => "propose_phase_transition",
        }
    }

    /// The tool `tag` names, if this lane owns one by that name.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|tool| tool.tag() == tag)
    }
}

/// What a parameter accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ParamType {
    /// Free text.
    Text,
    /// The identity of an entry the object already holds.
    EntryRef,
    /// One of a listed set of words.
    Choice,
}

impl ParamType {
    /// Every parameter type the contract may use.
    pub const ALL: &'static [Self] = &[Self::Text, Self::EntryRef, Self::Choice];

    /// The name this type is written under in the contract.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Text => "string",
            Self::EntryRef => "entry_id",
            Self::Choice => "enum",
        }
    }

    /// The type `tag` names, if the contract knows one.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.tag() == tag)
    }
}

/// One parameter of one tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parameter {
    /// What the argument is called.
    pub name: String,
    /// What it accepts.
    pub kind: ParamType,
    /// Whether a call without it is a call at all.
    pub required: bool,
    /// The admissible words, for a [`ParamType::Choice`]; empty otherwise.
    pub choices: Vec<String>,
}

/// One tool's whole contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSpec {
    /// Which tool.
    pub tool: CaptureTool,
    /// What the model is told the tool is for.
    pub description: String,
    /// Its parameters, in the contract's order.
    pub parameters: Vec<Parameter>,
}

impl ToolSpec {
    /// The parameter called `name`, if the tool has one.
    #[must_use]
    pub fn parameter(&self, name: &str) -> Option<&Parameter> {
        self.parameters.iter().find(|param| param.name == name)
    }
}

/// Why the contract file is not a contract.
///
/// The schema is closed: a key nobody reads is a key a harness will be
/// registered with and nothing will honour, which is the quiet half of a
/// contract drifting from its implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractError {
    /// The file is not readable as JSON lines.
    Unreadable(String),
    /// A row names a tool this lane does not offer.
    UnknownTool(String),
    /// Two rows name one tool.
    DuplicateTool(String),
    /// A tool in [`CaptureTool::ALL`] that the file does not describe.
    Undescribed(&'static str),
    /// A required key is missing.
    MissingKey {
        /// Where: a tool name, or `tool.parameter`.
        at: String,
        /// Which key.
        key: String,
    },
    /// A key the schema does not define.
    UnexpectedKey {
        /// Where.
        at: String,
        /// Which key.
        key: String,
    },
    /// A key holding the wrong sort of value.
    WrongType {
        /// Where.
        at: String,
        /// Which key.
        key: String,
        /// What it should have held.
        want: &'static str,
    },
    /// A parameter whose `type` names no [`ParamType`].
    UnknownParamType {
        /// Where.
        at: String,
        /// What it said.
        named: String,
    },
    /// A choice parameter with no choices, or a non-choice parameter with
    /// some. Either way the `of` list and the `type` disagree, and which one
    /// the harness honours decides what the model may say.
    ChoicesDisagree {
        /// Where.
        at: String,
    },
}

impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable(why) => write!(f, "the contract is not JSON lines: {why}"),
            Self::UnknownTool(name) => {
                write!(f, "the contract offers `{name}`, which this lane does not")
            }
            Self::DuplicateTool(name) => write!(f, "the contract describes `{name}` twice"),
            Self::Undescribed(name) => {
                write!(
                    f,
                    "`{name}` is offered and the contract does not describe it"
                )
            }
            Self::MissingKey { at, key } => write!(f, "{at}: no `{key}`"),
            Self::UnexpectedKey { at, key } => write!(
                f,
                "{at}: `{key}` is a key nothing reads, so a harness would be \
                 registered with a promise nothing keeps"
            ),
            Self::WrongType { at, key, want } => write!(f, "{at}: `{key}` is not {want}"),
            Self::UnknownParamType { at, named } => {
                write!(f, "{at}: `{named}` is not a parameter type")
            }
            Self::ChoicesDisagree { at } => write!(
                f,
                "{at}: the `of` list and the `type` disagree about whether this \
                 parameter is a closed choice"
            ),
        }
    }
}

impl Error for ContractError {}

/// The contract, read from the shipped file.
///
/// # Errors
///
/// Returns [`ContractError`] for a file that is not JSON lines, a tool this
/// lane does not offer, a tool it offers that the file omits, or any key the
/// closed schema above does not define.
pub fn contract() -> Result<Vec<ToolSpec>, ContractError> {
    read_contract(CONTRACT)
}

/// Read a contract out of `text`.
///
/// Separate from [`contract`] so that the closed schema can be shown refusing
/// things. A schema nobody has watched reject a key is a schema that is closed
/// in the comment and open in the code.
fn read_contract(text: &str) -> Result<Vec<ToolSpec>, ContractError> {
    let rows = objects(text).map_err(|err| ContractError::Unreadable(err.to_string()))?;
    let mut specs: Vec<ToolSpec> = Vec::with_capacity(rows.len());
    for mut row in rows {
        let name = take_text(&mut row, CONTRACT_AT, "tool")?;
        let tool = CaptureTool::from_tag(&name).ok_or(ContractError::UnknownTool(name))?;
        let at = tool.tag().to_owned();
        let description = take_text(&mut row, &at, "description")?;
        let parameters = take_parameters(&mut row, &at)?;
        no_other_keys(&row, &at)?;
        if specs.iter().any(|spec| spec.tool == tool) {
            return Err(ContractError::DuplicateTool(at));
        }
        specs.push(ToolSpec {
            tool,
            description,
            parameters,
        });
    }
    for tool in CaptureTool::ALL {
        if !specs.iter().any(|spec| spec.tool == *tool) {
            return Err(ContractError::Undescribed(tool.tag()));
        }
    }
    Ok(specs)
}

/// Where a row's own errors are reported before its tool is known.
const CONTRACT_AT: &str = "contract.jsonl";

/// Take a string-valued key out of an object.
fn take_text(
    object: &mut BTreeMap<String, Value>,
    at: &str,
    key: &str,
) -> Result<String, ContractError> {
    match object.remove(key) {
        Some(Value::String(text)) => Ok(text),
        Some(_) => Err(ContractError::WrongType {
            at: at.to_owned(),
            key: key.to_owned(),
            want: "a string",
        }),
        None => Err(ContractError::MissingKey {
            at: at.to_owned(),
            key: key.to_owned(),
        }),
    }
}

/// Refuse whatever the schema left behind.
///
/// The schema is the sequence of removes that read the object, and this is
/// asked once they have all run: anything still in the map is a key nothing
/// read. A list of admissible keys beside the reads would be a second copy of
/// the schema, kept in step by hand, and a key added to the list and to
/// nothing else would be admitted and then ignored -- which is the promise
/// nothing keeps that this check exists to refuse.
fn no_other_keys(object: &BTreeMap<String, Value>, at: &str) -> Result<(), ContractError> {
    match object.keys().next() {
        Some(key) => Err(ContractError::UnexpectedKey {
            at: at.to_owned(),
            key: key.clone(),
        }),
        None => Ok(()),
    }
}

/// Read one row's `parameters` object.
fn take_parameters(
    row: &mut BTreeMap<String, Value>,
    at: &str,
) -> Result<Vec<Parameter>, ContractError> {
    let members = match row.remove("parameters") {
        Some(Value::Object(members)) => members,
        Some(_) => {
            return Err(ContractError::WrongType {
                at: at.to_owned(),
                key: "parameters".to_owned(),
                want: "an object",
            });
        }
        None => {
            return Err(ContractError::MissingKey {
                at: at.to_owned(),
                key: "parameters".to_owned(),
            });
        }
    };
    members
        .into_iter()
        .map(|(name, value)| parameter(at, name, value))
        .collect()
}

/// Read one parameter.
fn parameter(at: &str, name: String, value: Value) -> Result<Parameter, ContractError> {
    let where_ = format!("{at}.{name}");
    let Value::Object(mut members) = value else {
        return Err(ContractError::WrongType {
            at: at.to_owned(),
            key: name,
            want: "an object",
        });
    };
    let named = take_text(&mut members, &where_, "type")?;
    let kind = ParamType::from_tag(&named).ok_or(ContractError::UnknownParamType {
        at: where_.clone(),
        named,
    })?;
    let required = match members.remove("required") {
        Some(Value::Boolean(flag)) => flag,
        Some(_) => {
            return Err(ContractError::WrongType {
                at: where_,
                key: "required".to_owned(),
                want: "a boolean",
            });
        }
        None => {
            return Err(ContractError::MissingKey {
                at: where_,
                key: "required".to_owned(),
            });
        }
    };
    let choices = match members.remove("of") {
        Some(Value::Array(items)) => items
            .into_iter()
            .map(|item| match item {
                Value::String(text) => Ok(text),
                _ => Err(ContractError::WrongType {
                    at: where_.clone(),
                    key: "of".to_owned(),
                    want: "a list of strings",
                }),
            })
            .collect::<Result<Vec<String>, ContractError>>()?,
        Some(_) => {
            return Err(ContractError::WrongType {
                at: where_,
                key: "of".to_owned(),
                want: "a list of strings",
            });
        }
        None => Vec::new(),
    };
    if choices.is_empty() == (kind == ParamType::Choice) {
        return Err(ContractError::ChoicesDisagree { at: where_ });
    }
    no_other_keys(&members, &where_)?;
    Ok(Parameter {
        name,
        kind,
        required,
        choices,
    })
}

// ---------------------------------------------------------------------------
// what a call produces
// ---------------------------------------------------------------------------

/// What the model says became of an entry.
///
/// The collector owns supersession *detection* and the grammar this vocabulary
/// is written in; this enum is the shape a `resolve_entry` argument decodes to,
/// so that the two lanes can be built apart and joined by mapping one onto the
/// other rather than by one waiting on the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Confirmation {
    /// Settled. The entry is resolved.
    Done,
    /// Partly settled, which is not settled: nothing is written.
    Partial,
    /// The wrong entry. Nothing is written.
    NotThis,
    /// Replaced by something said since. The replacement is a fact, and a fact
    /// arrives through `update_record`'s `supersedes`, so this verdict alone
    /// writes nothing either.
    Superseded,
}

impl Confirmation {
    /// Every verdict a `resolve_entry` call may carry.
    pub const ALL: &'static [Self] = &[Self::Done, Self::Partial, Self::NotThis, Self::Superseded];

    /// The word this verdict is written as.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Partial => "partial",
            Self::NotThis => "not_this",
            Self::Superseded => "superseded",
        }
    }

    /// The verdict `written` names, whatever case it arrived in.
    ///
    /// Case-folded because the word travels through a chat template and a
    /// harness before it reaches here, and a verdict rejected for its shift
    /// key is a verdict the model gave and nobody counted.
    #[must_use]
    pub fn from_written(written: &str) -> Option<Self> {
        let normalised = written.trim().to_ascii_lowercase();
        Self::ALL
            .iter()
            .copied()
            .find(|verdict| verdict.tag() == normalised)
    }
}

/// A verdict on an entry, for the collector to reconcile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirmed {
    /// The entry the model named.
    pub entry: EntryId,
    /// What it said became of it.
    pub verdict: Confirmation,
}

/// A request to advance the session to the next phase.
///
/// A typed event and never a patch. The proposal is a request for a ruling;
/// writing it into the object as a fact would make the model's wish for a
/// phase change indistinguishable from a phase change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    /// The tool call that made it.
    pub from_call: String,
    /// The turn it was made in.
    pub at_turn: u32,
    /// The phase proposed.
    pub to: String,
    /// Why.
    pub reason: String,
}

/// What one capture tool call did.
///
/// Not a bare `Vec<Patch>`. Two of the three tools legitimately write no
/// patch, and the interesting thing about each of those calls is what it
/// produced *instead* -- an advisory proposal, a verdict for the collector, or
/// a groundedness report saying an entry was dropped and why. A return type
/// that could only carry patches would drop all three on the floor, and the
/// dropped-entry case is the one the acceptance is written against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Effect {
    /// Patches for the reconciler, in emission order.
    pub patches: Vec<Patch>,
    /// What the groundedness gate said, when the call wrote content.
    pub grounding: Option<LaneReport>,
    /// The verdict the collector reconciles, when the call gave one.
    pub confirmation: Option<Confirmed>,
    /// The advisory proposal, when the call made one.
    pub proposal: Option<Proposal>,
}

/// Why a tool call produced nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolError {
    /// The event is not a tool call at all.
    NotAToolCall(&'static str),
    /// A tool this lane does not own. A harness tool call is not ours, and
    /// reading one as a capture is how another system's output becomes a fact
    /// about this session.
    NotACaptureTool(String),
    /// A ledger row: the record kept no arguments, so there is nothing to
    /// apply. Not an error about the model.
    NoArguments(String),
    /// A required argument the call did not carry.
    MissingArgument {
        /// Which tool.
        tool: &'static str,
        /// Which parameter.
        parameter: String,
    },
    /// An argument the contract does not define.
    UnknownArgument {
        /// Which tool.
        tool: &'static str,
        /// What it was called.
        parameter: String,
    },
    /// An argument of the wrong sort.
    WrongArgument {
        /// Which tool.
        tool: &'static str,
        /// Which parameter.
        parameter: String,
        /// What the contract says it accepts.
        want: &'static str,
    },
    /// A choice argument outside the contract's list.
    NotAChoice {
        /// Which tool.
        tool: &'static str,
        /// Which parameter.
        parameter: String,
        /// What it said.
        given: String,
    },
    /// A `field` argument the interview vocabulary does not know. Distinct
    /// from [`ToolError::NotAChoice`]: this is the contract and the field
    /// vocabulary having drifted apart, not the model saying something odd.
    UnknownField(String),
    /// An entry id the object cannot hold.
    BadEntry(ObjectError),
    /// The contract itself is broken, so no call can be checked against it.
    NoContract(ContractError),
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAToolCall(kind) => write!(f, "a `{kind}` row is not a tool call"),
            Self::NotACaptureTool(name) => write!(
                f,
                "`{name}` is not a capture tool: a harness tool call is not \
                 ours, and reading one as a capture makes another system's \
                 output a fact about this session"
            ),
            Self::NoArguments(id) => write!(
                f,
                "the row `{id}` kept no arguments, so there is no call to apply"
            ),
            Self::MissingArgument { tool, parameter } => {
                write!(f, "{tool}: no `{parameter}`")
            }
            Self::UnknownArgument { tool, parameter } => {
                write!(f, "{tool}: `{parameter}` is not in the contract")
            }
            Self::WrongArgument {
                tool,
                parameter,
                want,
            } => write!(f, "{tool}: `{parameter}` is not {want}"),
            Self::NotAChoice {
                tool,
                parameter,
                given,
            } => write!(f, "{tool}: `{parameter}` may not be {given:?}"),
            Self::UnknownField(named) => write!(
                f,
                "the contract admits the field `{named}` and the interview \
                 vocabulary does not know it"
            ),
            Self::BadEntry(err) => write!(f, "the call names an entry that cannot exist: {err}"),
            Self::NoContract(err) => {
                write!(f, "the contract is broken, so no call is checkable: {err}")
            }
        }
    }
}

impl Error for ToolError {}

// ---------------------------------------------------------------------------
// what the model saw
// ---------------------------------------------------------------------------

/// What the model had in front of it when it made a call.
///
/// Assembled from the record rather than passed in beside it, because the
/// gate's whole value is that its input is *what happened* and not what a
/// caller believed happened.
///
/// A self-capture is never grounded in itself, and that takes two exclusions
/// rather than one. A call's `args` are not read here at all: this takes a
/// tool call's `output` and a response's `text`. And a *capture* tool's
/// `output` is not read either. A harness that answers `update_record` with
/// `recorded: <content>` is the ordinary shape, and a gate that admitted that
/// line would let a fabrication certify itself by being repeated back to the
/// model that made it up.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seen {
    source: String,
    session_prefix: String,
}

impl Seen {
    /// What the model saw by turn `turn`.
    ///
    /// The source is that turn's tool output and that turn's own prose on the
    /// canonical lane. Everything from earlier turns is the session prefix:
    /// visible, and not what the lane was told to work from. The split is the
    /// finding the gate is built on -- a string traceable to the prefix was
    /// read, one traceable to nothing was invented, and the two need different
    /// repairs.
    #[must_use]
    pub fn of_turn(events: &[Event], turn: u32) -> Self {
        let canonical = canonical_requests(events);
        let mut seen = Self::default();
        let mut walking = 0_u32;
        for event in events {
            match event {
                Event::Turn { index, .. } => walking = *index,
                Event::ToolCall {
                    at_turn,
                    tool,
                    output: Some(output),
                    ..
                } if CaptureTool::from_tag(tool).is_none() => seen.push(*at_turn, turn, output),
                Event::Response {
                    to_request,
                    text: Some(text),
                    ..
                } if canonical.contains(to_request.as_str()) => seen.push(walking, turn, text),
                _ => {}
            }
        }
        seen
    }

    /// File `text`, seen at `at`, against the turn being grounded.
    fn push(&mut self, at: u32, turn: u32, text: &str) {
        let bucket = match at.cmp(&turn) {
            Ordering::Equal => &mut self.source,
            Ordering::Less => &mut self.session_prefix,
            // Nothing from a later turn was in front of the model here.
            Ordering::Greater => return,
        };
        bucket.push_str(text);
        bucket.push('\n');
    }

    /// As the groundedness gate's contract input.
    #[must_use]
    pub fn contract_input(&self) -> ContractInput<'_> {
        ContractInput {
            source: &self.source,
            session_prefix: &self.session_prefix,
        }
    }
}

/// The ids of every request sent on the canonical lane.
fn canonical_requests(events: &[Event]) -> BTreeSet<&str> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Request { id, lane, .. } if lane == CANONICAL_LANE => Some(id.as_str()),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// applying a call
// ---------------------------------------------------------------------------

/// The floor a self-capture call is judged against.
///
/// Structural, not measured, and it says so: one call writes one entry, so the
/// lane's score is 1/1 or 0/1 and every floor above zero and at or below one
/// gives the same verdict. There is no number here that a later measurement
/// could contradict, which is the only honest way to pre-register a threshold
/// before the lane has ever run.
fn floor() -> Floor {
    Floor::pre_registered(
        1,
        1,
        "structural: one call writes one entry, so the lane grounds all of its \
         output or none of it",
    )
    .expect("1/1 with a provenance is a well-formed floor")
}

/// Apply one capture tool call.
///
/// `index` is where this call sat in the lane's emission for its turn; the
/// turn itself comes from the row, which already carries it. A turn passed
/// beside a row that states its own turn is a second copy that can disagree
/// with the first, and the record schema refuses that shape for the same
/// reason everywhere else.
///
/// # Errors
///
/// Returns [`ToolError`] for an event that is not a tool call, a tool this
/// lane does not own, a ledger row with no arguments, and any argument the
/// contract refuses.
pub fn apply(call: &Event, index: u32, saw: ContractInput<'_>) -> Result<Effect, ToolError> {
    let Event::ToolCall {
        id,
        at_turn,
        tool,
        args,
        ..
    } = call
    else {
        return Err(ToolError::NotAToolCall(call.kind().tag()));
    };
    let Some(named) = CaptureTool::from_tag(tool) else {
        return Err(ToolError::NotACaptureTool(tool.clone()));
    };
    let args = args
        .as_ref()
        .ok_or_else(|| ToolError::NoArguments(id.clone()))?;
    let specs = contract().map_err(ToolError::NoContract)?;
    let spec = specs
        .iter()
        .find(|spec| spec.tool == named)
        .ok_or(ToolError::NoContract(ContractError::Undescribed(
            named.tag(),
        )))?;
    let args = admissible(spec, args)?;

    let provenance = Provenance {
        turn: *at_turn,
        lane: LANE.to_owned(),
        fork: None,
        index,
    };
    match named {
        CaptureTool::UpdateRecord => update_record(id, &args, saw, provenance),
        CaptureTool::ResolveEntry => resolve_entry(&args, provenance),
        CaptureTool::ProposePhaseTransition => Ok(advisory(Proposal {
            from_call: id.clone(),
            at_turn: *at_turn,
            to: text_argument(&args, "to"),
            reason: text_argument(&args, "reason"),
        })),
    }
}

/// A proposal writes nothing.
///
/// Its own function so that "advisory" is one statement in one place rather
/// than a habit at three call sites. The phase-transition tool was the most
/// successful capture-adjacent mechanism the program ran, and it was that
/// because it asked; a version that wrote would be a different mechanism
/// wearing its name.
fn advisory(proposal: Proposal) -> Effect {
    Effect {
        proposal: Some(proposal),
        ..Effect::default()
    }
}

/// Check a call's arguments against the tool's contract, and spell every
/// closed choice the way the contract spells it.
///
/// The contract is not decoration: what it admits is what this lane accepts,
/// so a parameter deleted from the file stops being accepted here rather than
/// staying accepted by code that has forgotten the file exists.
///
/// Case is decided **here and nowhere else**. A choice word travels through a
/// chat template and a harness before it arrives, so it is matched without
/// regard to case -- and then replaced by the contract's own spelling, so that
/// everything downstream reads one word. Two places each deciding what
/// `SUPERSEDED` means is how a vocabulary ends up with a second opinion.
fn admissible(
    spec: &ToolSpec,
    args: &BTreeMap<String, Value>,
) -> Result<BTreeMap<String, String>, ToolError> {
    let tool = spec.tool.tag();
    let mut given: BTreeMap<String, String> = BTreeMap::new();
    for (name, value) in args {
        let Some(param) = spec.parameter(name) else {
            return Err(ToolError::UnknownArgument {
                tool,
                parameter: name.clone(),
            });
        };
        let Value::String(text) = value else {
            return Err(ToolError::WrongArgument {
                tool,
                parameter: name.clone(),
                want: "text",
            });
        };
        let settled = if param.kind == ParamType::Choice {
            let Some(choice) = param
                .choices
                .iter()
                .find(|choice| choice.eq_ignore_ascii_case(text.trim()))
            else {
                return Err(ToolError::NotAChoice {
                    tool,
                    parameter: name.clone(),
                    given: text.clone(),
                });
            };
            choice.clone()
        } else {
            text.clone()
        };
        given.insert(name.clone(), settled);
    }
    for param in &spec.parameters {
        if param.required && !given.contains_key(&param.name) {
            return Err(ToolError::MissingArgument {
                tool,
                parameter: param.name.clone(),
            });
        }
    }
    Ok(given)
}

/// A required argument's text. Present because [`admissible`] has established
/// it is.
fn text_argument(args: &BTreeMap<String, String>, name: &str) -> String {
    args.get(name).cloned().unwrap_or_default()
}

/// `update_record`: a fact, if the model actually saw it.
fn update_record(
    call: &str,
    args: &BTreeMap<String, String>,
    saw: ContractInput<'_>,
    provenance: Provenance,
) -> Result<Effect, ToolError> {
    let named = text_argument(args, "field");
    let field = field_kind(&named).ok_or(ToolError::UnknownField(named.clone()))?;
    let content = text_argument(args, "content");
    // Derived from the row that carried it, never minted. An entry that cannot
    // be walked back to the event it came from is an entry whose provenance is
    // a claim.
    let id = EntryId::new(&format!("{call}/{named}")).map_err(ToolError::BadEntry)?;

    let report = grounded::check(slice::from_ref(&content), field, saw, &floor());
    let kept = !report.kept().is_empty();
    let mut patches = Vec::new();
    if kept {
        patches.push(match args.get("supersedes") {
            Some(voids) => Patch::Supersede {
                id,
                content,
                voids: EntryId::new(voids).map_err(ToolError::BadEntry)?,
                provenance,
            },
            None => Patch::Add {
                id,
                content,
                provenance,
            },
        });
    }
    Ok(Effect {
        patches,
        grounding: Some(report),
        ..Effect::default()
    })
}

/// `resolve_entry`: a verdict, and the one patch a verdict alone can justify.
fn resolve_entry(
    args: &BTreeMap<String, String>,
    provenance: Provenance,
) -> Result<Effect, ToolError> {
    let entry = EntryId::new(&text_argument(args, "entry")).map_err(ToolError::BadEntry)?;
    let written = text_argument(args, "verdict");
    let verdict = Confirmation::from_written(&written).ok_or(ToolError::NotAChoice {
        tool: CaptureTool::ResolveEntry.tag(),
        parameter: "verdict".to_owned(),
        given: written,
    })?;
    // Only `Done` is a patch. `Superseded` needs the replacing content, which
    // arrives through `update_record`'s `supersedes`; writing a supersession
    // from a verdict alone would have to invent the fact that replaces the old
    // one, and inventing content is the failure this whole directory exists
    // to prevent.
    let patches = if verdict == Confirmation::Done {
        vec![Patch::Resolve {
            target: entry.clone(),
            provenance,
        }]
    } else {
        Vec::new()
    };
    Ok(Effect {
        patches,
        confirmation: Some(Confirmed { entry, verdict }),
        ..Effect::default()
    })
}

/// The interview field a contract tag names.
fn field_kind(tag: &str) -> Option<FieldKind> {
    FieldKind::ALL
        .iter()
        .copied()
        .find(|kind| kind.canonical_tag() == tag)
}

// ---------------------------------------------------------------------------
// the reminder cadence
// ---------------------------------------------------------------------------

/// How often a silent lane is reminded.
///
/// Data rather than a literal in the code, because the propensity axis this
/// module pre-registers is measured by varying it. Every model eventually
/// stops recording; how many turns that takes is the cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cadence {
    /// How many consecutive silent turns pass before a reminder.
    every: u32,
}

impl Cadence {
    /// The pre-registered starting point: every three silent turns.
    pub const DEFAULT: Self = Self { every: 3 };

    /// A cadence of `every` silent turns.
    ///
    /// # Errors
    ///
    /// Returns [`CadenceError::Never`] for zero, which reads as either "remind
    /// every turn" or "never remind" depending on who is asking. A setting
    /// with two readings is a reminder that silently stops firing.
    pub fn every(every: u32) -> Result<Self, CadenceError> {
        if every == 0 {
            return Err(CadenceError::Never);
        }
        Ok(Self { every })
    }

    /// How many silent turns pass before a reminder.
    #[must_use]
    pub fn interval(self) -> u32 {
        self.every
    }
}

/// Why a cadence is not a cadence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CadenceError {
    /// A cadence of zero turns.
    Never,
}

impl fmt::Display for CadenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Never => write!(
                f,
                "a cadence of zero turns reads as both `every turn` and `never`"
            ),
        }
    }
}

impl Error for CadenceError {}

/// What this lane asks for.
///
/// The router owns the full ask vocabulary and the templates it renders from;
/// these are the two kinds self-capture produces, named here so that the two
/// lanes can be built apart. Joining them is mapping this onto that, which is
/// a table, not a rewrite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AskKind {
    /// The cadence firing: the model has recorded nothing for a while.
    Reminder,
    /// The recovery sweep: a turn went by with nothing recorded, and the
    /// interview asks what self-capture missed.
    Sweep,
}

impl AskKind {
    /// Every kind this lane produces.
    pub const ALL: &'static [Self] = &[Self::Reminder, Self::Sweep];

    /// The name this kind is written under.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Reminder => "reminder",
            Self::Sweep => "sweep",
        }
    }

    /// The words this kind asks in.
    ///
    /// Advisory, both of them. An imperative here is the framing the injection
    /// experiments measured as producing over-pivoting and sycophantic
    /// capitulation, and the whole argument for tool mediation is that its
    /// framing is advisory by construction -- so the reminder that guards it
    /// must not be an order.
    #[must_use]
    pub fn opening(self) -> &'static str {
        match self {
            Self::Reminder => "Anything you meant to record?",
            Self::Sweep => "What did this turn establish that a later turn would need?",
        }
    }
}

/// One ask, addressed to one turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    /// Which kind.
    pub kind: AskKind,
    /// The turn it is about.
    pub at_turn: u32,
    /// What the router put off in that turn, when it put something off.
    pub about: Option<String>,
}

impl Ask {
    /// The ask, in words.
    #[must_use]
    pub fn text(&self) -> String {
        match &self.about {
            Some(about) => format!(
                "{} It went by without an answer: {about}.",
                self.kind.opening()
            ),
            None => self.kind.opening().to_owned(),
        }
    }
}

/// The reminder cadence, and the record of what self-capture missed.
///
/// A model that records eagerly for twenty turns and then stops is the common
/// case, not the pathological one, and a lane that only offers a tool has no
/// way to notice. This is that way: it counts silent turns, asks on the
/// cadence, and keeps enough to sweep the turns nobody captured once the drive
/// is over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reminder {
    cadence: Cadence,
    since: u32,
    silent: BTreeSet<u32>,
    deferred: BTreeMap<u32, String>,
}

impl Reminder {
    /// A reminder on `cadence`.
    #[must_use]
    pub fn on(cadence: Cadence) -> Self {
        Self {
            cadence,
            since: 0,
            silent: BTreeSet::new(),
            deferred: BTreeMap::new(),
        }
    }

    /// Observe one turn, and ask if the cadence has come round.
    ///
    /// A turn that recorded something resets the count: the cadence measures
    /// silence, not elapsed time, and a model recording every turn should
    /// never be asked whether it meant to.
    ///
    /// A driver that watches a turn go by before that turn's capture lands
    /// observes the same turn twice, and the later word wins: a turn that
    /// recorded in the end is struck off the sweep it was already on. Asking
    /// again about a fact the model did record is the one thing a recovery
    /// sweep must not do, because it teaches that recording changes nothing.
    pub fn observe(&mut self, turn: u32, saw_update_record: bool) -> Option<Ask> {
        if saw_update_record {
            self.since = 0;
            self.silent.remove(&turn);
            return None;
        }
        self.silent.insert(turn);
        self.since += 1;
        if self.since < self.cadence.interval() {
            return None;
        }
        self.since = 0;
        Some(Ask {
            kind: AskKind::Reminder,
            at_turn: turn,
            about: self.deferred.get(&turn).cloned(),
        })
    }

    /// Record that a turn's ask was put off.
    ///
    /// The router decides deferral, by class, and this is where its decision
    /// lands so that the sweep can pick it back up. Kept here rather than read
    /// out of the router because the sweep runs after the drive, when the
    /// router's per-turn state is gone.
    pub fn deferred(&mut self, turn: u32, about: &str) {
        self.deferred.insert(turn, about.to_owned());
    }

    /// The turns in `turns` that recorded nothing.
    #[must_use]
    pub fn missed(&self, turns: Range<u32>) -> Vec<u32> {
        self.silent.range(turns).copied().collect()
    }

    /// The asks that cover what self-capture missed over `turns`.
    ///
    /// One ask per silent turn, carrying whatever the router put off in that
    /// turn. A sweep that asked once about the whole range would get one
    /// answer about the most recent thing, which is the forget rate the
    /// bakeoff measures, applied to the mechanism meant to fix it.
    #[must_use]
    pub fn recovery_sweep(&self, turns: Range<u32>) -> Vec<Ask> {
        self.missed(turns)
            .into_iter()
            .map(|at_turn| Ask {
                kind: AskKind::Sweep,
                at_turn,
                about: self.deferred.get(&at_turn).cloned(),
            })
            .collect()
    }
}

impl Default for Reminder {
    fn default() -> Self {
        Self::on(Cadence::DEFAULT)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};

    use super::{
        Ask, AskKind, CONTRACT, Cadence, CadenceError, CaptureTool, Confirmation, ContractError,
        Effect, LANE, ParamType, Reminder, Seen, ToolError, ToolSpec, apply, contract,
        read_contract,
    };
    use crate::capture::grounded::{ContractInput, Verdict};
    use crate::formats::interview::FieldKind;
    use crate::formats::record::json::Value;
    use crate::formats::record::{Count, Event, objects, parse};
    use crate::object::{EntryId, Patch};

    /// What the model was told to work from in the turn under test.
    const SOURCE: &str = "diet/src/object.rs:412: fn supersede\n\
                          It voids the old entry and links the pair.";

    /// What it could see and was not told to work from.
    const PREFIX: &str = "Earlier the resolver refused a stale binary.";

    fn saw() -> ContractInput<'static> {
        ContractInput {
            source: SOURCE,
            session_prefix: PREFIX,
        }
    }

    /// A tool call row, with string arguments.
    fn call(id: &str, at_turn: u32, tool: &str, args: &[(&str, &str)]) -> Event {
        Event::ToolCall {
            id: id.to_owned(),
            at_turn,
            tool: tool.to_owned(),
            args: Some(
                args.iter()
                    .map(|(key, value)| ((*key).to_owned(), Value::String((*value).to_owned())))
                    .collect(),
            ),
            exit: Some(0),
            output: None,
        }
    }

    /// An `update_record` call writing `content` into `field`.
    fn update(id: &str, at_turn: u32, field: FieldKind, content: &str) -> Event {
        call(
            id,
            at_turn,
            CaptureTool::UpdateRecord.tag(),
            &[("content", content), ("field", field.canonical_tag())],
        )
    }

    fn spec_for(specs: &[ToolSpec], tool: CaptureTool) -> &ToolSpec {
        specs
            .iter()
            .find(|spec| spec.tool == tool)
            .unwrap_or_else(|| panic!("the contract describes `{}`", tool.tag()))
    }

    // -----------------------------------------------------------------
    // the contract
    // -----------------------------------------------------------------

    #[test]
    fn the_contract_describes_every_tool_this_lane_offers_and_no_other() {
        let specs = contract().expect("the shipped contract reads");
        let mut described: Vec<CaptureTool> = specs.iter().map(|spec| spec.tool).collect();
        described.sort_unstable();
        let mut offered: Vec<CaptureTool> = CaptureTool::ALL.to_vec();
        offered.sort_unstable();
        assert_eq!(
            described, offered,
            "a tool offered and undescribed cannot be registered, and one \
             described and unoffered is a promise nothing keeps"
        );
    }

    /// Phrases that turn an offer into an order.
    ///
    /// Small on purpose, and not a grammar of politeness: these are the shapes
    /// that make "you may use this tool" into "use this tool", which is the
    /// framing this modality was chosen for not having.
    const ORDERS: &[&str] = &["you must", "you should", "always call", "do not fail to"];

    /// The fewest words a tool can be described in and still say when to call
    /// it. A floor rather than a judgement: prose cannot be asserted, and a
    /// one-character description passing for a description is the failure
    /// this catches.
    const DESCRIBED_IN: usize = 12;

    #[test]
    fn every_tool_is_offered_in_words_rather_than_ordered() {
        for spec in contract().expect("the shipped contract reads") {
            let words = spec.description.split_whitespace().count();
            assert!(
                words >= DESCRIBED_IN,
                "`{}` is described in {words} word(s): a foreign harness \
                 registers this text verbatim, and it is all the model is told \
                 about when the tool is worth calling",
                spec.tool.tag()
            );
            let lowered = spec.description.to_ascii_lowercase();
            for order in ORDERS {
                assert!(
                    !lowered.contains(order),
                    "`{}` is described as an order (`{order}`): advisory \
                     framing is the fourth argument for this whole modality, \
                     and it lives in these bytes",
                    spec.tool.tag()
                );
            }
        }
    }

    /// The other direction -- that every [`Confirmation`] the lane knows is
    /// admissible -- is proved by driving each of them through `apply` in
    /// `only_a_settled_entry_is_resolved_by_a_verdict_alone`. This is the
    /// direction that ships a word to a harness.
    #[test]
    fn every_verdict_the_contract_admits_is_one_this_lane_can_honour() {
        let specs = contract().expect("the shipped contract reads");
        let verdict = spec_for(&specs, CaptureTool::ResolveEntry)
            .parameter("verdict")
            .expect("`resolve_entry` takes a verdict");
        for choice in &verdict.choices {
            assert!(
                Confirmation::from_written(choice).is_some(),
                "the contract offers a harness `{choice}` and this lane \
                 refuses it at runtime: the model is invited to say a word \
                 that is then thrown away"
            );
        }
    }

    #[test]
    fn a_contract_that_omits_an_offered_tool_is_refused() {
        let one = r#"{"tool":"update_record","description":"d","parameters":{"content":{"type":"string","required":true}}}"#;
        let refused = read_contract(one)
            .expect_err("an offered tool with no row is refused before a harness ever sees it");
        assert!(
            matches!(refused, ContractError::Undescribed(_)),
            "a tool this lane offers and the file omits was accepted as {refused:?}"
        );
    }

    #[test]
    fn a_contract_that_describes_one_tool_twice_is_refused() {
        let first = CONTRACT
            .lines()
            .next()
            .expect("the contract has a first row");
        let twice = format!("{CONTRACT}{first}\n");
        let refused = read_contract(&twice)
            .expect_err("one tool described twice leaves the harness to the order of the file");
        assert!(
            matches!(refused, ContractError::DuplicateTool(_)),
            "a tool described twice was accepted as {refused:?}"
        );
    }

    #[test]
    fn every_field_kind_is_admissible_in_the_contract() {
        let specs = contract().expect("the shipped contract reads");
        let field = spec_for(&specs, CaptureTool::UpdateRecord)
            .parameter("field")
            .expect("`update_record` takes a field");
        assert_eq!(field.kind, ParamType::Choice);
        for kind in FieldKind::ALL {
            assert!(
                field.choices.iter().any(|of| of == kind.canonical_tag()),
                "the contract does not admit `{}`, so the model cannot record \
                 into a field the interview format defines",
                kind.canonical_tag()
            );
        }
        for choice in &field.choices {
            assert!(
                FieldKind::ALL
                    .iter()
                    .any(|kind| kind.canonical_tag() == choice),
                "the contract admits `{choice}`, which is not an interview field"
            );
        }
    }

    #[test]
    fn the_contract_schema_refuses_a_key_nothing_reads() {
        let row = r#"{"tool":"update_record","description":"d","parameters":{"content":{"type":"string","required":true}},"examples":[]}"#;
        let refused = read_contract(row).expect_err("a key nothing reads is refused");
        assert!(
            matches!(refused, ContractError::UnexpectedKey { .. }),
            "an unread key was accepted as {refused:?}"
        );
    }

    #[test]
    fn the_contract_schema_refuses_a_parameter_key_nothing_reads() {
        let row = r#"{"tool":"resolve_entry","description":"d","parameters":{"entry":{"type":"entry_id","required":true,"default":"e1"}}}"#;
        let refused = read_contract(row).expect_err("a key nothing reads is refused");
        assert!(
            matches!(refused, ContractError::UnexpectedKey { .. }),
            "an unread parameter key was accepted as {refused:?}"
        );
    }

    #[test]
    fn a_choice_parameter_and_its_list_must_agree() {
        let no_list = r#"{"tool":"resolve_entry","description":"d","parameters":{"verdict":{"type":"enum","required":true}}}"#;
        assert!(
            matches!(
                read_contract(no_list),
                Err(ContractError::ChoicesDisagree { .. })
            ),
            "a closed choice with nothing to choose from was accepted"
        );
        let stray_list = r#"{"tool":"resolve_entry","description":"d","parameters":{"entry":{"type":"entry_id","required":true,"of":["e1"]}}}"#;
        assert!(
            matches!(
                read_contract(stray_list),
                Err(ContractError::ChoicesDisagree { .. })
            ),
            "a free-text parameter carrying a closed list was accepted"
        );
    }

    #[test]
    fn every_capture_tool_is_reachable_from_its_tag() {
        for tool in CaptureTool::ALL {
            assert_eq!(CaptureTool::from_tag(tool.tag()), Some(*tool));
        }
        assert_eq!(CaptureTool::from_tag("bash"), None);
    }

    #[test]
    fn every_parameter_type_is_reachable_from_its_tag() {
        for kind in ParamType::ALL {
            assert_eq!(ParamType::from_tag(kind.tag()), Some(*kind));
        }
        assert_eq!(ParamType::from_tag("float"), None);
    }

    // -----------------------------------------------------------------
    // grounding
    // -----------------------------------------------------------------

    #[test]
    fn a_self_captured_entry_absent_from_what_the_model_saw_is_dropped() {
        let invented = "WorkingObject::rollback_supersede";
        let effect = apply(&update("t9", 4, FieldKind::ApiSurface, invented), 0, saw())
            .expect("a well-formed call");
        assert!(
            effect.patches.is_empty(),
            "modality does not exempt a lane from grounding: `{invented}` is in \
             nothing the model saw, and it was written anyway as {:?}",
            effect.patches
        );
        let report = effect
            .grounding
            .expect("an `update_record` call is always grounded");
        assert_eq!(
            report.count(Verdict::Invention),
            1,
            "the entry was dropped without being called an invention"
        );
    }

    #[test]
    fn an_entry_the_model_only_saw_earlier_is_dropped_as_a_bleed() {
        let effect = apply(
            &update(
                "t9",
                4,
                FieldKind::Evidence,
                "Earlier the resolver refused a stale binary.",
            ),
            0,
            saw(),
        )
        .expect("a well-formed call");
        assert!(
            effect.patches.is_empty(),
            "a lane's contract input is what it was told to work from, not \
             everything it could see"
        );
        let report = effect.grounding.expect("grounded");
        assert_eq!(report.count(Verdict::Bleed), 1);
    }

    #[test]
    fn a_grounded_self_capture_becomes_a_patch_on_the_self_capture_lane() {
        let effect = apply(
            &update("t2", 1, FieldKind::Evidence, "fn supersede"),
            3,
            saw(),
        )
        .expect("a well-formed call");
        let [
            Patch::Add {
                id,
                content,
                provenance,
            },
        ] = effect.patches.as_slice()
        else {
            panic!(
                "a grounded capture is one added entry, not {:?}",
                effect.patches
            );
        };
        assert_eq!(
            id.as_str(),
            "t2/evidence",
            "an entry id is derived from the row that carried it, never minted"
        );
        assert_eq!(content, "fn supersede");
        assert_eq!(provenance.lane, LANE);
        assert_eq!(provenance.turn, 1);
        assert_eq!(provenance.index, 3);
        assert_eq!(provenance.fork, None);
    }

    #[test]
    fn a_judgment_field_is_written_without_being_grounded() {
        let effect = apply(
            &update(
                "t4",
                2,
                FieldKind::Plan,
                "Try the resolver against a stale build.",
            ),
            0,
            saw(),
        )
        .expect("a well-formed call");
        assert_eq!(
            effect.patches.len(),
            1,
            "grounding a plan is a category error, and doing it rejects \
             legitimate content"
        );
    }

    #[test]
    fn naming_what_it_replaces_makes_the_capture_a_supersede() {
        let row = call(
            "t7",
            2,
            CaptureTool::UpdateRecord.tag(),
            &[
                ("content", "fn supersede"),
                ("field", FieldKind::Evidence.canonical_tag()),
                ("supersedes", "t2/evidence"),
            ],
        );
        let effect = apply(&row, 0, saw()).expect("a well-formed call");
        let [
            Patch::Supersede {
                id,
                content,
                voids,
                provenance,
            },
        ] = effect.patches.as_slice()
        else {
            panic!(
                "a capture naming what it replaces is a supersede, not {:?}",
                effect.patches
            );
        };
        assert_eq!(
            id.as_str(),
            "t7/evidence",
            "a superseding entry id must be derived from the row that carried \
             it, and this one was minted"
        );
        assert_eq!(
            content, "fn supersede",
            "a supersede that replaces a live entry with nothing is a deletion \
             wearing a link"
        );
        assert_eq!(
            voids,
            &EntryId::new("t2/evidence").expect("a well-formed id")
        );
        assert_eq!(
            provenance.lane, LANE,
            "a modality whose patches are indistinguishable in provenance from \
             another's cannot be one of the bakeoff's arms"
        );
        assert_eq!(provenance.turn, 2);
    }

    // -----------------------------------------------------------------
    // whose call it is
    // -----------------------------------------------------------------

    #[test]
    fn a_harness_tool_call_is_not_a_patch_source() {
        let foreign = call("t1", 1, "bash", &[("command", "ls diet/src")]);
        let refused = apply(&foreign, 0, saw())
            .expect_err("a harness tool call is not a capture tool and writes nothing");
        assert!(
            matches!(refused, ToolError::NotACaptureTool(_)),
            "a foreign tool was refused for the wrong reason: {refused:?}"
        );
    }

    #[test]
    fn a_row_that_is_not_a_tool_call_is_not_a_call() {
        let turn = Event::Turn {
            index: 1,
            prefill_tokens: Count::new(10).expect("a small count"),
        };
        assert!(matches!(
            apply(&turn, 0, saw()),
            Err(ToolError::NotAToolCall(_))
        ));
    }

    #[test]
    fn a_ledger_row_carrying_no_arguments_is_not_a_call() {
        let ledger = Event::ToolCall {
            id: "t1".to_owned(),
            at_turn: 1,
            tool: CaptureTool::UpdateRecord.tag().to_owned(),
            args: None,
            exit: Some(0),
            output: None,
        };
        assert!(matches!(
            apply(&ledger, 0, saw()),
            Err(ToolError::NoArguments(_))
        ));
    }

    #[test]
    fn an_argument_the_contract_does_not_define_is_refused() {
        let odd = call(
            "t1",
            1,
            CaptureTool::UpdateRecord.tag(),
            &[
                ("content", "fn supersede"),
                ("field", FieldKind::Evidence.canonical_tag()),
                ("confidence", "high"),
            ],
        );
        assert!(matches!(
            apply(&odd, 0, saw()),
            Err(ToolError::UnknownArgument { .. })
        ));
    }

    #[test]
    fn a_call_missing_a_required_argument_is_refused() {
        let bare = call(
            "t1",
            1,
            CaptureTool::UpdateRecord.tag(),
            &[("field", FieldKind::Evidence.canonical_tag())],
        );
        assert!(matches!(
            apply(&bare, 0, saw()),
            Err(ToolError::MissingArgument { .. })
        ));
    }

    #[test]
    fn a_choice_arrives_in_whatever_case_the_harness_used() {
        let shouted = call(
            "t1",
            1,
            CaptureTool::ResolveEntry.tag(),
            &[("entry", "t2/evidence"), ("verdict", " SUPERSEDED ")],
        );
        let effect = apply(&shouted, 0, saw()).expect("a well-formed verdict");
        assert_eq!(
            effect.confirmation.expect("a verdict is reported").verdict,
            Confirmation::Superseded,
            "a verdict rejected for its shift key is a verdict the model gave \
             and nobody counted"
        );
    }

    #[test]
    fn an_argument_in_the_case_the_harness_used_is_settled_by_the_contract() {
        let shouted = call(
            "t2",
            1,
            CaptureTool::UpdateRecord.tag(),
            &[("content", "fn supersede"), ("field", "EVIDENCE")],
        );
        let effect =
            apply(&shouted, 0, saw()).expect("a harness may shout a closed choice back at us");
        let [Patch::Add { id, .. }] = effect.patches.as_slice() else {
            panic!(
                "a grounded capture is one added entry, not {:?}",
                effect.patches
            );
        };
        assert_eq!(
            id.as_str(),
            "t2/evidence",
            "case is settled against the contract's own spelling and nowhere \
             else: a second opinion downstream mints `t2/EVIDENCE` beside \
             `t2/evidence` for one entry"
        );
    }

    #[test]
    fn a_field_outside_the_contracts_list_is_refused() {
        let odd = call(
            "t1",
            1,
            CaptureTool::UpdateRecord.tag(),
            &[("content", "fn supersede"), ("field", "vibes")],
        );
        assert!(matches!(
            apply(&odd, 0, saw()),
            Err(ToolError::NotAChoice { .. })
        ));
    }

    // -----------------------------------------------------------------
    // verdicts and proposals
    // -----------------------------------------------------------------

    #[test]
    fn every_confirmation_is_reachable_from_what_the_model_writes() {
        for verdict in Confirmation::ALL {
            assert_eq!(Confirmation::from_written(verdict.tag()), Some(*verdict));
            assert_eq!(
                Confirmation::from_written(&verdict.tag().to_ascii_uppercase()),
                Some(*verdict),
                "a verdict rejected for its shift key is a verdict nobody counted"
            );
        }
        assert_eq!(Confirmation::from_written("maybe"), None);
    }

    #[test]
    fn only_a_settled_entry_is_resolved_by_a_verdict_alone() {
        for verdict in Confirmation::ALL {
            let row = call(
                "t8",
                5,
                CaptureTool::ResolveEntry.tag(),
                &[("entry", "t2/evidence"), ("verdict", verdict.tag())],
            );
            let effect = apply(&row, 0, saw()).expect("a well-formed verdict");
            let confirmed = effect.confirmation.expect("a verdict is reported");
            assert_eq!(confirmed.verdict, *verdict);
            assert_eq!(
                confirmed.entry.as_str(),
                "t2/evidence",
                "the collector was handed a verdict about an entry the model \
                 did not name"
            );
            let want = usize::from(*verdict == Confirmation::Done);
            assert_eq!(
                effect.patches.len(),
                want,
                "`{}` wrote {} patch(es): a replacement is a fact, and a fact \
                 arrives with its content or not at all",
                verdict.tag(),
                effect.patches.len()
            );
            if *verdict == Confirmation::Done {
                let [Patch::Resolve { target, provenance }] = effect.patches.as_slice() else {
                    panic!("`done` settles the entry, and wrote {:?}", effect.patches);
                };
                assert_eq!(
                    target.as_str(),
                    "t2/evidence",
                    "a verdict resolved an entry the model never named, which \
                     is the one patch a verdict alone is allowed to justify \
                     pointed somewhere else"
                );
                assert_eq!(provenance.lane, LANE);
            }
        }
    }

    #[test]
    fn a_phase_transition_proposal_is_advisory_and_writes_nothing() {
        let row = call(
            "t6",
            3,
            CaptureTool::ProposePhaseTransition.tag(),
            &[
                ("reason", "the reconciler is covered and the gate is green"),
                ("to", "review"),
            ],
        );
        let effect = apply(&row, 0, saw()).expect("a well-formed proposal");
        assert!(
            effect.patches.is_empty(),
            "a phase-transition proposal is advisory and writes nothing, and \
             this one wrote {:?}",
            effect.patches
        );
        let proposal = effect.proposal.expect("the proposal is reported");
        assert_eq!(proposal.to, "review");
        assert_eq!(proposal.at_turn, 3);
        assert_eq!(proposal.from_call, "t6");
        assert_eq!(
            proposal.reason, "the reconciler is covered and the gate is green",
            "a proposal is a request for a ruling, and the reason is the whole \
             of what it carries into one"
        );
    }

    // -----------------------------------------------------------------
    // the reminder cadence
    // -----------------------------------------------------------------

    #[test]
    fn ten_silent_turns_are_reminded_on_the_cadence_and_the_sweep_covers_them() {
        let mut reminder = Reminder::default();
        let fired: Vec<u32> = (1..=10)
            .filter(|turn| reminder.observe(*turn, false).is_some())
            .collect();
        assert_eq!(
            fired,
            vec![3, 6, 9],
            "ten silent turns must be reminded at every third turn, and were \
             reminded at {fired:?}"
        );
        let swept: Vec<u32> = reminder
            .recovery_sweep(1..11)
            .iter()
            .map(|ask| ask.at_turn)
            .collect();
        assert_eq!(
            swept,
            (1..=10).collect::<Vec<u32>>(),
            "the recovery sweep must cover every turn self-capture missed, and \
             covered {swept:?}"
        );
    }

    #[test]
    fn a_turn_that_recorded_something_resets_the_cadence() {
        let mut reminder = Reminder::default();
        let fired: Vec<u32> = (1..=10)
            .filter(|turn| reminder.observe(*turn, *turn == 2).is_some())
            .collect();
        assert_eq!(
            fired,
            vec![5, 8],
            "the cadence measures silence, not elapsed time"
        );
        assert_eq!(reminder.missed(1..11), vec![1, 3, 4, 5, 6, 7, 8, 9, 10]);
    }

    #[test]
    fn the_sweep_carries_what_the_router_put_off() {
        let mut reminder = Reminder::default();
        reminder.deferred(2, "a write whose effect nobody asked about");
        for turn in 1..=3 {
            reminder.observe(turn, false);
        }
        let asks = reminder.recovery_sweep(2..3);
        let [ask] = asks.as_slice() else {
            panic!("one silent turn in range, and {} asks", asks.len());
        };
        assert_eq!(
            ask.kind,
            AskKind::Sweep,
            "the recovery sweep asked as a {:?}, so the sweep and the cadence \
             are one ask wearing two names",
            ask.kind
        );
        assert_eq!(
            ask.text(),
            format!(
                "{} It went by without an answer: a write whose effect nobody \
                 asked about.",
                AskKind::Sweep.opening()
            ),
            "an ask carrying a deferral is the question and then the deferral, \
             and this one is neither: {}",
            ask.text()
        );
    }

    /// What each kind is called and what it asks, written out rather than
    /// read off the enum. A test that iterates the table it is guarding
    /// certifies the table against itself, and passes when the table is
    /// emptied.
    const WORDS: &[(AskKind, &str, &str)] = &[
        (
            AskKind::Reminder,
            "reminder",
            "Anything you meant to record?",
        ),
        (
            AskKind::Sweep,
            "sweep",
            "What did this turn establish that a later turn would need?",
        ),
    ];

    #[test]
    fn every_ask_kind_is_named_and_asks_in_its_own_words() {
        for (kind, tag, opening) in WORDS {
            assert_eq!(kind.tag(), *tag);
            assert_eq!(
                kind.opening(),
                *opening,
                "`{tag}` asks something else now, and the words this lane says \
                 out loud are the only product it has"
            );
        }
        assert_eq!(
            WORDS.len(),
            AskKind::ALL.len(),
            "a kind this lane produces is not in the words above, or a kind is \
             in the words above and nothing produces it"
        );
        for kind in AskKind::ALL {
            assert!(
                WORDS.iter().any(|(named, _, _)| named == kind),
                "`{}` asks in words nothing checks",
                kind.tag()
            );
        }
    }

    #[test]
    fn an_ask_without_a_deferral_is_the_opening_alone() {
        let ask = Ask {
            kind: AskKind::Reminder,
            at_turn: 3,
            about: None,
        };
        assert_eq!(ask.text(), AskKind::Reminder.opening());
    }

    #[test]
    fn a_fired_reminder_carries_its_kind_and_what_the_router_put_off() {
        let mut reminder = Reminder::default();
        reminder.deferred(3, "a write whose effect nobody asked about");
        let fired: Vec<Ask> = (1..=3)
            .filter_map(|turn| reminder.observe(turn, false))
            .collect();
        let [ask] = fired.as_slice() else {
            panic!("three silent turns fire the cadence once, and fired {fired:?}");
        };
        assert_eq!(ask.kind, AskKind::Reminder);
        assert_eq!(ask.at_turn, 3);
        assert_eq!(
            ask.about.as_deref(),
            Some("a write whose effect nobody asked about"),
            "the cadence reminded without what the router put off in that \
             turn, so the deferral reaches only the post-drive sweep"
        );
    }

    #[test]
    fn a_turn_that_records_after_it_looked_silent_is_not_swept() {
        let mut reminder = Reminder::default();
        reminder.observe(2, false);
        reminder.observe(2, true);
        assert!(
            !reminder.missed(1..4).contains(&2),
            "a turn the model did record in the end was swept anyway, which \
             teaches that recording changes nothing"
        );
    }

    #[test]
    fn a_cadence_of_no_turns_is_refused() {
        assert_eq!(Cadence::every(0), Err(CadenceError::Never));
        assert_eq!(
            Cadence::every(4)
                .expect("a cadence of four turns")
                .interval(),
            4
        );
        assert_eq!(Cadence::DEFAULT.interval(), 3);
    }

    // -----------------------------------------------------------------
    // what the model saw
    // -----------------------------------------------------------------

    fn request(id: &str, lane: &str) -> Event {
        Event::Request {
            id: id.to_owned(),
            lane: lane.to_owned(),
            retry_of: None,
            text: None,
        }
    }

    fn response(id: &str, to: &str, text: &str) -> Event {
        Event::Response {
            id: id.to_owned(),
            to_request: to.to_owned(),
            output_tokens: Count::new(1).expect("a small count"),
            text: Some(text.to_owned()),
        }
    }

    fn tool_output(id: &str, at_turn: u32, output: &str) -> Event {
        Event::ToolCall {
            id: id.to_owned(),
            at_turn,
            tool: "bash".to_owned(),
            args: None,
            exit: Some(0),
            output: Some(output.to_owned()),
        }
    }

    fn turn_row(index: u32) -> Event {
        Event::Turn {
            index,
            prefill_tokens: Count::new(1).expect("a small count"),
        }
    }

    /// The same call, with what the harness answered it.
    fn answered(call: Event, output: &str) -> Event {
        let Event::ToolCall {
            id,
            at_turn,
            tool,
            args,
            exit,
            ..
        } = call
        else {
            panic!("only a tool call is answered");
        };
        Event::ToolCall {
            id,
            at_turn,
            tool,
            args,
            exit,
            output: Some(output.to_owned()),
        }
    }

    #[test]
    fn what_the_model_saw_splits_this_turn_from_everything_before_it() {
        let events = vec![
            turn_row(1),
            request("q1", super::CANONICAL_LANE),
            tool_output("t1", 1, "the first turn's output"),
            response("a1", "q1", "the first turn's prose"),
            turn_row(2),
            request("q2", super::CANONICAL_LANE),
            request("q3", "interview"),
            tool_output("t2", 2, "the second turn's output"),
            response("a2", "q2", "the second turn's prose"),
            response("a3", "q3", "an interview answer"),
        ];
        let seen = Seen::of_turn(&events, 2);
        let input = seen.contract_input();
        assert!(input.source.contains("the second turn's output"));
        assert!(input.source.contains("the second turn's prose"));
        assert!(
            !input.source.contains("an interview answer"),
            "an answer this system asked for is not the model's own prose"
        );
        assert!(input.session_prefix.contains("the first turn's output"));
        assert!(input.session_prefix.contains("the first turn's prose"));
        assert!(!input.source.contains("the first turn's output"));
    }

    #[test]
    fn a_capture_is_not_grounded_in_a_turn_the_model_had_not_reached() {
        let only_later = "diet/src/object.rs:168:    Retired,";
        let events = vec![
            turn_row(1),
            request("q1", super::CANONICAL_LANE),
            response("a1", "q1", "Looking at what supersede does."),
            update("c1", 1, FieldKind::Evidence, only_later),
            turn_row(2),
            request("q2", super::CANONICAL_LANE),
            tool_output("t2", 2, only_later),
            response("a2", "q2", "Retire marks the entry and keeps it."),
        ];
        let seen = Seen::of_turn(&events, 1);
        let effect = apply(&events[3], 0, seen.contract_input()).expect("a well-formed call");
        assert!(
            effect.patches.is_empty(),
            "a turn-1 capture was grounded in a turn-2 tool output, which was \
             not in front of the model when it wrote: {:?}",
            effect.patches
        );
        let report = effect
            .grounding
            .expect("an `update_record` call is grounded");
        assert_eq!(
            report.count(Verdict::Invention),
            1,
            "a string the model had not yet seen is an invention, not a bleed: \
             the two need different repairs"
        );
    }

    #[test]
    fn a_capture_is_not_grounded_in_a_harness_repeating_it_back() {
        let invented = "WorkingObject::rollback_supersede";
        let events = vec![
            turn_row(1),
            request("q1", super::CANONICAL_LANE),
            response("a1", "q1", "Nothing surprising in the object."),
            answered(
                update("c1", 1, FieldKind::ApiSurface, invented),
                &format!("recorded: {invented}"),
            ),
        ];
        let seen = Seen::of_turn(&events, 1);
        let effect = apply(&events[3], 0, seen.contract_input()).expect("a well-formed call");
        assert!(
            effect.patches.is_empty(),
            "a harness that repeats a capture back grounded the capture in \
             itself, so `{invented}` certified its own invention: {:?}",
            effect.patches
        );
        assert_eq!(
            effect
                .grounding
                .expect("an `update_record` call is grounded")
                .count(Verdict::Invention),
            1
        );
    }

    // -----------------------------------------------------------------
    // the corpus
    // -----------------------------------------------------------------

    fn corpus_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("capture")
            .join("tools")
            .join("corpus")
    }

    fn files_in(dir: &Path) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap_or_else(|err| panic!("{}: {err}", dir.display()))
            .map(|entry| entry.expect("a readable directory").path())
            .collect();
        paths.sort();
        paths
    }

    /// Whether `path` is a corpus case rather than its expectation.
    fn is_case(path: &Path) -> bool {
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"))
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
    }

    /// What replaying one corpus case produced.
    #[derive(Debug, PartialEq, Eq)]
    struct Replayed {
        captures: Vec<(String, usize, String)>,
        reminders: Vec<u32>,
        sweep: Vec<u32>,
    }

    /// Drive the lane over a record the way a session driver would.
    fn replay(source: &str) -> Replayed {
        let record = parse(source).expect("a corpus case is a record");
        let events = record.events.as_slice();
        let mut captures = Vec::new();
        let mut per_turn: BTreeMap<u32, u32> = BTreeMap::new();
        let mut recorded: BTreeSet<u32> = BTreeSet::new();
        let mut turns = 0_u32;
        for event in events {
            match event {
                Event::Turn { index, .. } => turns = turns.max(*index),
                Event::ToolCall {
                    id, at_turn, tool, ..
                } => {
                    let Some(named) = CaptureTool::from_tag(tool) else {
                        continue;
                    };
                    let index = per_turn.entry(*at_turn).or_default();
                    let at = *index;
                    *index += 1;
                    if named == CaptureTool::UpdateRecord {
                        recorded.insert(*at_turn);
                    }
                    let seen = Seen::of_turn(events, *at_turn);
                    let effect = apply(event, at, seen.contract_input())
                        .unwrap_or_else(|err| panic!("{id}: {err}"));
                    captures.push((id.clone(), effect.patches.len(), produced_by(&effect)));
                }
                _ => {}
            }
        }
        let mut reminder = Reminder::default();
        let mut reminders = Vec::new();
        for turn in 1..=turns {
            if reminder.observe(turn, recorded.contains(&turn)).is_some() {
                reminders.push(turn);
            }
        }
        let sweep = reminder
            .recovery_sweep(1..turns + 1)
            .iter()
            .map(|ask| ask.at_turn)
            .collect();
        Replayed {
            captures,
            reminders,
            sweep,
        }
    }

    /// The word an expectation names a call's effect by.
    ///
    /// Three tools, three shapes. A call that wrote content is named by the
    /// gate's verdict on it, a verdict call by the word the model chose, and a
    /// proposal by [`ADVISORY`] -- which is the point of that tool and not a
    /// missing verdict. Reading the grounding report of all three is what kept
    /// two of this lane's tools out of its own corpus.
    fn produced_by(effect: &Effect) -> String {
        if let Some(report) = effect.grounding.as_ref() {
            let [entry] = report.entries.as_slice() else {
                panic!(
                    "one call writes one entry, and this one wrote {}",
                    report.entries.len()
                );
            };
            return entry.verdict.name().to_owned();
        }
        if let Some(confirmed) = effect.confirmation.as_ref() {
            return confirmed.verdict.tag().to_owned();
        }
        assert!(
            effect.proposal.is_some(),
            "a capture tool call grounds content, gives a verdict or proposes, \
             and this one did none of the three"
        );
        ADVISORY.to_owned()
    }

    /// What an expectation calls a call that wrote nothing on purpose.
    const ADVISORY: &str = "advisory";

    fn expectation(source: &str) -> BTreeMap<String, Value> {
        let mut rows = objects(source).expect("an expectation is JSON");
        assert_eq!(rows.len(), 1, "an expectation is one object on one line");
        rows.remove(0)
    }

    fn turn_list(row: &BTreeMap<String, Value>, key: &str) -> Vec<u32> {
        let Some(Value::Array(items)) = row.get(key) else {
            panic!("the expectation has no `{key}` list");
        };
        items
            .iter()
            .map(|item| {
                let Value::Integer(number) = item else {
                    panic!("`{key}` holds something that is not a turn");
                };
                u32::try_from(*number).expect("a turn number is small")
            })
            .collect()
    }

    fn capture_list(row: &BTreeMap<String, Value>) -> Vec<(String, usize, String)> {
        let Some(Value::Array(items)) = row.get("captures") else {
            panic!("the expectation has no `captures` list");
        };
        items
            .iter()
            .map(|item| {
                let Value::Object(members) = item else {
                    panic!("a capture expectation is an object");
                };
                let Some(Value::String(id)) = members.get("call") else {
                    panic!("a capture expectation names no call");
                };
                let Some(Value::Integer(patches)) = members.get("patches") else {
                    panic!("a capture expectation counts no patches");
                };
                let Some(Value::String(produced)) = members.get("produced") else {
                    panic!("a capture expectation says nothing about what the call produced");
                };
                (
                    id.clone(),
                    usize::try_from(*patches).expect("a patch count is small"),
                    produced.clone(),
                )
            })
            .collect()
    }

    #[test]
    fn every_corpus_case_is_paired_with_its_expectation() {
        let dir = corpus_dir();
        let mut cases = 0;
        for path in files_in(&dir) {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_else(|| panic!("{}: not a UTF-8 name", path.display()));
            if name.ends_with(".expected.json") {
                let case = dir.join(name.replace(".expected.json", ".jsonl"));
                assert!(
                    case.is_file(),
                    "{name}: an expectation with no case, so nothing checks it"
                );
                continue;
            }
            assert!(
                is_case(&path),
                "{name}: neither a case nor an expectation, so the walk would \
                 skip it silently"
            );
            assert!(
                dir.join(name.replace(".jsonl", ".expected.json")).is_file(),
                "{name}: a case with no expectation, so replaying it asserts \
                 nothing"
            );
            cases += 1;
        }
        assert!(
            cases >= 3,
            "{}: {cases} case(s); a corpus that walks nothing passes vacuously",
            dir.display()
        );
    }

    /// One case covering a tool is one case away from covering nothing. The
    /// corpus could not hold a `resolve_entry` or a `propose_phase_transition`
    /// call at all until the replay stopped reading every call as content, and
    /// a corpus shaped by what its helper can represent is a corpus that
    /// certifies the tools it happens to like.
    #[test]
    fn the_corpus_drives_every_tool_this_lane_offers() {
        let mut called: BTreeSet<CaptureTool> = BTreeSet::new();
        for path in files_in(&corpus_dir()) {
            if !is_case(&path) {
                continue;
            }
            let record = parse(&read(&path)).expect("a corpus case is a record");
            for event in &record.events {
                if let Event::ToolCall { tool, .. } = event {
                    called.extend(CaptureTool::from_tag(tool));
                }
            }
        }
        for tool in CaptureTool::ALL {
            assert!(
                called.contains(tool),
                "`{}` is offered to the model and no corpus case ever calls it",
                tool.tag()
            );
        }
    }

    #[test]
    fn the_corpus_replays_to_the_captures_and_reminders_it_states() {
        let dir = corpus_dir();
        let mut checked = 0;
        for path in files_in(&dir) {
            if !is_case(&path) {
                continue;
            }
            let name = path.display().to_string();
            let expected = expectation(&read(
                &dir.join(
                    path.file_name()
                        .and_then(|file| file.to_str())
                        .expect("a UTF-8 name")
                        .replace(".jsonl", ".expected.json"),
                ),
            ));
            let want = Replayed {
                captures: capture_list(&expected),
                reminders: turn_list(&expected, "reminders"),
                sweep: turn_list(&expected, "sweep"),
            };
            assert_eq!(replay(&read(&path)), want, "{name}: replayed differently");
            checked += 1;
        }
        assert!(checked >= 3, "only {checked} corpus case(s) were replayed");
    }
}
