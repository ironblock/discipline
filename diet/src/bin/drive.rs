//! `diet-drive` -- run a scripted session and write the record it produced.
//!
//! The half a function call cannot reach. `diet::drive::run` is exercised by
//! unit tests; this runs the same drive as a *program*, writes the archive to
//! a file, and leaves `diet check-record` to return the verdict on it. A lane
//! that only ever called the library would be a lane that never proved a file
//! could be written and read back by a second process.
//!
//! # Two modes, and neither of them guesses
//!
//! With no endpoint it serves the canned script itself: a loopback stub
//! playing [`diet::drive::canned::acts`], no weights, no GPU, no network out.
//! With an endpoint it calls that server. **No model is required and none is
//! invented** -- a run that needs one and has not been given one is the
//! second mode, and it fails with what the server said rather than falling
//! back to the first.
//!
//! # What it will not do
//!
//! Record v0's `regime` needs six facts about the substrate. Regimen v1
//! carries `arm`, `dogma_version` and a substrate *name*; the other four --
//! the model, the quantization, the reasoning state, the hardware -- are read
//! from keys named after the record's own field paths, and a regimen missing
//! any of them is refused. Inventing "unknown" for a regime tag is the
//! incomparable-regime failure this whole schema exists to prevent, and it
//! costs nothing to refuse instead.
//!
//! **Whether those four belong in the regimen format is the regimen owner's
//! call, not this program's.** Until it is made, the key names here are this
//! program's convention and are disclosed as one.

use std::fmt::Write as _;
use std::process::ExitCode;

use diet::client::Client;
use diet::client::shape::{
    Concurrency, Dialect, Limits, Message, RequestShape, Role, SamplerCard, Serving,
};
use diet::client::stub::Stub;
use diet::client::transport::{Endpoint, Http};
use diet::drive::{Gym, canned, run};
use diet::formats::record::json::{self, Value};
use diet::formats::record::{Reasoning, Regime, Substrate};
use diet::formats::regimen::{self, Regimen};
use diet::isolation::{self, Policy as IsolationPolicy};
use diet::seam::policy::Policy as SeamPolicy;

/// Exit code for a usage error, kept distinct from a drive that did not run.
const EXIT_USAGE: u8 = 2;
/// Exit code for a drive that halted. Two, matching
/// [`diet::drive::Halt::EXIT`] and [`isolation::Unavailable::EXIT`]: a
/// session that could not start is not a session that failed, and a census
/// that could not tell them apart would count a missing substrate as a
/// failed run.
const EXIT_HALT: u8 = 2;
/// Exit code for a regimen that is not one, or does not carry what a regime
/// needs.
const EXIT_INPUT: u8 = 1;

/// The keys this program reads for the four regime facts a regimen v1 has no
/// place for, named after the record's own field paths.
const SUBSTRATE_KEYS: &[&str] = &[
    "substrate_model",
    "substrate_quantization",
    "substrate_reasoning",
    "substrate_hardware",
];

fn usage() -> String {
    let mut out = String::from("usage: diet-drive <regimen> <output.jsonl> [endpoint]\n\n");
    out.push_str("Runs the pinned three-turn script through <regimen> and writes the\n");
    out.push_str("record to <output.jsonl>. With no endpoint the canned server answers\n");
    out.push_str("on loopback -- no model, no network out. With one, that server does.\n\n");
    out.push_str("The regimen must carry `arm`, `dogma_version`, `substrate` and:\n");
    for key in SUBSTRATE_KEYS {
        let _ = writeln!(out, "  {key}");
    }
    out.push_str("\nA JSON result goes to stdout. Exit 0 when the drive ran and its\n");
    out.push_str("record parses, 1 when the regimen is not usable, 2 on a usage error\n");
    out.push_str("or a drive that could not run.\n");
    out
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(regimen_path), Some(out_path)) = (args.first(), args.get(1)) else {
        eprint!("{}", usage());
        return ExitCode::from(EXIT_USAGE);
    };
    if args.len() > 3 {
        eprint!("{}", usage());
        return ExitCode::from(EXIT_USAGE);
    }

    let (regime, isolation_policy, seam_policy) = match declared(regimen_path) {
        Ok(three) => three,
        Err((code, why)) => return fail(code, &why),
    };
    let confinement = match isolation::open(&isolation_policy) {
        Ok(confinement) => confinement,
        Err(why) => return fail(EXIT_HALT, &why.to_string()),
    };
    let worktree = match std::env::current_dir() {
        Ok(here) => here,
        Err(why) => return fail(EXIT_HALT, &format!("there is no working directory: {why}")),
    };

    let script = canned::script(regime);
    // `held` is not an unused binding: dropping the stub stops its serving
    // thread, and a canned server that stopped mid-drive would reach the
    // client as a substrate that hung up. It lives as long as the run does.
    let (held, endpoint) = match answering(args.get(2)) {
        Ok(pair) => pair,
        Err((code, why)) => return fail(code, &why),
    };
    let client = Client::new(
        Http::new(endpoint),
        Serving {
            // Declared, and declared as one: this program makes one call at a
            // time, and a throughput number taken from it carries that.
            concurrency: Concurrency::Declared(1),
            dialect: Dialect::llama_cpp(),
        },
    );

    let driven = run(
        &script,
        &Gym {
            client: &client,
            shape: shape(&script.regime),
            confinement: &confinement,
            isolation: &isolation_policy,
            worktree: &worktree,
            seam: seam_policy,
        },
    );

    let written = match driven {
        Err(halt) => fail(EXIT_HALT, &halt.to_string()),
        Ok(drive) => written(&drive, out_path),
    };
    drop(held);
    written
}

