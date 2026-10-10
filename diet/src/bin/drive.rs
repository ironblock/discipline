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

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::net::{IpAddr, Ipv4Addr};
use std::process::ExitCode;

use diet::client::Client;
use diet::client::shape::{
    Concurrency, Dialect, Limits, Message, RequestShape, Role, SamplerCard, Serving,
};
use diet::client::stream::{Bearer, HttpStream};
use diet::client::stub::Stub;
use diet::client::transport::{Endpoint, Http};
use diet::drive::attach::Attaching;
use diet::drive::regimen::{SUBSTRATE_KEYS, regime_of};
use diet::drive::serve::{Config, Credential, Server};
use diet::drive::session::{self, Interview, Session};
use diet::drive::tool_loop::{self, Tools};
use diet::drive::{Gym, Halt, canned, run};
use diet::formats::record::Regime;
use diet::formats::record::json::{self, Value};
use diet::formats::regimen;
use diet::isolation::{self, Policy as IsolationPolicy};
use diet::seam::policy::Policy as SeamPolicy;

/// Exit code for a usage error, kept distinct from a drive that did not run.
const EXIT_USAGE: u8 = 2;
/// Exit code for a drive that halted.
///
/// Taken from [`Halt::EXIT`] rather than written again here: a session that
/// could not start is not a session that failed, and the two files used to
/// spell the same number twice with a comment asserting they matched.
const EXIT_HALT: u8 = Halt::EXIT;
/// Exit code for a regimen that is not one, or does not carry what a regime
/// needs.
const EXIT_INPUT: u8 = 1;
/// Exit code for a record that could not be written where it was asked for.
///
/// Its own code, and not [`EXIT_HALT`]: a drive that ran and could not file
/// its record is not a drive that could not start, and the two collapsed into
/// one number would be counted together.
const EXIT_OUTPUT: u8 = 3;

/// The subcommand that serves an interactive session (#117 R2c, I5).
const SERVE: &str = "serve";

/// The stream-replay server's subcommand (#411).
const REPLAY: &str = "replay";

/// The port `diet-drive replay` listens on unless told another.
const REPLAY_PORT: u16 = 7902;

fn serve_usage() -> String {
    let mut out = String::from(
        "usage: diet-drive serve --endpoint URL --model NAME --head FILE [--key-file FILE]\n\
         \x20                       [--listen IP] [--port N] [--auth-file FILE] [--regimen FILE]\n\
         \x20                       [--log FILE] [--record FILE]\n\
         \x20                       [--allow-origin URL]... [--max-output-tokens N]\n\
         \x20                       [--worktree DIR]\n\n",
    );
    out.push_str("Serves one interactive session over HTTP + SSE on 127.0.0.1, or on\n");
    out.push_str("--listen's address:\n");
    out.push_str("GET /events streams the session's log, POST /commands takes its\n");
    out.push_str("commands. <FILE> is the trunk's system message. The first line on\n");
    out.push_str("stdout is JSON naming the address it listens on and when it opened.\n");
    out.push_str("--allow-origin admits a page's origin (a development proxy's).\n");
    out.push_str("--key-file names a file holding the endpoint's key, sent as a bearer\n");
    out.push_str("credential; it is never taken as an argument.\n");
    out.push_str("--auth-file names a file holding user:password; every request must then\n");
    out.push_str("present it as Basic auth. --listen off loopback refuses to start without\n");
    out.push_str("it, and a wildcard (0.0.0.0, ::) is refused: name one interface.\n");
    out.push_str("--record FILE (needs --regimen) writes the session's record there once it\n");
    out.push_str("ends, projected from its log, beside FILE.unspellable.json naming what\n");
    out.push_str("the record could not spell, and FILE.product.txt, the working memory at\n");
    out.push_str("the end, whose sha256 is the summary's product_sha256; all three digests\n");
    out.push_str("are reported on stdout.\n");
    out.push_str("--max-output-tokens beats the regimen's `max_output_tokens`, which beats the\n");
    out.push_str(
        "default, 64000 (the harness vote, #569); a turn that hits it is logged capped.\n",
    );
    out.push_str("The regimen's top-level `max_steps` beats its `[limits] max_steps`.\n");
    out.push_str("--log FILE writes the session's log there as each line is appended, the\n");
    out.push_str("same lines GET /events streams; nothing is written without it.\n");
    out.push_str("--regimen names the regimen the session runs under; its substrate is\n");
    out.push_str("resolved from the registry, and an unregistered one refuses to start.\n");
    out.push_str("The server's GET /props build_info must be the registry's engine_build_info\n");
    out.push_str("for the substrate exactly, or else name its engine_commit, or serve refuses\n");
    out.push_str("to start. A canned substrate's literal is canned-<acts sha256>, which only\n");
    out.push_str("this crate's own canned server reports.\n");
    out.push_str("A substrate whose entry declares engine_check = \"declared\" (an engine that\n");
    out.push_str("reports no build, TabbyAPI's) is not asked: its engine is the declared one,\n");
    out.push_str("and the client speaks the dialect its entry names (llama.cpp by default).\n");
    out.push_str("--endpoint given as a base URL (no path, or /v1) is completed to its\n");
    out.push_str("/v1/chat/completions; any other path is used as written.\n");
    out.push_str("--help prints this to stdout and exits 0; a usage error exits 2.\n");
    out.push_str("--worktree DIR, absolute, is where the model's commands run; a regimen\n");
    out.push_str("that runs commands (it declares `allowed_commands`, the pre-seeded set) needs\n");
    out.push_str("it. Each command runs under the regimen's confinement, opened at start;\n");
    out.push_str("`isolation = \"vm\"` is refused, and `isolation = \"none\"` needs\n");
    out.push_str("`max_steps`, and --auth-file is required: every route then asks for\n");
    out.push_str("it, and the file joins the session's secrets: under a sandbox no command\n");
    out.push_str("reads it, so none answers its own prompt. Under `isolation = \"none\"` a\n");
    out.push_str("command can read it, and nothing stops that (#436). A command no approval\n");
    out.push_str("covers waits on the operator:\n");
    out.push_str("GET /events shows it as `event: waiting` {request, id, command, cwd,\n");
    out.push_str("reason, segments}, then `event: answered` {request, id}; POST /approve\n");
    out.push_str("answers it with {\"call\": ID, \"scope\": \"once\"|\"session\"|\"workspace\"|\n");
    out.push_str("\"decline\"}: 204, or 409 {\"refused\": \"nothing-waiting\"|\"stale\"|...}.\n");
    out.push_str("After the session the receipt is written beside the record (or the log),\n");
    out.push_str("as FILE.receipt.json.\n");
    out
}

/// What `serve`'s flags say.
struct ServeArgs {
    endpoint: String,
    model: String,
    head: String,
    key_file: Option<String>,
    auth_file: Option<String>,
    regimen_file: Option<String>,
    log_file: Option<String>,
    record_file: Option<String>,
    listen: IpAddr,
    port: u16,
    allowed_origins: Vec<String>,
    /// `--max-output-tokens`, when given: it beats the regimen's (#569).
    max_output_tokens: Option<u32>,
    worktree: Option<String>,
}

/// Whether `args` asks for the usage: `--help` or `-h` where a flag goes.
///
/// Asked for, the usage is an answer, printed to stdout with exit 0; only a
/// usage that is wrong exits 2 (#219 item 8). A flag's value is never read as
/// the question, so `--model --help` is a model named `--help`. Pairs, as
/// `serve_args` reads them: every `serve` flag takes a value, and a flag
/// that took none would need this and `serve_args` to change together.
fn asks_for_help(args: &[String]) -> bool {
    args.iter()
        .step_by(2)
        .any(|flag| flag == "--help" || flag == "-h")
}

/// `serve`'s flags, or nothing when they are not a usage it takes.
fn serve_args(args: &[String]) -> Option<ServeArgs> {
    let mut endpoint = None;
    let mut model = None;
    let mut head = None;
    let mut key_file = None;
    let mut auth_file = None;
    let mut regimen_file = None;
    let mut log_file = None;
    let mut record_file = None;
    let mut listen = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let mut port: u16 = 0;
    let mut allowed_origins = Vec::new();
    let mut max_output_tokens = None;
    let mut worktree = None;
    let mut given = args.iter();
    while let Some(flag) = given.next() {
        let value = given.next()?;
        let parsed = if flag == "--endpoint" {
            endpoint = Some(value.clone());
            true
        } else if flag == "--model" {
            model = Some(value.clone());
            true
        } else if flag == "--head" {
            head = Some(value.clone());
            true
        } else if flag == "--key-file" {
            key_file = Some(value.clone());
            true
        } else if flag == "--auth-file" {
            auth_file = Some(value.clone());
            true
        } else if flag == "--regimen" {
            regimen_file = Some(value.clone());
            true
        } else if flag == "--log" {
            log_file = Some(value.clone());
            true
        } else if flag == "--record" {
            record_file = Some(value.clone());
            true
        } else if flag == "--listen" {
            value.parse().map(|given| listen = given).is_ok()
        } else if flag == "--allow-origin" {
            let origin = is_an_origin(value);
            if origin {
                allowed_origins.push(value.clone());
            }
            origin
        } else if flag == "--port" {
            value.parse().map(|given| port = given).is_ok()
        } else if flag == "--max-output-tokens" {
            value
                .parse()
                .map(|given| max_output_tokens = Some(given))
                .is_ok()
        } else if flag == "--worktree" {
            // Absolute, as the gym's is resolved before anything runs: a
            // relative one would be resolved again inside a sandbox.
            worktree = Some(value.clone());
            std::path::Path::new(value).is_absolute()
        } else {
            false
        };
        if !parsed {
            return None;
        }
    }
    Some(ServeArgs {
        endpoint: endpoint?,
        model: model?,
        head: head?,
        key_file,
        auth_file,
        regimen_file,
        log_file,
        record_file,
        listen,
        port,
        allowed_origins,
        max_output_tokens,
        worktree,
    })
}

