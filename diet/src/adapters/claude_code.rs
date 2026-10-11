//! The Claude Code session log, read into this crate's events.
//!
//! Claude Code writes one JSON object per line to
//! `~/.claude/projects/<slug>/<session>.jsonl`. The first log this adapter was
//! written against is this repository's own.
//!
//! # THE ONE READING. Every absolute count from that log is here and nowhere
//! else.
//!
//! The log is the session this adapter was written IN, so it grew while it
//! was being read -- 5,577 rows at the first pass, 6,348 at the second,
//! **7,674 at the reading below**. An earlier version of this file quoted
//! whichever number was current when each paragraph was written, and ended up
//! carrying four different readings as though each were *the* number: 2,173
//! assistant rows in one comment and 2,122 in another, 628 `thinking` blocks
//! here and 646 in the regimen beside it. Every one was true when written.
//! That is [#77]'s class exactly -- prose about the tree that nothing checks
//! -- committed by the file whose whole subject is counts that are true.
//!
//! So: the counts live in this block, stamped, and **no other comment in this
//! lane restates one.** Elsewhere the argument is made in proportions and
//! shapes, which do not drift. A reader who wants to check a proportion reads
//! it here; a reader who reruns the adapter gets different absolutes and
//! should, because the file moved.
//!
//! These cannot be bound to a test the way a claim about a committed fixture
//! can: **the log is not in the tree and cannot be** -- it is an unscrubbed
//! session transcript. That is the same limit `Start.source.source_available
//! = pinned_only` exists to make machine-readable, and it is stated here
//! rather than implied.
//!
//! ```text
//! reading of 2026-09-13, 7,674 rows, 9 top-level kinds
//!
//!   mapped     assistant 2,471   user 1,454                    3,925  51%
//!   unmapped   attachment 1,641  last-prompt 501  atis-latch 445
//!              custom-title 443  mode 402  queue-operation 261
//!              system 56                                       3,749  49%
//!
//!   of the 1,454 `user` rows, 1,357 carry only tool output            93%
//!   `thinking` blocks, which have no event kind here                   689
//!   tool calls                                                       1,359
//! ```
//!
//! **Roughly half the file has no schema kind**, which is the fact this
//! adapter exists to make visible rather than to improve.
//!
//! # The mapping, and the two places it would be easy to lie
//!
//! **A `user` row is not a turn.** Nine in ten `user` rows carry nothing but
//! `tool_result` blocks -- the harness handing a tool's output back to the
//! model, which is not a person saying anything. Mapping every `user` row to
//! a turn would have invented a turn for each of them, in a record whose
//! whole purpose is that its counts are true. So a `user` row opens a turn only when it carries TEXT; otherwise
//! its `tool_result` blocks are joined to the call they answer.
//!
//! Text, and not "text a person wrote", which is what this said first and is
//! a claim the predicate cannot make. A handful of the turns the census
//! counts in the reference log are slash-command envelopes the harness
//! composed (`<command-name>…</command-name>`), and a log written by a
//! subagent
//! carries its parent's prompts in the same position. The row says a person
//! is the author; this adapter reports what the row says and does not
//! second-guess it, and the distance between "the `user` role" and "a human
//! being" is the operator's to judge rather than a heuristic's to erase.
//!
//! **`input_tokens` is not the prefill.** A typical assistant row in that log
//! reads something like `input_tokens: 2` beside
//! `cache_read_input_tokens: 38639`. Those are one row's fields rather than a
//! count of the file, and the shape is the point: recording the first as the
//! turn's
//! prefill would say a turn prefilled two tokens when it prefilled sixty-one
//! thousand: cached tokens are still tokens that were fed to the model, and
//! the cache is an accounting detail of what they cost, not of whether they
//! were sent. The prefill here is the sum of all three.
//!
//! # What it cannot carry
//!
//! `thinking` blocks have no event kind in this schema, so they are counted
//! as dropped content rather than quietly discarded; the reading above says
//! how many, and a dropped block is not in the record -- there is no
//! `Event` a block of content can become on its own.
//!
//! A row this adapter has no KIND for is different. Ruling 1 on #76 refused
//! the position this paragraph used to state -- that the census alone
//! discharges "never dropping silently" -- on the grounds that a census is a
//! count of what was lost, not the content, and a replay viewer cannot render
//! a row that does not exist. So the seven unmapped row kinds are counted as
//! unmapped rows **and** carried into the record as [`Event::Unknown`], one
//! per row, holding the row's own spelling of its kind and the row verbatim.
//! An adapted record is still **lossy by declaration** -- content inside a
//! mapped row that this schema has no home for does not survive -- but an
//! unmapped ROW does, typed as what it is: unread.
//!
//! # `isSidechain` and `isMeta`: read, because the harness declared them
//!
//! The "does not second-guess" rule above is about CONTENT -- the adapter
//! will not decide that a `user` row's text is not really a person's because
//! of what the text says. `isSidechain` and `isMeta` are not content: they
//! are booleans the harness itself writes on the row, the same category as
//! `is_error` on a `tool_result`. Reading them is not a guess.
//!
//! A row carrying `isSidechain: true` is a subagent's own exchange, logged
//! inline with the session that spawned it; one carrying `isMeta: true` is
//! text the harness composed rather than a person -- a resumption prompt, a
//! hook's feedback, some (not all) slash-command envelopes. Left unread, both
//! open a turn the census reports as the top-level session's own turn, which
//! is [#76]'s known defects 1 and 2. Either flag routes the row to
//! [`Census::no_event`] instead: mapped, because the kind is one this adapter
//! knows, and silent, because it is not this session's turn to count.
//!
//! A row carrying `isCompactSummary: true` is the harness's own replacement
//! for a history it discarded, and it is neither of the above: it is an event
//! of its own kind ([`Event::Compaction`], #79). Before that kind existed the
//! row carried text, so the turn rule took it, and a compaction became **a
//! turn a person did not take** -- while the thing that actually happened,
//! the harness throwing away every session's prefix and moving the
//! session-start marker, went unrecorded. Read from the flag, never from the
//! prose: matching "This session is being continued from" would fire on any
//! transcript that quotes the phrase.
//!
//! **`isMeta` is not a complete fix for defect 2.** Checked against this
//! adapter's own reference log (see the reading above): of seven rows
//! carrying a `<command-name>` envelope, only three carry `isMeta: true`. The
//! harness does not tag every synthetic row it composes, and the remaining
//! four are turns still, exactly the defect this paragraph is not closing.
//! Text-sniffing `<command-name>` to catch the rest would be the content
//! guess this module refuses to make. What is fixed is real: every row the
//! harness DOES declare synthetic is no longer misattributed.

use std::collections::BTreeMap;
use std::path::Path;

use super::{Adapted, Adapter, Census, Drift};
use crate::formats::record::{CompactionSource, Count, CountSource, Event, json::Value};

/// The Claude Code session-log adapter.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClaudeCode;

/// The row kinds this adapter has a mapping for.
///
/// Kept beside [`Row`] rather than derived from it because `Adapter::maps`
/// hands back a `&'static [&'static str]` and a const cannot walk an enum.
/// `the_two_foreign_vocabularies_have_one_definition_each` holds the two in
/// step, which is the weaker guarantee and is named as such.
const MAPS: &[&str] = &["user", "assistant"];

/// A row kind this adapter maps.
///
/// A type rather than a string, because `diet/src` admits no match arm on a
/// string literal and the reason is this exact situation: the foreign
/// vocabulary is matched in several places, and a sixth kind added as an arm
/// somewhere would compile while the others quietly stopped covering it. A
/// tag becomes a `Row` in ONE place -- [`Row::from_tag`], walking
/// [`Row::ALL`] -- and every use after that is exhaustive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    /// A person's turn, or the harness handing back a tool's output.
    User,
    /// What the model said, called, or thought.
    Assistant,
}

impl Row {
    /// Every row kind this adapter maps.
    const ALL: &'static [Self] = &[Self::User, Self::Assistant];

    /// The tag Claude Code writes for it.
    const fn tag(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }

    /// The one place a foreign tag becomes a row kind.
    fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|row| row.tag() == tag)
    }
}

/// A content block this adapter reads.
///
/// Same reason as [`Row`], and the same shape. A block type that is not one
/// of these is counted as dropped content under its own foreign name, which
/// is why `from_tag` returns an `Option` rather than a fallback variant: "a
/// block this adapter has no word for" is a fact about the census, not a kind
/// of block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    /// Text the model wrote, or a person did.
    Text,
    /// A call the model made.
    ToolUse,
    /// The answer to one.
    ToolResult,
}

impl Block {
    /// Every block type this adapter reads.
    const ALL: &'static [Self] = &[Self::Text, Self::ToolUse, Self::ToolResult];

    /// The tag Claude Code writes for it.
    const fn tag(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::ToolUse => "tool_use",
            Self::ToolResult => "tool_result",
        }
    }

    /// The one place a foreign tag becomes a block type.
    fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|block| block.tag() == tag)
    }
}

/// Claude Code's tool names, in the vocabulary the capture lanes speak.
///
/// **This table is why the adapter exists.** `diet::capture::mechanical`
/// knows `bash`, `read_file` and `edit_file`; Claude Code writes `Bash`,
/// `Read`, `Edit` and `Write`. Pointed at a real log without this, the lane
/// matched not ONE tool call in the reference log and derived nothing -- no
/// working directory, no file touched, no failed `cd` -- while every count in
/// the census still read as a success. A translation nobody wrote is a lane
/// that silently observes nothing, which is this issue's own failure mode
/// wearing the library's clothes.
///
/// Only names whose CONTRACT matches are translated. `Grep`, `Glob`, `Task`
/// and every `mcp__*` tool are left as they are: the lane records them as
/// commands it does not interpret, which is true, where guessing at their
/// arguments would be a claim about a contract nobody wrote down.
const TOOL_NAMES: &[(&str, &str)] = &[
    ("Bash", "bash"),
    ("Read", "read_file"),
    ("Edit", "edit_file"),
    ("Write", "write_file"),
];

/// Claude Code's argument for the file a path tool acts on.
const FOREIGN_PATH: &str = "file_path";

/// The argument name the capture lane reads a path from.
const NATIVE_PATH: &str = "path";

/// `name` in the capture lanes' vocabulary, and whether it was translated.
///
/// **Matched without regard to case**, per the ruling on #76, and for the
/// reason the table exists at all: the specimen was a lane that knew `bash`
/// while the harness wrote `Bash`, and a lookup that is exact about case is
/// the same defect waiting on the next harness to write `BASH` or the next
/// release to lower-case its own table. Case-folding joined the record's
/// normalisation of model identifiers on #65; it joins the adapter's here.
///
/// ASCII-folded rather than Unicode-folded on purpose. A tool name is an
/// identifier in a harness's source, and the four this table knows are ASCII;
/// full Unicode case folding would make `ẞ` and `SS` the same tool, which is
/// a claim about a foreign vocabulary nobody made. A name outside ASCII is
/// left alone and reaches the lane as written.
fn native_tool(name: &str) -> (&str, bool) {
    TOOL_NAMES
        .iter()
        .find(|(foreign, _)| foreign.eq_ignore_ascii_case(name))
        .map_or((name, false), |(_, native)| (*native, true))
}

impl Adapter for ClaudeCode {
    fn name(&self) -> &'static str {
        "claude-code"
    }

    fn maps(&self) -> &'static [&'static str] {
        MAPS
    }

    fn adapt(&self, log: &str, substrate: &str) -> Result<Adapted, Drift> {
        let mut run = Run::new(self.name(), substrate);
        let rows = unrepeated(parse(log)?, &mut run.census)?;
        for (at, (at_row, raw, row)) in rows.iter().enumerate() {
            run.row(*at_row, raw, row, rows.get(at + 1..).unwrap_or(&[]))?;
        }
        Ok(Adapted {
            events: run.events,
            census: run.census,
        })
    }
}

/// One delegated run's transcript, found beside its session's log (#708).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sidechain {
    /// The agent that ran it: the id its spawning call's result names, and
    /// the name of its file, `agent-<agentId>.jsonl`.
    pub agent_id: String,
    /// The workflow run it belongs to, for a workflow's agent: the directory
    /// it sits in, `subagents/workflows/<runId>/`. `None` for an agent the
    /// session spawned directly.
    pub run_id: Option<String>,
    /// The `agentType` its metadata file names, when it has one.
    pub agent_type: Option<String>,
    /// The transcript.
    pub text: String,
}

/// What [`sidechains_beside`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    /// Each transcript, agents first, then each run's, in name order.
    pub sidechains: Vec<Sidechain>,
    /// Workflow journals: a run's own event log, not a model's turns, so
    /// counted and never replayed.
    pub journals: u64,
}

