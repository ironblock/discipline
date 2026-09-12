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
pub mod regimen;
pub mod script;

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
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
use crate::object::{Applied, EntryId, ObjectError, Patch, Provenance, WorkingObject};
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
    /// A lane's answer could not be read at all.
    ///
    /// Not the same as a lane that had nothing to say, and not the same as
    /// one whose regions this fold does not keep. Those are counted in
    /// [`Uncaptured`]; this is the drive stopping.
    Unreadable {
        /// The turn.
        turn: u32,
        /// The lane whose answer it was.
        lane: String,
        /// What the parser said.
        why: String,
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
    ///
    /// A `u8` because that is what an `ExitCode` is built from. It was an
    /// `i32`, so the binary could not use it and declared its own `2` beside
    /// a comment asserting the two matched -- two spellings of one number in
    /// two files, and changing this one failed nothing.
    pub const EXIT: u8 = 2;
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
            Self::Unreadable { turn, lane, why } => write!(
                f,
                "turn {turn}: the `{lane}` answer could not be read ({why}); a lane that \
                 said something unreadable is not a lane that said nothing, and the \
                 record has one spelling for both"
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
/// Counted rather than summarised, because "the fork produced nothing", "the
/// fork produced six things this fold does not know how to keep" are
/// different facts about the machinery, and the first is what the second gets
/// mistaken for. An answer the fold cannot read at all is neither: it is a
/// [`Halt::Unreadable`], because a capture row of `entries: 0` says the same
/// thing for all three unless somebody writes down which happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uncaptured {
    /// The turn the fork was opened in.
    pub turn: u32,
    /// The fork.
    pub fork: String,
    /// Regions the answer carried.
    pub regions: usize,
    /// Regions that became patches.
    pub captured: usize,
    /// The tags that appeared and were not kept, as they were written.
    pub passed_over: Vec<String>,
    /// Whether the answer was cut off.
    ///
    /// The interview grammar types truncation deliberately, and a truncated
    /// answer's surviving fields are still real -- so they are folded, and
    /// the fact that there were more is carried here. Dropped, it made a
    /// cut-off answer indistinguishable from a whole one.
    pub truncated: bool,
    /// How many entries this capture MOVED.
    ///
    /// `Applied::touched` is the object's own answer to which entries a patch
    /// moved: a `Deduped` patch counts, because a provenance was recorded on
    /// the entry that already held the content, and only `Unchanged` returns
    /// none. This answers *did the fork produce signal*.
    pub entries_touched: u32,
    /// How many entries this capture BROUGHT INTO EXISTENCE.
    ///
    /// `Created`, plus the `added` half of a `Supersede`. This answers *did
    /// the fork produce NEW signal*, which is the collector's dedup
    /// economics and a different question from the one above.
    ///
    /// **Ruling 6: two fields, not one reading.** A single number named
    /// `entries` is the qualifier-loss class waiting to happen -- it reads as
    /// whichever of the two questions its reader had in mind, and the two
    /// diverge exactly where the interesting case is. Record v0 has one
    /// field; `capture.entries` in a v0 record means `entries_touched`, which
    /// is what it has always been counting, and record v1 splits it by a
    /// superseding row rather than an edit. Both numbers are computed here so
    /// that the day the schema has somewhere to put them, nothing has to be
    /// recomputed from an archive that never carried the second.
    pub entries_created: u32,
}

/// What became of one seam's ratification.
///
/// **The audit verdict grammar is not implemented and this module will not
/// improvise one.** The ask a seam puts (`seam::pinned_ask`) demands
/// `1. KEEP` / `2. UPDATE: …` / `3. REMOVE — …`; ruling 1 on #25 puts that
/// vocabulary in `diet/formats/` as its own verdict enum and it is not built.
/// The seam lane refused to write a second parser for it and said so; this
/// module briefly did the worse thing, folding the audit answer with the
/// *interview* grammar -- so a model answering the ask exactly as pinned
/// produced **zero patches and no warning**, while an answer carrying a
/// `DECISION:` line the ask never requested was banked as a ratified note.
///
/// So the ask is put, the answer is kept, and nothing is folded from it until
/// the verdict format exists. A seam still fires and still re-renders; what
/// it does not do is pretend to have read a reply it cannot read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Audit {
    /// The turn the seam settled after.
    pub turn: u32,
    /// The ask that was put, as the dogma pinned it.
    pub asked: String,
    /// The entries the ask handed over, in the order it numbered them.
    pub items: usize,
    /// What came back, when anything did.
    pub answered: Option<String>,
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
    /// What became of each seam's ratification. See [`Audit`]: the verdict
    /// grammar is not built, so every one of these is an ask put and an
    /// answer kept and not folded.
    pub audits: Vec<Audit>,
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
    audits: Vec<Audit>,
}

