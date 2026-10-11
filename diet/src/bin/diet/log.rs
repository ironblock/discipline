//! `diet log find <log> [key=value | key~text]...` (#757): the lines of a log
//! that pass every filter, one JSON line each, as the log's projection
//! writes them -- `seq` and every field -- so a seq a run's README cites
//! comes from a query. Exit 0 with what matched (nothing is no error), 2 on
//! a usage error, 3 when the file is not a log this reader reads.

use std::process::ExitCode;

use diet::formats::log_query::{Filter, find};
use diet::formats::record::json;

const USAGE: &str = "usage: diet log find <log> [key=value | key~text]...\n\n\
Prints each line of the log that passes every filter, one JSON line each,\n\
as the log's projection writes it. A key may be a dotted path (entry.id).\n\
key=value: the value is exactly this (a string as written, else its JSON).\n\
key~text: the value's text contains this.\n";

/// The verb, from what follows `log` on the command line.
pub fn run(args: &[String]) -> ExitCode {
    let [find_word, path, filters @ ..] = args else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    if find_word != "find" {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    }
    let filters = match filters
        .iter()
        .map(|spelled| Filter::parse(spelled))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(filters) => filters,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::from(2);
        }
    };
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(why) => {
            eprintln!("{path}: {why}");
            return ExitCode::from(2);
        }
    };
    match find(&source, &filters) {
        Ok(lines) => {
            let mut out = String::new();
            for line in &lines {
                json::render(line, &mut out);
                out.push('\n');
            }
            print!("{out}");
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("{path}: {why}");
            ExitCode::from(3)
        }
    }
}