/// The sidechain transcripts the harness wrote beside `log` (#708), by its
/// convention: a directory named for the log, `subagents/agent-*.jsonl` for
/// the agents the session spawned, `subagents/workflows/<runId>/agent-*.jsonl`
/// for a workflow's, each with a `.meta.json` beside it naming its type, and a
/// `journal.jsonl` per run. Nothing there is nothing found.
///
/// # Errors
///
/// When a directory that is there cannot be read.
pub fn sidechains_beside(log: &Path) -> std::io::Result<Found> {
    let mut found = Found::default();
    let subagents = log.with_extension("").join("subagents");
    if !subagents.is_dir() {
        return Ok(found);
    }
    found.sidechains.extend(agents_in(&subagents, None)?);
    let workflows = subagents.join("workflows");
    if workflows.is_dir() {
        let mut runs: Vec<_> = std::fs::read_dir(&workflows)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        runs.sort();
        for run in runs {
            let run_id = run.file_name().and_then(|name| name.to_str());
            found.journals += u64::from(run.join("journal.jsonl").is_file());
            found.sidechains.extend(agents_in(&run, run_id)?);
        }
    }
    Ok(found)
}

/// Each `agent-<id>.jsonl` directly in `dir`, in name order.
fn agents_in(dir: &Path, run_id: Option<&str>) -> std::io::Result<Vec<Sidechain>> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    paths.sort();
    let mut found = Vec::new();
    for path in paths {
        let Some(agent_id) = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.strip_prefix("agent-"))
        else {
            continue;
        };
        let agent_type = std::fs::read_to_string(path.with_extension("meta.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|meta| meta.get("agentType")?.as_str().map(str::to_owned));
        found.push(Sidechain {
            agent_id: agent_id.to_owned(),
            run_id: run_id.map(str::to_owned),
            agent_type,
            text: std::fs::read_to_string(&path)?,
        });
    }
    Ok(found)
}

impl ClaudeCode {
    /// [`Adapter::adapt`], with the sidechains `found` beside the log
    /// joined as side lanes (#708): each off the call that spawned it -- an
    /// `Agent` call by the `agentId` its result names, a `Workflow` call by
    /// its `runId` -- at that call's turn, its lane `sidechain/<agentId>`
    /// (`sidechain/<runId>/<agentId>` for a workflow's agent, whose id need
    /// not be unique across runs). Each is read as the main log is, repeats
    /// dropped and models counted; what did not join is counted, never
    /// dropped silently.
    ///
    /// # Errors
    ///
    /// As [`Adapter::adapt`], for the log or any sidechain.
    pub fn adapt_with(&self, log: &str, found: &Found, substrate: &str) -> Result<Adapted, Drift> {
        let mut run = Run::new(self.name(), substrate);
        let rows = unrepeated(parse(log)?, &mut run.census)?;
        for (at, (at_row, raw, row)) in rows.iter().enumerate() {
            run.row(*at_row, raw, row, rows.get(at + 1..).unwrap_or(&[]))?;
        }
        let spawns = std::mem::take(&mut run.spawns);
        let mut joined = vec![false; spawns.len()];
        for side in &found.sidechains {
            let spawned = spawns.iter().position(|spawn| match &side.run_id {
                Some(run_id) => spawn.run_id.as_ref() == Some(run_id),
                None => spawn.agent_id.as_ref() == Some(&side.agent_id),
            });
            let Some(at) = spawned else {
                run.census.sidechains_add("transcripts naming no call", 1);
                continue;
            };
            joined[at] = true;
            let lane = match &side.run_id {
                Some(run_id) => format!("sidechain/{run_id}/{}", side.agent_id),
                None => format!("sidechain/{}", side.agent_id),
            };
            let kind = if side.run_id.is_some() {
                "joined/workflow agent"
            } else {
                "joined/agent"
            };
            run.census.sidechains_add(kind, 1);
            if let Some(agent_type) = &side.agent_type {
                run.census.sidechains_add(&format!("type/{agent_type}"), 1);
            }
            run.side_lane(&lane, spawns[at].turn, &side.text)?;
        }
        let unjoined = joined.iter().filter(|joined| !**joined).count();
        run.census.sidechains_add(
            "spawning calls with no transcript",
            u64::try_from(unjoined).unwrap_or(u64::MAX),
        );
        run.census
            .sidechains_add("journals not replayed", found.journals);
        Ok(Adapted {
            events: run.events,
            census: run.census,
        })
    }
}

/// A delegated run's spawning call, as the main log showed it (#708).
#[derive(Debug, Clone)]
struct Spawn {
    /// The turn that made the call.
    turn: u32,
    /// The agent its result names, for an `Agent` call.
    agent_id: Option<String>,
    /// The workflow run its result names, for a `Workflow` call.
    run_id: Option<String>,
}

/// The side lane a walk is reading (#708): its name, and how many asks it
/// has been given so far.
#[derive(Debug, Clone)]
struct Side {
    lane: String,
    asks: u32,
}

/// The tools whose calls spawn a delegated run with its own transcript.
const SPAWNING: &[&str] = &["Agent", "Workflow"];

/// Every non-blank line, as its own text, JSON, and the FILE line it came
/// from.
///
/// The line number is carried rather than recovered, because it cannot be
/// recovered: blank lines are skipped, so a row's position in this vector is
/// not its position in the file. The first version returned bare values and
/// numbered the refusals by vector index, which meant `NotJson` counted file
/// lines and `MissingField` counted rows -- two numbering systems inside one
/// enum whose whole doc comment is that a refusal says WHERE. One blank line
/// anywhere above the fault was enough to send a reader to the wrong line.
///
/// The line's own text is carried too, alongside the value parsed from it,
/// for `Event::Unknown::raw`: that field is documented "as it appeared,
/// verbatim", and a value re-serialized from a parsed `serde_json::Value` is
/// not verbatim -- key order and spacing are not guaranteed to survive a
/// round trip, and the record's own byte-identical-render discipline is the
/// reason that distinction is not academic here.
fn parse(log: &str) -> Result<Vec<(usize, &str, serde_json::Value)>, Drift> {
    let mut rows = Vec::new();
    for (at, line) in log.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let row: serde_json::Value = serde_json::from_str(line).map_err(|why| Drift::NotJson {
            at_row: at + 1,
            why: why.to_string(),
        })?;
        if !row.is_object() {
            return Err(Drift::NotAnObject { at_row: at + 1 });
        }
        rows.push((at + 1, line, row));
    }
    Ok(rows)
}

/// The fields that are a row's content: what a repeat must match to be one
/// (#500). Everything else -- `slug`, `promptId`, `cwd`, `gitBranch`,
/// `parentUuid` -- is the envelope the app rewrites when a session resumes.
const CONTENT: &[&str] = &["type", "message", "toolUseResult", "attachment"];