/// Everything the regimen at `path` declares that a drive needs.
fn declared(path: &str) -> Result<(Regime, IsolationPolicy, SeamPolicy), (u8, String)> {
    let text = std::fs::read_to_string(path)
        .map_err(|why| (EXIT_INPUT, format!("{path} could not be read: {why}")))?;
    let regimen = regimen::parse(&text)
        .map_err(|why| (EXIT_INPUT, format!("{path} is not a regimen: {why:?}")))?;
    let regime = regime_of(&regimen).map_err(|why| (EXIT_INPUT, why))?;
    let isolation_policy = IsolationPolicy::from_regimen(&regimen)
        .map_err(|why| (EXIT_INPUT, format!("its isolation policy: {why}")))?;
    let seam_policy = SeamPolicy::from_regimen(&regimen)
        .map_err(|why| (EXIT_INPUT, format!("its seam policy: {why:?}")))?;
    Ok((regime, isolation_policy, seam_policy))
}

/// The server this run calls, and the stub serving it when that is this
/// program.
///
/// The stub comes back so the caller can hold it: dropped, it stops the
/// serving thread, and a canned server that stopped mid-drive would look like
/// a substrate that hung up.
fn answering(given: Option<&String>) -> Result<(Option<Stub>, Endpoint), (u8, String)> {
    let (served, url) = if let Some(endpoint) = given {
        (None, endpoint.clone())
    } else {
        let stub = Stub::serving(canned::acts())
            .map_err(|why| (EXIT_HALT, format!("the canned server did not bind: {why}")))?;
        let url = stub.url();
        (Some(stub), url)
    };
    let endpoint = Endpoint::parse(&url)
        .map_err(|why| (EXIT_INPUT, format!("`{url}` is not an endpoint: {why:?}")))?;
    Ok((served, endpoint))
}

/// Write the record and the product, and report where they went.
fn written(drive: &diet::drive::Drive, out_path: &str) -> ExitCode {
    if let Err(why) = std::fs::write(out_path, &drive.rendered) {
        return fail(
            EXIT_HALT,
            &format!("{out_path} could not be written: {why}"),
        );
    }
    // Beside it, because the summary's `product_sha256` is a digest OF
    // something and a digest of a file nobody wrote is a digest nobody can
    // check. `sha256sum` on this file is the whole audit.
    let product_path = format!("{out_path}.product");
    if let Err(why) = std::fs::write(&product_path, &drive.product) {
        return fail(
            EXIT_HALT,
            &format!("{product_path} could not be written: {why}"),
        );
    }
    let count = |many: usize| Value::Integer(i64::try_from(many).unwrap_or(-1));
    let mut out = String::new();
    json::render(
        &Value::Object(
            [
                ("ok".to_owned(), Value::Boolean(true)),
                ("record".to_owned(), Value::String(out_path.to_owned())),
                ("product".to_owned(), Value::String(product_path)),
                ("events".to_owned(), count(drive.record.events.len())),
                ("seams".to_owned(), count(drive.seams.len())),
                ("unspellable".to_owned(), count(drive.unspellable.len())),
                (
                    "uncaptured".to_owned(),
                    Value::Array(
                        drive
                            .uncaptured
                            .iter()
                            .map(|census| count(census.regions - census.captured))
                            .collect(),
                    ),
                ),
            ]
            .into_iter()
            .collect(),
        ),
        &mut out,
    );
    println!("{out}");
    ExitCode::SUCCESS
}

