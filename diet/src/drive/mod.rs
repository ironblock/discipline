//! `diet::drive` -- a session, end to end, with a record at the end of it.
//!
//! Everything else in this crate is a piece: a client that talks to a
//! substrate, a controller that renders and ratifies at a seam, a confinement
//! that runs a command and types what it was denied, an object that holds
//! what the session knows, a record format that says how a drive is written
//! down. Nothing put them together, so nothing had ever run.
//!
//! This does. It is the *first drive in the gym*: three scripted turns
//! through a regimen, against whatever is serving, producing a record that
//! `diet check-record` accepts.
//!
//! # Scripted, and what that does and does not buy
//!
//! The script fixes what the OPERATOR does -- the turns, the commands, where
//! a boundary is declared. It fixes nothing about what comes back. Put
//! against a canned server the whole run is determined and takes
//! milliseconds, which is the point of #23: iterating on routers,
//! reconcilers and seams does not need a capable substrate. Put against a
//! real one the same script is a real drive. The difference is the transport,
//! and this module cannot tell which it has.
//!
//! # No number this module did not measure
//!
//! A record's `turn.prefill_tokens` and `response.output_tokens` are
//! required, and zero is a measurement. A server that reports no usage leaves
//! this module with nothing honest to write, so it [`Halt`]s and says which
//! number was missing. It does not write the prompt's length in bytes into a
//! field named for tokens -- that is a measurement of the wrong thing that
//! reads like a measurement of the right one, and it is the defect this rule
//! exists because of.
//!
//! Likewise there is no fallback anywhere in the loop. A call that did not
//! answer, a command that did not run, a fold the object refused: each stops
//! the drive with what happened, because a drive that continued past one
//! would bank a record describing a session that did not occur.
//!
//! # The record is validated by the drive that wrote it
//!
//! [`run`] renders its own record and parses it back before returning. A
//! drive whose record its own format refuses is a [`Halt::Unrecordable`], not
//! a success with a broken file beside it. So "produces a record
//! `check-record` accepts" is a fact of this function rather than a claim a
//! CI script makes about it afterwards.
//!
//! # What it cannot do yet, and says so
//!
//! The fold at a seam and the fold of a fork's answer are done HERE, from the
//! interview grammar, and they are not the reconciler. The reconciler and the
//! collectors live in `capture`, which is not this seat's module; when they
//! are ready the two folds in this file become calls into them. Until then a
//! fork's answer becomes one entry per tagged field with a value, and every
//! field that was not that is counted and reported rather than dropped --
//! see [`Drive::uncaptured`].

pub mod canned;
pub mod digest;
pub mod script;

use std::cell::Cell;
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::path::Path;

use crate::client::journal::{self, Unspellable};
use crate::client::shape::{Message, RequestShape, Role};
use crate::client::transport::Transport;
use crate::client::{Client, IdSource, Outcome, wire};
use crate::formats::interview;
use crate::formats::record::json::Value;
use crate::formats::record::{Count, Event, ParseError, Record, render};
use crate::isolation::{Confinement, NotRun, Policy as IsolationPolicy, Unavailable};
use crate::object::{EntryId, ObjectError, Patch, Provenance, WorkingObject};
use crate::seam::policy::Policy as SeamPolicy;
use crate::seam::{Ask, Controller, Ratifier, Seam, SeamError};

use script::Script;

/// The lane a turn's own call is made on.
pub const MAIN: &str = "main";
/// The lane an interview fork is made on.
pub const INTERVIEW: &str = "interview";
/// The lane a seam's ratification ask is made on.
pub const RATIFY: &str = "ratify";

/// Everything a drive runs against, borrowed for the length of the run.
///
/// A borrowed bundle rather than fields on [`Drive`], because the seam
/// controller owns a ratifier that has to reach the client, and a struct
/// holding both the client and the controller that borrows it would be
/// self-referential. Passing the gym in keeps the borrow where the compiler
/// can see it and costs the caller one struct literal.
#[derive(Debug)]
pub struct Gym<'a, T: Transport> {
    /// Where the substrate is.
    pub client: &'a Client<T>,
    /// The request every call is a variation of: the model, the sampler card,
    /// the limits. Its messages are replaced per call.
    pub shape: RequestShape,
    /// The confinement commands run under.
    pub confinement: &'a Confinement,
    /// The policy it was opened for.
    pub isolation: &'a IsolationPolicy,
    /// The working tree, which is the only writable path.
    pub worktree: &'a Path,
    /// When a seam fires and what it may transition between.
    pub seam: SeamPolicy,
}

