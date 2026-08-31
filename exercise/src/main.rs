//! `exercise` — the reference harness.
//!
//! A harness that consumes [`diet`] with total instrumentation. Today it does
//! the smallest real thing that proves the wiring: resolve a regimen file and
//! render it back canonically, so that two regimens differing only in
//! formatting render identically.
//!
//! `exercise/ui/` is reserved for the SPA and is empty by design.

use std::fmt::Write as _;
use std::process::ExitCode;

use diet::formats::regimen::{self, Value};

/// Exit code for a usage error, kept distinct from a parse failure so that a
/// caller can tell "you invoked me wrong" from "your regimen is wrong".
const EXIT_USAGE: u8 = 2;
/// Exit code for a regimen that could not be read or parsed.
const EXIT_INPUT: u8 = 1;

const USAGE: &str = "usage: exercise <regimen.toml>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [path] = args.as_slice() else {
        eprintln!("{USAGE}");
        return ExitCode::from(EXIT_USAGE);
    };

    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(err) => {
            eprintln!("exercise: cannot read {path}: {err}");
            return ExitCode::from(EXIT_INPUT);
        }
    };

    match regimen::parse(&source) {
        Ok(parsed) => {
            print!("{}", render(&parsed));
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("exercise: {path}: {err}");
            ExitCode::from(EXIT_INPUT)
        }
    }
}

/// Render a regimen canonically: sorted keys, one binding per line, no
/// comments, no incidental whitespace.
fn render(parsed: &regimen::Regimen) -> String {
    let mut out = String::new();
    for (key, value) in parsed.iter() {
        let rendered = match value {
            Value::String(text) => format!("\"{text}\""),
            Value::Integer(number) => number.to_string(),
            Value::Boolean(flag) => flag.to_string(),
        };
        writeln!(out, "{key} = {rendered}").expect("writing to a String cannot fail");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::render;
    use diet::formats::regimen;

    #[test]
    fn rendering_is_canonical() {
        let messy = regimen::parse("  replay=false\n# a comment\narm  =  \"baseline\"\n")
            .expect("document is a regimen");
        assert_eq!(render(&messy), "arm = \"baseline\"\nreplay = false\n");
    }

    #[test]
    fn rendering_a_regimen_round_trips_through_the_parser() {
        let source = "arm = \"baseline\"\ndogma_version = 0\nreplay = true\n";
        let once = regimen::parse(source).expect("document is a regimen");
        let twice = regimen::parse(&render(&once)).expect("rendering is itself a regimen");
        assert_eq!(once, twice);
    }
}
