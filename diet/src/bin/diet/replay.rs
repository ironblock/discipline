//! `diet replay` — run the capture lanes over a foreign harness's session log.
//!
//! The read-only front door from #28. It adapts a log this library did not
//! produce, runs the deterministic capture lane over the result, and prints
//! what the discipline would have captured from work that already happened.
//! **No model is called**, and none can be: nothing here holds a transport.
//!
//! ```text
//! diet replay --adapter claude-code --regimen <regimen.toml> \
//!              --source-available committed|pinned_only <log.jsonl>
//! ```
//!
//! # Not a second binary
//!
//! #28's own acceptance line names `diet-replay` as its own program. Ruled on
//! #76: it is a verb on `diet` instead, the first one that needed more than
//! `diet`'s existing `[command, path]` shape -- required flags ahead of its
//! one positional -- so it is dispatched before that shape is read rather
//! than forced through it. [`run`] is what `diet.rs`'s `main` calls when the
//! first argument is `replay`; everything below it is unchanged from when
//! this was `diet-replay`'s own `main`.
//!
//! # Why a regimen is required, when the issue's command line does not show one
//!
//! A record's `start` row carries a regime — the arm, the substrate that
//! served, the dogma version — and a Claude Code log carries none of it. It
//! records no model, no quantization and no sampler settings; the `version`
//! field is the harness's version, not the model's. So the regime cannot be
//! derived from the log, and deriving it anyway would mean writing
//! `model = "unknown"` into the one field every result is compared across.
//! The operator knows what they were running; the log does not. They declare
//! it, exactly as `diet-drive` makes them declare it.
//!
//! This is a deliberate divergence from the issue's acceptance line, and it
//! is disclosed on the PR rather than smoothed over.
//!
//! # The exit codes, which are the CLI's and not this verb's
//!
//! Ruled on #76, CLI-wide, so that no two meanings ever share a number again:
//!
//! | code | meaning |
//! | ---- | ------- |
//! | `0` | ok |
//! | `1` | a verdict was reached and it is a failure or a finding |
//! | `2` | usage, or could not run |
//! | `3` | input refused -- the format moved, a version is unsupported, a schema the tool recognises and declines |
//!
//! **This supersedes #28's row four, which says the adapter exits `2`.** That
//! row was written when replay was to be its own program; the ruling moved
//! the verb onto `diet`, where `2` already meant a usage error, and *one code
//! meaning two things was the sentinel class wearing an exit status*. The
//! refusal is still a declared refusal under its own code -- which is what
//! the row was for -- and the number is `3`.
//!
//! Replay never exits `1`, and that is not an oversight: `1` is a verdict on
//! a document, and this verb does not reach one. It reads a log or refuses
//! it. A census is not a verdict.
//!
//! One judgement call inside the ruled vocabulary, disclosed rather than
//! buried: **a failed WRITE exits `2`.** It is not input being refused, so it
//! is not `3`; it is not a verdict, so it is not `1`; "could not run" is the
//! closest true thing in the vocabulary and it is what `2` is for.

use std::collections::BTreeMap;
use std::process::ExitCode;

use diet::adapters::{Adapter, claude_code::ClaudeCode};
use diet::capture::mechanical::Lane;
use diet::drive::regimen::regime_of;
use diet::formats::record::{self, Availability, Event};
use diet::formats::regimen;
use diet::object::{EntryId, Patch, Provenance, WorkingObject};

/// Everything ran.
const EXIT_OK: u8 = 0;
/// The command line was not one this verb serves, a file it was pointed at
/// could not be read, or its output could not be written.
///
/// The CLI's `2`: *usage, or could not run*. One number for one meaning, and
/// the meaning is "this invocation never got as far as an answer".
const EXIT_COULD_NOT_RUN: u8 = 2;
/// Input refused: the adapter found a row whose format has moved, or the
/// regimen is a document this verb recognises and declines.
///
/// The CLI's `3`. #28's row four asks for a declared refusal under its own
/// code rather than a mapped guess; the ruling on #76 says which code.
const EXIT_REFUSED: u8 = 3;