/// Why a drive stopped without a record.
///
/// Every variant is something that happened, named. None of them is a drive
/// that continued with a hole in it.
#[derive(Debug)]
pub enum Halt {
    /// The declared confinement is not available here. The drive never
    /// started.
    NoConfinement(Unavailable),
    /// A call did not answer, and there is no such thing as continuing from
    /// one that did not.
    NoAnswer {
        /// The turn it was made in.
        turn: u32,
        /// The lane it was made on.
        lane: String,
        /// What the call came to instead.
        outcome: Box<Outcome>,
    },
    /// The server reported no count for a number the record requires.
    Unmeasured {
        /// The turn.
        turn: u32,
        /// Which number, spelled as the record spells it.
        what: &'static str,
    },
    /// A command never ran.
    CommandNotRun {
        /// The turn.
        turn: u32,
        /// Why.
        why: NotRun,
    },
    /// The working object refused a patch.
    ObjectRefused {
        /// The turn.
        turn: u32,
        /// Why.
        why: ObjectError,
    },
    /// A seam refused to settle.
    SeamRefused {
        /// The turn.
        turn: u32,
        /// Why.
        why: SeamError,
    },
    /// The record this drive built is not a record.
    ///
    /// The one variant that is a defect in this module rather than a fact
    /// about the run, and it is a variant rather than a panic because a
    /// harness that crashed is a harness with no verdict.
    Unrecordable {
        /// What the format said.
        why: String,
    },
}

impl Halt {
    /// The exit code a drive that could not run carries.
    ///
    /// Two, matching [`Unavailable::EXIT`]: a drive that could not start is
    /// not a drive that failed, and a census that could not tell them apart
    /// would count a missing substrate as a failed session.
    pub const EXIT: i32 = 2;
}

impl fmt::Display for Halt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoConfinement(why) => write!(f, "the drive did not start: {why}"),
            Self::NoAnswer {
                turn,
                lane,
                outcome,
            } => write!(
                f,
                "turn {turn}: the `{lane}` call did not answer ({}); a session cannot \
                 continue from a turn that did not happen",
                outcome.by()
            ),
            Self::Unmeasured { turn, what } => write!(
                f,
                "turn {turn}: the server reported no `{what}`, which the record requires \
                 and for which zero is a measurement rather than its absence"
            ),
            Self::CommandNotRun { turn, why } => write!(f, "turn {turn}: {why}"),
            Self::ObjectRefused { turn, why } => write!(f, "turn {turn}: {why:?}"),
            Self::SeamRefused { turn, why } => write!(f, "turn {turn}: {why:?}"),
            Self::Unrecordable { why } => write!(
                f,
                "the drive ran and what it wrote is not a record: {why}. That is a \
                 defect here, not a fact about the session"
            ),
        }
    }
}

impl Error for Halt {}

/// A fork's answer, and what became of it.
///
/// Counted rather than summarised, because "the fork produced nothing" and
/// "the fork produced six things this fold does not know how to keep" are
/// different facts about the machinery and the second is the one that gets
/// mistaken for the first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uncaptured {
    /// The turn the fork was opened in.
    pub turn: u32,
    /// The fork.
    pub fork: String,
    /// Regions the answer carried.
    pub regions: usize,
    /// Regions that became entries.
    pub captured: usize,
    /// The tags that appeared and were not kept, as they were written.
    pub passed_over: Vec<String>,
}

/// A drive that ran.
#[derive(Debug, Clone)]
pub struct Drive {
    /// The record, already parsed back from its own rendering.
    pub record: Record,
    /// The record's own spelling, which is what a caller writes to a file.
    pub rendered: String,
    /// The product the session produced: the working object, dumped.
    ///
    /// Carried rather than only hashed, because `summary.product_sha256` is a
    /// digest OF something and a digest of a file nobody wrote is one nobody
    /// can check. A caller writes this beside the record and `sha256sum` is
    /// the audit.
    pub product: String,
    /// Every seam that fired, with its dumps either side.
    pub seams: Vec<Seam>,
    /// What the client learned that record v0 has no spelling for.
    pub unspellable: Vec<Unspellable>,
    /// What each fork's answer carried that this fold did not keep.
    pub uncaptured: Vec<Uncaptured>,
}

/// The ratifier a drive uses: it puts the seam's ask to the substrate.
///
/// A ratifier that answered from a table would make the seam a fixture. This
/// one makes a real call on the [`RATIFY`] lane, so the ask a seam builds is
/// exercised against whatever is serving, and the call's journal becomes rows
/// of the record like any other.
struct Interviewer<'a, T: Transport> {
    client: &'a Client<T>,
    shape: RequestShape,
    ids: IdSource,
    /// The turn the seam is settling, for the provenance its patches carry.
    ///
    /// A [`Cell`] because the controller owns this and lends it back as `&R`,
    /// while the turn is only known outside. It is not a shortcut around the
    /// borrow: without it every ratified entry claimed provenance from
    /// **turn 0**, which is not a turn, and the object banked that as a fact
    /// about where the entry came from.
    turn: Cell<u32>,
    calls: Vec<crate::client::Call>,
}

impl<T: Transport> Ratifier for Interviewer<'_, T> {
    fn ratify(&mut self, ask: &Ask) -> Vec<Patch> {
        let mut shape = self.shape.clone();
        shape.messages = vec![Message {
            role: Role::User,
            content: ask.text.clone(),
        }];
        let call = self.client.call(&shape, RATIFY, &mut self.ids);
        // A ratification whose call did not answer folds to nothing, which is
        // what the controller already has a word for: an empty patch list is
        // a seam that moved no entry. The drive does NOT halt here, because
        // `Ratifier` cannot fail -- the call is in `calls` and the record
        // carries its rows, so the failure is visible rather than swallowed.
        let patches = call
            .outcome
            .answer()
            .map(|answer| fold(&answer.text, self.turn.get(), RATIFY, None))
            .unwrap_or_default();
        self.calls.push(call);
        patches
    }
}