/// `rows` with every repeat of a row id already read dropped (#500): the
/// first occurrence kept, each drop counted by what it rewrote, and every
/// row without a `uuid` kept.
///
/// The desktop app writes a resumed session's earlier rows again, under
/// their own ids, sometimes with the envelope rewritten. A repeat is the
/// same row written twice, so it is dropped; a row under a known id whose
/// content differs is two rows, and is refused.
fn unrepeated<'a>(
    rows: Vec<(usize, &'a str, serde_json::Value)>,
    census: &mut Census,
) -> Result<Vec<(usize, &'a str, serde_json::Value)>, Drift> {
    let mut first: BTreeMap<String, usize> = BTreeMap::new();
    let mut kept: Vec<(usize, &'a str, serde_json::Value)> = Vec::with_capacity(rows.len());
    for (at_row, raw, row) in rows {
        let Some(id) = row.get("uuid").and_then(serde_json::Value::as_str) else {
            kept.push((at_row, raw, row));
            continue;
        };
        let Some(&at) = first.get(id) else {
            first.insert(id.to_owned(), kept.len());
            kept.push((at_row, raw, row));
            continue;
        };
        let (first_row, _, original) = &kept[at];
        if let Some(field) = CONTENT
            .iter()
            .find(|field| row.get(**field) != original.get(**field))
        {
            return Err(Drift::RepeatDiffers {
                at_row,
                first_row: *first_row,
                field: (*field).to_owned(),
            });
        }
        census.repeated_one(if row == *original {
            "exact"
        } else if row.get("parentUuid") != original.get("parentUuid") {
            "parent-rewritten"
        } else {
            "envelope-rewritten"
        });
    }
    Ok(kept)
}

/// The walk over one log.
struct Run {
    events: Vec<Event>,
    census: Census,
    /// The declared substrate every request row names.
    ///
    /// Carried rather than looked up, because there is nowhere to look it up:
    /// the log names a model per response (#501), not the substrate that
    /// served it. It comes from the regimen the operator wrote.
    substrate: String,
    /// The turn a person most recently opened. Zero means none yet.
    turn: u32,
    /// `tool_use_id` -> where its [`Event::ToolCall`] sits in `events`, so the
    /// `tool_result` that answers it can be joined to the call it answers
    /// rather than filed as a row of its own.
    awaiting: BTreeMap<String, usize>,
    /// The turn the last compaction was in, and how many have been emitted
    /// for it. A pair rather than a session-wide counter so a compaction's id
    /// reads as *the nth compaction of turn k*.
    compacted: (u32, u32),
    /// The delegated runs the main log's calls spawned (#708).
    spawns: Vec<Spawn>,
    /// The side lane this walk reads, when it reads a sidechain (#708).
    side: Option<Side>,
}

impl Run {
    fn new(adapter: &str, substrate: &str) -> Self {
        Self {
            events: Vec::new(),
            census: Census {
                adapter: adapter.to_owned(),
                ..Census::default()
            },
            substrate: substrate.to_owned(),
            turn: 0,
            awaiting: BTreeMap::new(),
            compacted: (0, 0),
            spawns: Vec::new(),
            side: None,
        }
    }

    /// The sidechain `text` read as the side lane `lane`, off turn `turn`
    /// (#708): a fork opening the lane, then its rows walked as the main
    /// log's are, on the lane.
    fn side_lane(&mut self, lane: &str, turn: u32, text: &str) -> Result<(), Drift> {
        let (main_turn, awaiting) = (self.turn, std::mem::take(&mut self.awaiting));
        self.turn = turn;
        self.side = Some(Side {
            lane: lane.to_owned(),
            asks: 0,
        });
        self.events.push(Event::Fork {
            id: lane.to_owned(),
            lane: lane.to_owned(),
            substrate: self.substrate.clone(),
            of_turn: turn,
            cache_read_share: None,
        });
        let rows = unrepeated(parse(text)?, &mut self.census)?;
        self.census
            .sidechains_add("rows", u64::try_from(rows.len()).unwrap_or(u64::MAX));
        for (at, (at_row, raw, row)) in rows.iter().enumerate() {
            self.row(*at_row, raw, row, rows.get(at + 1..).unwrap_or(&[]))?;
        }
        let asks = self.side.take().map_or(0, |side| side.asks);
        self.census.sidechains_add("turns", u64::from(asks));
        self.turn = main_turn;
        self.awaiting = awaiting;
        Ok(())
    }

    /// A call that spawns a delegated run, answered (#708): its turn and
    /// the agent or run its result names, kept for the sidechains to join.
    fn spawned(&mut self, block: &serde_json::Value, row: &serde_json::Value) {
        let Some(at) = block
            .get("tool_use_id")
            .and_then(serde_json::Value::as_str)
            .and_then(|id| self.awaiting.get(id))
        else {
            return;
        };
        let Some(Event::ToolCall { tool, at_turn, .. }) = self.events.get(*at) else {
            return;
        };
        if !SPAWNING.contains(&tool.as_str()) {
            return;
        }
        let named = |key: &str| {
            row.pointer(&format!("/toolUseResult/{key}"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        self.spawns.push(Spawn {
            turn: *at_turn,
            agent_id: named("agentId"),
            run_id: named("runId"),
        });
    }

    /// One row. `rest` is what follows it, for the prefill lookahead.
    fn row(
        &mut self,
        at_row: usize,
        raw: &str,
        row: &serde_json::Value,
        rest: &[(usize, &str, serde_json::Value)],
    ) -> Result<(), Drift> {
        let Some(kind) = row.get("type").and_then(serde_json::Value::as_str) else {
            return Err(Drift::NoKind { at_row });
        };
        // A kind this adapter has no word for is NEWS, not a failure: the
        // census carries it, ruling 1 on #76 carries the row itself into the
        // record as `Event::Unknown` -- a census is a count of what was lost,
        // not the content -- and the walk goes on.
        let Some(mapped) = Row::from_tag(kind) else {
            self.census.unmapped_one(kind);
            self.events.push(Event::Unknown {
                source_kind: kind.to_owned(),
                raw: raw.to_owned(),
            });
            return Ok(());
        };
        self.census.mapped_one(kind);
        // Declared, not guessed: `isSidechain`/`isMeta` are booleans the
        // harness writes on the row, not a reading of what the row says. A
        // row either flag names is mapped -- the kind is one this adapter
        // knows -- and silent, because it is a subagent's own exchange or
        // text the harness composed, never the top-level session's turn.
        // A sidechain's own rows are the lane's turns when the walk reads
        // that sidechain as a side lane (#708); its `isMeta` text is still
        // the harness's.
        let excluded =
            excluded_from_parent(row).filter(|why| self.side.is_none() || *why != "isSidechain");
        if let Some(why) = excluded {
            self.census.no_event_one(&format!("{}/{why}", mapped.tag()));
            return Ok(());
        }
        let before_events = self.events.len();
        let before_turn = self.turn;
        match mapped {
            Row::User => self.user(at_row, row, rest),
            Row::Assistant => self.assistant(at_row, row),
        }
        .or_else(|drift| {
            self.unrepresentable_row(at_row, raw, kind, before_events, before_turn, drift)
        })
    }

    /// #76 known defect 4, ruled: a mapped row whose OWN content the record's
    /// value space cannot hold becomes an [`Event::Unknown`] row rather than
    /// refusing the whole log -- the "counted and carried on from" treatment
    /// ruling 1 gave an unmapped KIND, extended to a mapped kind whose
    /// content did not fit. Format drift (a renamed or absent field) still
    /// refuses: that is a claim the schema itself moved, which puts every
    /// row already read in doubt, not only this one.
    ///
    /// Scoped to `drift`'s own `at_row` matching THIS row: the prefill
    /// lookahead raises `Unrepresentable` naming a LATER row (the assistant
    /// row that answers this turn), and downgrading this row for that would
    /// show the wrong row's raw text beside someone else's bad value, so
    /// that case still propagates as a refusal.
    ///
    /// `events`/`awaiting` are rolled back to what they were before this row
    /// was attempted: a `user`/`assistant` row can process several blocks
    /// (several tool calls, say), and one further along failing must not
    /// leave the earlier ones' events standing beside the `Unknown` row that
    /// replaces the whole thing.
    fn unrepresentable_row(
        &mut self,
        at_row: usize,
        raw: &str,
        kind: &str,
        before_events: usize,
        before_turn: u32,
        drift: Drift,
    ) -> Result<(), Drift> {
        let Drift::Unrepresentable {
            at_row: bad_row,
            field,
            why,
        } = &drift
        else {
            return Err(drift);
        };
        if *bad_row != at_row {
            return Err(drift);
        }
        self.events.truncate(before_events);
        self.awaiting.retain(|_, at| *at < before_events);
        self.turn = before_turn;
        self.census
            .unrepresentable_one(&format!("{kind}: {field} ({why})"));
        self.events.push(Event::Unknown {
            source_kind: kind.to_owned(),
            raw: raw.to_owned(),
        });
        Ok(())
    }

    /// A `user` row: a person's turn, or a tool's answer, never both invented.
    fn user(
        &mut self,
        at_row: usize,
        row: &serde_json::Value,
        rest: &[(usize, &str, serde_json::Value)],
    ) -> Result<(), Drift> {
        let content = content_of(at_row, Row::User.tag(), row)?;
        let mut said = String::new();
        match content {
            serde_json::Value::String(text) => said.push_str(text),
            serde_json::Value::Array(blocks) => {
                for block in blocks {
                    let tag = block_type(block);
                    match tag.and_then(Block::from_tag) {
                        Some(Block::Text) => {
                            // Declared, so read or refused. Shrugging here
                            // drops a person's turn on the floor with a
                            // census that says nothing happened -- which is
                            // the module header's own example of the failure
                            // this adapter exists to refuse.
                            let Some(text) = block.get("text").and_then(serde_json::Value::as_str)
                            else {
                                return Err(Drift::MissingField {
                                    at_row,
                                    kind: Row::User.tag().to_owned(),
                                    field: "content[].text.text".to_owned(),
                                });
                            };
                            said.push_str(text);
                        }
                        Some(Block::ToolResult) => {
                            if self.side.is_none() {
                                self.spawned(block, row);
                            }
                            self.tool_result(block);
                        }
                        // A call inside a `user` row. The mapping reads calls
                        // from `assistant` rows only, so this is content with
                        // no home rather than a call -- counted under its own
                        // name, like any other.
                        Some(known @ Block::ToolUse) => {
                            self.census.dropped_one(&format!("user/{}", known.tag()));
                        }
                        None => match tag {
                            Some(other) => self.census.dropped_one(&format!("user/{other}")),
                            None => self.census.dropped_one("user/<untyped block>"),
                        },
                    }
                }
            }
            _ => {
                return Err(Drift::MissingField {
                    at_row,
                    kind: "user".to_owned(),
                    field: "message.content (a string or a list of blocks)".to_owned(),
                });
            }
        }

        // BEFORE THE TURN RULE. #79's third specimen: a harness that
        // compacts injects a synthetic summary message and moves the
        // session-start marker, and this row -- which carries text, so the
        // turn rule would take it -- became A TURN A PERSON DID NOT TAKE.
        // It is not a turn; it is the harness discarding its own prefix,
        // which is a double-cold event even when the model is reused.
        if compacted(row) {
            return self.compaction(at_row, &said);
        }

        if said.trim().is_empty() {
            // Not a turn. The row carried a tool's answer, which has been
            // joined to the call above, or content with no home, which has
            // been counted. Inventing a turn here is the thousand-turn lie.
            //
            // Counted all the same: this is a row whose kind was mapped and
            // which produced no event, and `mapped` alone would report it as
            // carried.
            self.census.no_event_one("user/carried no text of its own");
            return Ok(());
        }

        // A sidechain's ask (#708): a request on its lane, no turn of the
        // session's own.
        if let Some(side) = self.side.as_mut() {
            side.asks += 1;
            self.events.push(Event::Request {
                id: format!("{}/q{}", side.lane, side.asks),
                lane: side.lane.clone(),
                substrate: self.substrate.clone(),
                retry_of: None,
                text: Some(said),
                head_sha256: None,
            });
            return Ok(());
        }

        self.turn += 1;
        let prefill = if let Some(counted) = prefill_after(rest)? {
            counted
        } else {
            // The zero goes in and is declared in the census. (Since #645 a
            // record can leave the count absent; this adapter keeps its
            // declared assumption, which its census pins.)
            self.census.assumed_one(PREFILL_ASSUMED);
            Count::default()
        };
        self.events.push(Event::Turn {
            index: self.turn,
            prefill_tokens: Some(prefill),
            prefill_from: None,
            files: None,
        });
        self.events.push(Event::Request {
            id: format!("u/{}", self.turn),
            lane: "main".to_owned(),
            // Declared, never inferred: the log does not say what answered.
            substrate: self.substrate.clone(),
            retry_of: None,
            text: Some(said),
            // NOT HASHED, and that is the adapted state the field exists
            // for. This log records what was SAID; the bytes the harness
            // actually sent -- its system prompt, its tool definitions, the
            // history it chose to resend -- are not in it. A digest computed
            // from what is here would be a fingerprint of this adapter's
            // reconstruction, which is the copied-from-the-wrong-place
            // reading `request.head_sha256` exists to make impossible.
            head_sha256: None,
        });
        Ok(())
    }

    /// A `user` row the harness itself marked as a compaction summary.
    ///
    /// **Read off a flag the harness writes, never sniffed from prose.**
    /// `isCompactSummary` is a boolean on the row, the same category as the
    /// `isSidechain`/`isMeta` this adapter already reads -- and the same
    /// category the module header insists on. Matching "This session is being
    /// continued from" would be a content guess that fires on any transcript
    /// quoting the phrase, including this one.
    ///
    /// # Before any turn
    ///
    /// Counted with its reason rather than emitted. `Event::Compaction`
    /// names a turn, and the record refuses a row naming a turn that never
    /// happened -- a session resumed from a compaction, whose log opens with
    /// the summary, is exactly that log.
    fn compaction(&mut self, at_row: usize, said: &str) -> Result<(), Drift> {
        if self.turn == 0 {
            self.census
                .no_event_one("user/isCompactSummary before any turn was opened");
            return Ok(());
        }
        // Reset per turn, so an id reads as "the nth compaction of turn k"
        // rather than as a session-wide counter that happens to be unique.
        if self.compacted.0 != self.turn {
            self.compacted = (self.turn, 0);
        }
        let id = match &self.side {
            Some(side) => format!("{}/compact/{}", side.lane, self.compacted.1),
            None => format!("compact/{}/{}", self.turn, self.compacted.1),
        };
        self.compacted.1 += 1;
        // CHARACTERS, through the record's own bound, like every other count
        // this adapter crosses. What compaction cost is the difference
        // between the history that was there and what stands in for it, and
        // this is the half the log affords.
        let summary_chars = count_of(
            at_row,
            "message.content (the compaction summary)",
            said.chars().count() as u64,
        )?;
        self.events.push(Event::Compaction {
            id,
            at_turn: self.turn,
            source: CompactionSource::HarnessCompaction,
            summary_chars,
        });
        Ok(())
    }

    /// An `assistant` row: what it said, what it called, what it thought.
    fn assistant(&mut self, at_row: usize, row: &serde_json::Value) -> Result<(), Drift> {
        let content = content_of(at_row, "assistant", row)?;
        let mut spoke = String::new();
        let blocks = match content {
            serde_json::Value::Array(blocks) => blocks.clone(),
            serde_json::Value::String(text) => {
                spoke.push_str(text);
                Vec::new()
            }
            _ => {
                return Err(Drift::MissingField {
                    at_row,
                    kind: "assistant".to_owned(),
                    field: "message.content (a string or a list of blocks)".to_owned(),
                });
            }
        };

        for block in &blocks {
            let tag = block_type(block);
            match tag.and_then(Block::from_tag) {
                Some(Block::Text) => {
                    let Some(text) = block.get("text").and_then(serde_json::Value::as_str) else {
                        return Err(Drift::MissingField {
                            at_row,
                            kind: Row::Assistant.tag().to_owned(),
                            field: "content[].text.text".to_owned(),
                        });
                    };
                    spoke.push_str(text);
                }
                Some(Block::ToolUse) => self.tool_use(at_row, block)?,
                // An answer in the row that asks. The mapping joins answers
                // from `user` rows, so this is content with no home.
                Some(known @ Block::ToolResult) => {
                    self.census
                        .dropped_one(&format!("assistant/{}", known.tag()));
                }
                // Reasoning has no event kind here. Counted, not discarded.
                None => match tag {
                    Some(other) => self.census.dropped_one(&format!("assistant/{other}")),
                    None => self.census.dropped_one("assistant/<untyped block>"),
                },
            }
        }

        // A response names its token count, so the count has to be there. It
        // is the one field of the mapping that cannot be absent without the
        // event becoming a guess.
        let Some(tokens) = row
            .get("message")
            .and_then(|m| m.get("usage"))
            .and_then(|u| u.get("output_tokens"))
            .and_then(serde_json::Value::as_u64)
        else {
            return Err(Drift::MissingField {
                at_row,
                kind: Row::Assistant.tag().to_owned(),
                field: "message.usage.output_tokens".to_owned(),
            });
        };

        // EVERY assistant row emits a response, whether or not it said
        // anything in words. This used to return early when there was no
        // text, and a row that made a tool call and spoke not at all is the
        // commonest shape in a real log -- three assistant rows in four --
        // and with them FOUR FIFTHS of every token the model generated left
        // through that early return, under a census that read as a clean
        // success.
        // Silence is not absence -- the model ran, the tokens were spent, and
        // `text: None` says exactly that where an omitted event said nothing.
        if self.turn == 0 {
            // Nothing to answer. The record's `Response::to_request` is
            // required and a response naming a request that is not in the
            // stream is a dangling link the record itself would reject; this
            // used to emit `u/1` regardless, inventing turn one in the very
            // field `tool_use` refuses to invent it in sixty lines below.
            self.census
                .no_event_one("assistant/spoke before any turn was opened");
            return Ok(());
        }
        if self.side.as_ref().is_some_and(|side| side.asks == 0) {
            self.census
                .no_event_one("assistant/spoke before its sidechain was asked anything");
            return Ok(());
        }
        let (id, to_request) = match &self.side {
            Some(side) => (
                format!("{}/a/{}", side.lane, self.events.len()),
                format!("{}/q{}", side.lane, side.asks),
            ),
            None => (
                format!("a/{}", self.events.len()),
                format!("u/{}", self.turn),
            ),
        };
        let model = self.model_of(row);
        self.events.push(Event::Response {
            model,
            cache_read: None,
            cache_creation: None,
            cache_creation_5m: None,
            cache_creation_1h: None,
            id,
            to_request,
            output_tokens: Some(count_of(at_row, "message.usage.output_tokens", tokens)?),
            output_from: Some(CountSource::Usage),
            text: (!spoke.trim().is_empty()).then_some(spoke),
            timings: None,
        });
        Ok(())
    }

    /// The model an assistant row names as having answered (#501), counted:
    /// `message.model`, unless it is `<synthetic>` -- the harness's
    /// placeholder for a message no model wrote -- or absent, as in a log
    /// from before the harness recorded one.
    fn model_of(&mut self, row: &serde_json::Value) -> Option<String> {
        match row
            .pointer("/message/model")
            .and_then(serde_json::Value::as_str)
        {
            Some("<synthetic>") => {
                self.census.unattributed_one("synthetic");
                None
            }
            Some(model) => {
                self.census.model_one(model);
                Some(model.to_owned())
            }
            None => {
                self.census.unattributed_one("no model named");
                None
            }
        }
    }

    /// A `tool_use` block becomes a call awaiting its answer.
    fn tool_use(&mut self, at_row: usize, block: &serde_json::Value) -> Result<(), Drift> {
        if self.turn == 0 {
            // Nothing to attach it to: the record refuses a row naming a turn
            // that never happened, and inventing turn one would be the same
            // fabrication in a different field.
            // Under `assistant/tool_use`, like any other unhomed block of
            // that type, with the reason in `no_event` where reasons live.
            // Two vocabularies in one map -- `assistant/thinking` beside
            // `assistant/tool_use before any turn` -- gave a reader tallying
            // by block type two keys for one type.
            self.census
                .dropped_one(&format!("assistant/{}", Block::ToolUse.tag()));
            self.census
                .no_event_one("assistant/called a tool before any turn was opened");
            return Ok(());
        }
        let Some(id) = block.get("id").and_then(serde_json::Value::as_str) else {
            return Err(Drift::MissingField {
                at_row,
                kind: "assistant".to_owned(),
                field: "content[].tool_use.id".to_owned(),
            });
        };
        let Some(tool) = block.get("name").and_then(serde_json::Value::as_str) else {
            return Err(Drift::MissingField {
                at_row,
                kind: "assistant".to_owned(),
                field: "content[].tool_use.name".to_owned(),
            });
        };
        let (native, translated) = native_tool(tool);
        if translated {
            // `tool` and not the table's key: the census records the name the
            // LOG carried, so a harness that started writing `BASH` shows up
            // as its own row rather than being folded into `Bash`'s count and
            // becoming invisible.
            self.census.translated_one(tool, native);
        }
        // `input` is a field this mapping declares, so it is read or refused.
        // Reading a reshaped one as "no arguments" is the same defect
        // `TOOL_NAMES` exists to prevent, one field across: the call is
        // recognised, the census reads as a success, the translation is even
        // counted, and the capture lane derives nothing from a call it can
        // see. `null` is refused by `value_of` for the same reason.
        let args = match block.get("input") {
            None => {
                return Err(Drift::MissingField {
                    at_row,
                    kind: Row::Assistant.tag().to_owned(),
                    field: "content[].tool_use.input".to_owned(),
                });
            }
            Some(serde_json::Value::Object(input)) => {
                let mut args = BTreeMap::new();
                for (key, raw) in input {
                    // The lane reads a path from `path`; Claude Code writes it
                    // as `file_path`. Renamed only for the tools whose
                    // contract this adapter claims to know, so an unknown
                    // tool's `file_path` is carried across untouched rather
                    // than reinterpreted as a contract nobody stated.
                    let key = if translated && key == FOREIGN_PATH {
                        NATIVE_PATH.to_owned()
                    } else {
                        key.clone()
                    };
                    args.insert(key, value_of(at_row, "input", raw)?);
                }
                Some(args)
            }
            Some(other) => {
                return Err(Drift::Unrepresentable {
                    at_row,
                    field: "content[].tool_use.input".to_owned(),
                    why: format!(
                        "it is {}, and a call's arguments are an object",
                        shape_of(other)
                    ),
                });
            }
        };
        self.awaiting.insert(id.to_owned(), self.events.len());
        self.events.push(Event::ToolCall {
            id: id.to_owned(),
            at_turn: self.turn,
            tool: native.to_owned(),
            args,
            exit: None,
            output: None,
            exec: None,
            // A delegated run's call names its lane (#708); the trunk's names none.
            lane: self.side.as_ref().map(|side| side.lane.clone()),
        });
        Ok(())
    }

    /// A `tool_result` block answers the call it names.
    fn tool_result(&mut self, block: &serde_json::Value) {
        let Some(id) = block
            .get("tool_use_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
        else {
            self.census
                .dropped_one(&format!("user/{}", Block::ToolResult.tag()));
            self.census
                .no_event_one("user/a tool result naming no call");
            return;
        };
        let Some(at) = self.awaiting.remove(&id) else {
            // An answer to a call this log never showed. Counted rather than
            // attached to whichever call happened to be last.
            self.census
                .dropped_one(&format!("user/{}", Block::ToolResult.tag()));
            self.census
                .no_event_one("user/a tool result answering no known call");
            return;
        };
        let Some(Event::ToolCall { exit, output, .. }) = self.events.get_mut(at) else {
            self.census
                .dropped_one(&format!("user/{}", Block::ToolResult.tag()));
            self.census
                .no_event_one("user/a tool result answering no known call");
            return;
        };
        // `is_error` is a boolean, not an exit code. True becomes 1 because
        // the record's field is an exit status and one is the honest
        // stand-in; ABSENT stays absent rather than becoming zero, because
        // "the harness did not say" is not the same as "it succeeded".
        *exit = block
            .get("is_error")
            .and_then(serde_json::Value::as_bool)
            .map(i64::from);
        // Absent stays absent, for the same reason `is_error` does: "the
        // harness said nothing" and "the tool printed nothing" are different
        // facts about a call, and `Some("")` says the second about both.
        *output = block.get("content").map(|content| flatten(Some(content)));
    }
}

/// The `message.content` of a row, or the drift of a mapping that lost it.
fn content_of<'a>(
    at_row: usize,
    kind: &str,
    row: &'a serde_json::Value,
) -> Result<&'a serde_json::Value, Drift> {
    let Some(message) = row.get("message") else {
        return Err(Drift::MissingField {
            at_row,
            kind: kind.to_owned(),
            field: "message".to_owned(),
        });
    };
    message.get("content").ok_or_else(|| Drift::MissingField {
        at_row,
        kind: kind.to_owned(),
        field: "message.content".to_owned(),
    })
}

/// What a JSON value is, for a refusal that has to say why it will not fit.
///
/// Named rather than `{:?}`-rendered: a debug rendering of a whole foreign
/// object in an error message is how a log's contents end up in a terminal
/// nobody meant to paste them into.
fn shape_of(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "a list",
        serde_json::Value::Object(_) => "an object",
    }
}

/// A content block's `type`, if it has one.
fn block_type(block: &serde_json::Value) -> Option<&str> {
    block.get("type").and_then(serde_json::Value::as_str)
}

/// Why a row is excluded from the parent session's turn stream, if the
/// harness declared it so.
///
/// See this module's header for why reading these two fields is not the
/// content guess the rest of this adapter refuses to make.
fn excluded_from_parent(row: &serde_json::Value) -> Option<&'static str> {
    if row.get("isSidechain").and_then(serde_json::Value::as_bool) == Some(true) {
        return Some("isSidechain");
    }
    if row.get("isMeta").and_then(serde_json::Value::as_bool) == Some(true) {
        return Some("isMeta");
    }
    None
}

/// The prefill of the assistant row that answers THIS turn, if one does.
///
/// All three token fields, because all three were fed to the model. See this
/// module's header for why `input_tokens` alone is a lie.
///
/// # Why the scan stops
///
/// It used to run to the next `assistant` row anywhere ahead of it. Two
/// person turns in a row -- a second message queued before the model replied,
/// which the reference log does contain -- then gave BOTH turns the same
/// prefill, and `Event::Summary` sums prefill across
/// turns, so the session total counted those tokens twice. The scan now stops
/// at the next row that opens a turn: an assistant row after that answers
/// that turn, not this one.
///
/// `None` means the log affords no answer -- an interrupted session's last
/// turn, or a usage object carrying none of the three keys. The caller turns
/// that into the zero the record gives it no way to avoid, and counts it.
fn prefill_after(rest: &[(usize, &str, serde_json::Value)]) -> Result<Option<Count>, Drift> {
    for (at_row, _raw, row) in rest {
        // The same exclusion `Run::row` applies before this lookahead ever
        // runs, applied here too: a sidechain assistant row is not the
        // answer to this turn, and a meta user row does not open one, so
        // neither may end or answer this scan. Skipped, not found: a
        // fresh-instance review (PR #98) confirmed this row's own drift
        // without it -- a sidechain row's usage read as the PARENT's
        // prefill, or a meta row's text stopping the scan before the real
        // answer, in each case content this PR declares irrelevant still
        // reaching the parent's turn.
        if excluded_from_parent(row).is_some() {
            continue;
        }
        match row.get("type").and_then(serde_json::Value::as_str) {
            Some(kind) if kind == Row::Assistant.tag() => {}
            // A row that opens a turn of its own ends the lookahead: whatever
            // answers it is that turn's prefill and not this one's.
            Some(kind) if kind == Row::User.tag() && opens_a_turn(row) => return Ok(None),
            _ => continue,
        }
        let Some(usage) = row.get("message").and_then(|m| m.get("usage")) else {
            return Ok(None);
        };
        let mut sum = 0_u64;
        let mut said = false;
        for key in PREFILL_KEYS {
            if let Some(part) = usage.get(*key).and_then(serde_json::Value::as_u64) {
                said = true;
                sum = sum
                    .checked_add(part)
                    .ok_or_else(|| Drift::Unrepresentable {
                        at_row: *at_row,
                        field: PREFILL_FIELD.to_owned(),
                        why: "the three counts sum past what a count can hold".to_owned(),
                    })?;
            }
        }
        if !said {
            return Ok(None);
        }
        return count_of(*at_row, PREFILL_FIELD, sum).map(Some);
    }
    Ok(None)
}

/// The three token fields that together are what was fed to the model.
const PREFILL_KEYS: &[&str] = &[
    "input_tokens",
    "cache_creation_input_tokens",
    "cache_read_input_tokens",
];

/// What a refusal calls the field the three counts live in.
const PREFILL_FIELD: &str = "message.usage (the three prefill counts)";

/// What is assumed when the log affords no prefill for a turn.
const PREFILL_ASSUMED: &str = "turn.prefill_tokens = 0 (no assistant row answers this turn)";

/// A count the record can hold, or a refusal naming the one it cannot.
///
/// `Count::new(n).unwrap_or_default()` was here, and it turned a number past
/// the cap into a silent zero -- reinstating, one line at a time, exactly the
/// failure `Count` exists to prevent: a value that reads back different from
/// the one written. A number this record cannot spell is drift, and [`Drift`]
/// already has the word for it.
fn count_of(at_row: usize, field: &str, raw: u64) -> Result<Count, Drift> {
    Count::new(raw).map_err(|_| Drift::Unrepresentable {
        at_row,
        field: field.to_owned(),
        why: format!("the count {raw} is past what the record can hold"),
    })
}

/// The flag the harness writes on a row it composed to replace a history.
const COMPACT_SUMMARY: &str = "isCompactSummary";

/// Whether the harness marked this row as a compaction summary.
///
/// Its own function beside [`excluded_from_parent`], not a third arm inside
/// it: the two answer different questions. That one says *this row is not the
/// top-level session's turn, and nothing should come of it*; this one says
/// *this row is an event of its own kind*. Folding them would make a
/// compaction reach the record as a silence.
fn compacted(row: &serde_json::Value) -> bool {
    row.get(COMPACT_SUMMARY)
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}

/// Whether a `user` row carries text, and so opens a turn.
///
/// The same predicate the mapping uses, factored out because the prefill
/// lookahead has to agree with it exactly. Two copies that disagreed would
/// give a turn the prefill of a different turn.
fn opens_a_turn(row: &serde_json::Value) -> bool {
    match row
        .get("message")
        .and_then(|message| message.get("content"))
    {
        Some(serde_json::Value::String(text)) => !text.trim().is_empty(),
        Some(serde_json::Value::Array(blocks)) => blocks.iter().any(|block| {
            block_type(block).and_then(Block::from_tag) == Some(Block::Text)
                && block
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|text| !text.trim().is_empty())
        }),
        _ => false,
    }
}

