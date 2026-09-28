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
use diet::drive::regimen::{SUBSTRATE_KEYS, regime_of};
use diet::drive::serve::{Config, Credential, Server};
use diet::drive::session::Session;
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

fn serve_usage() -> String {
    let mut out = String::from(
        "usage: diet-drive serve --endpoint URL --model NAME --head FILE [--key-file FILE]\n\
         \x20                       [--listen IP] [--port N] [--auth-file FILE]\n\
         \x20                       [--allow-origin URL]... [--max-output-tokens N]\n\n",
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
    out
}

/// `diet-drive serve`: one interactive session, over HTTP + SSE, on loopback
/// unless `--listen` names another address.
///
/// Fail-closed (#117 R2c, I7): off loopback it will not start without a
/// credential, so nothing here binds a session that runs the model beyond the
/// machine it is on for anyone who can reach it. A wildcard is refused: D17's
/// `Host` check answers to the bound address, and a wildcard has none.
fn serve(args: &[String]) -> ExitCode {
    let mut endpoint = None;
    let mut model = None;
    let mut head = None;
    let mut key_file = None;
    let mut auth_file = None;
    let mut listen = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let mut port: u16 = 0;
    let mut allowed_origins = Vec::new();
    let mut max_output_tokens: u32 = 512;
    let mut given = args.iter();
    while let Some(flag) = given.next() {
        let Some(value) = given.next() else {
            eprint!("{}", serve_usage());
            return ExitCode::from(EXIT_USAGE);
        };
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
            value.parse().map(|given| max_output_tokens = given).is_ok()
        } else {
            false
        };
        if !parsed {
            eprint!("{}", serve_usage());
            return ExitCode::from(EXIT_USAGE);
        }
    }
    let (Some(endpoint), Some(model), Some(head)) = (endpoint, model, head) else {
        eprint!("{}", serve_usage());
        return ExitCode::from(EXIT_USAGE);
    };
    let endpoint = match Endpoint::parse(&endpoint) {
        Ok(endpoint) => endpoint,
        Err(why) => return fail(EXIT_INPUT, &format!("{endpoint} is not an endpoint: {why}")),
    };
    let system = match std::fs::read_to_string(&head) {
        Ok(system) => system,
        Err(why) => return fail(EXIT_INPUT, &format!("{head} cannot be read: {why}")),
    };
    let shape = trunk(model, system, max_output_tokens);
    let credential = match auth_file.as_deref().map(credential_from).transpose() {
        Ok(credential) => credential,
        Err(why) => return fail(EXIT_INPUT, &why),
    };
    let listener = match listener(listen, port, credential.is_some()) {
        Ok(listener) => listener,
        Err(refused) => return refused,
    };
    let mut transport = HttpStream::new(endpoint);
    if let Some(key_file) = key_file {
        match bearer_from(&key_file) {
            Ok(bearer) => transport = transport.with_bearer(bearer),
            Err(why) => return fail(EXIT_INPUT, &why),
        }
    }
    let session = std::sync::Arc::new(Session::open(transport, shape));
    let opened = session.opened();
    let config = Config {
        allowed_origins,
        credential,
        ..Config::default()
    };
    let server = match Server::start(listener, session, config, diet::drive::session::render) {
        Ok(server) => server,
        Err(why) => return fail(EXIT_HALT, &format!("the server did not start: {why}")),
    };
    println!("{}", announcement(&server.addr().to_string(), opened));
    // Serves until the process is stopped. The server's threads do the work;
    // this one only keeps the process, and the server, alive.
    loop {
        std::thread::park();
    }
}

/// The session's trunk: the system message, and nothing else fixed yet.
fn trunk(model: String, system: String, max_output_tokens: u32) -> RequestShape {
    RequestShape {
        model,
        messages: vec![Message::new(Role::System, system)],
        sampler: SamplerCard::empty(),
        limits: Limits {
            attempt: std::time::Duration::from_secs(60),
            call: std::time::Duration::from_secs(180),
            max_output_tokens,
            retries: 0,
        },
        grammar: None,
        template_kwargs: BTreeMap::new(),
        tools: Vec::new(),
    }
}

/// The listener `serve` binds: on `listen`, which is refused when it is a
/// wildcard, or off loopback with no credential (fail-closed, I7).
fn listener(
    listen: IpAddr,
    port: u16,
    credentialed: bool,
) -> Result<std::net::TcpListener, ExitCode> {
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
    std::net::TcpListener::bind((listen, port)).map_err(|why| {
        fail(
            EXIT_HALT,
            &format!("cannot listen on {listen}:{port}: {why}"),
        )
    })
}

/// The first line `serve` prints: where it listens and when it opened, as
/// JSON.
fn announcement(listening: &str, opened: u64) -> String {
    let mut out = String::new();
    json::render(
        &Value::Object(BTreeMap::from([
            ("listening".to_owned(), Value::String(listening.to_owned())),
            (
                "opened".to_owned(),
                Value::Integer(i64::try_from(opened).unwrap_or(i64::MAX)),
            ),
        ])),
        &mut out,
    );
    out
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

fn usage() -> String {
    let mut out =
        String::from("usage: diet-drive <regimen> <worktree> <output.jsonl> [endpoint]\n\n");
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
    out
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some(SERVE) {
        return serve(&args[1..]);
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
    println!("{out}");
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