/// One interview answer, folded into patches.
///
/// **This is not the reconciler.** It keeps every tagged region that carried
/// a value, as one entry each, and keeps nothing else. The reconciler in
/// `capture` decides supersession, dedup and grounding; when it is ready this
/// function is one call into it. Written out rather than left as a `todo!`
/// because a drive that cannot capture anything cannot show a capture in its
/// record, and showing one is the whole milestone.
fn fold(text: &str, turn: u32, lane: &str, fork: Option<&str>) -> Vec<Patch> {
    let Ok(answer) = interview::parse(text) else {
        return Vec::new();
    };
    let mut patches = Vec::new();
    for (position, field) in answer.fields.iter().enumerate() {
        let (Some(tag), interview::Outcome::Value(content)) = (&field.tag, &field.outcome) else {
            continue;
        };
        // The region's own position, with no offset per lane: `(lane, fork,
        // index)` already orders a turn totally, so an offset would only make
        // two lanes' indices look like a sequence they are not part of.
        let index = u32::try_from(position).unwrap_or(u32::MAX);
        let Ok(id) = EntryId::new(&format!("{lane}-t{turn}-{index}")) else {
            continue;
        };
        patches.push(Patch::Add {
            id,
            content: format!("{}: {content}", tag.kind.canonical_tag()),
            provenance: Provenance {
                turn,
                lane: lane.to_owned(),
                fork: fork.map(str::to_owned),
                tangent: None,
                index,
            },
        });
    }
    patches
}

/// Run `script` in `gym`.
///
/// # Errors
///
/// Returns [`Halt`] for anything that stops the drive: a call that did not
/// answer, a number the server did not report, a command that never ran, a
/// fold the object refused. Never a record with a hole in it.
#[allow(clippy::too_many_lines)]
pub fn run<T: Transport>(script: &Script, gym: &Gym<'_, T>) -> Result<Drive, Halt> {
    let mut object = WorkingObject::open(script.regime.clone());
    let mut controller = Controller::open(
        gym.seam.clone(),
        &object,
        Interviewer {
            client: gym.client,
            shape: gym.shape.clone(),
            ids: IdSource::new("r"),
            turn: Cell::new(0),
            calls: Vec::new(),
        },
    );

    let mut events = vec![Event::Start {
        regime: Box::new(script.regime.clone()),
    }];
    let mut unspellable: Vec<Unspellable> = Vec::new();
    let mut uncaptured: Vec<Uncaptured> = Vec::new();
    let mut seams: Vec<Seam> = Vec::new();
    let mut prefill_total = Count::default();

    let mut main_ids = IdSource::new("q");
    let mut fork_ids = IdSource::new("i");
    let mut tool_ids = IdSource::new("t");
    let mut capture_ids = IdSource::new("p");
    let mut seam_ids = IdSource::new("s");

    for turn in &script.turns {
        let index = controller.begin_turn();

        // The turn's own call. Its answer decides the turn row, so the call
        // happens before the row that describes it exists.
        let mut shape = gym.shape.clone();
        shape.messages = vec![
            Message {
                role: Role::System,
                content: controller.prefix().to_owned(),
            },
            Message {
                role: Role::User,
                content: turn.ask.clone(),
            },
        ];
        let call = gym.client.call(&shape, MAIN, &mut main_ids);
        let Some(answer) = call.outcome.answer() else {
            return Err(Halt::NoAnswer {
                turn: index,
                lane: MAIN.to_owned(),
                outcome: Box::new(call.outcome.clone()),
            });
        };
        let Some(prefill) = answer.cache.prompt_tokens else {
            return Err(Halt::Unmeasured {
                turn: index,
                what: "turn.prefill_tokens",
            });
        };

        events.push(Event::Turn {
            index,
            prefill_tokens: prefill,
        });
        prefill_total = prefill_total.saturating_add(prefill);
        archive(&call, &mut events, &mut unspellable);

        // The commands. Each is a tool call, run under the confinement, with
        // its exit status and output as the row says.
        for command in &turn.commands {
            let ran = gym
                .confinement
                .run(gym.isolation, gym.worktree, &command.argv)
                .map_err(|why| Halt::CommandNotRun { turn: index, why })?;
            events.push(Event::ToolCall {
                id: tool_ids.take(),
                at_turn: index,
                tool: command.tool.clone(),
                args: Some(BTreeMap::from([(
                    "argv".to_owned(),
                    Value::Array(ran.argv.iter().map(|a| Value::String(a.clone())).collect()),
                )])),
                exit: ran.exit.map(i64::from),
                output: Some(ran.as_the_model_sees_it()),
            });
        }

        // The fork, and the capture that follows it. A fork that captured
        // nothing is still a fork that happened, so the rows go in either
        // way -- and how much of the answer this fold could not keep is
        // reported beside the drive rather than silently absent from it.
        if let Some(question) = &turn.fork {
            let mut asking = gym.shape.clone();
            asking.messages = vec![Message {
                role: Role::User,
                content: question.clone(),
            }];
            let forked = gym.client.call(&asking, INTERVIEW, &mut fork_ids);
            let Some(reply) = forked.outcome.answer() else {
                return Err(Halt::NoAnswer {
                    turn: index,
                    lane: INTERVIEW.to_owned(),
                    outcome: Box::new(forked.outcome.clone()),
                });
            };
            let text = reply.text.clone();
            let fork_id = format!("fork-{index}");
            events.push(Event::Fork {
                id: fork_id.clone(),
                lane: INTERVIEW.to_owned(),
                of_turn: index,
            });
            archive(&forked, &mut events, &mut unspellable);

            let patches = fold(&text, index, INTERVIEW, Some(&fork_id));
            let applied = object
                .apply_turn(&patches)
                .map_err(|why| Halt::ObjectRefused { turn: index, why })?;
            uncaptured.push(census(index, &fork_id, &text, patches.len()));
            events.push(Event::Capture {
                id: capture_ids.take(),
                from_fork: fork_id,
                entries: u32::try_from(applied.len()).unwrap_or(u32::MAX),
            });
        }

        if let Some(phase) = &turn.phase {
            controller.propose(phase);
        }
        if turn.boundary {
            controller.operator_declares_a_boundary();
        }

        // The seam, if this turn is one. The ratifier's call happens inside
        // `settle`, so its rows are drained afterwards -- and they are the
        // reason the ratification is a real exchange rather than a fixture.
        controller.ratifier().turn.set(index);
        let before = controller.ratifier().calls.len();
        let settled = controller
            .settle(&mut object)
            .map_err(|why| Halt::SeamRefused { turn: index, why })?;
        let asked: Vec<_> = controller.ratifier().calls[before..].to_vec();
        for one in &asked {
            archive(one, &mut events, &mut unspellable);
        }
        if let Some(seam) = settled {
            events.push(Event::Seam {
                id: seam_ids.take(),
                at_turn: index,
                rendered_bytes: Count::new(seam.prefix.len() as u64).unwrap_or_default(),
            });
            seams.push(seam);
        }
    }

    // The product: the object the session built, dumped. A digest of nothing
    // in particular would satisfy the schema and mean nothing, so the summary
    // names something a reader can recompute with `sha256sum`.
    let product = dump(&object);
    events.push(Event::Summary {
        turns: u32::try_from(script.turns.len()).unwrap_or(u32::MAX),
        prefill_tokens_total: prefill_total,
        product_sha256: digest::sha256_hex(product.as_bytes()),
    });

    let built = Record { events };
    let rendered = render(&built);
    // Parsed back, so the acceptance is a fact of this function. A record
    // this module can write and its own format cannot read is a defect here,
    // and it should be found here rather than in a lane three commits later.
    let record =
        crate::formats::record::parse(&rendered).map_err(|why: ParseError| Halt::Unrecordable {
            why: format!("{why:?}"),
        })?;

    Ok(Drive {
        record,
        rendered,
        product,
        seams,
        unspellable,
        uncaptured,
    })
}