/// A tool result's content as text, however the harness shaped it.
fn flatten(content: Option<&serde_json::Value>) -> String {
    match content {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(serde_json::Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| match block {
                serde_json::Value::String(text) => Some(text.clone()),
                block => block
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
            })
            .collect::<String>(),
        _ => String::new(),
    }
}

/// A foreign value in the record's value space, or a refusal.
///
/// The record admits no `null` and no binary floats. A tool argument that
/// cannot be spelled is refused by name rather than coerced into a string --
/// a number rendered as text is a lie about its type, and this is the layer
/// where that lie would be introduced.
fn value_of(at_row: usize, field: &str, raw: &serde_json::Value) -> Result<Value, Drift> {
    match raw {
        serde_json::Value::Null => Err(Drift::Unrepresentable {
            at_row,
            field: field.to_owned(),
            why: "null".to_owned(),
        }),
        serde_json::Value::Bool(b) => Ok(Value::Boolean(*b)),
        serde_json::Value::String(s) => Ok(Value::String(s.clone())),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                return Ok(Value::Integer(i));
            }
            crate::formats::record::json::Decimal::new(&n.to_string())
                .map(Value::Decimal)
                .ok_or_else(|| Drift::Unrepresentable {
                    at_row,
                    field: field.to_owned(),
                    why: format!("the number {n}"),
                })
        }
        serde_json::Value::Array(items) => items
            .iter()
            .map(|item| value_of(at_row, field, item))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        serde_json::Value::Object(entries) => entries
            .iter()
            .map(|(key, item)| value_of(at_row, field, item).map(|v| (key.clone(), v)))
            .collect::<Result<BTreeMap<_, _>, _>>()
            .map(Value::Object),
    }
}

