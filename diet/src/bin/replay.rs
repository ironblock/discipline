//! `diet-replay` — run the capture lanes over a foreign harness's session log.
//!
//! The read-only front door from #28. It adapts a log this library did not
//! produce, runs the deterministic capture lane over the result, and prints
//! what the discipline would have captured from work that already happened.
//! **No model is called**, and none can be: nothing here holds a transport.
//!
//! ```text
//! diet-replay --adapter claude-code --regimen <regimen.toml> <log.jsonl>
//! ```
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
//! # The exit codes, which are the CLI's and not this program's
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
//! a document, and this program does not reach one. It reads a log or refuses
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
use diet::formats::record::Event;
use diet::formats::regimen;
use diet::object::{EntryId, Patch, Provenance, WorkingObject};

/// Everything ran.
const EXIT_OK: u8 = 0;
/// The command line was not one this program serves, a file it was pointed at
/// could not be read, or its output could not be written.
///
/// The CLI's `2`: *usage, or could not run*. One number for one meaning, and
/// the meaning is "this invocation never got as far as an answer".
const EXIT_COULD_NOT_RUN: u8 = 2;
/// Input refused: the adapter found a row whose format has moved, or the
/// regimen is a document this program recognises and declines.
///
/// The CLI's `3`. #28's row four asks for a declared refusal under its own
/// code rather than a mapped guess; the ruling on #76 says which code.
const EXIT_REFUSED: u8 = 3;

/// Write one line to stdout, saying how the run should end.
///
/// Not `println!`, which panics when the write fails, and the write fails
/// routinely: `diet-replay | head` closes the pipe, and this program's output
/// is a census followed by every entry in the object -- 223 lines from the
/// log it was written against. A reader that stops reading is not an error in
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

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(parsed) = Args::of(&args) else {
        eprintln!(
            "usage: diet-replay --adapter claude-code --regimen <regimen.toml> <log.jsonl>\n\
             \n\
             Runs the capture lanes over a session log this library did not produce.\n\
             No model is called. The census on stdout says what the adapter could not\n\
             type -- read it, because an adapted log is a view of a session and not a\n\
             transcript of one."
        );
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
        .and_then(|declared| regime_of(&declared))
    {
        Ok(regime) => regime,
        Err(why) => {
            // `3`, not `2`. A regimen this program cannot READ is a file that
            // could not be opened; a regimen it reads and declines is a
            // document whose schema it recognises and refuses, which is the
            // same class as a log whose format moved.
            eprintln!("{why}");
            return ExitCode::from(EXIT_REFUSED);
        }
    };

    let read = match adapter.adapt(&log) {
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

    let mut object = WorkingObject::open(regime);
    let turns = derived(&lane, &read.events);
    let entries: usize = turns.iter().map(|(_, patches)| patches.len()).sum();
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
    let written = line(&read.census.render())
        .and_then(|()| {
            line(&format!(
                "{{\"entries\":{entries},\"events\":{},\"object_version\":{}}}",
                read.events.len(),
                object.version()
            ))
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
            // one-based -- so the provenance of all 221 entries of a real
            // replay named a turn that did not exist.
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

/// A flag this program reads, and the one place its spelling lives.
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
}

impl Flag {
    /// Every flag this program takes.
    const ALL: &'static [Self] = &[Self::Adapter, Self::Regimen];

    /// How it is spelled on the command line.
    const fn tag(self) -> &'static str {
        match self {
            Self::Adapter => "--adapter",
            Self::Regimen => "--regimen",
        }
    }

    /// The one place an argument becomes a flag.
    fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|flag| flag.tag() == tag)
    }
}

/// The command line, or nothing when it is not one this program can serve.
struct Args {
    adapter: String,
    regimen: String,
    log: String,
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
            regimen: flags.get(&Flag::Regimen)?.clone(),
            log: positional.remove(0).clone(),
        })
    }
}
