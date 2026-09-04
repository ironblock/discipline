//! The dev-loop integration lane: one scripted drive, end to end.
//!
//! The battery's own unit tests call it as a function, and that leaves the
//! half a function call cannot reach: whether a drive writes a record that the
//! one authorized reader accepts. A record assembled in memory and asserted on
//! in memory is asserted on by the code that built it. This walks the whole
//! round trip instead -- run the battery against a canned server, render the
//! drive to a file, and let `diet check-record` return the verdict -- which is
//! what `verify.sh --only integration` gates.
//!
//! An example rather than a subcommand: `diet`'s verbs are formats, one verb
//! per format, and a drive is not a format. Nothing outside this repository
//! runs this.
//!
//! ```text
//! drive --server <name> [--record <path>] [--require-failure <behaviour>]
//! ```
//!
//! Exit 0 when every behaviour held, 1 when one did not, 2 on a usage error.
//! With `--require-failure`, exit 0 only when exactly the named behaviour
//! failed: a non-compliant fixture that starts passing is as much a broken
//! fixture as one that starts failing for a new reason.

use std::process::ExitCode;

use diet::capture::battery::{Battery, Behaviour, CannedResponder, render_value, servers_dir};
use diet::formats::record;

/// A usage error, kept distinct from a battery failure the way the CLI keeps
/// it distinct from a bad document.
const EXIT_USAGE: u8 = 2;
/// A behaviour that did not hold.
const EXIT_FAIL: u8 = 1;

fn usage() -> String {
    let mut out = String::from(
        "usage: drive --server <name> [--record <path>] [--require-failure <behaviour>]\n\n\
         canned servers live in diet/capture/battery/servers/; behaviours are:\n",
    );
    for behaviour in Behaviour::ALL {
        out.push_str("  ");
        out.push_str(behaviour.tag());
        out.push('\n');
    }
    out
}

/// What the arguments asked for.
struct Asked {
    server: String,
    record: Option<String>,
    require_failure: Option<Behaviour>,
}

fn read_args(args: &[String]) -> Result<Asked, String> {
    let (mut server, mut record, mut require) = (None, None, None);
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let Some(value) = rest.next() else {
            return Err(format!("`{flag}` needs a value"));
        };
        if flag == "--server" {
            server = Some(value.clone());
        } else if flag == "--record" {
            record = Some(value.clone());
        } else if flag == "--require-failure" {
            require = Some(
                Behaviour::from_tag(value)
                    .ok_or_else(|| format!("`{value}` is not a behaviour"))?,
            );
        } else {
            return Err(format!("`{flag}` is not an option"));
        }
    }
    Ok(Asked {
        server: server.ok_or_else(|| "no --server was named".to_owned())?,
        record,
        require_failure: require,
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let asked = match read_args(&args) {
        Ok(asked) => asked,
        Err(why) => {
            eprintln!("drive: {why}\n");
            eprint!("{}", usage());
            return ExitCode::from(EXIT_USAGE);
        }
    };

    let dir = servers_dir().join(&asked.server);
    let mut responder = match CannedResponder::load(&dir) {
        Ok(responder) => responder,
        Err(err) => {
            eprintln!("drive: {err}");
            return ExitCode::from(EXIT_USAGE);
        }
    };

    let driven = Battery::drive(&mut responder);
    println!("{}", render_value(&driven.report.value()));

    if let Some(path) = &asked.record
        && let Err(err) = std::fs::write(path, record::render(&driven.record()))
    {
        eprintln!("drive: cannot write {path}: {err}");
        return ExitCode::from(EXIT_USAGE);
    }

    let failed: Vec<Behaviour> = driven
        .report
        .failures()
        .into_iter()
        .map(|(behaviour, reason)| {
            eprintln!("drive: {} failed: {reason}", behaviour.tag());
            behaviour
        })
        .collect();

    match asked.require_failure {
        None => {
            if failed.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(EXIT_FAIL)
            }
        }
        Some(wanted) => {
            if failed == vec![wanted] {
                ExitCode::SUCCESS
            } else {
                eprintln!(
                    "drive: `{}` was the only failure this fixture may have, and it \
                     failed {:?}",
                    wanted.tag(),
                    failed
                        .iter()
                        .copied()
                        .map(Behaviour::tag)
                        .collect::<Vec<_>>()
                );
                ExitCode::from(EXIT_FAIL)
            }
        }
    }
}