/// Write one line to stdout, saying how the run should end.
///
/// Not `println!`, which panics when the write fails, and the write fails
/// routinely: `diet replay | head` closes the pipe, and this verb's output is
/// a census followed by every entry in the object -- hundreds of lines on a
/// real session. A reader that stops reading is not an error in
/// the replay, so a broken pipe ends the run at [`EXIT_OK`] rather than at a
/// panic's 101. Found by running the program, not by reading it: the acid
/// test against a 14 MB log was piped into `head` and exited 101 with the
/// census on screen and every count correct.
fn line(text: &str) -> Result<(), u8> {
    use std::io::Write as _;
    let mut out = std::io::stdout().lock();
    match out
        .write_all(text.as_bytes())
        .and_then(|()| out.write_all(b"\n"))
    {
        Ok(()) => Ok(()),
        Err(why) if why.kind() == std::io::ErrorKind::BrokenPipe => Err(EXIT_OK),
        Err(why) => {
            // Stderr may be gone too; there is nothing useful to do if it is.
            let _ = writeln!(std::io::stderr(), "the census could not be written: {why}");
            Err(EXIT_COULD_NOT_RUN)
        }
    }
}

/// The usage banner, printed on any command line [`Args::of`] refuses.
fn usage() -> &'static str {
    "usage: diet replay --adapter claude-code --regimen <regimen.toml> \\\n\
     \x20      --source-available committed|pinned_only <log.jsonl>\n\
     \n\
     Runs the capture lanes over a session log this library did not produce.\n\
     No model is called. The census on stdout says what the adapter could not\n\
     type -- read it, because an adapted log is a view of a session and not a\n\
     transcript of one.\n\
     \n\
     --source-available is not a guess: `committed` claims the log itself is\n\
     checked into a repository beside the record, recoverable by whoever reads\n\
     it later; `pinned_only` claims only that its digest is pinned. An\n\
     operator's own log is almost always the second -- it is not in any\n\
     repository this program knows of, and saying otherwise would be a promise\n\
     this program cannot keep."
}