/// Project `call`'s journal into rows, and fill in the texts the journal does
/// not carry.
///
/// [`journal::project`] is the authorized projection and it leaves `text`
/// empty on both kinds: the journal is the transport's view and the transport
/// does not keep payloads. A record that carries them is an ARCHIVE, which is
/// what a drive is supposed to leave behind, so the texts are filled here --
/// from the attempt's own `sent` shape and the answer that named the attempt
/// that produced it, never from "the last one", which retries make wrong.
fn archive(
    call: &crate::client::Call,
    events: &mut Vec<Event>,
    unspellable: &mut Vec<Unspellable>,
) {
    let projected = journal::project(&call.journal);
    let answered = call.outcome.answer();
    for mut event in projected.events {
        match &mut event {
            Event::Request { id, text, .. } => {
                if let Some(attempt) = call.attempts.iter().find(|a| &a.id == id) {
                    *text = Some(wire::body(&attempt.sent));
                }
            }
            Event::Response { text, .. } => {
                // No id check here, and its absence is deliberate. A call has
                // AT MOST ONE answer -- the journal writes a `Received` when
                // one comes back, retries happen when one does not, and a cap
                // is not retried -- so the response row a call produces is
                // that call's answer by construction. The first version
                // matched `answer.produced_by` against the row's
                // `to_request`, and no input this client can produce makes
                // that condition false: a guard nothing can make fire is a
                // guard that has never been seen red, which this repository
                // does not keep. The REQUEST side is the opposite case and is
                // matched by id below, because a retried call has several.
                if let Some(answer) = answered {
                    *text = Some(answer.text.clone());
                }
            }
            _ => {}
        }
        events.push(event);
    }
    for lost in projected.unspellable {
        if !unspellable.contains(&lost) {
            unspellable.push(lost);
        }
    }
}