#[cfg(test)]
mod tests {

    /// The substrate id these tests file rows against.
    ///
    /// Any declared id would do: what is being tested is that the row names
    /// the one it was GIVEN, never one read out of the log.
    const A_SUBSTRATE: &str = "claude-code";

    use super::{
        Block, ClaudeCode, Found, MAPS, PREFILL_ASSUMED, Row, native_tool, sidechains_beside,
    };
    use crate::adapters::{Adapter, Drift};
    use crate::formats::record::{CompactionSource, Event};

    /// One `user` row carrying text a person wrote.
    fn user_says(text: &str) -> String {
        format!("{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"{text}\"}}}}")
    }

    /// One assistant row, with `content` and `usage` written in full.
    fn assistant(content: &str, output_tokens: u64, prefill: [u64; 3]) -> String {
        format!(
            "{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":{content},\
             \"usage\":{{\"input_tokens\":{},\"cache_creation_input_tokens\":{},\
             \"cache_read_input_tokens\":{},\"output_tokens\":{output_tokens}}}}}}}",
            prefill[0], prefill[1], prefill[2]
        )
    }

    /// One `user` row the harness marked as its own compaction summary.
    fn compaction_summary(text: &str) -> String {
        format!(
            "{{\"type\":\"user\",\"isCompactSummary\":true,\
             \"message\":{{\"role\":\"user\",\"content\":\"{text}\"}}}}"
        )
    }