/// Write `why` as a structured result and return `code`.
///
/// Structured on the failure path too, because a caller that has to parse an
/// error message is a caller that has re-implemented the format.
fn fail(code: u8, why: &str) -> ExitCode {
    let mut out = String::new();
    json::render(
        &Value::Object(
            [
                ("ok".to_owned(), Value::Boolean(false)),
                ("error".to_owned(), Value::String(why.to_owned())),
            ]
            .into_iter()
            .collect(),
        ),
        &mut out,
    );
    println!("{out}");
    ExitCode::from(code)
}

/// The regime `regimen` declares, or the list of what it is missing.
fn regime_of(regimen: &Regimen) -> Result<Regime, String> {
    let mut missing = Vec::new();
    let mut text = |key: &'static str| match regimen.get(key) {
        Some(regimen::Value::String(value)) if !value.is_empty() => value.clone(),
        _ => {
            missing.push(key);
            String::new()
        }
    };

    let arm = text("arm");
    let name = text("substrate");
    let model = text(SUBSTRATE_KEYS[0]);
    let quantization = text(SUBSTRATE_KEYS[1]);
    let reasoning_written = text(SUBSTRATE_KEYS[2]);
    let hardware = text(SUBSTRATE_KEYS[3]);

    let Some(regimen::Value::Integer(dogma_version)) = regimen.get("dogma_version") else {
        missing.push("dogma_version");
        return Err(complaint(&missing));
    };
    if !missing.is_empty() {
        return Err(complaint(&missing));
    }

    let Some(reasoning) = Reasoning::ALL
        .iter()
        .copied()
        .find(|state| state.tag() == reasoning_written)
    else {
        return Err(format!(
            "`substrate_reasoning = \"{reasoning_written}\"` is not one of {}",
            Reasoning::ALL
                .iter()
                .map(|state| state.tag())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    };

    let sampler = match regimen.get("sampler") {
        Some(regimen::Value::Table(table)) if !table.is_empty() => table
            .iter()
            .map(|(key, value)| (key.clone(), sampled(value)))
            .collect(),
        _ => {
            return Err(
                "`[sampler]` is required and must not be empty: the record refuses a blank \
                 `substrate.sampler`, because \"nobody wrote the settings down\" and \"the \
                 settings were these\" are different facts about a run"
                    .to_owned(),
            );
        }
    };

    let Ok(dogma_version) = u32::try_from(*dogma_version) else {
        return Err(format!(
            "`dogma_version = {dogma_version}` is not a version"
        ));
    };

    Ok(Regime {
        arm,
        substrate: Substrate {
            name,
            model,
            quantization,
            sampler,
            reasoning,
            hardware,
        },
        dogma_version,
    })
}

/// What is missing, named, with why it is not defaulted.
fn complaint(missing: &[&str]) -> String {
    format!(
        "the regimen does not carry {}. A regime tag this program invented would make every \
         result under it incomparable with every other, which is what the tags are for",
        missing.join(", ")
    )
}

/// One sampler setting, in the record's value space.
///
/// The regimen's value space and the record's are different formats with
/// different readers; this is the crossing, and it is one function so there
/// is one place a new kind has to be decided.
fn sampled(value: &regimen::Value) -> Value {
    // Every variant, and no fallback. The first version had a `{other:?}`
    // arm and a regimen's `temperature = 0.6` reached the record as the
    // string `Float(Decimal("0.6"))` -- a regime tag that is a Rust debug
    // rendering, which compares equal to nothing and is what the exact
    // decimal exists to prevent. A crossing with a default arm is a crossing
    // that loses whatever nobody thought of.
    match value {
        regimen::Value::String(text) => Value::String(text.clone()),
        regimen::Value::Integer(number) => Value::Integer(*number),
        regimen::Value::Float(decimal) => Value::Decimal(decimal.clone()),
        regimen::Value::Boolean(flag) => Value::Boolean(*flag),
        regimen::Value::Array(items) => Value::Array(items.iter().map(sampled).collect()),
        regimen::Value::Table(table) => Value::Object(
            table
                .iter()
                .map(|(key, value)| (key.clone(), sampled(value)))
                .collect(),
        ),
    }
}

/// The request every call is a variation of.
fn shape(regime: &Regime) -> RequestShape {
    RequestShape {
        model: regime.substrate.model.clone(),
        messages: vec![Message::new(Role::User, "replaced per call")],
        // Nothing pinned. A drive that pinned a sampler setting the regimen
        // declares would be pinning it twice -- once in the regime it records
        // and once on the wire -- and the two could disagree. Pinning from
        // the regimen is a decision the client already has the machinery for
        // and this program does not make on its own.
        sampler: SamplerCard::empty(),
        limits: Limits {
            attempt: std::time::Duration::from_secs(60),
            call: std::time::Duration::from_secs(180),
            max_output_tokens: 512,
            retries: 1,
        },
        grammar: None,
    }
}