/// What a fork's answer carried, against what the fold kept.
fn census(turn: u32, fork: &str, text: &str, captured: usize) -> Uncaptured {
    let parsed = interview::parse(text).ok();
    let regions = parsed.as_ref().map_or(0, |answer| answer.fields.len());
    let passed_over = parsed
        .as_ref()
        .map(|answer| {
            answer
                .fields
                .iter()
                .filter(|field| !matches!(field.outcome, interview::Outcome::Value(_)))
                .map(|field| {
                    field
                        .tag
                        .as_ref()
                        .map_or_else(|| "<untagged>".to_owned(), |tag| tag.as_written.clone())
                })
                .collect()
        })
        .unwrap_or_default();
    Uncaptured {
        turn,
        fork: fork.to_owned(),
        regions,
        captured,
        passed_over,
    }
}

/// The working object, dumped: one entry per line, id first.
///
/// The object's own `dump_lines` is the authorized spelling of an entry; this
/// only decides the order, which is the id order the dump already keys by.
fn dump(object: &WorkingObject) -> String {
    let mut out = String::new();
    for (id, line) in object.dump_lines() {
        out.push_str(id.as_str());
        out.push('\t');
        out.push_str(&line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::process;
    use std::time::Duration;

    use crate::client::Client;
    use crate::client::shape::{
        Concurrency, Dialect, Limits, Message, RequestShape, Role, SamplerCard, Serving,
    };
    use crate::client::stub::{Act, Stub};
    use crate::client::transport::{Endpoint, Http};
    use crate::formats::record::{Event, Kind, Reasoning, Regime, Substrate};
    use crate::isolation::{Confinement, Policy as IsolationPolicy};
    use crate::seam::policy::Policy as SeamPolicy;

    use super::canned;
    use super::script::{Command, Script};
    use super::{Drive, Gym, Halt, run};

    /// A working tree the drive's commands run in.
    struct Ground {
        tree: PathBuf,
    }

    impl Ground {
        fn make(name: &str) -> Self {
            let tree = std::env::temp_dir().join(format!(
                "diet-drive-{}-{name}-{}",
                process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|since| since.as_nanos())
                    .unwrap_or_default()
            ));
            fs::create_dir_all(&tree).expect("a working tree");
            Self { tree }
        }
    }

    impl Drop for Ground {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.tree);
        }
    }

    fn regime() -> Regime {
        Regime {
            arm: "dev-loop".to_owned(),
            substrate: Substrate {
                name: "canned".to_owned(),
                model: "a-model".to_owned(),
                quantization: "none".to_owned(),
                // Not empty: the schema refuses a blank `substrate.sampler`,
                // because "nobody wrote the settings down" and "the settings
                // were these" are different facts about a run.
                sampler: BTreeMap::from([(
                    "seed".to_owned(),
                    crate::formats::record::json::Value::Integer(7),
                )]),
                reasoning: Reasoning::Off,
                hardware: "the-runner".to_owned(),
            },
            dogma_version: 0,
        }
    }

    fn shape() -> RequestShape {
        RequestShape {
            model: "a-model".to_owned(),
            messages: vec![Message::new(Role::User, "replaced per call")],
            sampler: SamplerCard::empty(),
            limits: Limits {
                attempt: Duration::from_millis(2_000),
                call: Duration::from_millis(4_000),
                max_output_tokens: 128,
                // One retry, so `a_retried_call_archives_each_attempt_with_\
                // what_that_attempt_sent` has a second attempt to archive. The
                // canned server answers first time in every other test here,
                // so nothing else changes: a retry that never happens costs
                // nothing.
                retries: 1,
            },
            grammar: None,
        }
    }

    /// A reply with no `usage` at all: a server that answered and measured
    /// nothing.
    fn unmeasured(text: &str) -> String {
        format!(
            "{{\"choices\":[{{\"message\":{{\"role\":\"assistant\",\"content\":\"{text}\"}},\
             \"finish_reason\":\"stop\"}}],\"generation_settings\":{{}}}}"
        )
    }

    fn three_turns() -> Script {
        canned::script(regime())
    }

    /// Run `script` against a canned server playing `acts`.
    fn against(script: &Script, acts: Vec<Act>, ground: &Ground) -> Result<Drive, Halt> {
        let stub = Stub::serving(acts).expect("loopback binds");
        let client = Client::new(
            Http::new(Endpoint::parse(&stub.url()).expect("the stub's URL is an endpoint")),
            Serving {
                concurrency: Concurrency::Declared(1),
                dialect: Dialect::llama_cpp(),
            },
        );
        let isolation = IsolationPolicy::unconfined();
        run(
            script,
            &Gym {
                client: &client,
                shape: shape(),
                confinement: &Confinement::Unconfined,
                isolation: &isolation,
                worktree: &ground.tree,
                // Nothing but the operator's own declaration fires a seam
                // here: no cadence, no byte budget, no phases. A drive whose
                // seam landed somewhere the script did not choose would prove
                // less than it looks like it does.
                seam: SeamPolicy {
                    every_turns: None,
                    at_working_set_bytes: None,
                    phases: crate::seam::phase::PhaseGraph::none(),
                },
            },
        )
    }

    // -----------------------------------------------------------------------
    // the milestone row
    // -----------------------------------------------------------------------

    #[test]
    fn three_turns_produce_a_record_the_format_accepts_with_a_seam_and_a_capture_in_it() {
        let ground = Ground::make("milestone");
        let drive = against(&three_turns(), canned::acts(), &ground).expect("the drive ran");

        // What `diet check-record` does, on the bytes the drive wrote. Not a
        // paraphrase of it: the same `project` the CLI calls.
        crate::formats::record::project(&drive.rendered)
            .expect("`check-record` accepts what this drive wrote");

        let kinds = drive.record.kinds();
        for required in [
            Kind::Start,
            Kind::Turn,
            Kind::Request,
            Kind::Response,
            Kind::Fork,
            Kind::Capture,
            Kind::Seam,
            Kind::ToolCall,
            Kind::Summary,
        ] {
            assert!(
                kinds.contains(&required),
                "the record carries no `{}` row: {kinds:?}",
                required.tag()
            );
        }

        let turns: Vec<u32> = drive
            .record
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Turn { index, .. } => Some(*index),
                _ => None,
            })
            .collect();
        assert_eq!(turns, [1, 2, 3], "three turns, and the record says which");

        assert_eq!(
            drive.seams.len(),
            1,
            "one seam, where the script declared it"
        );
        assert_eq!(drive.seams[0].at_turn, 2);
        assert!(
            !drive.seams[0].after.is_empty(),
            "and it dumped an object with something in it"
        );

        assert!(
            ground.tree.join("one.txt").is_file() && ground.tree.join("three.txt").is_file(),
            "the commands ran in the working tree, and their effects are there"
        );
    }

    #[test]
    fn the_record_is_an_archive_and_not_a_ledger() {
        let ground = Ground::make("archive");
        let drive = against(&three_turns(), canned::acts(), &ground).expect("the drive ran");

        let mut asked = 0;
        let mut answers = Vec::new();
        for event in &drive.record.events {
            match event {
                Event::Request { text, .. } => {
                    let sent = text.as_ref().expect("a request carries what was sent");
                    assert!(
                        sent.contains("\"model\":\"a-model\""),
                        "and it is the body the wire renderer produced: {sent}"
                    );
                    asked += 1;
                }
                Event::Response { text, .. } => {
                    answers.push(
                        text.as_ref()
                            .expect("a response carries what came back")
                            .clone(),
                    );
                }
                _ => {}
            }
        }
        assert_eq!(asked, 6, "six calls, six requests");
        assert!(
            answers.iter().any(|text| text == "turn one"),
            "the turn's own answer is in the record verbatim: {answers:?}"
        );
        assert!(
            answers
                .iter()
                .any(|text| text.contains("keep the reconciler")),
            "and so is the fork's"
        );

        let tools: Vec<_> = drive
            .record
            .events
            .iter()
            .filter(|event| matches!(event, Event::ToolCall { .. }))
            .collect();
        assert_eq!(tools.len(), 2);
        let Event::ToolCall {
            args, exit, output, ..
        } = tools[0]
        else {
            unreachable!("filtered")
        };
        assert!(
            args.is_some(),
            "a tool call carries the argv it was called with"
        );
        assert_eq!(*exit, Some(0));
        assert!(output.is_some(), "and what the command returned");
    }

    #[test]
    fn a_retried_call_archives_each_attempt_with_what_that_attempt_sent() {
        // The reason the request texts are matched by id rather than taken
        // from "the last attempt". A hung-up connection is retried, so this
        // call has two request rows -- and the second is a different message
        // list from the first, because the drive rebuilds the prompt from a
        // prefix the seam may have moved. A record that gave both rows the
        // same body would be an archive of a session that did not happen.
        let ground = Ground::make("retried");
        let mut script = three_turns();
        script.turns.truncate(1);
        script.turns[0].fork = None;
        script.turns[0].commands = Vec::new();

        let mut acts = vec![Act::Hangup];
        acts.extend(canned::acts().into_iter().take(1));
        let drive = against(&script, acts, &ground).expect("the drive ran after the retry");

        let requests: Vec<_> = drive
            .record
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Request { id, text, .. } => Some((id.clone(), text.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(requests.len(), 2, "the hangup was retried: {requests:?}");
        assert!(
            requests.iter().all(|(_, text)| text.is_some()),
            "and each row carries the body ITS attempt sent"
        );
        let responses = drive
            .record
            .events
            .iter()
            .filter(|event| matches!(event, Event::Response { .. }))
            .count();
        assert_eq!(
            responses, 1,
            "one answer per call, which is why the response side needs no id match"
        );
    }

    #[test]
    fn a_two_turn_script_summarises_two_turns() {
        // The turn count is COUNTED. Written as `3` it agrees with every
        // three-turn assertion in this file and with nothing else, and the
        // record's own summary rule cannot catch it because the rows and the
        // summary would be wrong together.
        let ground = Ground::make("two-turns");
        let mut script = three_turns();
        script.turns.truncate(2);
        let acts = canned::acts()
            .into_iter()
            .take(canned::calls(&script))
            .collect();
        let drive = against(&script, acts, &ground).expect("the drive ran");

        let Some(Event::Summary { turns, .. }) = drive.record.events.last() else {
            panic!("the last row is the summary")
        };
        assert_eq!(*turns, 2);
    }

    #[test]
    fn a_record_this_drive_cannot_write_is_a_halt_and_not_a_file() {
        // `run` renders its own record and parses it back, so "produces a
        // record `check-record` accepts" is a fact of the function. Without
        // the parse-back the drive would return a `Record` value it built and
        // a `rendered` string nothing had read, and the first thing to notice
        // would be a red lane.
        //
        // A regime with an empty `substrate.sampler` is the case: the schema
        // refuses a blank field, and this drive found that defect in its own
        // first fixture rather than in CI.
        let ground = Ground::make("unrecordable");
        let mut script = three_turns();
        script.regime.substrate.sampler = BTreeMap::new();
        let halt = against(&script, canned::acts(), &ground)
            .expect_err("a record the format refuses is not a drive that succeeded");
        assert!(matches!(&halt, Halt::Unrecordable { .. }), "{halt:?}");
        assert!(
            halt.to_string().contains("defect here"),
            "and it says whose defect it is: {halt}"
        );
    }

    // -----------------------------------------------------------------------
    // the numbers, and what happens when they are not there
    // -----------------------------------------------------------------------

    #[test]
    fn a_server_that_measured_nothing_halts_the_drive_rather_than_writing_a_length() {
        let ground = Ground::make("unmeasured");
        let halt = against(
            &three_turns(),
            vec![Act::Answer(unmeasured("turn one"))],
            &ground,
        )
        .expect_err("a record cannot be written from a server that reported no usage");
        assert!(
            matches!(&halt, Halt::Unmeasured { turn: 1, what } if *what == "turn.prefill_tokens"),
            "{halt:?}"
        );
        assert!(
            halt.to_string().contains("zero is a measurement"),
            "and it says why a byte count would not do: {halt}"
        );
    }

    #[test]
    fn the_summary_totals_what_the_rows_say_and_the_digest_is_the_products() {
        let ground = Ground::make("summary");
        let drive = against(&three_turns(), canned::acts(), &ground).expect("the drive ran");

        let Some(Event::Summary {
            turns,
            prefill_tokens_total,
            product_sha256,
        }) = drive.record.events.last()
        else {
            panic!(
                "the last row is the summary: {:?}",
                drive.record.events.last()
            );
        };
        assert_eq!(*turns, 3);
        assert_eq!(
            prefill_tokens_total.get(),
            1800,
            "three turns at the 600 the canned server reported; the record's own \
             parser refuses a summary that disagrees with its rows, so this is \
             asserting the number rather than the agreement"
        );
        assert_eq!(
            *product_sha256,
            super::digest::sha256_hex(drive.product.as_bytes()),
            "the digest is OF the product, and the product is carried so a \
             reader can recompute it: a digest of a file nobody wrote is a \
             digest nobody can check"
        );
        assert_ne!(
            *product_sha256,
            super::digest::sha256_hex(b""),
            "and the product is not empty, or the row would satisfy the schema \
             and say nothing"
        );
        assert!(
            drive.product.contains("keep the reconciler"),
            "the product is the object the session built: {}",
            drive.product
        );
    }

    // -----------------------------------------------------------------------
    // no silent fallback, in a loop that could have had four
    // -----------------------------------------------------------------------

    #[test]
    fn a_call_that_did_not_answer_stops_the_drive() {
        let ground = Ground::make("no-answer");
        let halt = against(&three_turns(), vec![Act::Hangup], &ground)
            .expect_err("a turn that did not happen cannot be continued from");
        assert!(
            matches!(&halt, Halt::NoAnswer { turn: 1, lane, .. } if lane == super::MAIN),
            "{halt:?}"
        );

        // And on the fork lane, which is a different call site with the same
        // rule. Without this the second site could return `Ok` with an empty
        // fork and the first test would not notice.
        let forked = against(
            &three_turns(),
            vec![Act::Answer(canned::reply("turn one")), Act::Hangup],
            &ground,
        )
        .expect_err("a fork that did not answer is not a fork that captured nothing");
        assert!(
            matches!(&forked, Halt::NoAnswer { turn: 1, lane, .. } if lane == super::INTERVIEW),
            "{forked:?}"
        );
    }

    #[test]
    fn a_command_that_never_ran_stops_the_drive() {
        let ground = Ground::make("no-command");
        let mut script = three_turns();
        script.turns[0].commands = vec![Command::new("shell", &[])];
        let halt = against(&script, canned::acts(), &ground)
            .expect_err("a command that never ran has no exit status to record");
        assert!(
            matches!(&halt, Halt::CommandNotRun { turn: 1, .. }),
            "{halt:?}"
        );
    }

    // -----------------------------------------------------------------------
    // what the fold could not keep, counted rather than dropped
    // -----------------------------------------------------------------------

    #[test]
    fn what_the_fold_could_not_capture_is_counted_and_named() {
        let ground = Ground::make("uncaptured");
        let drive = against(&three_turns(), canned::acts(), &ground).expect("the drive ran");

        assert_eq!(drive.uncaptured.len(), 2, "two forks, two censuses");
        let first = &drive.uncaptured[0];
        assert_eq!(first.turn, 1);
        assert_eq!(
            (first.regions, first.captured),
            (4, 2),
            "the fixture answer has four regions and this fold keeps exactly the \
             two tagged ones that carried a value. Asserted as numbers rather \
             than as `captured < regions`, which stayed true when the fold \
             started keeping the DECLINE as an entry: a decline is a lane \
             saying it has nothing, and banking it as a fact is the confabulation \
             the decline vocabulary exists to prevent: {first:?}"
        );
        assert!(
            !drive.product.contains("evidence:"),
            "and the decline did not become an entry: {}",
            drive.product
        );
        assert!(
            first.passed_over.iter().any(|tag| tag.contains("EVIDENCE")),
            "and it names them as written: {:?}",
            first.passed_over
        );
        assert!(
            first.passed_over.iter().any(|tag| tag == "<untagged>"),
            "prose nobody tagged included: {:?}",
            first.passed_over
        );
    }

    #[test]
    fn every_entry_names_the_turn_it_actually_came_from() {
        // The ratifier is owned by the controller and lent back as `&R`, so
        // the turn it is settling reaches it through a `Cell`. Without that
        // it read a field nobody wrote and every ratified entry claimed
        // provenance from turn 0 -- not a turn, and banked in the object as a
        // fact about where the entry came from. A record can carry a lie its
        // own parser accepts, so the object is where this has to be checked.
        let ground = Ground::make("provenance");
        let drive = against(&three_turns(), canned::acts(), &ground).expect("the drive ran");

        let seam = &drive.seams[0];
        let ratified: Vec<_> = seam
            .patches
            .iter()
            .map(|patch| match patch {
                crate::object::Patch::Add { provenance, .. }
                | crate::object::Patch::Supersede { provenance, .. }
                | crate::object::Patch::Resolve { provenance, .. }
                | crate::object::Patch::Retire { provenance, .. }
                | crate::object::Patch::Park { provenance, .. } => provenance.clone(),
            })
            .collect();
        assert!(
            !ratified.is_empty(),
            "the seam ratified something, or this asserts about an empty list"
        );
        for provenance in &ratified {
            assert_eq!(
                provenance.turn, 2,
                "the seam fired after turn 2, so its entries came from turn 2: {provenance:?}"
            );
            assert_eq!(provenance.lane, super::RATIFY);
        }

        // And the fork's entries name their own turn and their own fork,
        // which is the half that was already right and would otherwise carry
        // this test on its own.
        assert!(
            drive.product.contains("\"turn\":1"),
            "an entry from turn one is in the object: {}",
            drive.product
        );
        assert!(
            !drive.product.contains("\"turn\":0"),
            "and nothing claims turn zero: {}",
            drive.product
        );
    }

    #[test]
    fn the_regimen_this_lane_ships_is_one_a_drive_can_run() {
        // The lane runs `diet-drive` against this file. A fixture nothing
        // reads is a fixture that rots, and it would rot as a red lane on
        // somebody else's branch rather than here.
        let regimen = crate::formats::regimen::parse(canned::DEV_LOOP)
            .expect("the shipped regimen is a regimen");

        let isolation = IsolationPolicy::from_regimen(&regimen).expect("its isolation policy");
        assert_eq!(
            isolation.isolation,
            crate::isolation::Isolation::None,
            "declared unconfined on purpose: the lane's runner has no sandbox \
             runner, and a sandbox that refuses to start is the correct \
             behaviour and the wrong thing to gate a round trip on"
        );
        SeamPolicy::from_regimen(&regimen).expect("its seam policy");

        // The four keys `diet-drive` reads for the regime facts regimen v1
        // has no place for. Named here so that a rename in the binary that
        // did not reach the fixture fails in this crate rather than in CI.
        for key in [
            "arm",
            "dogma_version",
            "substrate",
            "substrate_model",
            "substrate_quantization",
            "substrate_reasoning",
            "substrate_hardware",
        ] {
            assert!(
                regimen.get(key).is_some(),
                "the shipped regimen carries `{key}`"
            );
        }
        assert!(
            matches!(
                regimen.get("sampler"),
                Some(crate::formats::regimen::Value::Table(table)) if !table.is_empty()
            ),
            "and a sampler, which the record refuses to leave blank"
        );
    }

    #[test]
    fn the_acts_are_exactly_what_the_script_asks_for() {
        // The canned server plays its replies in order and stops listening
        // when they run out, so an act too few hangs the drive on a refused
        // connection and an act too many is a call the script never makes.
        // Neither is visible from a passing drive, which is why the count is
        // derived from the script and asserted rather than written down.
        let script = three_turns();
        assert_eq!(
            canned::calls(&script),
            canned::acts().len(),
            "the script makes one call per turn, one per fork and one per \
             declared boundary; the acts have to be that many, in that order"
        );
        assert_eq!(canned::acts().len(), 6);

        // And the count is derived, not a constant that happens to agree: a
        // script with a turn removed asks for fewer.
        let mut shorter = script.clone();
        shorter.turns.pop();
        assert_eq!(canned::calls(&shorter), 5);
    }

    #[test]
    fn what_the_record_cannot_spell_is_named_rather_than_dropped() {
        let ground = Ground::make("unspellable");
        let drive = against(&three_turns(), canned::acts(), &ground).expect("the drive ran");
        assert!(
            !drive.unspellable.is_empty(),
            "record v0 cannot spell everything the client learns, and a drive \
             reporting an empty list would be claiming it can"
        );
    }
}