    /// #79's third specimen, mapped: the harness discarding its own history
    /// is an event, not a turn.
    ///
    /// **Unverified against a committed log**, on the same terms the module
    /// header already declares: the reference log is unscrubbed and cannot
    /// enter this repository. The flag is what the harness writes; these
    /// inline fixtures are what pins the mapping.
    #[test]
    fn a_compaction_summary_is_an_event_and_not_a_turn() {
        let log = [
            user_says("do the thing"),
            assistant("[{\"type\":\"text\",\"text\":\"done\"}]", 3, [1, 0, 0]),
            compaction_summary("the session so far, in brief"),
        ]
        .join("\n");

        let adapted = ClaudeCode
            .adapt(&log, A_SUBSTRATE)
            .expect("the fixture adapts");
        let turns = adapted
            .events
            .iter()
            .filter(|e| matches!(e, Event::Turn { .. }))
            .count();
        assert_eq!(
            turns, 1,
            "the person took one turn; the summary is the harness's own row"
        );
        let compactions: Vec<_> = adapted
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Compaction {
                    id,
                    at_turn,
                    source,
                    summary_chars,
                } => Some((id.clone(), *at_turn, *source, summary_chars.get())),
                _ => None,
            })
            .collect();
        assert_eq!(
            compactions,
            vec![(
                "compact/1/0".to_owned(),
                1,
                CompactionSource::HarnessCompaction,
                28
            )],
            "one compaction, in the turn it interrupted, sized in characters"
        );
        // MAPPED, and not counted as a row that produced nothing: the kind is
        // one this adapter knows and it produced an event.
        assert_eq!(adapted.census.mapped.get("user").copied(), Some(2));
        assert!(
            !adapted
                .census
                .no_event
                .keys()
                .any(|why| why.contains("isCompactSummary")),
            "a compaction that produced an event is not a row that produced none: {:?}",
            adapted.census.no_event
        );
    }

    /// The flag, and only the flag. A row whose prose says a session was
    /// continued is still a turn if the harness did not mark it.
    #[test]
    fn a_compaction_is_read_from_the_harnesss_flag_and_never_from_its_prose() {
        let log = [
            user_says("do the thing"),
            assistant("[{\"type\":\"text\",\"text\":\"done\"}]", 3, [1, 0, 0]),
            user_says("This session is being continued from a previous one"),
        ]
        .join("\n");

        let adapted = ClaudeCode
            .adapt(&log, A_SUBSTRATE)
            .expect("the fixture adapts");
        assert!(
            !adapted
                .events
                .iter()
                .any(|e| matches!(e, Event::Compaction { .. })),
            "sniffing the prose would fire on any transcript quoting the phrase"
        );
        assert_eq!(
            adapted
                .events
                .iter()
                .filter(|e| matches!(e, Event::Turn { .. }))
                .count(),
            2,
            "an unmarked row is an ordinary turn, whatever it says"
        );
    }

    /// A log that OPENS with a compaction -- a session resumed from one.
    ///
    /// `Event::Compaction` names a turn, and the record refuses a row naming
    /// a turn that never happened. Counted with its reason rather than
    /// emitted, which is the treatment every other row this adapter cannot
    /// place already gets.
    #[test]
    fn a_compaction_before_any_turn_is_counted_rather_than_named_against_turn_zero() {
        let log = [
            compaction_summary("the session so far, in brief"),
            user_says("carry on"),
        ]
        .join("\n");

        let adapted = ClaudeCode
            .adapt(&log, A_SUBSTRATE)
            .expect("the fixture adapts");
        assert!(
            !adapted
                .events
                .iter()
                .any(|e| matches!(e, Event::Compaction { .. }))
        );
        assert_eq!(
            adapted
                .census
                .no_event
                .get("user/isCompactSummary before any turn was opened")
                .copied(),
            Some(1)
        );
        // And the record it produced is one the format accepts, which is what
        // "counted rather than emitted" has to mean.
        assert_eq!(
            adapted
                .events
                .iter()
                .filter(|e| matches!(e, Event::Turn { .. }))
                .count(),
            1
        );
    }

    /// Two compactions in one turn get two ids, and they read as the first
    /// and second compaction OF THAT TURN.
    #[test]
    fn two_compactions_in_one_turn_are_numbered_within_it() {
        let log = [
            user_says("do the thing"),
            assistant("[{\"type\":\"text\",\"text\":\"done\"}]", 3, [1, 0, 0]),
            compaction_summary("first summary"),
            compaction_summary("second summary"),
        ]
        .join("\n");

        let adapted = ClaudeCode
            .adapt(&log, A_SUBSTRATE)
            .expect("the fixture adapts");
        let ids: Vec<_> = adapted
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Compaction { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(ids, ["compact/1/0", "compact/1/1"]);
    }

    /// THE THOUSAND-TURN LIE, refused.
    ///
    /// Nine in ten `user` rows in the reference log carry nothing but a
    /// `tool_result` -- the harness handing output back to the model.
    /// Counting those as turns would put a turn in the record for every tool
    /// call, in a record whose entire value is that its counts are true.
    #[test]
    fn a_user_row_carrying_only_a_tool_result_is_not_a_turn() {
        let log = [
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"do the thing\"}}",
            &assistant(
                "[{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Bash\",\
                 \"input\":{\"command\":\"ls\"}}]",
                3,
                [1, 0, 0],
            ),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\
             [{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":\"a b\"}]}}",
        ]
        .join("\n");

        let adapted = ClaudeCode
            .adapt(&log, A_SUBSTRATE)
            .expect("the fixture adapts");
        let turns = adapted
            .events
            .iter()
            .filter(|e| matches!(e, Event::Turn { .. }))
            .count();
        assert_eq!(
            turns, 1,
            "one person said one thing; the other user row was a tool answering"
        );
        // Both user rows are still MAPPED -- the second one's content went
        // somewhere, onto the call it answers. Mapped and turn are different
        // claims and the census makes only the first.
        assert_eq!(adapted.census.mapped.get("user").copied(), Some(2));
    }

    /// #76 known defect 1: a subagent's own exchange is not the parent's turn.
    ///
    /// `isSidechain` is the harness's own declaration that a row belongs to a
    /// subagent's conversation logged inline with the session that spawned
    /// it, not the operator's text second-guessed. Left unread, the row's
    /// text opened a turn the census reported as the top-level session's own.
    #[test]
    fn a_sidechain_row_does_not_open_a_turn_and_is_counted_instead() {
        let log = [
            user_says("do the real work"),
            "{\"type\":\"user\",\"isSidechain\":true,\"message\":{\"role\":\"user\",\
             \"content\":\"a subagent's own prompt\"}}"
                .to_owned(),
        ]
        .join("\n");

        let adapted = ClaudeCode
            .adapt(&log, A_SUBSTRATE)
            .expect("the fixture adapts");
        let turns = adapted
            .events
            .iter()
            .filter(|e| matches!(e, Event::Turn { .. }))
            .count();
        assert_eq!(
            turns, 1,
            "the sidechain row's text is not a second turn of this session"
        );
        assert_eq!(
            adapted.census.mapped.get("user").copied(),
            Some(2),
            "still a kind this adapter knows"
        );
        assert_eq!(
            adapted.census.no_event.get("user/isSidechain").copied(),
            Some(1),
            "silent, and said why, rather than passed over or miscounted"
        );
    }

    /// The same declaration on an `assistant` row: no response, no call.
    #[test]
    fn a_sidechain_assistant_row_emits_no_response_and_is_counted() {
        let log = [
            user_says("go"),
            "{\"type\":\"assistant\",\"isSidechain\":true,\"message\":{\"role\":\"assistant\",\
             \"content\":[{\"type\":\"tool_use\",\"id\":\"sub1\",\"name\":\"Bash\",\
             \"input\":{\"command\":\"ls\"}}],\"usage\":{\"output_tokens\":5}}}"
                .to_owned(),
        ]
        .join("\n");

        let adapted = ClaudeCode
            .adapt(&log, A_SUBSTRATE)
            .expect("the fixture adapts");
        assert!(
            !adapted
                .events
                .iter()
                .any(|e| matches!(e, Event::Response { .. } | Event::ToolCall { .. })),
            "a subagent's own call and answer are not this session's"
        );
        assert_eq!(
            adapted
                .census
                .no_event
                .get("assistant/isSidechain")
                .copied(),
            Some(1)
        );
    }

    /// #76 known defect 2, the part `isMeta` actually closes: a row the
    /// harness declares synthetic is not a person's turn either.
    ///
    /// Not a complete fix -- see this module's header. The reference log has
    /// `<command-name>` envelopes with no `isMeta` at all, and those are
    /// still counted as turns; text-sniffing the envelope to catch them would
    /// be the content guess this adapter refuses to make.
    #[test]
    fn a_harness_composed_meta_row_does_not_open_a_turn() {
        let log = [
            user_says("go"),
            "{\"type\":\"user\",\"isMeta\":true,\"message\":{\"role\":\"user\",\
             \"content\":\"<command-name>/compact</command-name>\"}}"
                .to_owned(),
        ]
        .join("\n");

        let adapted = ClaudeCode
            .adapt(&log, A_SUBSTRATE)
            .expect("the fixture adapts");
        let turns = adapted
            .events
            .iter()
            .filter(|e| matches!(e, Event::Turn { .. }))
            .count();
        assert_eq!(turns, 1, "the isMeta row is not a second turn");
        assert_eq!(adapted.census.no_event.get("user/isMeta").copied(), Some(1));
    }

    /// A fresh-instance review of #98 found the exclusion applied at
    /// `Run::row` was not applied to `prefill_after`'s own lookahead, which
    /// re-scans the same rows independently: a sidechain assistant row
    /// between a turn and its real answer was read as THAT turn's prefill.
    #[test]
    fn a_sidechain_assistant_row_does_not_supply_the_parents_prefill() {
        let log = [
            user_says("do the real work"),
            "{\"type\":\"assistant\",\"isSidechain\":true,\"message\":{\"role\":\"assistant\",\
             \"content\":[{\"type\":\"text\",\"text\":\"a subagent's own answer\"}],\
             \"usage\":{\"input_tokens\":9000,\"output_tokens\":1}}}"
                .to_owned(),
            assistant(
                "[{\"type\":\"text\",\"text\":\"the real answer\"}]",
                2,
                [7, 0, 0],
            ),
        ]
        .join("\n");

        let adapted = ClaudeCode
            .adapt(&log, A_SUBSTRATE)
            .expect("the fixture adapts");
        let prefill = adapted
            .events
            .iter()
            .find_map(|e| match e {
                Event::Turn { prefill_tokens, .. } => {
                    prefill_tokens.map(crate::formats::record::Count::get)
                }
                _ => None,
            })
            .expect("one turn opened");
        assert_eq!(
            prefill, 7,
            "the sidechain row's usage (9000) is not this turn's prefill; \
             the real answer's (7) is"
        );
    }

    /// The same review's second finding: `opens_a_turn`'s check inside the
    /// lookahead stopped it at ANY user row carrying text, including an
    /// `isMeta` one -- so a hook-injected reminder between a turn and its
    /// real answer made the lookahead give up before reaching that answer,
    /// and the turn's prefill was assumed zero with a real usage object
    /// sitting right there.
    #[test]
    fn a_meta_row_does_not_stop_the_prefill_lookahead_before_the_real_answer() {
        let log = [
            user_says("do the real work"),
            "{\"type\":\"user\",\"isMeta\":true,\"message\":{\"role\":\"user\",\
             \"content\":\"<system-reminder>a hook injected this</system-reminder>\"}}"
                .to_owned(),
            assistant(
                "[{\"type\":\"text\",\"text\":\"the real answer\"}]",
                2,
                [7, 0, 0],
            ),
        ]
        .join("\n");

        let adapted = ClaudeCode
            .adapt(&log, A_SUBSTRATE)
            .expect("the fixture adapts");
        let prefill = adapted
            .events
            .iter()
            .find_map(|e| match e {
                Event::Turn { prefill_tokens, .. } => {
                    prefill_tokens.map(crate::formats::record::Count::get)
                }
                _ => None,
            })
            .expect("one turn opened");
        assert_eq!(
            prefill, 7,
            "the meta row does not end the lookahead before the real answer"
        );
        assert!(
            !adapted.census.assumed.contains_key(PREFILL_ASSUMED),
            "a real answer exists; nothing should have been assumed"
        );
    }

    /// A tool's answer lands on the call it names, not on whichever was last.
    #[test]
    fn a_tool_result_joins_the_call_it_names() {
        let log = [
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}",
            &assistant(
                "[{\"type\":\"tool_use\",\"id\":\"first\",\"name\":\"Bash\",\
                  \"input\":{\"command\":\"one\"}},\
                  {\"type\":\"tool_use\",\"id\":\"second\",\"name\":\"Bash\",\
                  \"input\":{\"command\":\"two\"}}]",
                3,
                [1, 0, 0],
            ),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\
             [{\"type\":\"tool_result\",\"tool_use_id\":\"second\",\"content\":\"B\",\
             \"is_error\":true}]}}",
        ]
        .join("\n");

        let adapted = ClaudeCode
            .adapt(&log, A_SUBSTRATE)
            .expect("the fixture adapts");
        for event in &adapted.events {
            let Event::ToolCall {
                id, output, exit, ..
            } = event
            else {
                continue;
            };
            // `if` rather than a `match` on the id, because `diet/src` admits
            // no match arm on a string literal and a call id is exactly the
            // kind of loose vocabulary that rule is about.
            if id == "second" {
                assert_eq!(output.as_deref(), Some("B"));
                assert_eq!(*exit, Some(1), "`is_error` true is a failure");
            } else if id == "first" {
                assert_eq!(output.as_deref(), None, "nothing answered it");
                assert_eq!(
                    *exit, None,
                    "absent is not zero: the harness did not say it succeeded"
                );
            } else {
                panic!("unexpected call {id}");
            }
        }
    }

    /// `input_tokens` alone is a lie by two orders of magnitude.
    #[test]
    fn the_prefill_is_every_token_that_was_fed_in_not_the_uncached_ones() {
        let log = [
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}",
            &assistant(
                "[{\"type\":\"text\",\"text\":\"ok\"}]",
                7,
                [2, 22468, 38639],
            ),
        ]
        .join("\n");

        let adapted = ClaudeCode
            .adapt(&log, A_SUBSTRATE)
            .expect("the fixture adapts");
        let Some(Event::Turn { prefill_tokens, .. }) = adapted
            .events
            .iter()
            .find(|e| matches!(e, Event::Turn { .. }))
        else {
            panic!("the turn is there")
        };
        assert_eq!(
            prefill_tokens.map(crate::formats::record::Count::get),
            Some(2 + 22468 + 38639),
            "cached tokens were still fed to the model; `input_tokens` alone would \
             record a prefill of 2 for a turn that prefilled sixty-one thousand"
        );
    }

    /// A kind the adapter never heard of is news, and the walk goes on.
    #[test]
    fn a_kind_this_adapter_does_not_map_is_counted_rather_than_dropped() {
        let log = concat!(
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n",
            "{\"type\":\"a-kind-nobody-has-seen\",\"payload\":\"x\"}"
        );
        let adapted = ClaudeCode
            .adapt(log, A_SUBSTRATE)
            .expect("an unknown kind is not a refusal");
        assert_eq!(
            adapted
                .census
                .unmapped
                .get("a-kind-nobody-has-seen")
                .copied(),
            Some(1),
            "silence is the failure this issue names"
        );
        assert_eq!(
            adapted.census.rows(),
            2,
            "every row lands in exactly one bucket"
        );
    }

    /// A field that moved is a refusal, because the alternative is reading
    /// whatever happened to be beside it.
    #[test]
    fn a_renamed_field_on_a_mapped_kind_is_refused_rather_than_guessed_at() {
        let log = "{\"type\":\"user\",\"msg\":{\"role\":\"user\",\"content\":\"go\"}}";
        let refused = ClaudeCode
            .adapt(log, A_SUBSTRATE)
            .expect_err("the format moved");
        assert!(
            matches!(&refused, Drift::MissingField { kind, field, at_row }
                if kind == "user" && field == "message" && *at_row == 1),
            "{refused}"
        );
    }

    /// The translation that made the capture lane see anything at all.
    #[test]
    fn claude_codes_tool_names_are_translated_into_the_lanes_vocabulary() {
        // Checked against the lane's own table rather than against a copy of
        // it here: a rename on either side that broke the pairing would make
        // this test the place it shows up.
        for (foreign, native) in super::TOOL_NAMES {
            assert_eq!(native_tool(foreign), (*native, true));
            assert!(
                crate::capture::mechanical::tool_kind(native).is_some(),
                "`{native}` is not a tool the mechanical lane knows, so translating \
                 `{foreign}` into it would still derive nothing"
            );
        }
        assert_eq!(
            native_tool("Grep"),
            ("Grep", false),
            "a tool whose contract this adapter does not claim to know is carried \
             across untouched rather than guessed at"
        );
    }

    /// The rename is declared in the census, because it is an interpretation.
    #[test]
    fn a_translated_tool_name_is_declared_in_the_census() {
        let log = [
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}",
            &assistant(
                "[{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Read\",\
                  \"input\":{\"file_path\":\"/srv/x.rs\"}}]",
                3,
                [1, 0, 0],
            ),
        ]
        .join("\n");
        let adapted = ClaudeCode
            .adapt(&log, A_SUBSTRATE)
            .expect("the fixture adapts");
        assert_eq!(
            adapted.census.translated.get("Read -> read_file").copied(),
            Some(1)
        );
        // And the path argument is renamed WITH it, or the lane reads a tool
        // it knows and finds no path on it.
        let Some(Event::ToolCall { args, tool, .. }) = adapted
            .events
            .iter()
            .find(|e| matches!(e, Event::ToolCall { .. }))
        else {
            panic!("the call is there")
        };
        assert_eq!(tool, "read_file");
        assert!(
            args.as_ref().is_some_and(|a| a.contains_key("path")),
            "`file_path` is renamed to the `path` the lane reads: {args:?}"
        );
    }

    /// Reasoning has no event kind here, so it is counted rather than lost.
    #[test]
    fn thinking_has_no_home_in_the_schema_and_is_counted_as_dropped() {
        let log = [
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}",
            &assistant(
                "[{\"type\":\"thinking\",\"thinking\":\"...\"},\
                  {\"type\":\"text\",\"text\":\"ok\"}]",
                3,
                [1, 0, 0],
            ),
        ]
        .join("\n");
        let adapted = ClaudeCode
            .adapt(&log, A_SUBSTRATE)
            .expect("the fixture adapts");
        assert_eq!(
            adapted.census.dropped.get("assistant/thinking").copied(),
            Some(1),
            "a mapped row can still have thrown something away, and the census \
             is where that is said"
        );
    }

    /// A tool name is translated whatever case the harness wrote it in.
    ///
    /// Ruled on #76. The specimen the table was written for was a lane that
    /// knew `bash` while the harness wrote `Bash`; a lookup exact about case
    /// is that same defect waiting on the next harness to write `BASH`, and
    /// the failure is silent -- the call is recorded under a name the lane has
    /// no word for, and the census reads as a success.
    #[test]
    fn a_tool_name_is_translated_whatever_case_it_arrives_in() {
        for spelling in ["Bash", "bash", "BASH", "bAsH"] {
            let (native, translated) = native_tool(spelling);
            assert_eq!(
                (native, translated),
                ("bash", true),
                "`{spelling}` is the lane's `bash`"
            );
        }

        // And the census says what the LOG carried, not what the table is
        // keyed on: a harness that changed case is a fact about the harness,
        // and folding it into one count is how that fact disappears.
        let log = [
            user_says("go"),
            assistant(
                "[{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"BASH\",\
                 \"input\":{\"command\":\"cd /srv/p\"}}]",
                1,
                [1, 0, 0],
            ),
        ]
        .join("\n");
        let census = ClaudeCode
            .adapt(&log, A_SUBSTRATE)
            .expect("it adapts")
            .census;
        assert_eq!(
            census.translated.get("BASH -> bash"),
            Some(&1),
            "the census carries the spelling that arrived: {census:?}"
        );

        // Untranslated names are still untranslated, in any case. `Grep`'s
        // contract is not one this adapter claims to know, and folding case
        // does not change which contracts it knows.
        for spelling in ["Grep", "grep", "Task", "mcp__x__y"] {
            let (native, translated) = native_tool(spelling);
            assert_eq!(
                (native, translated),
                (spelling, false),
                "`{spelling}` is carried as written"
            );
        }
    }

    /// `MAPS` and [`Row`] say the same thing, and `Block`'s tags are distinct.
    ///
    /// The weaker half of the vocabulary discipline, named rather than
    /// implied: `Adapter::maps` hands back a `&'static [&'static str]`, which
    /// a const cannot build by walking an enum, so the two definitions are
    /// held in step by this test instead of by the compiler. A kind added to
    /// `Row` and not to `MAPS` would otherwise be mapped while the adapter
    /// declared it was not.
    #[test]
    fn the_two_foreign_vocabularies_have_one_definition_each() {
        let from_type: Vec<&str> = Row::ALL.iter().map(|row| row.tag()).collect();
        assert_eq!(
            MAPS, from_type,
            "the kinds this adapter declares are the kinds it maps"
        );
        for tag in MAPS {
            assert!(
                Row::from_tag(tag).is_some(),
                "`{tag}` is declared and does not parse"
            );
        }

        // Round-tripping is the whole claim of a `from_tag` that walks `ALL`:
        // no two variants answer to one tag, and none answers to none.
        for block in Block::ALL {
            assert_eq!(
                Block::from_tag(block.tag()),
                Some(*block),
                "{block:?} round-trips through its own tag"
            );
        }
        assert_eq!(
            Block::ALL.len(),
            Block::ALL
                .iter()
                .map(|block| block.tag())
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            "and no two block types share a tag"
        );
    }

    /// THE EIGHTY-PERCENT DROP.
    ///
    /// An assistant row that makes a tool call and says nothing in words is
    /// the commonest shape in a real log -- three rows in four -- and it
    /// used to emit no response at all, taking its `output_tokens` with it.
    /// Four fifths of every token the model generated left through that hole
    /// under a census that read as a clean success. Found by a fresh-instance
    /// review measuring the adapter against the log it was written from, not
    /// by any test here, which is why this one exists.
    #[test]
    fn an_assistant_row_that_said_nothing_still_reports_what_it_spent() {
        let log = [
            user_says("go"),
            assistant(
                "[{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Read\",\
                 \"input\":{\"file_path\":\"/srv/p/a.rs\"}}]",
                4242,
                [1, 0, 0],
            ),
        ]
        .join("\n");

        let adapted = ClaudeCode.adapt(&log, A_SUBSTRATE).expect("it adapts");
        let spent: Vec<(u64, Option<&str>)> = adapted
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Response {
                    output_tokens,
                    text,
                    ..
                } => Some((
                    output_tokens.map_or(0, crate::formats::record::Count::get),
                    text.as_deref(),
                )),
                _ => None,
            })
            .collect();
        assert_eq!(
            spent,
            vec![(4242, None)],
            "the row spent 4,242 tokens saying nothing, and both halves of \
             that are recorded: the count, and `text: None` rather than an \
             omitted event"
        );
        assert_eq!(
            adapted.census.silent_rows(),
            0,
            "and no assistant row is silent: the census would say so if one were"
        );
    }

    /// A response cannot name a request that is not in the stream.
    ///
    /// `to_request` used to be `u/{turn.max(1)}`, which emits `u/1` when no
    /// turn has been opened -- a dangling link the record's own structure
    /// check rejects, invented in the very field `tool_use` refuses to invent
    /// it in sixty lines below. Reachable from any resumed or compacted
    /// session whose first mapped row is an assistant row.
    #[test]
    fn an_assistant_row_before_any_turn_names_no_request_and_is_counted() {
        let log = assistant("[{\"type\":\"text\",\"text\":\"resumed\"}]", 9, [1, 0, 0]);

        let adapted = ClaudeCode.adapt(&log, A_SUBSTRATE).expect("it adapts");
        assert!(
            !adapted
                .events
                .iter()
                .any(|event| matches!(event, Event::Response { .. })),
            "no response, because there is no request for one to answer"
        );
        assert_eq!(
            adapted.census.no_event["assistant/spoke before any turn was opened"], 1,
            "and the row is counted rather than passed over in silence"
        );
        assert_eq!(
            adapted.census.mapped_rows(),
            1,
            "it was still a kind we map"
        );
    }

    /// Two person turns in a row do not share one prefill.
    ///
    /// The lookahead used to run to the next assistant row wherever it was,
    /// so a message queued before the model replied gave BOTH turns the same
    /// prefill -- and `Event::Summary` sums prefill across turns, so the
    /// session total counted those tokens twice. The reference log contains
    /// such pairs; `assumed` counts what they cost.
    #[test]
    fn two_turns_in_a_row_do_not_both_claim_the_same_prefill() {
        let log = [
            user_says("first"),
            user_says("second, before it answered"),
            assistant("[{\"type\":\"text\",\"text\":\"ok\"}]", 3, [10, 20, 70]),
        ]
        .join("\n");

        let adapted = ClaudeCode.adapt(&log, A_SUBSTRATE).expect("it adapts");
        let prefills: Vec<u64> = adapted
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Turn { prefill_tokens, .. } => {
                    prefill_tokens.map(crate::formats::record::Count::get)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            prefills,
            vec![0, 100],
            "the assistant row answers the SECOND turn; the first has none, \
             and 100 counted twice would overstate the session by 100"
        );
        assert_eq!(
            adapted.census.assumed["turn.prefill_tokens = 0 (no assistant row answers this turn)"],
            1,
            "and the zero the record forces is declared rather than passed off \
             as a measurement"
        );
    }

    /// A number the record cannot hold is drift, not a zero.
    ///
    /// `Count::new(n).unwrap_or_default()` was here. It turned a count past
    /// the cap into `0` with a success census -- reinstating one line at a
    /// time the exact failure `Count` exists to prevent: a value that reads
    /// back different from the one written.
    #[test]
    fn a_count_past_what_the_record_holds_is_refused_rather_than_zeroed() {
        let huge = u64::MAX;
        let log = [
            user_says("go"),
            assistant("[{\"type\":\"text\",\"text\":\"ok\"}]", huge, [1, 0, 0]),
        ]
        .join("\n");
        // #76 defect 4, ruled: THIS row's own count overflowing downgrades
        // the row to `Event::Unknown` rather than refusing the whole log --
        // "named, not silently zeroed" still holds: the census says exactly
        // what would not fit, and the row survives as `Unknown` rather than
        // vanishing into a success that never happened.
        let adapted = ClaudeCode
            .adapt(&log, A_SUBSTRATE)
            .expect("named, not refused");
        assert!(
            !adapted
                .events
                .iter()
                .any(|e| matches!(e, Event::Response { .. })),
            "the row that overflowed never became a response"
        );
        assert!(
            adapted
                .census
                .unrepresentable
                .keys()
                .any(|what| what.contains("output_tokens")),
            "and the overflow is named in the census: {:?}",
            adapted.census.unrepresentable
        );

        // The three prefill counts are different: they are read by the
        // LOOKAHEAD from the user row's own turn, so the `Unrepresentable`
        // this raises names the answering assistant row, not the row
        // `Run::row` was called with. Downgrading the user row for a
        // different row's bad value would show the wrong row's raw text
        // beside it, so this case still refuses the whole log.
        let log = [
            user_says("go"),
            assistant(
                "[{\"type\":\"text\",\"text\":\"ok\"}]",
                1,
                [huge, huge, huge],
            ),
        ]
        .join("\n");
        assert!(
            matches!(
                ClaudeCode.adapt(&log, A_SUBSTRATE),
                Err(Drift::Unrepresentable { .. })
            ),
            "a prefill sum that overflows is a refusal, not a wrap or a panic"
        );
    }

    /// Every field the mapping declares is read or refused -- all of them.
    ///
    /// Three declared fields used to be read with a shrug: a `text` block
    /// with its text renamed dropped a person's turn on the floor, and a
    /// `tool_use` whose `input` was renamed or reshaped became a call with no
    /// arguments -- recognised, counted, translated, and useless to the
    /// capture lane. That is the failure `TOOL_NAMES` exists to prevent,
    /// reproduced one field across.
    #[test]
    fn every_declared_field_is_refused_when_it_moves_not_read_around() {
        let moved: &[(&str, &str)] = &[
            (
                "a user row's text block",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\
                 [{\"type\":\"text\",\"body\":\"please fix the parser\"}]}}",
            ),
            (
                "an assistant row's text block",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\
                 [{\"type\":\"text\",\"body\":\"hi\"}],\"usage\":{\"output_tokens\":1}}}",
            ),
        ];
        for (what, row) in moved {
            let log = [user_says("go").as_str(), row].join("\n");
            assert!(
                matches!(
                    ClaudeCode.adapt(&log, A_SUBSTRATE),
                    Err(Drift::MissingField { .. })
                ),
                "{what}: a declared field that moved is a refusal"
            );
        }

        // `input` renamed away entirely: the field this mapping declares is
        // simply not there, which is `MissingField` and still a refusal.
        let log = [
            user_says("go"),
            assistant(
                "[{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Read\",\
                 \"arguments\":{\"file_path\":\"/srv/p/a.rs\"}}]",
                1,
                [1, 0, 0],
            ),
        ]
        .join("\n");
        assert!(
            matches!(
                ClaudeCode.adapt(&log, A_SUBSTRATE),
                Err(Drift::MissingField { .. })
            ),
            "`input` renamed away entirely is a call read as one with none, refused"
        );

        // `input` reshaped -- present, but not the object this mapping
        // reads. #76 defect 4, ruled: this is content THIS row carries that
        // does not fit, the same class as a null argument, so it downgrades
        // the row to `Event::Unknown` rather than refusing the whole log.
        let log = [
            user_says("go"),
            assistant(
                "[{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Read\",\"input\":\"ls -la\"}]",
                1,
                [1, 0, 0],
            ),
        ]
        .join("\n");
        let adapted = ClaudeCode
            .adapt(&log, A_SUBSTRATE)
            .expect("a reshaped `input` is named, not a refusal");
        assert!(
            !adapted
                .events
                .iter()
                .any(|e| matches!(e, Event::ToolCall { .. })),
            "the call whose arguments reshaped is not read as one with none"
        );
    }

    /// A tool argument the record's value space cannot hold is refused.
    ///
    /// The whole `Drift::Unrepresentable` path had no test, no fixture and no
    /// seeded fault: four of the five refusal variants were never produced by
    /// anything in the suite. Under this repository's own rule, most of the
    /// refusal vocabulary had never been seen at all.
    #[test]
    fn a_tool_argument_the_record_cannot_spell_is_named_rather_than_coerced() {
        // #76 defect 4, ruled: the argument's own row becomes `Event::Unknown`
        // rather than refusing the whole log -- "named by the census, not
        // coerced into a string" is still exactly the claim, just carried by
        // a row that survives instead of a refusal that stops the walk.
        for (why, value) in [("null", "null"), ("an exponent", "1e5")] {
            let log = [
                user_says("go"),
                assistant(
                    &format!(
                        "[{{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Read\",\
                         \"input\":{{\"file_path\":{value}}}}}]"
                    ),
                    1,
                    [1, 0, 0],
                ),
            ]
            .join("\n");
            let adapted = ClaudeCode
                .adapt(&log, A_SUBSTRATE)
                .unwrap_or_else(|err| panic!("{why} is named, not a refusal: {err}"));
            assert!(
                !adapted
                    .events
                    .iter()
                    .any(|e| matches!(e, Event::ToolCall { .. })),
                "{why}: the call that could not be spelled is not in the record as a call"
            );
            assert!(
                adapted
                    .events
                    .iter()
                    .any(|e| matches!(e, Event::Unknown { source_kind, .. } if source_kind == "assistant")),
                "{why}: and the row survives, typed as unread"
            );
        }
    }

    /// A whole row downgrades, not just the block that failed.
    ///
    /// An assistant row can carry several tool calls, and the first can
    /// succeed before the second's argument is what the row's own content
    /// downgrades for. Without a rollback, the valid call would stand beside
    /// the `Unknown` row that is supposed to replace the WHOLE thing --
    /// half a row that adapted and half that did not, which is not what "an
    /// unrepresentable row becomes an Unknown row" says.
    #[test]
    fn an_unrepresentable_block_rolls_back_the_calls_that_came_before_it_in_the_same_row() {
        let log = [
            user_says("go"),
            assistant(
                "[{\"type\":\"tool_use\",\"id\":\"first\",\"name\":\"Bash\",\
                 \"input\":{\"command\":\"ls\"}},\
                 {\"type\":\"tool_use\",\"id\":\"second\",\"name\":\"Read\",\
                 \"input\":{\"file_path\":null}}]",
                1,
                [1, 0, 0],
            ),
        ]
        .join("\n");

        let adapted = ClaudeCode
            .adapt(&log, A_SUBSTRATE)
            .expect("the row is named, not refused");
        assert!(
            !adapted
                .events
                .iter()
                .any(|e| matches!(e, Event::ToolCall { .. })),
            "the first call, valid on its own, does not survive the row it \
             was part of: {:?}",
            adapted.events
        );
        let unknown_rows = adapted
            .events
            .iter()
            .filter(|e| matches!(e, Event::Unknown { .. }))
            .count();
        assert_eq!(unknown_rows, 1, "one row, one Unknown, not one per block");
    }

    /// A refusal points at the line of the FILE, blank lines and all.
    ///
    /// `parse` numbered by file line and the walk renumbered by position in
    /// the filtered vector, so one blank line anywhere above a fault sent a
    /// reader to the wrong line -- two numbering systems inside one enum
    /// whose whole doc comment is that a refusal says where.
    #[test]
    fn a_refusal_counts_the_files_lines_and_not_the_rows_it_kept() {
        let log = [
            "",
            "",
            "{\"type\":\"assistant\",\"msg\":{\"role\":\"assistant\",\"content\":[]}}",
        ]
        .join("\n");
        assert!(
            matches!(
                ClaudeCode.adapt(&log, A_SUBSTRATE),
                Err(Drift::MissingField { at_row: 3, .. })
            ),
            "the bad row is on line three of the file, not row one of the vector"
        );
    }

    /// "The harness said nothing" is not "the tool printed nothing", and the
    /// same for "it did not fail".
    ///
    /// The `is_error` half of this had no test, and a seeded fault turning an
    /// absent `is_error` into `Some(0)` went green: the one test that asserts
    /// an absent exit does it on a call that was never ANSWERED, where
    /// `tool_result` never runs and the `None` comes from construction. An
    /// answered call whose `is_error` the harness omitted -- which is every
    /// successful call Claude Code writes -- went through the mutated line
    /// and nothing looked.
    #[test]
    fn a_tool_result_that_said_neither_leaves_both_absent() {
        let log = [
            user_says("go"),
            assistant(
                "[{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Bash\",\
                 \"input\":{\"command\":\"true\"}}]",
                1,
                [1, 0, 0],
            ),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\
             [{\"type\":\"tool_result\",\"tool_use_id\":\"t1\"}]}}"
                .to_owned(),
        ]
        .join("\n");

        let adapted = ClaudeCode.adapt(&log, A_SUBSTRATE).expect("it adapts");
        let answered = adapted.events.iter().find_map(|event| match event {
            Event::ToolCall { output, exit, .. } => Some((output.clone(), *exit)),
            _ => None,
        });
        let (output, exit) = answered.expect("the call is in the stream");
        assert_eq!(
            output, None,
            "the call was answered and the answer said nothing about output; \
             `Some(\"\")` would claim the tool printed an empty string"
        );
        assert_eq!(
            exit, None,
            "and nothing about failure either; `Some(0)` would claim the \
             harness reported a success it never reported"
        );
    }

    /// #501: each response carries the model its assistant row names, and
    /// the census counts the models seen by rows; `<synthetic>` -- the app's
    /// placeholder for a message no model wrote -- names no model and is
    /// counted as synthetic.
    #[test]
    fn each_response_carries_the_model_its_row_names() {
        let log = include_str!("../../adapters/fixtures/claude-code/turns-from-three-models.jsonl");
        let adapted = ClaudeCode.adapt(log, A_SUBSTRATE).expect("it adapts");
        let models: Vec<Option<String>> = adapted
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Response { model, .. } => Some(model.clone()),
                _ => None,
            })
            .collect();
        let named = |name: &str| Some(name.to_owned());
        assert_eq!(
            models,
            [
                named("model-one"),
                named("model-two"),
                named("model-two"),
                None,
                named("model-three")
            ]
        );
        assert_eq!(
            adapted.census.models,
            std::collections::BTreeMap::from([
                ("model-one".to_owned(), 1),
                ("model-three".to_owned(), 1),
                ("model-two".to_owned(), 2),
            ])
        );
        assert_eq!(
            adapted.census.unattributed,
            std::collections::BTreeMap::from([("synthetic".to_owned(), 1)])
        );
        let rendered = adapted.census.render();
        assert!(
            rendered.contains(r#""models":{"model-one":1,"model-three":1,"model-two":2}"#),
            "{rendered}"
        );
        assert!(
            rendered.contains(r#""unattributed":{"synthetic":1}"#),
            "{rendered}"
        );
    }

    /// #500: a log written by the desktop app repeats rows when a session
    /// resumes, under the same row `uuid`. The first is kept; a repeat whose
    /// content (`type`, `message`, `toolUseResult`, `attachment`) matches is
    /// dropped and counted -- as exact, as an envelope rewritten (`slug`,
    /// `promptId`, `cwd`, `gitBranch`), or as a `parentUuid` rewritten, on a
    /// line of its own because it changes the tree. Rows without a `uuid`
    /// are all kept.
    #[test]
    fn a_repeated_row_is_dropped_and_counted_by_what_its_repeat_rewrote() {
        let log = include_str!("../../adapters/fixtures/claude-code/rows-that-repeat.jsonl");
        let adapted = ClaudeCode.adapt(log, A_SUBSTRATE).expect("it adapts");
        assert_eq!(
            adapted.census.repeated,
            std::collections::BTreeMap::from([
                ("exact".to_owned(), 1),
                ("envelope-rewritten".to_owned(), 1),
                ("parent-rewritten".to_owned(), 1),
            ])
        );
        assert_eq!(adapted.census.repeated_rows(), 3);
        let calls = adapted
            .events
            .iter()
            .filter(|event| matches!(event, Event::ToolCall { .. }))
            .count();
        assert_eq!(calls, 1, "the repeated call is one call, not two");
        let rendered = adapted.census.render();
        assert!(
            rendered.contains(r#""repeated":{"envelope-rewritten":1,"exact":1,"parent-rewritten":1},"repeated_rows":3"#),
            "{rendered}"
        );
    }

    /// #501: a row that names no model at all -- a log from before the
    /// harness recorded one -- carries none, counted as such.
    #[test]
    fn a_row_naming_no_model_carries_none() {
        let log = include_str!("../../adapters/fixtures/claude-code/session.jsonl");
        let adapted = ClaudeCode.adapt(log, A_SUBSTRATE).expect("it adapts");
        assert!(
            adapted
                .events
                .iter()
                .all(|event| !matches!(event, Event::Response { model: Some(_), .. }))
        );
        assert!(adapted.census.models.is_empty());
        assert!(adapted.census.unattributed.contains_key("no model named"));
    }

    /// #500: a repeat whose content differs from the row it repeats is
    /// refused, naming both rows: two different rows under one id.
    #[test]
    fn a_repeat_whose_content_differs_is_refused() {
        let log = include_str!(
            "../../adapters/fixtures/claude-code/a-repeat-whose-content-differs.jsonl"
        );
        let refused = ClaudeCode.adapt(log, A_SUBSTRATE).expect_err("refused");
        assert_eq!(
            refused,
            Drift::RepeatDiffers {
                at_row: 4,
                first_row: 1,
                field: "message".to_owned()
            }
        );
        assert!(refused.to_string().contains("row 4"), "{refused}");
    }

    /// The delegated-session fixture: its main log, and the sidechains
    /// found beside it by convention.
    fn delegated() -> (String, Found) {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters/fixtures/claude-code");
        let log = dir.join("a-session-that-delegated.jsonl");
        let found = sidechains_beside(&log).expect("the sidechains");
        (std::fs::read_to_string(&log).expect("the log"), found)
    }

    /// #708: the sidechains beside a log are found by convention -- each
    /// agent's transcript under `subagents/`, each workflow run's under
    /// `subagents/workflows/<runId>/` -- each with its metadata's type, and
    /// each run's journal counted, never read as turns.
    #[test]
    fn a_logs_sidechains_are_found_beside_it_by_convention() {
        let (_, found) = delegated();
        let mut named: Vec<(String, Option<String>, Option<String>)> = found
            .sidechains
            .iter()
            .map(|side| {
                (
                    side.agent_id.clone(),
                    side.run_id.clone(),
                    side.agent_type.clone(),
                )
            })
            .collect();
        named.sort();
        let some = |text: &str| Some(text.to_owned());
        assert_eq!(
            named,
            [
                ("x1".to_owned(), None, some("general-purpose")),
                ("y1".to_owned(), some("wf_1"), some("workflow-agent")),
                ("z9".to_owned(), None, None),
            ]
        );
        assert_eq!(found.journals, 1);
        let nothing = sidechains_beside(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("adapters/fixtures/claude-code/session.jsonl"),
        )
        .expect("no sidechains is no error");
        assert!(nothing.sidechains.is_empty());
        assert_eq!(nothing.journals, 0);
    }

    /// #708: each sidechain joins the record as a side lane off the call
    /// that spawned it -- a fork at the call's turn, its asks on its lane,
    /// its responses with the model each names, its tool calls naming the
    /// lane -- and the census counts what joined, what did not, and the
    /// models across the main log and its sidechains.
    #[test]
    fn each_sidechain_joins_as_a_side_lane_off_the_call_that_spawned_it() {
        let (log, found) = delegated();
        let adapted = ClaudeCode
            .adapt_with(&log, &found, A_SUBSTRATE)
            .expect("it adapts");
        let forks: Vec<(String, u32)> = adapted
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Fork { lane, of_turn, .. } => Some((lane.clone(), *of_turn)),
                _ => None,
            })
            .collect();
        assert_eq!(
            forks,
            [
                ("sidechain/x1".to_owned(), 1),
                ("sidechain/wf_1/y1".to_owned(), 1)
            ]
        );
        assert!(adapted.events.iter().any(|event| matches!(
            event,
            Event::ToolCall { id, lane: Some(lane), .. } if id == "call-x" && lane == "sidechain/x1"
        )));
        assert!(adapted.events.iter().any(|event| matches!(
            event,
            Event::ToolCall { id, lane: None, .. } if id == "call-a"
        )));
        assert!(adapted.events.iter().any(|event| matches!(
            event,
            Event::Response { model: Some(model), to_request, .. }
                if model == "model-three" && to_request.starts_with("sidechain/wf_1/y1/")
        )));
        assert_eq!(
            adapted.census.models,
            std::collections::BTreeMap::from([
                ("model-one".to_owned(), 4),
                ("model-three".to_owned(), 1),
                ("model-two".to_owned(), 2),
            ])
        );
        assert_eq!(
            adapted.census.sidechains,
            std::collections::BTreeMap::from([
                ("joined/agent".to_owned(), 1),
                ("joined/workflow agent".to_owned(), 1),
                ("journals not replayed".to_owned(), 1),
                ("rows".to_owned(), 6),
                ("spawning calls with no transcript".to_owned(), 1),
                ("transcripts naming no call".to_owned(), 1),
                ("turns".to_owned(), 2),
                ("type/general-purpose".to_owned(), 1),
                ("type/workflow-agent".to_owned(), 1),
            ])
        );
    }

    /// The corpus is committed, and it is what the tests above are about.
    ///
    /// Table-driven against the DIRECTORY rather than against a list of
    /// `include_str!`s, so a fixture cannot be added without a claim about
    /// what it does. A corpus file nobody reads is a file, not a fixture, and
    /// the first version of this test named three by hand while a fourth sat
    /// beside them unmentioned.
    #[test]
    fn the_committed_corpus_adapts_and_refuses_as_it_says_it_does() {
        /// What one fixture is claimed to do: `(rows, unmapped rows, dropped
        /// content, repeats dropped)`, or `None` for a fixture whose whole
        /// point is that it is refused.
        type Claim = Option<(u64, u64, u64, u64)>;
        const EXPECTED: &[(&str, Claim)] = &[
            ("session", Some((9, 3, 1, 0))),
            ("an-unmapped-kind", Some((3, 1, 0, 0))),
            ("a-path-that-resolves-to-nothing", Some((3, 0, 0, 0))),
            ("a-renamed-field", None),
            ("turns-from-three-models", Some((8, 0, 0, 0))),
            ("a-session-that-delegated", Some((8, 0, 0, 0))),
            ("rows-that-repeat", Some((5, 1, 0, 3))),
            ("a-repeat-whose-content-differs", None),
        ];

        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters/fixtures/claude-code");
        let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
            .expect("the committed corpus")
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                (path.extension()? == "jsonl")
                    .then(|| path.file_stem()?.to_str().map(str::to_owned))?
            })
            .collect();
        on_disk.sort();
        let mut claimed: Vec<String> = EXPECTED
            .iter()
            .map(|(name, _)| (*name).to_owned())
            .collect();
        claimed.sort();
        assert_eq!(
            on_disk, claimed,
            "every fixture in the corpus is named here, and nothing here is missing \
             from the corpus"
        );

        for (name, expected) in EXPECTED {
            let log = std::fs::read_to_string(dir.join(format!("{name}.jsonl")))
                .unwrap_or_else(|why| panic!("{name}: {why}"));
            match (ClaudeCode.adapt(&log, A_SUBSTRATE), expected) {
                (Ok(adapted), Some((rows, unmapped, dropped, repeats))) => {
                    assert_eq!(adapted.census.rows(), *rows, "{name}: rows");
                    assert_eq!(
                        adapted.census.repeated_rows(),
                        *repeats,
                        "{name}: repeats dropped"
                    );
                    assert_eq!(
                        adapted.census.unmapped_rows(),
                        *unmapped,
                        "{name}: unmapped rows"
                    );
                    assert_eq!(
                        adapted.census.dropped_content(),
                        *dropped,
                        "{name}: dropped content"
                    );
                }
                (Err(drift), None) => {
                    assert!(
                        matches!(
                            (*name, &drift),
                            ("a-renamed-field", Drift::MissingField { .. })
                                | (
                                    "a-repeat-whose-content-differs",
                                    Drift::RepeatDiffers { .. }
                                )
                        ),
                        "{name}: it refuses for the reason it was written for, not \
                         another one: {drift}"
                    );
                }
                (Ok(_), None) => panic!("{name}: it was written to be refused and was not"),
                (Err(drift), Some(_)) => panic!("{name}: it was written to adapt: {drift}"),
            }
        }
    }
}
