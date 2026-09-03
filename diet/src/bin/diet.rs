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

use diet::formats::Format;
use diet::formats::record::json::{self, Value};

/// Exit code for a usage error, kept distinct from a format failure so that a
/// caller can tell "you invoked me wrong" from "your input is wrong".
const EXIT_USAGE: u8 = 2;
/// Exit code for input that is not what the subcommand says it is.
const EXIT_INPUT: u8 = 1;

/// The verb each format is exposed under.
///
/// A table, not a match: `scripts/check-library.py` refuses a match arm on a
/// string literal in this crate, and `every_format_has_a_command` refuses a
/// format that this table forgets.
const COMMANDS: &[(&str, &str)] = &[
    ("classify-decline", "decline"),
    ("parse-interview", "interview"),
    ("check-record", "record"),
    ("check-regimen", "regimen"),
    ("parse-shell", "shell"),
    ("parse-verdict", "verdict"),
];

fn usage() -> String {
    let mut out = String::from("usage: diet <command> <path>\n\ncommands:\n");
    for (command, format) in COMMANDS {
        let _ = writeln!(out, "  {command:<18} read a `{format}` document");
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

    let Some(format) = format_for(command) else {
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
    let outcome = match std::str::from_utf8(&source) {
        Ok(text) => (format.project)(text),
        Err(err) => Err(format!("not UTF-8: {err}")),
    };

    let (ok, key, held) = match outcome {
        Ok(value) => (true, "value", value),
        Err(reason) => (false, "error", Value::String(reason)),
    };
    let result = Value::Object(std::collections::BTreeMap::from([
        ("format".to_owned(), Value::String(format.name.to_owned())),
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

/// The format `command` reads, if it is a command.
fn format_for(command: &str) -> Option<&'static Format> {
    let (_, name) = COMMANDS.iter().find(|(verb, _)| *verb == command)?;
    diet::formats::format(name)
}

#[cfg(test)]
mod tests {
    use super::{COMMANDS, format_for};
    use diet::formats::FORMATS;

    // A format with no command is a format the boundary does not expose, so
    // anything that wants it re-implements it -- which is the one thing this
    // binary exists to prevent.
    #[test]
    fn every_format_has_a_command_and_every_command_a_format() {
        let exposed: std::collections::BTreeSet<&str> =
            COMMANDS.iter().map(|(_, format)| *format).collect();
        let declared: std::collections::BTreeSet<&str> =
            FORMATS.iter().map(|format| format.name).collect();
        assert_eq!(
            exposed, declared,
            "the CLI and FORMATS disagree about which formats exist"
        );
        for (command, name) in COMMANDS {
            assert_eq!(
                format_for(command).map(|format| format.name),
                Some(*name),
                "`{command}` does not resolve to `{name}`"
            );
        }
    }

    #[test]
    fn an_unknown_command_is_not_a_format() {
        assert!(format_for("parse-everything").is_none());
        assert!(format_for("").is_none());
    }
}