impl<T: Transport> Ratifier for Interviewer<'_, T> {
    fn ratify(&mut self, ask: &Ask) -> Vec<Patch> {
        let mut shape = self.shape.clone();
        shape.messages = vec![Message {
            role: Role::User,
            content: ask.text.clone(),
        }];
        let call = self.client.call(&shape, RATIFY, &mut self.ids);
        // NOTHING IS FOLDED. See [`Audit`]: the ask demands a verdict
        // vocabulary that is not implemented, and the interview grammar is a
        // different format. An empty patch list here is not "the model kept
        // everything" -- it is "this module cannot read the reply" -- and the
        // difference is recorded in `audits` rather than left for a reader of
        // the record to infer from a seam that moved nothing.
        self.audits.push(Audit {
            turn: self.turn.get(),
            asked: ask.text.clone(),
            items: ask.items.len(),
            answered: call.outcome.answer().map(|answer| answer.text.clone()),
        });
        self.calls.push(call);
        Vec::new()
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
fn fold(
    text: &str,
    turn: u32,
    lane: &str,
    fork: Option<&str>,
) -> Result<(Vec<Patch>, Uncaptured), String> {
    // An unreadable answer STOPS the drive. The first version returned
    // `Vec::new()` here, so a fork whose answer the grammar refused reached
    // the record as `capture { entries: 0 }` -- exactly as a fork that had
    // nothing to say does, and with the product's digest becoming
    // `sha256("")` and nothing anywhere saying why.
    //
    // A `Halt` rather than a third state on `Uncaptured`, because no input
    // this client can deliver reaches it: the interview grammar's only
    // content rejection is a NUL byte, and the wire reader refuses a raw
    // control character in a JSON string first. A state nothing can construct
    // is not a state; a refusal on a function's own contract is, and
    // `an_answer_the_fold_cannot_read_stops_the_drive` puts one to it
    // directly.
    let answer = interview::parse(text).map_err(|why| format!("{why:?}"))?;
    let truncated = answer.is_truncated();
    let mut passed_over = Vec::new();
    let mut patches = Vec::new();
    for (position, field) in answer.fields.iter().enumerate() {
        let (Some(tag), interview::Outcome::Value(content)) = (&field.tag, &field.outcome) else {
            passed_over.push(
                field
                    .tag
                    .as_ref()
                    .map_or_else(|| "<untagged>".to_owned(), |tag| tag.as_written.clone()),
            );
            continue;
        };
        // The region's own position, with no offset per lane: `(lane, fork,
        // index)` already orders a turn totally, so an offset would only make
        // two lanes' indices look like a sequence they are not part of.
        let Ok(index) = u32::try_from(position) else {
            passed_over.push(tag.as_written.clone());
            continue;
        };
        let Ok(id) = EntryId::new(&format!("{lane}-t{turn}-{index}")) else {
            passed_over.push(tag.as_written.clone());
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
    let census = Uncaptured {
        turn,
        fork: fork.unwrap_or(lane).to_owned(),
        regions: answer.fields.len(),
        captured: patches.len(),
        passed_over,
        truncated,
        // Filled by the caller: what the object made of these patches is not
        // known until they are applied, and a fold that guessed would be
        // guessing at the one number this census exists to be right about.
        entries_touched: 0,
        entries_created: 0,
    };
    Ok((patches, census))
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
            audits: Vec::new(),
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
        archive(&call, index, &mut events, &mut unspellable)?;

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
            archive(&forked, index, &mut events, &mut unspellable)?;

            let (patches, census) =
                fold(&text, index, INTERVIEW, Some(&fork_id)).map_err(|why| Halt::Unreadable {
                    turn: index,
                    lane: INTERVIEW.to_owned(),
                    why,
                })?;
            let applied = object
                .apply_turn(&patches)
                .map_err(|why| Halt::ObjectRefused { turn: index, why })?;

            // TWO counts, because they answer two questions -- ruling 6.
            // `apply_turn` returns one `Applied` per patch whatever it did,
            // so `applied.len()` is the patch count wearing the schema's word
            // for something else, and neither of these is that.
            let mut touched: BTreeSet<EntryId> = BTreeSet::new();
            let mut created: BTreeSet<EntryId> = BTreeSet::new();
            for one in &applied {
                touched.extend(one.touched());
                match one {
                    Applied::Created(id) => {
                        created.insert(id.clone());
                    }
                    Applied::Superseded { added, .. } => {
                        created.insert(added.clone());
                    }
                    Applied::Deduped(_) | Applied::StateChanged(_) | Applied::Unchanged(_) => {}
                }
            }
            let (Ok(entries_touched), Ok(entries_created)) =
                (u32::try_from(touched.len()), u32::try_from(created.len()))
            else {
                return Err(Halt::Unmeasured {
                    turn: index,
                    what: "capture.entries",
                });
            };
            uncaptured.push(Uncaptured {
                entries_touched,
                entries_created,
                ..census
            });

            // Record v0 has ONE field, and what it has always been counting
            // is the touched set -- so that is what goes in it, under its
            // true meaning. Record v1 splits it by a superseding row rather
            // than an edit, which is why nothing here writes `created` into
            // a v0 record: a field that meant one thing in old records and
            // another in new ones is unreadable across the archive.
            events.push(Event::Capture {
                id: capture_ids.take(),
                from_fork: fork_id,
                entries: entries_touched,
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
            // The same rule as the other two lanes, and it was missing here:
            // a ratify call that never answered used to produce a seam row
            // anyway, asserting the prompt was rebuilt after an audit that did
            // not happen. `Ratifier` cannot fail, so the check is made here,
            // after `settle`, where it can.
            if one.outcome.answer().is_none() {
                return Err(Halt::NoAnswer {
                    turn: index,
                    lane: RATIFY.to_owned(),
                    outcome: Box::new(one.outcome.clone()),
                });
            }
            archive(one, index, &mut events, &mut unspellable)?;
        }
        if let Some(seam) = settled {
            // No `unwrap_or_default()`. A render this module could not
            // count is not a render of zero bytes, and zero is the one
            // number a reader would believe. Unreachable on any platform --
            // `Count::MAX` is `i64::MAX` -- and it is a Halt rather than a
            // fabricated number because a total conversion still has to
            // produce something, and the something must not be a lie.
            let Ok(rendered_bytes) = Count::new(seam.prefix.len() as u64) else {
                return Err(Halt::Unmeasured {
                    turn: index,
                    what: "seam.rendered_bytes",
                });
            };
            events.push(Event::Seam {
                id: seam_ids.take(),
                at_turn: index,
                rendered_bytes,
            });
            seams.push(seam);
        }
    }

    let Ok(turns) = u32::try_from(script.turns.len()) else {
        return Err(Halt::Unmeasured {
            turn: 0,
            what: "summary.turns",
        });
    };

    // The product: the object the session built, dumped. A digest of nothing
    // in particular would satisfy the schema and mean nothing, so the summary
    // names something a reader can recompute with `sha256sum`.
    let product = dump(&object);
    events.push(Event::Summary {
        turns,
        prefill_tokens_total: prefill_total,
        product_sha256: crate::digest::sha256_hex(product.as_bytes()),
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
        audits: controller.ratifier().audits.clone(),
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
    turn: u32,
    events: &mut Vec<Event>,
    unspellable: &mut Vec<Unspellable>,
) -> Result<(), Halt> {
    // `response.output_tokens` is required and zero is a measurement, and
    // this is where that rule is enforced for EVERY lane. The module header
    // claimed it and only the prefill was checked: `journal::project` writes
    // `output_tokens.unwrap_or_default()` -- correctly, because the journal
    // cannot refuse -- so a server reporting `prompt_tokens` and no
    // `completion_tokens` (legal, and some serving stacks do it) put a
    // fabricated `0` on every response row of the record, at exit 0, with
    // `check-record` accepting it. That is the defect this module says it
    // exists to prevent, committed by this module.
    if let Some(answer) = call.outcome.answer()
        && answer.output_tokens.is_none()
    {
        return Err(Halt::Unmeasured {
            turn,
            what: "response.output_tokens",
        });
    }
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
    Ok(())
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
        Concurrency, Dialect, Limits, Message, RequestShape, Role, SamplerCard, SamplerSetting,
        Serving,
    };
    use crate::client::stub::{Act, Stub};
    use crate::client::transport::{Endpoint, Http};
    use crate::formats::record::json::Value;
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
        shaped(script, acts, ground, shape())
    }

    /// The same, with a request shape the caller chose.
    fn shaped(
        script: &Script,
        acts: Vec<Act>,
        ground: &Ground,
        shape: RequestShape,
    ) -> Result<Drive, Halt> {
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
                shape,
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
    fn a_stripped_retry_archives_the_body_that_attempt_actually_sent() {
        // The reason the request rows are matched by ID. A connection retry
        // sends the same body twice, so it cannot tell "this attempt's body"
        // from "the last attempt's body"; a 4xx-STRIP can, because the second
        // attempt sends LESS. A record that gave both rows the second body
        // would say the client never pinned the setting it pinned, which is
        // the regime evidence the whole echo mechanism exists to keep.
        let ground = Ground::make("stripped");
        let mut script = three_turns();
        script.turns.truncate(1);
        script.turns[0].fork = None;
        script.turns[0].commands = Vec::new();

        let mut pinned = shape();
        pinned.sampler = SamplerCard::empty()
            .with_decimal(SamplerSetting::Temperature, "0.6")
            .expect("a spelling the record can read back");

        let acts = vec![
            Act::Status(
                400,
                "{\"error\":{\"message\":\"unknown field: temperature\"}}".to_owned(),
            ),
            canned::acts().remove(0),
        ];
        let drive = shaped(&script, acts, &ground, pinned).expect("the drive ran after the strip");

        let bodies: Vec<String> = drive
            .record
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Request { text, .. } => text.clone(),
                _ => None,
            })
            .collect();
        assert_eq!(bodies.len(), 2, "the 4xx was retried with less: {bodies:?}");
        assert!(
            bodies[0].contains("temperature"),
            "the first attempt pinned it: {}",
            bodies[0]
        );
        assert!(
            !bodies[1].contains("temperature"),
            "the second did not, and the record has to show BOTH or it cannot \
             say what the run was served under: {}",
            bodies[1]
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
            crate::digest::sha256_hex(drive.product.as_bytes()),
            "the digest is OF the product, and the product is carried so a \
             reader can recompute it: a digest of a file nobody wrote is a \
             digest nobody can check"
        );
        assert_ne!(
            *product_sha256,
            crate::digest::sha256_hex(b""),
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
        assert!(!first.truncated, "and the canned answer is whole");
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
        assert!(
            !drive.product.contains("evidence:"),
            "and the decline did not become an entry: {}",
            drive.product
        );
    }

    #[test]
    fn an_answer_the_fold_cannot_read_stops_the_drive() {
        // Put to the fold's own contract rather than through the transport,
        // because no input this client can deliver reaches it: the interview
        // grammar's only content rejection is a NUL byte, and the wire reader
        // refuses a raw control character inside a JSON string first --
        // `Unreadable { by: "i/1", why: NotJson(...) }` is what a NUL in a
        // canned reply actually produces. A test that could not reach the
        // branch would be a branch with no test.
        let unreadable = super::fold(
            "DECISION: a NUL follows\u{0}here",
            1,
            super::INTERVIEW,
            None,
        )
        .expect_err("a NUL is not a text emission");
        assert!(
            unreadable.contains("NulByte"),
            "and the parser's own reason is carried: {unreadable}"
        );

        // The other side of it: an answer that parses and keeps nothing is a
        // census, not a refusal. Without this the fold could refuse everything
        // and the assertion above would still pass.
        let (patches, census) = super::fold("(none)", 1, super::INTERVIEW, None)
            .expect("a decline is a typed outcome, not a parse failure");
        assert!(patches.is_empty());
        assert_eq!(census.captured, 0);
        assert!(census.regions > 0, "it read a region and kept none");
    }

    #[test]
    fn a_truncated_fork_answer_is_recorded_as_truncated() {
        // The interview grammar types truncation deliberately, and dropping
        // it made an answer cut off at `max_output_tokens` indistinguishable
        // from a whole one: its surviving fields are folded either way, and
        // nothing said there had been more.
        let ground = Ground::make("truncated");
        let mut script = three_turns();
        script.turns.truncate(1);
        script.turns[0].commands = Vec::new();

        let acts = vec![
            Act::Answer(canned::reply("turn one")),
            Act::Answer(canned::reply(
                "```\nDECISION: keep it\nLEARNED: the answer stops he",
            )),
        ];
        let drive = against(&script, acts, &ground).expect("the drive ran");
        assert!(
            drive.uncaptured[0].truncated,
            "an unclosed fence is the grammar's own truncation signal: {:?}",
            drive.uncaptured[0]
        );
    }

    // -----------------------------------------------------------------------
    // the audit this module will not improvise a grammar for
    // -----------------------------------------------------------------------

    #[test]
    fn a_seams_audit_answer_is_kept_and_not_folded_with_the_wrong_grammar() {
        // The ask a seam puts demands `1. KEEP` / `2. UPDATE: ...`; that
        // vocabulary is #47 item 1 and is not built. The first version folded
        // the answer with the INTERVIEW grammar, so a model answering the ask
        // exactly as pinned produced zero patches and no warning, while an
        // answer carrying a `DECISION:` line the ask never asked for was
        // banked as a ratified note.
        let ground = Ground::make("audit");
        let acts: Vec<Act> = canned::acts()
            .into_iter()
            .enumerate()
            .map(|(at, act)| {
                if at == 4 {
                    Act::Answer(canned::reply("1. KEEP\n2. REMOVE — superseded"))
                } else {
                    act
                }
            })
            .collect();
        let drive = against(&three_turns(), acts, &ground).expect("the drive ran");

        assert_eq!(drive.audits.len(), 1, "one seam, one audit");
        let audit = &drive.audits[0];
        assert_eq!(audit.turn, 2);
        assert!(audit.items > 0, "the ask handed over entries: {audit:?}");
        assert_eq!(
            audit.answered.as_deref(),
            Some("1. KEEP\n2. REMOVE — superseded"),
            "the answer is kept whole, so that when the verdict format lands \
             this is what it will be given"
        );
        assert_eq!(
            drive.seams[0].patches.len(),
            0,
            "and NOTHING was folded from it: a fold with the wrong grammar is \
             worse than no fold, because it produces a number a reader believes"
        );
        assert!(
            !drive.product.contains("ratify"),
            "no entry claims the ratify lane, in either direction: {}",
            drive.product
        );
    }

    #[test]
    fn a_ratify_call_that_did_not_answer_stops_the_drive() {
        // The same rule as the other two lanes. `Ratifier` cannot fail, so
        // this is checked after `settle` -- and before it was, a hung-up
        // ratify call produced a seam row asserting the prompt had been
        // rebuilt after an audit that never happened.
        let ground = Ground::make("ratify-hangup");
        let mut acts: Vec<Act> = canned::acts().into_iter().take(4).collect();
        acts.push(Act::Hangup);
        acts.push(Act::Hangup);
        let halt = against(&three_turns(), acts, &ground)
            .expect_err("a seam cannot follow an audit that did not happen");
        assert!(
            matches!(&halt, Halt::NoAnswer { turn: 2, lane, .. } if lane == super::RATIFY),
            "{halt:?}"
        );
    }

    // -----------------------------------------------------------------------
    // every field of the record, against a run that produced it
    // -----------------------------------------------------------------------

    #[test]
    fn every_row_the_drive_writes_is_true_of_the_run_that_produced_it() {
        // A fresh-instance review replaced TWELVE fields here with fabricated
        // constants -- a tool call's exit and output and argv, a seam's
        // rendered size, a capture's count, the regime's arm, the fork's lane
        // and turn, the tool call's and the seam's turn, an entry's lane, an
        // entry's tag -- and the suite stayed green for every one. This is
        // that test. It is one test rather than twelve because they are one
        // claim: the record says what happened.
        let ground = Ground::make("every-row");
        let mut script = three_turns();
        // A command that PRINTS and FAILS. Both scripted commands are
        // `echo x > file`, so stdout was empty and the exit was zero in every
        // run: `assert!(output.is_some())` could not fail and `exit` could be
        // written as `Some(0)` for ever.
        script.turns[2].commands = vec![Command::new(
            "shell",
            &["sh", "-c", "echo loud; echo bad >&2; exit 3"],
        )];
        let drive = against(&script, canned::acts(), &ground).expect("the drive ran");

        assert_eq!(
            drive.record.regime(),
            &script.regime,
            "the start row is the regime the script declared, not one this \
             module wrote down"
        );

        let tools: Vec<_> = drive
            .record
            .events
            .iter()
            .filter_map(|event| match event {
                Event::ToolCall {
                    at_turn,
                    tool,
                    args,
                    exit,
                    output,
                    ..
                } => Some((*at_turn, tool.clone(), args.clone(), *exit, output.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].0, 1, "the first command ran in turn one");
        assert_eq!(tools[1].0, 3, "and the second in turn THREE, not turn one");
        assert_eq!(tools[1].3, Some(3), "the command's own status, not zero");
        let said = tools[1].4.clone().expect("a tool call carries its output");
        assert!(
            said.contains("loud") && said.contains("bad"),
            "and what the command actually said, both streams: {said:?}"
        );
        let Some(Value::Array(argv)) = tools[1].2.as_ref().and_then(|args| args.get("argv")) else {
            panic!(
                "a tool call carries the argv it was called with: {:?}",
                tools[1].2
            )
        };
        assert_eq!(
            argv,
            &script.turns[2].commands[0]
                .argv
                .iter()
                .map(|part| Value::String(part.clone()))
                .collect::<Vec<_>>(),
            "the argv the SCRIPT named, verbatim"
        );

        let forks: Vec<_> = drive
            .record
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Fork { lane, of_turn, .. } => Some((lane.clone(), *of_turn)),
                _ => None,
            })
            .collect();
        assert_eq!(
            forks,
            vec![
                (super::INTERVIEW.to_owned(), 1),
                (super::INTERVIEW.to_owned(), 2)
            ],
            "each fork names its own lane and its own turn"
        );

        let seams: Vec<_> = drive
            .record
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Seam {
                    at_turn,
                    rendered_bytes,
                    ..
                } => Some((*at_turn, rendered_bytes.get())),
                _ => None,
            })
            .collect();
        assert_eq!(seams.len(), 1);
        assert_eq!(seams[0].0, 2, "the seam fired after turn two");
        assert_eq!(
            seams[0].1,
            drive.seams[0].prefix.len() as u64,
            "and its size is the render's size, measured"
        );
    }

    #[test]
    fn every_entry_the_fold_writes_names_its_lane_and_keeps_its_tag() {
        // Two more of the twelve constants: the provenance's lane could be
        // written as `"ratify"` and the content's tag as `"constraint: "`,
        // and the suite stayed green for both. Invented tags and fabricated
        // provenance are the two failure classes this repository names as the
        // worst, and neither had a test.
        let ground = Ground::make("entries-lane");
        let drive = against(&three_turns(), canned::acts(), &ground).expect("the drive ran");

        assert!(
            drive.product.contains("\"lane\":\"interview\""),
            "the entries name the lane that produced them: {}",
            drive.product
        );
        assert!(
            !drive.product.contains("\"lane\":\"ratify\"")
                && !drive.product.contains("\"lane\":\"main\""),
            "and no other: {}",
            drive.product
        );
        assert!(
            drive.product.contains("decision: keep the reconciler")
                && drive.product.contains("learned: the seam refills"),
            "and carry the tag the answer used, not one this fold chose: {}",
            drive.product
        );
        assert!(
            !drive.product.contains("constraint:"),
            "a tag the answer never used is an invented one: {}",
            drive.product
        );
    }

    #[test]
    fn the_record_carries_what_the_drive_actually_sent() {
        // Four things the drive assembles and sends, none of which any test
        // checked: the seam's render as the system message, the turn's ask,
        // the fork's question, and the seam's audit ask. Each could be
        // replaced with text the script never said and the record still
        // archived it as what was sent. The working set reaching the model is
        // the artefact this whole architecture exists to produce.
        let ground = Ground::make("sent");
        let drive = against(&three_turns(), canned::acts(), &ground).expect("the drive ran");
        let script = three_turns();

        let sent: Vec<String> = drive
            .record
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Request { text, .. } => text.clone(),
                _ => None,
            })
            .collect();
        let all = sent.join("\n");

        for turn in &script.turns {
            assert!(
                all.contains(&turn.ask),
                "the operator's ask reached the model: {:?}",
                turn.ask
            );
            if let Some(question) = &turn.fork {
                assert!(
                    all.contains(question),
                    "and the fork's question: {question:?}"
                );
            }
        }
        assert!(
            all.contains("# working set"),
            "and the seam's render is the system message, not an empty string"
        );
        assert!(
            all.contains("keep the reconciler"),
            "carrying the entries the session captured, which is the whole \
             point of the render: {all}"
        );
        assert!(
            all.contains(&drive.audits[0].asked.replace('\n', "\\n")),
            "and the seam's audit ask is what was sent on the ratify lane, as \
             the dogma pinned it"
        );
    }

    #[test]
    fn a_server_that_counted_the_prompt_and_not_the_answer_halts_too() {
        // The module header claims `turn.prefill_tokens` AND
        // `response.output_tokens`, and only the first was checked: the
        // response row goes through `journal::project`, which writes
        // `output_tokens.unwrap_or_default()` -- correctly, because a journal
        // cannot refuse. So a server reporting the prompt and not the answer
        // put a fabricated `0` on every response row of the record, at exit
        // 0, with `check-record` accepting it.
        let ground = Ground::make("half-measured");
        let halt = against(
            &three_turns(),
            vec![Act::Answer(canned::reply_counting_only_the_prompt(
                "turn one",
            ))],
            &ground,
        )
        .expect_err("a response row cannot be written from a count nobody made");
        assert!(
            matches!(&halt, Halt::Unmeasured { turn: 1, what } if *what == "response.output_tokens"),
            "{halt:?}"
        );
    }

    #[test]
    fn a_capture_counts_entries_and_not_patches() {
        // The schema says `entries: How many entries it wrote`. `apply_turn`
        // returns one `Applied` per patch whatever it did, so `applied.len()`
        // was the patch count wearing the schema's word for something else --
        // and the shipped fixture was already wrong: turn two's fork repeats
        // turn one's answer, both patches came back `Deduped`, no entry was
        // written, and the record claimed two.
        let ground = Ground::make("entries");
        let drive = against(&three_turns(), canned::acts(), &ground).expect("the drive ran");

        let counted: Vec<u32> = drive
            .record
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Capture { entries, .. } => Some(*entries),
                _ => None,
            })
            .collect();
        assert_eq!(
            counted,
            vec![2, 2],
            "each capture wrote to two entries. `Applied::touched` is the \
             object's own answer to which entries a patch moved, and it is what \
             record v0's one field counts -- `applied.len()` was the PATCH count \
             wearing the schema's word for something else, and would say two \
             here for a fold that produced two patches and moved nothing."
        );

        // Ruling 6: the two questions, and the run where they diverge. Turn
        // two's fork repeats turn one's answer word for word -- so it TOUCHED
        // two entries and CREATED none, and a single number cannot say both.
        assert_eq!(
            drive
                .uncaptured
                .iter()
                .map(|census| (census.entries_touched, census.entries_created))
                .collect::<Vec<_>>(),
            vec![(2, 2), (2, 0)],
            "the first fork created what it touched; the second created \
             nothing and touched the same two. A reader summing `entries` \
             across captures to size the object would double it: {:?}",
            drive.uncaptured
        );
        assert_eq!(
            drive.product.lines().count(),
            2,
            "and the object holds two entries, which is the created total and \
             not the touched one"
        );
    }

    #[test]
    fn the_two_entry_counts_answer_two_questions_and_the_record_carries_one() {
        // The whole of ruling 6 in one test. `entries_touched` asks *did the
        // fork produce signal*; `entries_created` asks *did it produce NEW
        // signal*, which is the collector's dedup economics. A single field
        // named `entries` reads as whichever its reader had in mind, and the
        // two diverge exactly where the interesting case is.
        let ground = Ground::make("two-counts");
        let mut script = three_turns();
        script.turns.truncate(1);
        script.turns[0].commands = Vec::new();

        // One fork, two regions saying the same thing: two patches, one entry,
        // and the second patch dedupes onto the first.
        let acts = vec![
            Act::Answer(canned::reply("turn one")),
            Act::Answer(canned::reply(canned::FORK_REPEATS_ITSELF)),
        ];
        let drive = against(&script, acts, &ground).expect("the drive ran");

        let census = &drive.uncaptured[0];
        assert_eq!(census.captured, 2, "two patches were folded");
        assert_eq!(census.entries_touched, 1, "onto one entry");
        assert_eq!(census.entries_created, 1, "which it brought into existence");
        assert_eq!(drive.product.lines().count(), 1);

        // Record v0 carries the TOUCHED count under its one field, because
        // that is what it has always been counting. A v1 row will carry both;
        // nothing here writes `created` into a v0 record, because a field that
        // meant one thing in old records and another in new ones is
        // unreadable across the archive.
        let Some(Event::Capture { entries, .. }) = drive
            .record
            .events
            .iter()
            .find(|event| matches!(event, Event::Capture { .. }))
        else {
            panic!("a capture row")
        };
        assert_eq!(*entries, census.entries_touched);
    }

    #[test]
    fn every_audit_names_the_turn_it_actually_came_from() {
        // The ratifier is owned by the controller and lent back as `&R`, so
        // the turn it is settling reaches it through a `Cell`. Without that it
        // read a field nobody wrote and every ratified patch claimed
        // provenance from turn 0 -- not a turn, and banked in the object as a
        // fact about where the entry came from. Nothing is folded from an
        // audit any more, but the turn still reaches the ratifier the same
        // way, and this is what holds it.
        let ground = Ground::make("provenance");
        let drive = against(&three_turns(), canned::acts(), &ground).expect("the drive ran");

        assert_eq!(
            drive.audits[0].turn, 2,
            "the seam fired after turn 2, so its audit belongs to turn 2"
        );

        // And the fork lane's entries, which is the half that was already
        // right and would otherwise carry this test on its own.
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
    fn every_seeded_fault_still_names_source_that_is_there() {
        crate::gate::every_seeded_fault_still_names_source(
            include_str!("../../drive/gate.toml"),
            // The lane's whole source, so a catcher can be looked for
            // wherever its test lives rather than only in this file.
            concat!(
                include_str!("mod.rs"),
                include_str!("canned.rs"),
                include_str!("script.rs"),
                // The regimen reader, lifted out of `bin/drive.rs` so both
                // binaries share one. A fault targeting it is already in the
                // manifest; a catcher living beside it has to be findable
                // here too.
                include_str!("regimen.rs"),
                // The binary's own tests, which live outside `src/` and are the
                // only thing that runs the program. A `catches` naming one of
                // them has to be checkable here too, or the half of this lane
                // that is a second process is the half the guard does not see.
                include_str!("../../tests/drive_cli.rs")
            ),
            "drive",
        );
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

        // Named ONCE each. Six calls project the same losses six times, and a
        // list that repeated them would be a count of calls dressed as a
        // census of the schema's gaps -- and the deduplication that stops that
        // had no test.
        let mut seen = drive.unspellable.clone();
        let before = seen.len();
        seen.dedup();
        seen.sort_by_key(|lost| (lost.kind, lost.what));
        seen.dedup();
        assert_eq!(before, seen.len(), "each gap once: {:?}", drive.unspellable);
    }

    #[test]
    fn the_product_is_the_object_and_names_its_entries() {
        // `product_sha256` is self-consistent with whatever `dump` emits, so
        // the digest cannot catch a change in WHAT is digested. Dropping the
        // entry ids from every line left the suite green: the digest moved
        // with it and agreed with itself.
        let ground = Ground::make("product");
        let drive = against(&three_turns(), canned::acts(), &ground).expect("the drive ran");

        let lines: Vec<&str> = drive.product.lines().collect();
        assert!(!lines.is_empty(), "the session built something");
        for line in &lines {
            let (id, entry) = line
                .split_once('\t')
                .unwrap_or_else(|| panic!("a product line is `id \\t entry`: {line:?}"));
            assert!(!id.is_empty(), "and the id is there: {line:?}");
            assert!(
                entry.starts_with('{') && entry.contains(&format!("\"id\":\"{id}\"")),
                "and the entry beside it is the object's own dump of that id: {line:?}"
            );
        }
        assert_eq!(
            drive.product.matches('\n').count(),
            lines.len(),
            "every line terminated, so two products concatenate rather than run together"
        );
    }
}