/// `tag` as the [`Availability`] it names, or every tag this program accepts,
/// for the refusal.
///
/// Declared, not inferred -- the same rule every other provenance fact in
/// this crate is held to (the regimen, the substrate, the reasoning state).
/// This program has no way to know whether the log it was pointed at lives
/// in a repository somewhere; the operator invoking it does, and saying so
/// is their claim to make, not this program's to guess from the path.
fn source_available_of(tag: &str) -> Result<Availability, String> {
    Availability::ALL
        .iter()
        .copied()
        .find(|state| state.tag() == tag)
        .ok_or_else(|| {
            format!(
                "`--source-available {tag}` is not one of {}",
                Availability::ALL
                    .iter()
                    .map(|state| state.tag())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

/// `diet replay`'s whole verb, called from `diet.rs`'s `main` with everything
/// after the literal `replay` token.
pub fn run(args: &[String]) -> ExitCode {
    let Some(parsed) = Args::of(args) else {
        eprintln!("{}", usage());
        return ExitCode::from(EXIT_COULD_NOT_RUN);
    };

    // Asked of the adapter rather than matched against a literal here: the
    // name an adapter answers to is the one it declares, and `diet/src` admits
    // no match arm on a string literal for exactly the reason a second copy of
    // that name would be wrong. A second adapter turns this into a lookup over
    // a list of constructors returning `Box<dyn Adapter>`; with one, the list
    // is the one line below.
    let adapter = ClaudeCode;
    if parsed.adapter != adapter.name() {
        eprintln!(
            "`{}` names no adapter; this build has: {}",
            parsed.adapter,
            adapter.name()
        );
        return ExitCode::from(EXIT_COULD_NOT_RUN);
    }

    let source_available = match source_available_of(&parsed.source_available) {
        Ok(state) => state,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::from(EXIT_COULD_NOT_RUN);
        }
    };

    let log = match std::fs::read_to_string(&parsed.log) {
        Ok(text) => text,
        Err(why) => {
            eprintln!("{}: {why}", parsed.log);
            return ExitCode::from(EXIT_COULD_NOT_RUN);
        }
    };
    let regime = match std::fs::read_to_string(&parsed.regimen)
        .map_err(|why| format!("{}: {why}", parsed.regimen))
        .and_then(|text| regimen::parse(&text).map_err(|why| why.to_string()))
        // Replay never has an endpoint: it calls no model, and
        // `adapters_a_replay_takes_no_endpoint_and_the_binary_holds_no_transport`
        // asserts that against the artifact rather than in a comment.
        .and_then(|declared| regime_of(&declared, false))
    {
        Ok(regime) => regime,
        Err(why) => {
            // `3`, not `2`. A regimen this verb cannot READ is a file that
            // could not be opened; a regimen it reads and declines is a
            // document whose schema it recognises and refuses, which is the
            // same class as a log whose format moved.
            eprintln!("{why}");
            return ExitCode::from(EXIT_REFUSED);
        }
    };

    // The sha256 of the log this verb actually read, over its own bytes --
    // before adapting, so it names the file that was opened rather than
    // anything derived from it. `Start.source.source_digest` for the record
    // built below.
    let source_digest = diet::digest::sha256_hex(log.as_bytes());

    // The regimen declares exactly one substrate and the log names none, so
    // this is the id every adapted row is filed against. Reading it off the
    // regime rather than naming it here keeps one statement of what served.
    let substrate = regime.substrates[0].id.clone();
    let read = match adapter.adapt(&log, &substrate) {
        Ok(read) => read,
        Err(drift) => {
            eprintln!("{drift}");
            return ExitCode::from(EXIT_REFUSED);
        }
    };

    // The deterministic lane, over events that already happened. It reads
    // tool calls and nothing else, which is why this needs no model.
    let mut lane = Lane::default();
    for event in &read.events {
        lane.observe(event);
    }
    let turns = derived(&lane, &read.events);
    let entries: usize = turns.iter().map(|(_, patches)| patches.len()).sum();
    let census_line = read.census.render();
    let events_len = read.events.len();

    // The record: `Start` first, declaring `source = adapted` and the
    // `source_available` the operator declared above -- `pinned_only` for
    // the common case, an operator's own log, which is not in any repository
    // this program knows of; `committed`, correctly, when replay is pointed
    // at a log that IS checked in beside its own record, such as this lane's
    // fixture corpus. Then every event the adapter produced, `Event::Unknown`
    // included -- ruling 1's own point, that a row the adapter could not map
    // still has to be a row IN the record and not only a line in the census.
    //
    // Rendered and read back before anything is written, the same discipline
    // `diet::drive` holds itself to: a record this verb can write and its own
    // format cannot parse is a defect found here, not three lines into
    // somebody else's stdout.
    let built = read.into_record(regime.clone(), source_digest, source_available);
    let rendered_record = record::render(&built);
    if let Err(why) = record::parse(&rendered_record) {
        eprintln!("the record this replay built does not parse back: {why}");
        return ExitCode::from(EXIT_COULD_NOT_RUN);
    }

    let mut object = WorkingObject::open(regime);
    // One `apply_turn` per turn, in turn order. The object refuses a turn of
    // patches whose provenances name different turns -- "a turn is ordered
    // within itself" -- and it is right to: a replay of a six-turn session is
    // six turns of derived facts, not one turn with six turns' provenance
    // stamped on it. The first version applied everything at once and only
    // got away with it because every entry claimed turn zero.
    for (_, patches) in &turns {
        if let Err(why) = object.apply_turn(patches) {
            eprintln!("the derived facts did not apply: {why}");
            return ExitCode::from(EXIT_COULD_NOT_RUN);
        }
    }

    let dump = object.dump();
    let written = line(&census_line)
        .and_then(|()| {
            line(&format!(
                "{{\"entries\":{entries},\"events\":{events_len},\"object_version\":{}}}",
                object.version()
            ))
        })
        .and_then(|()| {
            // The record's own lines, each already newline-terminated JSON:
            // written one at a time for the same reason the object's dump is
            // below, and ahead of it, because it is what the log BECAME,
            // before saying what was derived from it.
            for entry in rendered_record.lines() {
                line(entry)?;
            }
            Ok(())
        })
        .and_then(|()| {
            // `dump` already ends every entry with a newline, so the lines are
            // written one at a time rather than as one blob: a reader that
            // leaves partway through gets whole lines, and the broken pipe is
            // noticed at the line it happened on.
            for entry in dump.lines() {
                line(entry)?;
            }
            Ok(())
        });
    match written {
        Ok(()) => ExitCode::from(EXIT_OK),
        Err(code) => ExitCode::from(code),
    }
}

/// What the deterministic lane derived, as turns of entries the object holds.
///
/// Grouped by the turn the fact belongs to and returned in turn order,
/// because that is the shape `WorkingObject::apply_turn` takes: its patches
/// must agree on their turn, since an entry's `index` is its position WITHIN
/// a turn and two turns' entries interleaved would have no order at all.
///
/// Only facts the lane actually established. A file it never saw touched
/// produces no entry, and the working directory produces one only when it is
/// a path -- `Cwd` is deliberately able to say "unknown", and an entry
/// asserting an unknown directory would be the invention this whole module is
/// written against.
fn derived(lane: &Lane, events: &[Event]) -> Vec<(u32, Vec<Patch>)> {
    // `(turn, id, content)`, gathered before any patch is cut so the turns
    // can be grouped without the index of one turn's entries depending on
    // what another turn happened to derive.
    let mut facts: Vec<(u32, String, String)> = Vec::new();

    if let Some(cwd) = lane.facts().cwd {
        // Turn zero, and meant: `Facts` is the lane's state at the END of the
        // walk, with no event and no turn behind it. Naming a turn here would
        // be picking one. Every other entry below carries the turn the lane
        // actually recorded.
        facts.push((
            0,
            "mechanical/cwd".to_owned(),
            format!("the working directory is {cwd}"),
        ));
    }
    for (path, touch) in lane.files() {
        // A touch with no path says "read " and names nothing. The lane can
        // derive one from a command whose operand it could not resolve, and
        // an entry asserting a read of the empty string is worse than no
        // entry -- it is a fact in the object that is not a fact.
        if path.as_os_str().is_empty() {
            continue;
        }
        facts.push((
            // The turn the LANE recorded. Every entry used to say turn zero,
            // which is a turn the adapter never emits -- its turns are
            // one-based -- so every entry of a real replay named a turn that
            // did not exist.
            touch.turn,
            format!("mechanical/file:{}", path.display()),
            format!("{} {}", touch.kind.verb(), path.display()),
        ));
    }
    for (at, failure) in lane.failures().iter().enumerate() {
        facts.push((
            // `Failure` carries no turn, so it is looked up through the event
            // that failed -- which the adapter DID stamp with a turn.
            at_turn_of(events, &failure.event),
            format!("mechanical/failure/{at}"),
            format!("{}: {}", failure.command, failure.report),
        ));
    }

    let mut turns: BTreeMap<u32, Vec<Patch>> = BTreeMap::new();
    for (turn, id, content) in facts {
        let Ok(id) = EntryId::new(&id) else {
            continue;
        };
        let patches = turns.entry(turn).or_default();
        let index = u32::try_from(patches.len()).unwrap_or(u32::MAX);
        patches.push(Patch::Add {
            id,
            content,
            provenance: Provenance {
                turn,
                lane: "mechanical".to_owned(),
                fork: None,
                tangent: None,
                index,
            },
        });
    }
    turns.into_iter().collect()
}

/// The turn a tool call happened in, by the event id the lane recorded.
fn at_turn_of(events: &[Event], event: &str) -> u32 {
    events
        .iter()
        .find_map(|held| match held {
            Event::ToolCall { id, at_turn, .. } if id == event => Some(*at_turn),
            _ => None,
        })
        .unwrap_or(0)
}

/// A flag this verb reads, and the one place its spelling lives.
///
/// Same rule as the adapter's own vocabularies: a tag becomes a flag in
/// [`Flag::from_tag`], walking [`Flag::ALL`], and nothing else compares a
/// string to decide what an argument is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Flag {
    /// Which adapter reads the log.
    Adapter,
    /// The regimen the replay is filed under.
    Regimen,
    /// Whether the log this replay reads can be reached again.
    SourceAvailable,
}

impl Flag {
    /// Every flag this verb takes.
    const ALL: &'static [Self] = &[Self::Adapter, Self::Regimen, Self::SourceAvailable];

    /// How it is spelled on the command line.
    const fn tag(self) -> &'static str {
        match self {
            Self::Adapter => "--adapter",
            Self::Regimen => "--regimen",
            Self::SourceAvailable => "--source-available",
        }
    }

    /// The one place an argument becomes a flag.
    fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|flag| flag.tag() == tag)
    }
}

