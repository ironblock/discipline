//! `diet` — the command-line face of the library.
//!
//! Resolves a regimen file and renders it back canonically, so that two
//! regimens differing only in formatting render identically. That makes the
//! regimen format load-bearing rather than decorative: `verify.sh` runs this
//! over every `regimen.toml` under `results/`.
//!
//! This is deliberately NOT a harness. A harness has a substrate, and
//! substrate is one of the variables a regimen names -- `regime.substrate` is
//! a required key in every results report. Binding the reference harness to a
//! language here would make a constant of something the project exists to
//! vary.

use std::fmt::Write as _;
use std::process::ExitCode;

use diet::formats::regimen::{self, Value};

/// Exit code for a usage error, kept distinct from a parse failure so that a
/// caller can tell "you invoked me wrong" from "your regimen is wrong".
const EXIT_USAGE: u8 = 2;
/// Exit code for a regimen that could not be read or parsed.
const EXIT_INPUT: u8 = 1;

const USAGE: &str = "usage: diet <regimen.toml>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [path] = args.as_slice() else {
        eprintln!("{USAGE}");
        return ExitCode::from(EXIT_USAGE);
    };

    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(err) => {
            eprintln!("diet: cannot read {path}: {err}");
            return ExitCode::from(EXIT_INPUT);
        }
    };

    match regimen::parse(&source) {
        Ok(parsed) => {
            print!("{}", render(&parsed));
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("diet: {path}: {err}");
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
