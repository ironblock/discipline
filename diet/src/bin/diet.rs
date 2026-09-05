//! `diet` — the command-line face of the library.
//!
//! One authorized implementation per format, and everything else crosses this
//! boundary to reach it. The research phase ended with roughly five hundred
//! lines of duplicated transport and parsing across Rust and Python -- the
//! same request shapes, the same answer parsing, the same decline rules,
//! re-implemented in whichever language the instrument of the day was written
//! in. The cost was not the lines: it was **two readers that could disagree**,
//! a grader in Python and a capture path in Rust parsing one answer
//! differently, and a whole gate that existed only to referee them.
//!
//! Every subcommand emits a **structured result** on stdout -- JSON, in the
//! record's own value space -- rather than prose. A caller that has to parse
//! an error message is a caller that has re-implemented the format.
//!
//! Each subcommand is one format's [`Format::project`], the same function the
//! conformance harness calls. There is no second projection to agree with.

use std::fmt::Write as _;
use std::process::ExitCode;

#[cfg(test)]
use diet::formats::Format;
use diet::formats::record::json::{self, Value};

/// Exit code for a usage error, kept distinct from a format failure so that a
/// caller can tell "you invoked me wrong" from "your input is wrong".
const EXIT_USAGE: u8 = 2;
/// Exit code for input that is not what the subcommand says it is.
const EXIT_INPUT: u8 = 1;

/// What a verb does with the file it is given.
///
/// Until the capture lanes arrived every verb read one format, and the table
/// below could be a pair. A lane is not a format: routing a drive reads a
/// record and answers with a census, so the answer is not that record's
/// value and the format table must not claim it. The column says which kind
/// a verb is, so that `every_format_has_a_command` can still insist every
/// format is exposed without also insisting every verb is a format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operation {
    /// Read a document of this format and answer with its value.
    Format(&'static str),
    /// Route an archived drive and answer with the router's census.
    Route,
}

/// The verb each operation is exposed under.
///
/// A table, not a match: `scripts/check-library.py` refuses a match arm on a
/// string literal in this crate, and `every_format_has_a_command` refuses a
/// format that this table forgets.
const COMMANDS: &[(&str, Operation)] = &[
    ("classify-decline", Operation::Format("decline")),
    ("parse-interview", Operation::Format("interview")),
    ("check-record", Operation::Format("record")),
    ("check-regimen", Operation::Format("regimen")),
    ("parse-shell", Operation::Format("shell")),
    ("route", Operation::Route),
];

fn usage() -> String {
    let mut out = String::from("usage: diet <command> <path>\n\ncommands:\n");
    for (command, operation) in COMMANDS {
        let _ = match operation {
            Operation::Format(format) => {
                writeln!(out, "  {command:<18} read a `{format}` document")
            }
            Operation::Route => writeln!(
                out,
                "  {command:<18} route an archived drive and report its census"
            ),
        };
    }
    out.push_str("\nEvery command writes a JSON result to stdout. Exit 0 when the\n");
    out.push_str("document is what the command says it is, 1 when it is not, 2 on\n");
    out.push_str("a usage error.\n");
    out
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [command, path] = args.as_slice() else {
        eprint!("{}", usage());
        return ExitCode::from(EXIT_USAGE);
    };

    let Some((_, operation)) = COMMANDS.iter().find(|(verb, _)| verb == command) else {
        eprintln!("diet: `{command}` is not a command\n");
        eprint!("{}", usage());
        return ExitCode::from(EXIT_USAGE);
    };

    // Bytes, not text: a format returns a verdict on whatever is on disk, and
    // "this file is not UTF-8" is one of its verdicts rather than a crash.
    let source = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("diet: cannot read {path}: {err}");
            return ExitCode::from(EXIT_USAGE);
        }
    };
    let text = std::str::from_utf8(&source).map_err(|err| format!("not UTF-8: {err}"));
    let (subject, outcome) = match operation {
        Operation::Format(name) => {
            let Some(format) = diet::formats::format(name) else {
                eprintln!("diet: `{name}` is not a format this binary carries");
                return ExitCode::from(EXIT_USAGE);
            };
            (format.name, text.and_then(|text| (format.project)(text)))
        }
        Operation::Route => ("route", text.and_then(route)),
    };

    let (ok, key, held) = match outcome {
        Ok(value) => (true, "value", value),
        Err(reason) => (false, "error", Value::String(reason)),
    };
    let result = Value::Object(std::collections::BTreeMap::from([
        ("format".to_owned(), Value::String(subject.to_owned())),
        ("path".to_owned(), Value::String(path.clone())),
        ("ok".to_owned(), Value::Boolean(ok)),
        (key.to_owned(), held),
    ]));
    let mut rendered = String::new();
    json::render(&result, &mut rendered);
    println!("{rendered}");

    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(EXIT_INPUT)
    }
}

/// The format `command` reads, if it is a command that reads one. Only the
/// test below asks: `main` dispatches on the operation itself.
#[cfg(test)]
fn format_for(command: &str) -> Option<&'static Format> {
    let (_, operation) = COMMANDS.iter().find(|(verb, _)| *verb == command)?;
    match operation {
        Operation::Format(name) => diet::formats::format(name),
        Operation::Route => None,
    }
}

/// Route a drive: read it as a record, replay it, answer with the census.
///
/// The census is the answer because it is the claim this lane makes -- how
/// many forks a drive spent against how many the naive design would have --
/// and a caller that had to count the decisions itself would be a second
/// implementation of the router.
fn route(text: &str) -> Result<Value, String> {
    let record = diet::formats::record::parse(text).map_err(|err| err.to_string())?;
    let replayed = diet::capture::router::replay(&record).map_err(|err| err.to_string())?;
    replayed.census.value().map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::{COMMANDS, Operation, format_for};
    use diet::formats::FORMATS;

    // A format with no command is a format the boundary does not expose, so
    // anything that wants it re-implements it -- which is the one thing this
    // binary exists to prevent.
    #[test]
    fn every_format_has_a_command_and_every_command_a_format() {
        let exposed: std::collections::BTreeSet<&str> = COMMANDS
            .iter()
            .filter_map(|(_, operation)| match operation {
                Operation::Format(name) => Some(*name),
                Operation::Route => None,
            })
            .collect();
        let declared: std::collections::BTreeSet<&str> =
            FORMATS.iter().map(|format| format.name).collect();
        assert_eq!(
            exposed, declared,
            "the CLI and FORMATS disagree about which formats exist"
        );
        for (command, operation) in COMMANDS {
            match operation {
                Operation::Format(name) => assert_eq!(
                    format_for(command).map(|format| format.name),
                    Some(*name),
                    "`{command}` does not resolve to `{name}`"
                ),
                // A lane's verb has no format, and must not borrow one: a
                // census answered under a format's name would read as that
                // format's value.
                Operation::Route => assert!(
                    format_for(command).is_none(),
                    "`{command}` is a lane, and it resolved to a format"
                ),
            }
        }
    }

    #[test]
    fn an_unknown_command_is_not_a_format() {
        assert!(format_for("parse-everything").is_none());
        assert!(format_for("").is_none());
    }
}