/// `diet-drive serve`: one interactive session, over HTTP + SSE, on loopback
/// unless `--listen` names another address.
///
/// Fail-closed (#117 R2c, I7): off loopback it will not start without a
/// credential, so nothing here binds a session that runs the model beyond the
/// machine it is on for anyone who can reach it. A wildcard is refused: D17's
/// `Host` check answers to the bound address, and a wildcard has none.
#[allow(clippy::too_many_lines)]
fn serve(args: &[String]) -> ExitCode {
    let Some(ServeArgs {
        endpoint,
        model,
        head,
        key_file,
        auth_file,
        regimen_file,
        log_file,
        record_file,
        listen,
        port,
        allowed_origins,
        max_output_tokens,
        worktree,
    }) = serve_args(args)
    else {
        eprint!("{}", serve_usage());
        return ExitCode::from(EXIT_USAGE);
    };
    let endpoint = match Endpoint::parse(&endpoint) {
        Ok(endpoint) => chat_endpoint(endpoint),
        Err(why) => return fail(EXIT_INPUT, &format!("{endpoint} is not an endpoint: {why}")),
    };
    let system = match std::fs::read_to_string(&head) {
        Ok(system) => system,
        Err(why) => return fail(EXIT_INPUT, &format!("{head} cannot be read: {why}")),
    };
    let (regime, mut read_at_start) = match regimen_file
        .as_deref()
        .map(|path| registered_regime(path, max_output_tokens))
        .transpose()
    {
        Ok(read) => read.unzip(),
        Err(why) => return fail(EXIT_INPUT, &why),
    };
    // The flag, else the regimen's, else the default (#569).
    let (max_output_tokens, _) = read_at_start.as_ref().map_or_else(
        || diet::drive::regimen::output_cap(max_output_tokens, None),
        |read| read.output_cap,
    );
    // The regimen's `[sampler]`, pinned on every request (#486): derived from
    // the regime's own `sampler_card`, so the record's claim and the wire's
    // pins are one value. Without a regimen, nothing is pinned, as before.
    let sampler = match regime
        .as_ref()
        .map(|regime| diet::drive::regimen::sampler_pins(&regime.substrates[0].sampler_card))
        .transpose()
    {
        Ok(pins) => pins.unwrap_or_default(),
        Err(why) => {
            let path = regimen_file.as_deref().unwrap_or_default();
            return fail(EXIT_USAGE, &format!("{path}: {why}"));
        }
    };
    // The instruction files the worktree and its parents carry (#559), as
    // the harnesses we compare against put them in the system prompt: on
    // unless the regimen says `instruction_files = "off"`, and only where
    // commands run, since that is the worktree the model works in.
    let instructions = match (worktree.as_deref(), regimen_file.as_deref()) {
        (Some(tree), Some(path)) => {
            let on = std::fs::read_to_string(path)
                .ok()
                .and_then(|text| regimen::parse(&text).ok())
                .is_none_or(|read| diet::drive::instructions::enabled(&read));
            if on {
                let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
                diet::drive::instructions::discover(std::path::Path::new(tree), home.as_deref())
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    };
    let system = diet::drive::instructions::into_system(&system, &instructions);
    let instruction_files: Vec<_> = instructions
        .iter()
        .filter(|file| !file.content.trim().is_empty())
        .map(|file| diet::formats::log::InstructionFile {
            path: file.path.clone(),
            sha256: file.sha256.clone(),
        })
        .collect();
    let mut shape = trunk(model, system, max_output_tokens, sampler);
    // The regime's reasoning state, on every request, the forks' included
    // (R1): a clone of the trunk carries it.
    // A budget no template variable carries is recorded and announced as
    // unsent, never refused for being missing (duty of care).
    // The substrate's model convention, as the registry declares it, goes
    // with them: `preserve_thinking`, sent rather than left to a server's
    // default, and the template's default level named when none is sent.
    let mut unsent_budget = None;
    let mut effort_default = None;
    if let Some(regime) = regime.as_ref() {
        let identity = diet::drive::registry::identity(
            diet::drive::registry::REGISTRY,
            &regime.substrates[0].id,
        )
        .ok();
        match diet::drive::regimen::template_kwargs_declared(
            &regime.substrates[0],
            identity.as_ref(),
        ) {
            Ok(wire) => {
                shape.template_kwargs = wire.kwargs;
                unsent_budget = wire.unsent_budget;
                effort_default = wire.reasoning_effort_default;
            }
            Err(why) => {
                let path = regimen_file.as_deref().unwrap_or_default();
                return fail(EXIT_USAGE, &format!("{path}: {why}"));
            }
        }
    }
    if record_file.is_some() && regime.is_none() {
        return fail(EXIT_USAGE, RECORD_NEEDS_A_REGIMEN);
    }
    // What the capture gap forks under (#374), when the regimen warrants
    // forks: its rules, and a working object under its regime.
    let interview = match serving_interview(regimen_file.as_deref(), regime.as_ref()) {
        Ok(interview) => interview,
        Err(why) => return fail(EXIT_INPUT, &why),
    };
    // What the model's calls run under, opened now: a confinement that
    // cannot open refuses before any request is made or anything binds
    // (#298 point 8).
    let tools = match serving_tools(
        regimen_file.as_deref(),
        worktree.as_deref(),
        (key_file.as_deref(), auth_file.as_deref()),
    ) {
        Ok(tools) => tools,
        Err((code, why)) => return fail(code, &why),
    };
    // A prune has nothing to prune in a session that runs no tools (#612).
    if tools.is_none() && interview.as_ref().is_some_and(|i| i.prune.is_some()) {
        return fail(
            EXIT_INPUT,
            "`model_pruning = \"on\"` needs a session that runs commands: it prunes tool \
             results, and this one makes no tool calls (give it `--worktree` and \
             `allowed_commands`)",
        );
    }
    // What an ask's named PNGs are checked against and copied to (#372):
    // the policy the commands run under, or the regimen's when it runs
    // none, and the recording's directory.
    let attaching = match attaching(
        regimen_file.as_deref(),
        tools.as_ref(),
        (key_file.as_deref(), auth_file.as_deref()),
        log_file.as_deref().or(record_file.as_deref()),
    ) {
        Ok(attaching) => attaching,
        Err(why) => return fail(EXIT_INPUT, &why),
    };
    // Where a capped tool output is kept whole (#554): the recording's
    // directory, absolute, since the pointer is read from the worktree.
    let mut tools = tools;
    if let Some(tools) = tools.as_mut() {
        tools.recording = attaching
            .recording
            .as_ref()
            .map(|dir| std::fs::canonicalize(dir).unwrap_or_else(|_| dir.clone()));
    }
    let credential = match auth_file.as_deref().map(credential_from).transpose() {
        Ok(credential) => credential,
        Err(why) => return fail(EXIT_INPUT, &why),
    };
    let listen = match listen_on(listen, credential.is_some()) {
        Ok(listen) => listen,
        Err(refused) => return refused,
    };
    let mut transport = HttpStream::new(endpoint);
    if let Some(key_file) = key_file {
        match key_file_in_declared_path(regimen_file.as_deref(), &key_file)
            .map_or_else(|| bearer_from(&key_file), Err)
        {
            Ok(bearer) => transport = transport.with_bearer(bearer),
            Err(why) => return fail(EXIT_INPUT, &why),
        }
    }
    // The engine check, before anything binds: the server must run the
    // engine the registry pins for the regimen's substrate (#157).
    let substrate = regime
        .as_ref()
        .map(|regime| regime.substrates[0].id.as_str());
    let engine = match substrate
        .map(|id| diet::drive::engine::check_served(&transport, id))
        .transpose()
    {
        Ok(build) => build,
        Err(why) => return fail(EXIT_INPUT, &why),
    };
    // The dialect the substrate's server speaks, as the registry names it
    // (#496); llama.cpp's without a regimen, or where the entry names none.
    let dialect = match substrate.map(served_dialect).transpose() {
        Ok(dialect) => dialect.unwrap_or_else(Dialect::llama_cpp),
        Err(why) => return fail(EXIT_INPUT, &why),
    };
    // What the announcement prints and the log's `session.start` claims,
    // built once from the same values (#292): the substrate, the registry's
    // digest, and the engine the check passed.
    // The rest of what the start confirms (#509): the right model, the
    // settings, warmed -- each declared field corroborated or declared, a
    // contradiction refused before anything binds.
    let confirmed = match substrate
        .zip(engine.as_ref())
        .map(|(id, passed)| confirmations(&transport, id, passed, &shape))
        .transpose()
    {
        Ok(confirmed) => confirmed,
        Err(why) => return fail(EXIT_INPUT, &why),
    };
    // Each request's output cap is clamped to the room its prompt leaves in
    // this window (#588); the record's lever says whether it was.
    let window = confirmed.as_ref().and_then(|(_, _, window)| *window);
    shape.limits.context_window = window.map(|(tokens, _)| tokens);
    if let Some(levers) = read_at_start.as_mut().map(|read| &mut read.levers) {
        levers
            .entry("step_and_output_limits".to_owned())
            .and_modify(|word| match window {
                Some((tokens, from)) => {
                    let _ = write!(word, ":clamped:{tokens}:{from}");
                }
                None => word.push_str(":unclamped"),
            });
    }
    let registry_sha256 = diet::drive::registry::registry_sha256();
    let claim = substrate.zip(engine.as_ref()).map(|(id, passed)| {
        let fields = confirmed
            .as_ref()
            .map(|(fields, _, _)| fields.clone())
            .unwrap_or_default();
        passed.claim_with(id, &registry_sha256, fields)
    });
    let log_path = log_file;
    let (log_file, record) = match outputs(log_path.as_deref(), record_file.as_deref()) {
        Ok(opened) => opened,
        Err(why) => return fail(EXIT_OUTPUT, &why),
    };
    let listener = match listener(listen, port) {
        Ok(listener) => listener,
        Err(refused) => return refused,
    };
    let session = served_session(
        transport,
        (shape, dialect),
        tools,
        (
            claim,
            interview,
            (
                unsent_budget.map(|budget_tokens| diet::formats::log::Unsent { budget_tokens }),
                effort_default.clone(),
                instruction_files,
                // #573: the start row's levers, from the same reading.
                read_at_start.as_ref().map(|read| read.levers.clone()),
            ),
        ),
    );
    let opened = session.opened();
    let watching = std::sync::Arc::clone(&session);
    // Where the projection reads an attached file back from (#372): the
    // directory its copy was kept in.
    let recording = attaching.recording.clone();
    let config = Config {
        attaching,
        allowed_origins,
        credential,
        ..Config::default()
    };
    let running = match started(
        session,
        listener,
        config,
        log_path.as_deref().zip(log_file),
        record.zip(regime.clone()),
    ) {
        Ok(started) => Running {
            recording,
            read_at_start,
            ..started
        },
        Err(code) => return code,
    };
    println!(
        "{}",
        announcement(
            &running.server.addr().to_string(),
            opened,
            substrate.map(|id| (id, registry_sha256.as_str())),
            engine
                .as_ref()
                .zip(confirmed.as_ref().map(|(_, warmed, _)| *warmed)),
            log_path.as_deref().zip(running.log_held),
            record_file.as_deref().zip(running.record_held),
            unsent_budget,
        )
    );
    ended(&watching, running)
}

/// Serve until the session ends, then let the writers finish, close the
/// listener, give open streams a moment to deliver `ended`, and exit 0 --
/// so `ended` is the log's last line and a recipe need not interrupt (#291).
fn ended(session: &Session<HttpStream>, running: Running) -> ExitCode {
    let mut next = 0;
    while session.settlement() != diet::drive::session::Settlement::Ended {
        next += session
            .wait_from(next, std::time::Duration::from_secs(60))
            .len() as u64;
    }
    // The log was written as each line was appended; the record is written
    // here, once, after `ended` and before the server stops -- in order, on
    // this thread, so nothing races the exit (#315's second review).
    if let Some((regime, (path, file))) = running.record {
        match written_record(
            session,
            (&regime, running.read_at_start.as_ref()),
            (&path, file),
            running.recording.as_deref(),
        ) {
            Ok(report) => {
                let _ = std::io::Write::write_all(
                    &mut std::io::stdout().lock(),
                    format!("{report}\n").as_bytes(),
                );
            }
            Err(why) => {
                return fail(
                    EXIT_OUTPUT,
                    &format!("the record could not be written: {why}"),
                );
            }
        }
    }
    // The receipt (#298 point 9), beside the record, or the log: what the
    // session's commands ran under and what was approved.
    if let Some(receipt) = session.receipt()
        && let Some(path) = &running.receipt
    {
        match written_receipt(&receipt, path) {
            Ok(report) => {
                let _ = std::io::Write::write_all(
                    &mut std::io::stdout().lock(),
                    format!("{report}\n").as_bytes(),
                );
            }
            Err(why) => {
                return fail(
                    EXIT_OUTPUT,
                    &format!("the receipt could not be written: {why}"),
                );
            }
        }
    }
    session.end_commands();
    running.server.finish(std::time::Duration::from_secs(5));
    ExitCode::SUCCESS
}

/// Write `receipt` to `path`, and the line `serve` reports for it.
fn written_receipt(receipt: &Value, path: &str) -> Result<String, String> {
    let mut text = String::new();
    json::render(receipt, &mut text);
    text.push('\n');
    std::fs::write(path, &text).map_err(|why| format!("{path}: {why}"))?;
    let mut out = String::new();
    json::render(
        &Value::Object(BTreeMap::from([
            ("receipt".to_owned(), Value::String(path.to_owned())),
            (
                "receipt_sha256".to_owned(),
                Value::String(diet::digest::sha256_hex(text.as_bytes())),
            ),
        ])),
        &mut out,
    );
    Ok(out)
}

/// The file at `path`, opened for writing (created if absent) but NOT yet
/// emptied, and whether it holds anything. It is emptied only once the
/// server's address is bound, so a start refused there leaves an operator's
/// file as it was.
fn created(path: &str) -> Result<(std::fs::File, bool), String> {
    let truncated = std::fs::metadata(path).is_ok_and(|held| held.len() > 0);
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map(|file| (file, truncated))
        .map_err(|why| format!("{path} cannot be written: {why}"))
}

/// The session's log, written to `file` as each line is appended (`--log`,
/// #157), by the appending thread itself: a line is in the file before its
/// append returns, so a kill tears at most the line being written (#230,
/// ruled: no signal handler). Unbuffered, one write per line. A write that
/// fails stops the process: a log that quietly stopped would be a shorter
/// log claiming to be the session's.
fn keep_log(
    session: &Session<HttpStream>,
    render: diet::drive::serve::Render,
    file: std::fs::File,
) {
    diet::drive::serve::write_through(session, render, file, |why| {
        let _ = fail(EXIT_OUTPUT, &format!("the log could not be written: {why}"));
        std::process::exit(i32::from(EXIT_OUTPUT));
    });
}

/// Why `--record` without `--regimen` is a usage refusal.
const RECORD_NEEDS_A_REGIMEN: &str =
    "--record needs --regimen: a record's `start` names the regime it ran under";

/// The files `--log` and `--record` name, opened before anything binds and
/// emptied only later, each with whether it held anything; the record's with
/// its path.
#[allow(clippy::type_complexity)]
fn outputs(
    log: Option<&str>,
    record: Option<&str>,
) -> Result<
    (
        Option<(std::fs::File, bool)>,
        Option<((String, std::fs::File), bool)>,
    ),
    String,
> {
    let log = log.map(created).transpose()?;
    let record = match record {
        Some(path) => {
            let (file, held) = created(path)?;
            Some(((path.to_owned(), file), held))
        }
        None => None,
    };
    Ok((log, record))
}

/// `serve`'s session, declaring what serves it in the log's `session.start`
/// (#292): its transport speaks llama-server's dialect, and nobody declared
/// how many streams the server serves.
fn served_session(
    transport: HttpStream,
    (mut shape, dialect): (RequestShape, Dialect),
    tools: Option<Tools>,
    declared: session::Declared,
) -> std::sync::Arc<Session<HttpStream>> {
    // A session that runs commands declares its surface's tools (#557):
    // `bash` alone, or `bash` and the standard set; then `prune_output`
    // when the regimen offers it (#612).
    if let Some(tools) = tools.as_ref() {
        shape.tools = tools.surface.tools();
        if declared
            .1
            .as_ref()
            .is_some_and(|interview| interview.prune.is_some())
        {
            shape.tools.push(diet::drive::prune::definition());
        }
    }
    // Self-capture's tools after them (#609), from the first request and
    // never changed; with self-capture off the tools are as they were.
    session::declare_self_capture(&mut shape, declared.1.as_ref());
    std::sync::Arc::new(Session::open_declaring(
        transport,
        shape,
        Some(Serving {
            concurrency: Concurrency::Undeclared,
            dialect,
        }),
        tools,
        declared,
    ))
}

/// The dialect the registry names for substrate `id` (#496): llama.cpp's
/// where its entry names none.
///
/// # Errors
///
/// When the entry cannot be read, or names a dialect this client does not
/// speak.
fn served_dialect(id: &str) -> Result<Dialect, String> {
    let identity = diet::drive::registry::identity(diet::drive::registry::REGISTRY, id)?;
    match identity.dialect.as_deref() {
        None => Ok(Dialect::llama_cpp()),
        Some(name) => Dialect::named(name).ok_or_else(|| {
            format!(
                "the registry names `{name}` as `{id}`'s dialect, which this client does not \
                 speak: `llama.cpp` or `tabbyapi`"
            )
        }),
    }
}

/// What `serve`'s capture gap forks under (#374): the rules the regimen at
/// `regimen` lists under `interview_warrant`, a working object under
/// `regime`, and the cadence and budget its derived seams fire on, the
/// budget taken of the substrate's registered `serving_context`. `None` when
/// it lists no rule, or there is no regimen: then no fork and no derived
/// seam ever fires, and a regimen that declares a seam trigger anyway is
/// refused.
fn serving_interview(
    regimen: Option<&str>,
    regime: Option<&Regime>,
) -> Result<Option<Interview>, String> {
    let (Some(path), Some(regime)) = (regimen, regime) else {
        return Ok(None);
    };
    let text =
        std::fs::read_to_string(path).map_err(|why| format!("{path} cannot be read: {why}"))?;
    let read = regimen::parse(&text).map_err(|why| format!("{path} is not a regimen: {why:?}"))?;
    let rules = session::interview_warrant(&read).map_err(|why| format!("{path}: {why}"))?;
    let window = diet::drive::registry::serving_context(
        diet::drive::registry::REGISTRY,
        &regime.substrates[0].id,
    );
    let seams = diet::seam::policy::Served::from_regimen(&read, window)
        .map_err(|why| format!("{path}: {why}"))?;
    let delivery = session::fork_delivery(&read).map_err(|why| format!("{path}: {why}"))?;
    // The phase graph, by the scripted drive's own reader (#563).
    let phases = diet::seam::policy::phase_graph(&read).map_err(|why| format!("{path}: {why}"))?;
    if rules.is_empty() && !phases.is_empty() {
        return Err(format!(
            "{path} declares phases and no `interview_warrant`: nothing fills working \
             memory, so no seam could ever move between them"
        ));
    }
    if rules.is_empty() && seams.declares_a_trigger() {
        return Err(format!(
            "{path} declares a seam trigger and no `interview_warrant`: nothing fills \
             working memory, so no seam could ever fire"
        ));
    }
    // Self-capture (#609) keeps working memory too, forks or none.
    let self_capture = session::self_capture(&read);
    // #610: a fork answers through the capture tools only where it is asked
    // to, in a set written for them, and where self-capture declares them.
    let capture = session::capture_modality(&read).map_err(|why| format!("{path}: {why}"))?;
    let asks = session::fork_asks(&read);
    if rules.is_empty() && self_capture.is_none() && diet::drive::prune::of(&read).is_some() {
        return Err(format!(
            "{path} declares `model_pruning = \"on\"` and neither an `interview_warrant` nor \
             self-capture: nothing fills working memory, so no seam could ever replace a \
             pruned result"
        ));
    }
    if capture == diet::dogma::asks::Modality::Tools {
        let refused = if rules.is_empty() {
            Some("no `interview_warrant`, so no fork ever answers".to_owned())
        } else if self_capture.is_none() {
            Some("self-capture off, so no fork is offered the capture tools".to_owned())
        } else if asks.modality != capture {
            Some(format!(
                "the ask set `{}`, whose asks have a fork answer in fields",
                asks.name
            ))
        } else {
            None
        };
        if let Some(why) = refused {
            return Err(format!(
                "{path} declares `{}` = \"tools\" with {why}",
                session::CAPTURE_MODALITY
            ));
        }
    }
    Ok(
        (!rules.is_empty() || self_capture.is_some()).then(|| Interview {
            rules,
            object: diet::object::WorkingObject::open(regime.clone()),
            seams,
            delivery,
            phases,
            // #566: how archived items are recalled; off unless declared.
            recall: diet::drive::archive::Recall::of(&read),
            view: session::fork_view(&read),
            self_capture,
            asks,
            capture,
            // #612: whether the model may prune its tool results; off unless
            // declared.
            prune: diet::drive::prune::of(&read),
        }),
    )
}

/// What `serve` runs the model's calls under, when the regimen at
/// `regimen` runs commands -- it declares `allowed_commands`, the pre-seeded
/// session set (#298 point 8). Everything is checked and opened before any
/// request is made: `isolation = "vm"` is refused; a regimen that runs
/// commands needs `--worktree`, and unconfined it needs `[limits]
/// max_steps`; the drive's key file joins the policy's secrets
/// (`declared::add_credential`, #299); the confinement is opened, and the
/// workspace approvals store, outside every writable path. The operator
/// answers prompts.
#[allow(clippy::too_many_lines)]
fn serving_tools(
    regimen: Option<&str>,
    worktree: Option<&str>,
    (key_file, auth_file): (Option<&str>, Option<&str>),
) -> Result<Option<Tools>, (u8, String)> {
    let no_commands = |path: &str| {
        Err((
            EXIT_USAGE,
            format!(
                "--worktree names where a regimen's commands run, and {path} runs none (it declares no `{}`)",
                tool_loop::ALLOWED_COMMANDS
            ),
        ))
    };
    let Some(path) = regimen else {
        return match worktree {
            Some(_) => no_commands("no --regimen"),
            None => Ok(None),
        };
    };
    let text = std::fs::read_to_string(path)
        .map_err(|why| (EXIT_INPUT, format!("{path} cannot be read: {why}")))?;
    let read = regimen::parse(&text)
        .map_err(|why| (EXIT_INPUT, format!("{path} is not a regimen: {why:?}")))?;
    let mut policy = IsolationPolicy::from_regimen(&read)
        .map_err(|why| (EXIT_INPUT, format!("{path}: its isolation policy: {why}")))?;
    if policy.isolation == isolation::Isolation::Vm {
        return Err((
            EXIT_HALT,
            format!(
                "{path} declares `isolation = \"vm\"`, which serve does not run: refusing to \
                 start rather than running under something weaker than the record would say"
            ),
        ));
    }
    let Some(declared) =
        tool_loop::declared(&read).map_err(|why| (EXIT_INPUT, format!("{path}: {why}")))?
    else {
        return match worktree {
            Some(_) => no_commands(path),
            None => Ok(None),
        };
    };
    let Some(worktree) = worktree.map(std::path::PathBuf::from) else {
        return Err((
            EXIT_USAGE,
            format!(
                "{path} runs commands (it declares `{}`), and serve needs --worktree DIR, \
                 absolute, for them to run in",
                tool_loop::ALLOWED_COMMANDS
            ),
        ));
    };
    if !worktree.is_dir() {
        return Err((
            EXIT_INPUT,
            format!(
                "--worktree {} is not a directory that exists",
                worktree.display()
            ),
        ));
    }
    if policy.isolation == isolation::Isolation::None && declared.max_steps.is_none() {
        return Err((
            EXIT_INPUT,
            format!(
                "{path} runs commands unconfined (`isolation = \"none\"`) with no `{}` (or \
                 `[{}] {}`): nothing would bound the loop",
                tool_loop::MAX_STEPS,
                tool_loop::LIMITS,
                tool_loop::MAX_STEPS
            ),
        ));
    }
    // A confined command reaches `serve` on loopback whenever its network
    // does, and could answer its own prompts: so the session demands a
    // credential on every route, and the credential is a secret no command
    // reads (#298 review round 1, finding 1).
    let Some(auth_file) = auth_file else {
        return Err((
            EXIT_USAGE,
            format!(
                "{path} runs commands, and serve needs --auth-file FILE for them: a command \
                 can reach this server, and without a credential it could answer its own \
                 prompts"
            ),
        ));
    };
    isolation::declared::add_credential(&mut policy, std::path::Path::new(auth_file)).map_err(
        |path| {
            (
                EXIT_INPUT,
                format!(
                    "--auth-file {auth_file} lies in `{}`, which the regimen declares readable \
                     or writable: a command could read the credential and answer its own prompts",
                    path.display()
                ),
            )
        },
    )?;
    if let Some(key_file) = key_file {
        isolation::declared::add_credential(&mut policy, std::path::Path::new(key_file)).map_err(
            |path| {
                (
                    EXIT_INPUT,
                    format!(
                        "--key-file {key_file} lies in `{}`, which the regimen declares readable \
                         or writable: a command could read the key",
                        path.display()
                    ),
                )
            },
        )?;
    }
    let confinement = isolation::open(&policy).map_err(|why| (EXIT_HALT, why.to_string()))?;
    let state = tool_loop::state_home().ok_or_else(|| {
        (
            EXIT_HALT,
            "no HOME and no XDG_STATE_HOME: nowhere outside the worktree to keep workspace \
             approvals"
                .to_owned(),
        )
    })?;
    let (store, stored) =
        tool_loop::Store::open(&state, &worktree, &policy).map_err(|why| (EXIT_HALT, why))?;
    let gate = tool_loop::Gate::standard(&worktree).passing(&policy.environment);
    let mut allowed = tool_loop::preseeded(&declared.allowed_commands, &gate)
        .map_err(|why| (EXIT_INPUT, format!("{path}: {why}")))?;
    allowed.extend(stored);
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    Ok(Some(Tools {
        confinement,
        policy,
        cwd: tool_loop::cwd_label(&worktree, home.as_deref()),
        worktree,
        max_steps: declared.max_steps,
        decider: tool_loop::Decider::Operator,
        gate,
        allowed,
        store: Some(store),
        approval_policy: declared.approval_policy,
        approvals_off: declared.approvals_off,
        text_fallback: declared.text_fallback,
        output_cap: declared.output_cap,
        // Set once the recording's directory is known (`serve`).
        recording: None,
        // The read tool the surface offers, which a capped output's notice
        // names (#554, #557).
        read_tool: declared.surface.read_tool(),
        surface: declared.surface,
    }))
}

/// What `serve` checks an ask's named PNGs against, and copies them to
/// (#372): the policy, worktree and confinement the model's commands run
/// under, when the regimen runs commands -- the confinement then reads each
/// file, so the kernel judges the open; otherwise the regimen's own policy,
/// read directly (no command runs to swap a path), with the
/// drive's key and auth files among its secrets, and no worktree; with no
/// regimen, no read scope, so nothing attaches. The copies go beside
/// `recorded_at`, the log (or, with none, the record).
fn attaching(
    regimen: Option<&str>,
    tools: Option<&Tools>,
    credentials: (Option<&str>, Option<&str>),
    recorded_at: Option<&str>,
) -> Result<Attaching, String> {
    let recording = recorded_at.map(|path| {
        std::path::Path::new(path)
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .map_or_else(
                || std::path::PathBuf::from("."),
                std::path::Path::to_path_buf,
            )
    });
    if let Some(tools) = tools {
        return Ok(Attaching {
            policy: Some(tools.policy.clone()),
            worktree: Some(tools.worktree.clone()),
            confinement: Some(tools.confinement.clone()),
            recording,
        });
    }
    let Some(path) = regimen else {
        return Ok(Attaching {
            recording,
            ..Attaching::default()
        });
    };
    let text =
        std::fs::read_to_string(path).map_err(|why| format!("{path} cannot be read: {why}"))?;
    let read = regimen::parse(&text).map_err(|why| format!("{path} is not a regimen: {why:?}"))?;
    let mut policy = IsolationPolicy::from_regimen(&read)
        .map_err(|why| format!("{path}: its isolation policy: {why}"))?;
    let (key_file, auth_file) = credentials;
    for file in [key_file, auth_file].into_iter().flatten() {
        let file = std::path::Path::new(file);
        // A regimen that runs no commands may declare the file's directory
        // readable; it is still never attached.
        if isolation::declared::add_credential(&mut policy, file).is_err() {
            policy.secrets.push(
                isolation::policy::canonical_prefix(file)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    Ok(Attaching {
        policy: Some(policy),
        worktree: None,
        confinement: None,
        recording,
    })
}

/// The writers, then the server. The writers start once the address is
/// bound and before the server does: no command can append a line before
/// the log's sink is in place (#264's review). Any refusal before this --
/// the engine check, the bind -- leaves an existing log, record and sidecar
/// as they were; the bind's refusal, coming after the outputs are opened,
/// creates an absent one empty. A failure from here on
/// names what it had already emptied (#264, ruled (i)): the writers empty
/// the record, then its sidecar, then the log, and the server starts last.
/// One render for the stream and the log, so they are one text.
#[allow(clippy::type_complexity)]
fn started(
    session: std::sync::Arc<Session<HttpStream>>,
    listener: std::net::TcpListener,
    config: Config,
    log: Option<(&str, (std::fs::File, bool))>,
    record: Option<(
        ((String, std::fs::File), bool),
        diet::formats::record::Regime,
    )>,
) -> Result<Running, ExitCode> {
    let render = diet::drive::session::render;
    let receipt = record
        .as_ref()
        .map(|(((path, _), _), _)| path.clone())
        .or_else(|| log.as_ref().map(|(path, _)| (*path).to_owned()))
        .map(|path| format!("{path}.receipt.json"));
    let (log_held, record_kept, mut emptied) =
        started_writers(&session, render, log, record).map_err(|why| fail(EXIT_OUTPUT, &why))?;
    // An earlier session's receipt beside these files would read as this
    // one's, as an earlier record would (#264's review).
    if let Some(path) = receipt
        .as_deref()
        .filter(|path| std::path::Path::new(path).exists())
    {
        std::fs::remove_file(path).map_err(|why| {
            fail(
                EXIT_OUTPUT,
                &emptied.named(format!("{path} cannot be removed: {why}")),
            )
        })?;
        emptied.push(format!("the previous receipt at {path}"));
    }
    let server = Server::start(listener, session, config, render).map_err(|why| {
        fail(
            EXIT_HALT,
            &emptied.named(format!("the server did not start: {why}")),
        )
    })?;
    let (record_held, record) = record_kept.unzip();
    Ok(Running {
        server,
        log_held,
        record_held,
        record,
        receipt,
        recording: None,
        read_at_start: None,
    })
}

/// The log and the record, started once the address is bound and before
/// the server starts. Both are emptied now: the log is written from the
/// session's first line on, and the record waits for the session to end --
/// with an earlier run's record and sidecar gone, so a session that never
/// ends leaves nothing that reads as its own (#264's review). Whether naming
/// each emptied something.
#[allow(clippy::type_complexity)]
fn started_writers(
    session: &std::sync::Arc<Session<HttpStream>>,
    render: diet::drive::serve::Render,
    log: Option<(&str, (std::fs::File, bool))>,
    record: Option<(
        ((String, std::fs::File), bool),
        diet::formats::record::Regime,
    )>,
) -> Result<
    (
        Option<bool>,
        Option<(
            bool,
            (diet::formats::record::Regime, (String, std::fs::File)),
        )>,
        diet::drive::serve::Emptied,
    ),
    String,
> {
    let mut emptied = diet::drive::serve::Emptied::default();
    let record_kept = record
        .map(|(((path, file), held), regime)| {
            let sidecar = format!("{path}.unspellable.json");
            let stale = std::path::Path::new(&sidecar).exists();
            file.set_len(0)
                .map_err(|why| format!("the record cannot be emptied: {why}"))?;
            if held {
                emptied.push(format!("the previous record at {path}"));
            }
            if stale {
                std::fs::remove_file(&sidecar)
                    .map_err(|why| emptied.named(format!("{sidecar} cannot be removed: {why}")))?;
                emptied.push(format!("the previous sidecar at {sidecar}"));
            }
            let product = format!("{path}.product.txt");
            let stale_product = std::path::Path::new(&product).exists();
            if stale_product {
                std::fs::remove_file(&product)
                    .map_err(|why| emptied.named(format!("{product} cannot be removed: {why}")))?;
                emptied.push(format!("the previous product at {product}"));
            }
            Ok::<_, String>((held || stale || stale_product, (regime, (path, file))))
        })
        .transpose()?;
    let log_held = log
        .map(|(path, (file, truncated))| {
            file.set_len(0)
                .map_err(|why| emptied.named(format!("the log cannot be emptied: {why}")))?;
            if truncated {
                emptied.push(format!("the previous log at {path}"));
            }
            keep_log(session, render, file);
            Ok::<_, String>(truncated)
        })
        .transpose()?;
    Ok((log_held, record_kept, emptied))
}

/// The server, running, and what the caller announces and waits on.
struct Running {
    server: Server,
    /// Whether naming the log emptied a file that held something.
    log_held: Option<bool>,
    /// Whether naming the record emptied anything.
    record_held: Option<bool>,
    /// The record's regime and file, emptied, written once the session has
    /// ended.
    record: Option<(diet::formats::record::Regime, (String, std::fs::File))>,
    /// Where the receipt goes: beside the record, else beside the log.
    receipt: Option<String>,
    /// The recording's directory, which an attached file's copy was kept in
    /// and the projection reads it back from (#372).
    recording: Option<std::path::PathBuf>,
    /// What the record's `start` says of the regimen serve read at start.
    read_at_start: Option<ReadAtStart>,
}

/// What the record's `start` says of the regimen, read once at start: the
/// sha256 of its bytes, and each lever's state it puts the session at.
struct ReadAtStart {
    regimen_sha256: String,
    levers: BTreeMap<String, String>,
    /// The output cap the session runs at, and where it came from.
    output_cap: (u32, &'static str),
}

/// Project the ended session, check the record reads back, and write it and
/// its sidecar: the line `serve` reports once the session has ended.
fn written_record(
    session: &Session<HttpStream>,
    (regime, read_at_start): (&diet::formats::record::Regime, Option<&ReadAtStart>),
    (path, mut file): (&str, std::fs::File),
    recording: Option<&std::path::Path>,
) -> Result<String, String> {
    use diet::drive::projection;
    use diet::formats::record::{self, Record};
    let engine =
        diet::drive::registry::identity(diet::drive::registry::REGISTRY, &regime.substrates[0].id)
            .ok()
            .and_then(|identity| projection::cited(&identity));
    let lines: Vec<_> = session
        .events_from(0)
        .iter()
        .map(diet::drive::session::line_of)
        .collect();
    let mut projected = projection::project_in(&lines, regime, engine, recording)?;
    // The levers come from the log's `session.start` (#573), so the record
    // and the log say the same.
    if let (Some(record::Event::Start { regimen_sha256, .. }), Some(read)) =
        (projected.events.first_mut(), read_at_start)
    {
        *regimen_sha256 = Some(read.regimen_sha256.clone());
    }
    // The summary: the turns and prefill the rows carry, and the product --
    // the working memory at the session's end, kept beside the record as
    // FILE.product.txt so `sha256sum` recomputes the digest the row claims.
    let product = session.product();
    let (turns, prefill) = projected
        .events
        .iter()
        .fold((0u32, 0u64), |(n, total), event| match event {
            record::Event::Turn { prefill_tokens, .. } => (n + 1, total + prefill_tokens.get()),
            _ => (n, total),
        });
    projected.events.push(record::Event::Summary {
        summary: record::Summary::Drive {
            turns,
            prefill_tokens_total: record::Count::new(prefill)
                .map_err(|why| format!("prefill_tokens_total: {why:?}"))?,
        },
        product_sha256: diet::digest::sha256_hex(product.as_bytes()),
    });
    let product_path = format!("{path}.product.txt");
    std::fs::write(&product_path, &product).map_err(|why| format!("{product_path}: {why}"))?;
    let text = record::render(&Record {
        events: projected.events.clone(),
    });
    record::parse(&text)
        .map_err(|why| format!("the projected record does not read back: {why:?}"))?;
    let sidecar = projection::sidecar(&projected) + "\n";
    let sidecar_path = format!("{path}.unspellable.json");
    file.set_len(0)
        .and_then(|()| std::io::Write::write_all(&mut file, text.as_bytes()))
        .map_err(|why| format!("{path}: {why}"))?;
    std::fs::write(&sidecar_path, &sidecar).map_err(|why| format!("{sidecar_path}: {why}"))?;
    let mut report = BTreeMap::from([
        ("record".to_owned(), Value::String(path.to_owned())),
        (
            "record_sha256".to_owned(),
            Value::String(diet::digest::sha256_hex(text.as_bytes())),
        ),
        ("sidecar".to_owned(), Value::String(sidecar_path)),
        (
            "sidecar_sha256".to_owned(),
            Value::String(diet::digest::sha256_hex(sidecar.as_bytes())),
        ),
        ("product".to_owned(), Value::String(product_path)),
        (
            "product_sha256".to_owned(),
            Value::String(diet::digest::sha256_hex(product.as_bytes())),
        ),
    ]);
    if let Value::Object(fields) = projection::sidecar_value(&projected) {
        report.extend(fields);
    }
    let mut out = String::new();
    json::render(&Value::Object(report), &mut out);
    Ok(out)
}

/// The session's trunk: the system message and the sampler pins, and
/// nothing else fixed yet. Every request -- each turn's, each tool step's,
/// the interview fork's -- is this template's clone, so each carries `sampler`.
fn trunk(
    model: String,
    system: String,
    max_output_tokens: u32,
    sampler: SamplerCard,
) -> RequestShape {
    RequestShape {
        model,
        messages: vec![Message::new(Role::System, system)],
        sampler,
        limits: Limits {
            attempt: std::time::Duration::from_secs(60),
            call: std::time::Duration::from_secs(180),
            max_output_tokens,
            retries: 0,
            context_window: None,
        },
        grammar: None,
        template_kwargs: BTreeMap::new(),
        tools: Vec::new(),
    }
}

/// The chat endpoint a base URL names: an endpoint given with no path, or
/// with only `/v1`, is the server's `/v1/chat/completions` -- the path every
/// engine `serve` speaks to answers on. A server's bare base URL was a 404
/// on the first request (#496's live turn). Any other path is the
/// operator's, kept as written.
fn chat_endpoint(mut endpoint: Endpoint) -> Endpoint {
    if matches!(endpoint.path.as_str(), "/" | "/v1" | "/v1/") {
        CHAT_COMPLETIONS.clone_into(&mut endpoint.path);
    }
    endpoint
}

/// The path [`chat_endpoint`] completes a base URL to.
const CHAT_COMPLETIONS: &str = "/v1/chat/completions";

/// The address `serve` may listen on: `listen`, refused when it is a
/// wildcard, or off loopback with no credential (fail-closed, I7). A usage
/// refusal, so it is made before anything touches the network.
fn listen_on(listen: IpAddr, credentialed: bool) -> Result<IpAddr, ExitCode> {
    // `::ffff:0.0.0.0` is the IPv4 wildcard, and `::ffff:127.0.0.1` loopback:
    // judged as IPv6, the first would pass the wildcard check.
    let listen = listen.to_canonical();
    if listen.is_unspecified() {
        return Err(fail(
            EXIT_USAGE,
            &format!(
                "--listen {listen} is a wildcard, and no Host could be checked against it: \
                 name one interface's address"
            ),
        ));
    }
    if !listen.is_loopback() && !credentialed {
        return Err(fail(
            EXIT_USAGE,
            &format!(
                "--listen {listen} is off loopback and needs --auth-file: without it, \
                 anyone who can reach {listen} drives the session"
            ),
        ));
    }
    Ok(listen)
}

/// The listener `serve` binds on `listen`, already admitted by [`listen_on`].
fn listener(listen: IpAddr, port: u16) -> Result<std::net::TcpListener, ExitCode> {
    std::net::TcpListener::bind((listen, port)).map_err(|why| {
        fail(
            EXIT_HALT,
            &format!("cannot listen on {listen}:{port}: {why}"),
        )
    })
}

/// The first line `serve` prints: where it listens and when it opened, as
/// JSON.
/// Who warmed the server before `serve` announced itself (#509).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Warmed {
    /// `serve`'s own probe request.
    ByServe,
    /// The engine, as its entry declares (`served_warmup = "true"`).
    ByEngine,
    /// Nothing: a canned server, or an API, which nothing warms.
    NotApplicable,
}

impl Warmed {
    fn tag(self) -> &'static str {
        match self {
            Self::ByServe => "serve",
            Self::ByEngine => "the engine (declared)",
            Self::NotApplicable => "n/a",
        }
    }
}

/// What the start confirmed of a server: each served field, how it was
/// warmed, and its context window and where that was read (#588).
type Confirmed = (
    Vec<diet::formats::log::ServedField>,
    Warmed,
    Option<(u64, &'static str)>,
);

/// What the start confirms of substrate `id`'s server beyond its engine
/// (#509): each declared `served_*` field and the chat template's digest,
/// against the engine's report on itself; then, unless the engine warms
/// itself, one probe request, which also corroborates a declared draft.
/// A canned server confirms none of it: its acts are the whole of it.
///
/// # Errors
///
/// An unreachable server, a contradiction, or a refused probe.
fn confirmations(
    transport: &HttpStream,
    id: &str,
    passed: &diet::drive::engine::Passed,
    shape: &RequestShape,
) -> Result<Confirmed, String> {
    use diet::drive::served::{self, Engine, ServerKind};
    let identity = diet::drive::registry::identity(diet::drive::registry::REGISTRY, id)?;
    if matches!(
        identity.weights,
        diet::formats::record::Weights::Canned { .. }
    ) {
        return Ok((Vec::new(), Warmed::NotApplicable, None));
    }
    // The window each request's output cap is clamped to (#588).
    let serving_context =
        diet::drive::registry::serving_context(diet::drive::registry::REGISTRY, id);
    if ServerKind::of(id, &identity)? == ServerKind::Api {
        let mut fields = served::corroborated(id, &identity, None)?;
        fields.extend(served::draft_corroborated(id, &identity, None)?);
        let window = served::window(&identity, None, serving_context);
        return Ok((fields, Warmed::NotApplicable, window));
    }
    let engine = Engine::of(&identity);
    let report = match (&passed.props, engine) {
        (Some(props), Engine::LlamaCpp) => serde_json::from_str(props).ok(),
        _ => served::report_of(
            engine.report_path(),
            transport.get(
                engine.report_path(),
                std::time::Instant::now() + diet::drive::engine::PROPS_DEADLINE,
            ),
        )?,
    };
    let mut fields = served::corroborated(id, &identity, report.as_ref())?;
    let window = served::window(&identity, report.as_ref(), serving_context);
    let warms = served::warms_itself(&identity);
    let timings = if warms && !identity.served.contains_key("draft") {
        None
    } else {
        served::probe(transport, shape)?
    };
    fields.extend(served::draft_corroborated(id, &identity, timings.as_ref())?);
    Ok((
        fields,
        if warms {
            Warmed::ByEngine
        } else {
            Warmed::ByServe
        },
        window,
    ))
}

fn announcement(
    listening: &str,
    opened: u64,
    substrate: Option<(&str, &str)>,
    engine: Option<(&diet::drive::engine::Passed, Warmed)>,
    log: Option<(&str, bool)>,
    record: Option<(&str, bool)>,
    unsent_budget: Option<u64>,
) -> String {
    let mut fields = BTreeMap::from([
        ("listening".to_owned(), Value::String(listening.to_owned())),
        (
            "opened".to_owned(),
            Value::Integer(i64::try_from(opened).unwrap_or(i64::MAX)),
        ),
    ]);
    if let Some((substrate, registry_sha256)) = substrate {
        fields.insert("substrate".to_owned(), Value::String(substrate.to_owned()));
        // Which registry answered (#204): its text's digest, for a reader
        // who has this line and not the binary.
        fields.insert(
            "registry_sha256".to_owned(),
            Value::String(registry_sha256.to_owned()),
        );
    }
    // The `build_info` the engine check passed, as the server reported it,
    // and how: on both paths, so a reader never infers the path from a
    // missing field.
    if let Some((engine, warmed)) = engine {
        fields.insert(
            "engine_build".to_owned(),
            Value::String(engine.build_info.clone()),
        );
        fields.insert(
            "engine_identity".to_owned(),
            Value::String(engine.identity.tag().to_owned()),
        );
        // Who warmed the server before this line (#509).
        fields.insert("warmed".to_owned(), Value::String(warmed.tag().to_owned()));
    }
    // A budget the regime declares and no request carries (R1): said, so
    // the operator knows the cap is not in force.
    if let Some(budget) = unsent_budget {
        fields.insert(
            "budget_tokens_unsent".to_owned(),
            Value::Integer(i64::try_from(budget).unwrap_or(i64::MAX)),
        );
    }
    // Where the log is written, and whether naming it emptied a file that
    // held something (ruled on #230).
    if let Some((path, truncated)) = log {
        fields.insert("log".to_owned(), Value::String(path.to_owned()));
        fields.insert("log_truncated".to_owned(), Value::Boolean(truncated));
    }
    // And the record's: whether naming it emptied a record or removed a
    // sidecar an earlier run left.
    if let Some((path, truncated)) = record {
        fields.insert("record".to_owned(), Value::String(path.to_owned()));
        fields.insert("record_truncated".to_owned(), Value::Boolean(truncated));
    }
    let mut out = String::new();
    json::render(&Value::Object(fields), &mut out);
    out
}

/// The regime the regimen at `path` declares, its substrate resolved from the
/// registry this program was built with (#157 Q2), and what the record's
/// `start` says of it: the sha256 of the bytes it was read from, the output
/// cap (`flag`, `--max-output-tokens`, beating the regimen's), and the
/// levers it sets.
fn registered_regime(
    path: &str,
    flag: Option<u32>,
) -> Result<(diet::formats::record::Regime, ReadAtStart), String> {
    let text =
        std::fs::read_to_string(path).map_err(|why| format!("{path} cannot be read: {why}"))?;
    let regimen =
        regimen::parse(&text).map_err(|why| format!("{path} is not a regimen: {why:?}"))?;
    diet::drive::regimen::regime_registered(&regimen, diet::drive::registry::REGISTRY)
        .map(|regime| {
            let output_cap = diet::drive::regimen::output_cap(flag, Some(&regimen));
            (
                regime,
                ReadAtStart {
                    regimen_sha256: diet::digest::sha256_hex(text.as_bytes()),
                    levers: diet::drive::regimen::serve_levers(&regimen, output_cap),
                    output_cap,
                },
            )
        })
        .map_err(|why| format!("{path}: {why}"))
}

/// The endpoint's key, from a FILE: an argument is readable by anyone who
/// can list the machine's processes. One trailing line break is the file's,
/// not the key's; an empty key, or one holding a control character (a line
/// break among them), is refused rather than sent.
fn bearer_from(path: &str) -> Result<Bearer, String> {
    let written =
        std::fs::read_to_string(path).map_err(|why| format!("{path} cannot be read: {why}"))?;
    let key = written
        .strip_suffix("\r\n")
        .or_else(|| written.strip_suffix('\n'))
        .unwrap_or(&written);
    Bearer::new(key)
        .ok_or_else(|| format!("{path} holds no usable key: empty, or holding a control character"))
}

/// The drive server's own credential, from a FILE, as `--key-file` reads the
/// endpoint's: `user:password` on one line, one trailing line break the
/// file's. A line with no `:`, or with a control character, is refused.
fn credential_from(path: &str) -> Result<Credential, String> {
    let written =
        std::fs::read_to_string(path).map_err(|why| format!("{path} cannot be read: {why}"))?;
    let pair = written
        .strip_suffix("\r\n")
        .or_else(|| written.strip_suffix('\n'))
        .unwrap_or(&written);
    Credential::basic(pair).ok_or_else(|| {
        format!("{path} holds no usable credential: user:password, with no control character")
    })
}

/// An origin as a browser sends it: a scheme, `://`, a host and port, and
/// no path -- not even a trailing slash. Anything else could never equal an
/// `Origin` header, and would be accepted and do nothing.
fn is_an_origin(value: &str) -> bool {
    value
        .split_once("://")
        .is_some_and(|(_, authority)| !authority.is_empty() && !authority.contains('/'))
}

/// Why `key_file` may not be read: it lies in a path the regimen declares
/// readable or writable, which a command can read (#299, ruling 5983673467,
/// H(1)).
///
/// The key file joins the session's secret set (#299 review of 0706745, O),
/// and a regimen whose isolation does not read is refused rather than
/// skipped.
fn key_file_in_declared_path(regimen: Option<&str>, key_file: &str) -> Option<String> {
    let regimen = regimen?;
    let Some(mut policy) = isolation_policy_of(regimen) else {
        return Some(format!(
            "{regimen}'s isolation lines do not read as a policy, so where --key-file may \
             lie cannot be checked; refusing to start"
        ));
    };
    let path =
        isolation::declared::add_credential(&mut policy, std::path::Path::new(key_file)).err()?;
    Some(format!(
        "--key-file {key_file} lies in `{}`, which the regimen declares readable or writable: \
         a command could read the key",
        path.display()
    ))
}

/// Re-execute this program under only the regimen's environment
/// (`PATH`, `HOME`, `LANG`, `TERM`, `TMPDIR` and its `env_passthrough`),
/// marked so it does not do it twice. Returns only if the exec failed.
fn scrubbed() -> ExitCode {
    use std::os::unix::process::CommandExt as _;
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let regimen = if args.first().is_some_and(|first| first == SERVE) {
        args.iter()
            .position(|arg| arg == "--regimen")
            .and_then(|at| args.get(at + 1))
    } else {
        args.first()
    };
    let names = regimen
        .and_then(|path| isolation_policy_of(&path.to_string_lossy()))
        .unwrap_or_else(IsolationPolicy::merged_usr)
        .environment;
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(why) => {
            return fail(
                EXIT_HALT,
                &format!("cannot find this program to re-execute: {why}"),
            );
        }
    };
    let why = isolation::scrubbed_drive(&exe, &args, &names, std::env::vars_os()).exec();
    fail(
        EXIT_HALT,
        &format!("could not re-execute with a scrubbed environment: {why}"),
    )
}

/// The isolation policy the regimen at `path` declares, if it reads as one.
fn isolation_policy_of(path: &str) -> Option<IsolationPolicy> {
    let text = std::fs::read_to_string(path).ok()?;
    IsolationPolicy::from_regimen(&regimen::parse(&text).ok()?).ok()
}

fn usage() -> String {
    let mut out =
        String::from("usage: diet-drive <regimen> <worktree> <output.jsonl> [endpoint]\n");
    // The interactive server, which the first form's usage once hid (#290).
    out.push_str("       diet-drive serve --endpoint URL --model NAME --head FILE ...\n");
    out.push_str("       (one interactive session over HTTP; `diet-drive serve --help`)\n");
    out.push_str("       diet-drive replay [--answers] [--port N]\n");
    out.push_str("       (substrate `canned-replay-tools` on loopback: captured llama.cpp\n");
    out.push_str("       replies, a `bash` call and then its answer, for a regimen rehearsed\n");
    out.push_str("       with no model; --answers serves `canned-replay`, one captured answer\n");
    let _ = writeln!(
        out,
        "       for every request; port {REPLAY_PORT} unless told; runs until stopped)\n"
    );
    out.push_str("Runs the pinned three-turn script through <regimen> in <worktree>\n");
    out.push_str("and writes the record to <output.jsonl>. With no endpoint the canned\n");
    out.push_str("server answers on loopback -- no model, no network out.\n\n");
    out.push_str("<worktree> is REQUIRED and is not defaulted to the current directory.\n");
    out.push_str("The script's commands write into it, and defaulting meant the\n");
    out.push_str("documented invocation dropped files into whatever checkout the\n");
    out.push_str("operator happened to be standing in.\n\n");
    out.push_str("The regimen must carry `arm`, `dogma_version`, `substrate` and:\n");
    for key in SUBSTRATE_KEYS {
        let _ = writeln!(out, "  {key}");
    }
    out.push_str("\nA JSON result goes to stdout. Exit 0 when the drive ran and its\n");
    out.push_str("record parses, 1 when the input is not usable, 2 on a usage error or\n");
    out.push_str("a drive that could not run, 3 when the record could not be filed.\n");
    out.push_str("--help prints this to stdout and exits 0.\n");
    out
}

/// `diet-drive replay [--answers] [--port N]`: the stream-replay
/// substrates' server (#411). By default `canned-replay-tools`: a request
/// ending in a tool result is answered with [`canned::REPLAYED_ANSWER`], any
/// other with [`canned::REPLAYED_CALL`]. With `--answers`, `canned-replay`:
/// every request answered with [`canned::REPLAYED`]. Byte for byte, and
/// `/props` with the substrate's literal, so `serve` under a regimen naming
/// it passes its engine check against this and against nothing else. One
/// JSON line on stdout says where it listens; it runs until stopped.
fn replay(args: &[String]) -> ExitCode {
    let answers = args.first().is_some_and(|first| first == "--answers");
    let rest = if answers { &args[1..] } else { args };
    let port = match rest {
        [] => REPLAY_PORT,
        [flag, port] if flag == "--port" => match port.parse() {
            Ok(port) => port,
            Err(_) => return fail(EXIT_USAGE, &format!("`{port}` is not a port")),
        },
        _ => {
            eprint!("{}", usage());
            return ExitCode::from(EXIT_USAGE);
        }
    };
    let (substrate, build_info, started) = if answers {
        let build_info = canned::replay_build_info();
        let started = Stub::replaying(canned::REPLAYED.to_vec(), &build_info, port);
        ("canned-replay", build_info, started)
    } else {
        let build_info = canned::replay_tools_build_info();
        let started = Stub::replaying_tool_turns(
            canned::REPLAYED_CALL.to_vec(),
            canned::REPLAYED_ANSWER.to_vec(),
            &build_info,
            port,
        );
        ("canned-replay-tools", build_info, started)
    };
    let stub = match started {
        Ok(stub) => stub,
        Err(why) => {
            return fail(
                EXIT_HALT,
                &format!("127.0.0.1:{port} could not be listened on: {why}"),
            );
        }
    };
    println!(
        "{{\"build_info\":\"{build_info}\",\"listening\":\"{}\",\"ok\":true,\"substrate\":\"{substrate}\"}}",
        stub.url()
    );
    let _ = std::io::Write::flush(&mut std::io::stdout());
    loop {
        std::thread::park();
    }
}

fn main() -> ExitCode {
    // FIRST, before any thread: a sandboxed command can read this process's
    // environment and argv (`KERN_PROCARGS2`), so the drive re-executes
    // itself holding only what a command is given anyway (#299, ruling
    // 5983673467, H(1)). The endpoint's key is never in either; serve reads
    // it from `--key-file` after this.
    if std::env::var_os(isolation::SCRUBBED).is_none() {
        return scrubbed();
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some(SERVE) {
        if asks_for_help(&args[1..]) {
            print!("{}", serve_usage());
            return ExitCode::SUCCESS;
        }
        return serve(&args[1..]);
    }
    if args.first().map(String::as_str) == Some(REPLAY) {
        return replay(&args[1..]);
    }
    if args
        .first()
        .is_some_and(|first| first == "--help" || first == "-h")
    {
        print!("{}", usage());
        return ExitCode::SUCCESS;
    }
    let (Some(regimen_path), Some(worktree), Some(out_path)) =
        (args.first(), args.get(1), args.get(2))
    else {
        eprint!("{}", usage());
        return ExitCode::from(EXIT_USAGE);
    };
    if args.len() > 4 {
        eprint!("{}", usage());
        return ExitCode::from(EXIT_USAGE);
    }

    let (regime, isolation_policy, seam_policy) =
        match declared(regimen_path, args.get(3).is_some()) {
            Ok(three) => three,
            Err((code, why)) => return fail(code, &why),
        };
    let confinement = match isolation::open(&isolation_policy) {
        Ok(confinement) => confinement,
        Err(why) => return fail(EXIT_HALT, &why.to_string()),
    };
    // Absolute, because `Confinement::run` refuses a relative working tree --
    // it is resolved once by the harness and again inside a sandbox -- and
    // because the record should not depend on where the operator stood.
    let worktree = match std::path::absolute(worktree) {
        Ok(path) if path.is_dir() => path,
        Ok(path) => {
            return fail(
                EXIT_INPUT,
                &format!("{} is not a directory that exists", path.display()),
            );
        }
        Err(why) => return fail(EXIT_INPUT, &format!("{worktree} is not a path: {why}")),
    };

    // The output path is claimed BEFORE anything runs. Validated last, a
    // drive against an unwritable path made six inference calls, ran both
    // commands into the working tree, and then exited -- reporting a code
    // whose documented meaning is "could not start" for a session that ran to
    // completion, which a census keyed on it would miscount.
    if let Err(why) = std::fs::write(out_path, "") {
        return fail(
            EXIT_OUTPUT,
            &format!("{out_path} cannot be written, and nothing has run: {why}"),
        );
    }

    let script = canned::script(regime);
    // `held` is not an unused binding: dropping the stub stops its serving
    // thread, and a canned server that stopped mid-drive would reach the
    // client as a substrate that hung up. It lives as long as the run does.
    let (held, endpoint) = match answering(args.get(3)) {
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
fn declared(
    path: &str,
    endpoint_given: bool,
) -> Result<(Regime, IsolationPolicy, SeamPolicy), (u8, String)> {
    let text = std::fs::read_to_string(path)
        .map_err(|why| (EXIT_INPUT, format!("{path} could not be read: {why}")))?;
    let regimen = regimen::parse(&text)
        .map_err(|why| (EXIT_INPUT, format!("{path} is not a regimen: {why:?}")))?;
    let regime = regime_of(&regimen, endpoint_given).map_err(|why| (EXIT_INPUT, why))?;
    // `regime_of` refused above already if an endpoint was given, so a regime
    // that reaches here is always about to run against this program's own
    // canned server. That server renders no chat template -- no effort level
    // to instruct it with and no think block for a budget to cap -- so a
    // reasoning control declared against it is a control nothing applies,
    // and driving it anyway does not fail cleanly: the kwarg-delivery
    // negative control makes an extra call the canned script never budgeted
    // for, consumes the first turn's answer, and the drive halts on turn
    // three blaming the wrong lane. Refused here, named for what it is,
    // rather than reached as that misattributed halt.
    if regime
        .substrates
        .iter()
        .any(|substrate| substrate.reasoning_control.is_some())
    {
        return Err((
            EXIT_INPUT,
            "the regimen declares `[reasoning]`, and this program only ever runs its own \
             canned server, which renders no chat template and has no think block for a \
             budget to cap -- a reasoning control here is a control nothing applies. \
             Supported once a real endpoint's substrate identity is resolvable; until then, \
             an endpoint given is refused above for the same missing-identity reason, and a \
             canned run declaring one is refused here"
                .to_owned(),
        ));
    }
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
        let stub = Stub::serving_with_props(canned::acts(), &canned::build_info())
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
            EXIT_OUTPUT,
            &format!("{out_path} could not be written: {why}"),
        );
    }
    // Beside it, because the summary's `product_sha256` is a digest OF
    // something and a digest of a file nobody wrote is a digest nobody can
    // check. `sha256sum` on this file is the whole audit.
    let product_path = format!("{out_path}.product");
    if let Err(why) = std::fs::write(&product_path, &drive.product) {
        return fail(
            EXIT_OUTPUT,
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
                    Value::Array(drive.uncaptured.iter().map(census).collect()),
                ),
                // Every audit, because none of them was folded: the verdict
                // grammar is not built. Reported as a count rather than
                // silently absent, so a reader of this line knows a seam's
                // ask was put and its answer not read.
                ("audits_unread".to_owned(), count(drive.audits.len())),
                // The cache census (#79). BESIDE the record, not in it: the
                // `expected` register needs an inter-call gap and record v0
                // carries no clock. The half that is in the record --
                // `head_sha256` per request, and the `prefix.changed` rows --
                // is what makes `mutation` re-derivable from the file alone.
                ("cache".to_owned(), cache(&drive.cache)),
            ]
            .into_iter()
            .collect(),
        ),
        &mut out,
    );
    println!("{out}");
    ExitCode::SUCCESS
}

/// The cache census, in the record's value space.
///
/// **EVERY REGISTER IS RENDERED WHETHER OR NOT IT FIRED.** A census that
/// omitted its empty registers would report a run with three unexplained
/// misses and a run with none in shapes a reader has to compare by absence --
/// and "unexplained" is exactly the register whose disappearance nobody
/// notices. `ttl_undeclared` is named for the same reason: a substrate with no
/// declared lifetime cannot produce an `expected` miss, so a reader seeing
/// everything in `unexplained` deserves to know why.
fn cache(census: &diet::client::cache::Census) -> Value {
    let count = |many: u64| Value::Integer(i64::try_from(many).unwrap_or(-1));
    Value::Object(BTreeMap::from([
        ("hits".to_owned(), count(census.hits)),
        (
            "misses".to_owned(),
            Value::Object(BTreeMap::from([
                ("expected".to_owned(), count(census.expected)),
                ("mutation".to_owned(), count(census.mutation)),
                ("unexplained".to_owned(), count(census.unexplained)),
                ("cold_start".to_owned(), count(census.cold_start)),
            ])),
        ),
        (
            "ttl_undeclared".to_owned(),
            Value::Array(
                census
                    .ttl_undeclared
                    .iter()
                    .map(|id| Value::String(id.clone()))
                    .collect(),
            ),
        ),
        ("unmeasured".to_owned(), count(census.unmeasured)),
    ]))
}

/// One fork's census, in the record's value space.
fn census(one: &diet::drive::Uncaptured) -> Value {
    let count = |many: usize| Value::Integer(i64::try_from(many).unwrap_or(-1));
    Value::Object(BTreeMap::from([
        ("turn".to_owned(), Value::Integer(i64::from(one.turn))),
        ("fork".to_owned(), Value::String(one.fork.clone())),
        ("regions".to_owned(), count(one.regions)),
        ("captured".to_owned(), count(one.captured)),
        // THE OUTCOME, not a `truncated` flag. #94 design point 5 adds a
        // third state -- a fork whose budget was spent inside the think
        // block -- and a boolean would have reported it as `false`, which
        // reads as "the answer is whole" about a fork that produced none.
        (
            "outcome".to_owned(),
            Value::String(one.outcome.tag().to_owned()),
        ),
        // BOTH counts -- ruling 6. Record v0 has one field and it means
        // `touched`; a reader of this line gets the question the record
        // cannot yet ask, which is whether the fork produced anything NEW.
        (
            "entries_touched".to_owned(),
            Value::Integer(i64::from(one.entries_touched)),
        ),
        (
            "entries_created".to_owned(),
            Value::Integer(i64::from(one.entries_created)),
        ),
        (
            "passed_over".to_owned(),
            Value::Array(
                one.passed_over
                    .iter()
                    .map(|tag| Value::String(tag.clone()))
                    .collect(),
            ),
        ),
    ]))
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
    // Never a panicking print: a reader that closed stdout must not stop the
    // caller from exiting with its code (the log's writer above all).
    let _ = std::io::Write::write_all(&mut std::io::stdout().lock(), format!("{out}\n").as_bytes());
    ExitCode::from(code)
}

/// The request every call is a variation of.
fn shape(regime: &Regime) -> RequestShape {
    RequestShape {
        // Per #68 item 3 the substrate's identity is typed and there is no
        // prose model name to send; this program drives the canned server,
        // which answers whatever it is asked. `regime_of` names the wire model
        // in its refusal for the day a real endpoint is drivable again.
        model: regime.substrates[0].id.clone(),
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
            context_window: None,
        },
        grammar: None,
        template_kwargs: std::collections::BTreeMap::new(),
        // NO TOOL SURFACE. The pinned script runs commands under the
        // confinement rather than exposing tools to the model, so a drive
        // here declares none -- which is why #79's tool-reordering
        // attribution is proved at the unit and not end to end. Disclosed
        // rather than left looking like an omission.
        tools: Vec::new(),
    }
}