/// The command line after `replay`, or nothing when it is not one this verb
/// can serve.
struct Args {
    adapter: String,
    regimen: String,
    log: String,
    /// `committed | pinned_only`, in the record's own spelling -- parsed
    /// against [`Availability::from_tag`] in [`run`], not here: this struct
    /// is the command line as text, and the one place a string becomes the
    /// typed value is where every other record vocabulary already becomes
    /// one.
    source_available: String,
}

impl Args {
    /// Parsed strictly: an unknown flag is a refusal rather than a default.
    fn of(args: &[String]) -> Option<Self> {
        let mut flags: BTreeMap<Flag, String> = BTreeMap::new();
        let mut positional = Vec::new();
        let mut rest = args.iter();
        while let Some(arg) = rest.next() {
            if let Some(flag) = Flag::from_tag(arg) {
                flags.insert(flag, rest.next()?.clone());
            } else if arg.starts_with("--") {
                // An unknown flag is a refusal rather than a default: a
                // misspelled `--regimen` that fell through to the positional
                // list would run the replay under a regime nobody declared.
                return None;
            } else {
                positional.push(arg);
            }
        }
        if positional.len() != 1 {
            return None;
        }
        Some(Self {
            adapter: flags.get(&Flag::Adapter)?.clone(),
            source_available: flags.get(&Flag::SourceAvailable)?.clone(),
            regimen: flags.get(&Flag::Regimen)?.clone(),
            log: positional.remove(0).clone(),
        })
    }
}
