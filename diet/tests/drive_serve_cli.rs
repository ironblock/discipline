//! `diet-drive serve` as a program (#117 R2c, I5): the session served over
//! HTTP + SSE by the binary a person runs, against a server whose reply is a
//! real llama-server stream replayed byte for byte.
//!
//! **Every test here has `drive` in its name**: the lane's command filters by
//! that substring, and an integration test is named by its function alone
//! (see `drive_cli.rs`).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use diet::client::stub::{Act, Stub};
use diet::formats::log;

const DRIVE: &str = env!("CARGO_BIN_EXE_diet-drive");

/// llama-server `4df29be`'s reply to one streamed ask (R2b's fixture). It
/// ends on `finish_reason: "length"`: a capped call (#290).
const CAPTURED: &[u8] = include_bytes!("../client/fixtures/llama-server-4df29be-stream.http");

/// llama-server `e7051ef`'s reply to one streamed ask, reasoning on, ending
/// on `finish_reason: "stop"`: an answer.
const ANSWERED: &[u8] =
    include_bytes!("../client/fixtures/llama-server-e7051ef-reasoning-stream.http");

const HEAD: &str = "you are the trunk, served\n";

/// The probe request's answer at start (#509): a warm turn whose timings
/// carry `draft_n`, captured off llama.cpp `e7051ef`.
const WARM: &[u8] =
    include_bytes!("../client/fixtures/llama-server-e7051ef-warm-turn2-stream.http");

/// The probe's answer, as an act.
fn warm() -> Act {
    Act::Raw(WARM.to_vec())
}

/// A running `diet-drive serve`, stopped when dropped.
struct Served {
    child: Child,
    listening: String,
    opened: u64,
    /// The registered substrate it announced, when it was given a regimen.
    substrate: Option<String>,
    /// And the digest of the registry that resolved it.
    registry_sha256: Option<String>,
    /// The `build_info` its engine check passed, when it made one.
    engine_build: Option<String>,
    /// And how the engine's identity was established.
    engine_identity: Option<String>,
    /// A reasoning budget the regime declares and no request carries (R1).
    budget_tokens_unsent: Option<u64>,
    /// Where `--log` writes, and whether naming it emptied a file.
    log: Option<(String, bool)>,
    /// Where `--record` writes, and whether naming it emptied anything.
    record: Option<(String, bool)>,
    /// Every later line it writes on stdout.
    said: std::sync::mpsc::Receiver<String>,
    _head: HeadFile,
}

impl Drop for Served {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The head file, removed afterwards.
struct HeadFile(PathBuf);

impl Drop for HeadFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn start(endpoint: &str, extra: &[&str]) -> Served {
    start_with(endpoint, extra, &[])
}

/// [`start`], with `env` set in the program's environment.
fn start_with(endpoint: &str, extra: &[&str], env: &[(&str, &str)]) -> Served {
    let head = file_holding("head", HEAD);
    let mut child = Command::new(DRIVE)
        .envs(env.iter().copied())
        .args([
            "serve",
            "--endpoint",
            endpoint,
            "--model",
            "a-model",
            "--head",
        ])
        .arg(&head.0)
        .args(extra)
        .stdout(Stdio::piped())
        .spawn()
        .expect("diet-drive starts");
    // The first line, with a deadline: a binary that exits or hangs before
    // announcing must fail this test, not hang it -- and not leave a server
    // running after it.
    let stdout = child.stdout.take().expect("stdout is piped");
    let (line, lines) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut next = String::new();
            if reader.read_line(&mut next).unwrap_or(0) == 0 || line.send(next).is_err() {
                break;
            }
        }
    });
    let Ok(first) = lines.recv_timeout(Duration::from_secs(10)) else {
        let _ = child.kill();
        let _ = child.wait();
        panic!("diet-drive serve did not announce itself within 10 s");
    };
    let announced = log_line_object(&first);
    Served {
        listening: announced["listening"]
            .as_str()
            .expect("the address it listens on")
            .to_owned(),
        opened: announced["opened"].as_u64().expect("when it opened"),
        substrate: announced["substrate"].as_str().map(str::to_owned),
        registry_sha256: announced["registry_sha256"].as_str().map(str::to_owned),
        engine_build: announced["engine_build"].as_str().map(str::to_owned),
        engine_identity: announced["engine_identity"].as_str().map(str::to_owned),
        budget_tokens_unsent: announced["budget_tokens_unsent"].as_u64(),
        log: announced["log"]
            .as_str()
            .zip(announced["log_truncated"].as_bool())
            .map(|(path, truncated)| (path.to_owned(), truncated)),
        record: announced["record"]
            .as_str()
            .zip(announced["record_truncated"].as_bool())
            .map(|(path, truncated)| (path.to_owned(), truncated)),
        said: lines,
        child,
        _head: head,
    }
}

fn log_line_object(text: &str) -> serde_json::Value {
    serde_json::from_str(text.trim()).unwrap_or_else(|why| panic!("{text:?} is not JSON: {why}"))
}

/// Send `raw`, then read until `done` holds, the connection closes, or ten
/// seconds pass -- one overall deadline, never a per-read timeout, which a
/// heartbeat would keep resetting.
fn exchange(address: &str, raw: &str, done: impl Fn(&str) -> bool) -> String {
    let mut stream = TcpStream::connect(address).expect("connects");
    stream
        .set_read_timeout(Some(Duration::from_millis(50)))
        .expect("a read timeout");
    stream
        .write_all(raw.as_bytes())
        .expect("the request is written");
    let give_up = Instant::now() + Duration::from_secs(10);
    // The bytes, decoded whole each time: a character split across two reads
    // is never decoded as two halves.
    let mut bytes = Vec::new();
    let mut read = String::new();
    let mut chunk = [0_u8; 8192];
    while !done(&read) && Instant::now() < give_up {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                bytes.extend_from_slice(&chunk[..count]);
                read = String::from_utf8_lossy(&bytes).into_owned();
            }
            Err(_) => {}
        }
    }
    read
}

/// How many whole server-sent events with `data:` a stream read holds: the
/// last, unterminated one is not counted until its blank line arrives.
fn complete_data_events(read: &str) -> usize {
    let mut events: Vec<&str> = read.split("\n\n").collect();
    events.pop();
    events
        .iter()
        .filter(|event| event.contains("data: "))
        .count()
}

fn post(address: &str, host: &str, json: &str) -> String {
    exchange(
        address,
        &format!(
            "POST /commands HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\n\r\n{json}",
            json.len()
        ),
        |_| false,
    )
}

fn status(reply: &str) -> u16 {
    reply
        .split(' ')
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("not an HTTP reply: {reply:?}"))
}

#[test]
fn a_real_servers_capped_reply_is_written_capped_and_settles_failed() {
    // #290, ruled 5969297103: `4df29be`'s captured reply ended on its cap.
    let stub = Stub::serving(vec![Act::Raw(CAPTURED.to_vec())]).expect("loopback");
    let served = start(&stub.url(), &[]);
    let address = served.listening.clone();
    let reply = post(&address, &address, r#"{"kind":"ask","text":"hi"}"#);
    assert_eq!(status(&reply), 200, "{reply}");
    let stream = exchange(
        &address,
        &format!("GET /events?from=0 HTTP/1.1\r\nHost: {address}\r\n\r\n"),
        |read| read.contains(r#""reason":"failed""#) && read.contains(r#""to":"awaiting""#),
    );
    let lines: Vec<log::Event> = stream
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .map(|data| {
            log::line(data)
                .unwrap_or_else(|why| panic!("{data}: {why}"))
                .event
        })
        .collect();
    assert!(
        lines.iter().any(|line| matches!(
            line,
            log::Event::Response {
                capped: Some(true),
                ..
            }
        )) && lines.iter().any(|line| matches!(
            line,
            log::Event::TurnSettled {
                reason: log::SettleReason::Failed,
                ..
            }
        )),
        "{stream}"
    );
}

#[test]
fn a_served_drive_streams_a_real_servers_answer_over_sse() {
    let stub = Stub::serving(vec![Act::Raw(ANSWERED.to_vec())]).expect("loopback");
    let served = start(&stub.url(), &[]);
    let address = served.listening.clone();

    let reply = post(&address, &address, r#"{"kind":"ask","text":"hi"}"#);
    assert_eq!(status(&reply), 200, "{reply}");

    let stream = exchange(
        &address,
        &format!("GET /events?from=0 HTTP/1.1\r\nHost: {address}\r\n\r\n"),
        |read| read.contains(r#""from":"capture""#) && read.contains(r#""to":"awaiting""#),
    );
    let lines: Vec<log::Line> = stream
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .map(|data| log::line(data).unwrap_or_else(|why| panic!("{data}: {why}")))
        .collect();
    let document = stream
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    log::parse(&document).expect("what the binary streams is a log the format reads");

    let log::Event::SessionStart {
        opened,
        head,
        serving,
        ..
    } = &lines[0].event
    else {
        panic!("the stream does not begin with the session: {stream}");
    };
    // What serves it, declared (#292): llama-server's dialect, concurrency
    // undeclared rather than assumed one.
    assert_eq!(
        serving
            .as_ref()
            .map(|serving| (serving.dialect.as_str(), serving.concurrency)),
        Some(("llama.cpp", None)),
        "{stream}"
    );
    assert_eq!(
        *opened, served.opened,
        "the announced `opened` is the log's"
    );
    assert_eq!(head[0].content, HEAD, "the head file is the trunk's head");

    let deltas: Vec<(u64, &str)> = lines
        .iter()
        .filter_map(|line| match &line.event {
            log::Event::Delta {
                request,
                piece: log::Piece::Text(text),
            } => Some((*request, text.as_str())),
            _ => None,
        })
        .collect();
    assert!(deltas.len() > 1, "the answer did not stream: {stream}");
    assert!(
        deltas.iter().all(|(request, _)| *request == deltas[0].0),
        "the pieces cite more than one request"
    );
    let streamed: String = deltas.iter().map(|(_, text)| *text).collect();
    let answered = lines.iter().find_map(|line| match &line.event {
        log::Event::Response { text, .. } => Some(text.as_str()),
        _ => None,
    });
    assert_eq!(answered, Some(streamed.as_str()));
    assert!(lines.iter().any(|line| matches!(
        line.event,
        log::Event::TurnSettled {
            reason: log::SettleReason::Final,
            ..
        }
    )));
    assert!(
        matches!(
            lines.last().map(|line| &line.event),
            Some(log::Event::Settlement {
                to: log::State::Awaiting,
                ..
            })
        ),
        "the session did not settle back to awaiting: {stream}"
    );
}

#[test]
fn a_drive_server_binds_loopback_by_default_and_listen_takes_an_ip() {
    let stub = Stub::serving(Vec::new()).expect("loopback");
    let served = start(&stub.url(), &[]);
    assert!(
        served.listening.starts_with("127.0.0.1:"),
        "{}",
        served.listening
    );

    // An address and a port together is not an IP: the port is `--port`'s.
    let (code, said) = run_briefly(&stub.url(), &["--listen", "127.0.0.1:0"]);
    assert_eq!(code, Some(2), "{said}");
}

/// Run `diet-drive serve` with `extra`, and return its exit code and what it
/// printed -- or `None` if it was still running after ten seconds, when it is
/// killed rather than left serving.
fn run_briefly(endpoint: &str, extra: &[&str]) -> (Option<i32>, String) {
    let head = file_holding("head", HEAD);
    let mut child = Command::new(DRIVE)
        .args(["serve", "--endpoint", endpoint, "--model", "m", "--head"])
        .arg(&head.0)
        .args(extra)
        .stdout(Stdio::piped())
        .spawn()
        .expect("diet-drive starts");
    let give_up = Instant::now() + Duration::from_secs(10);
    let code = loop {
        if let Some(exited) = child.try_wait().expect("the child is waited on") {
            break exited.code();
        }
        if Instant::now() >= give_up {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut said = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut said);
    }
    (code, said)
}

/// A file holding `text`, removed when dropped. Named by this process and a
/// count, never a clock: tests run in parallel, and two that read the clock
/// in the same tick would share a file, and one would remove the other's.
fn file_holding(what: &str, text: &str) -> HeadFile {
    static MADE: AtomicUsize = AtomicUsize::new(0);
    let file = HeadFile(std::env::temp_dir().join(format!(
        "diet-drive-serve-{what}-{}-{}.txt",
        std::process::id(),
        MADE.fetch_add(1, Ordering::Relaxed)
    )));
    std::fs::write(&file.0, text).expect("the file is written");
    file
}

/// `author:s3cret`, as a browser sends it (base64).
const AUTHOR: &str = "Authorization: Basic YXV0aG9yOnMzY3JldA==\r\n";

#[test]
fn a_drive_server_off_loopback_without_a_credential_refuses_to_start() {
    let stub = Stub::serving(Vec::new()).expect("loopback");
    // TEST-NET-1 (RFC 5737): off loopback, and never this machine's.
    let (code, said) = run_briefly(&stub.url(), &["--listen", "192.0.2.1"]);
    assert_eq!(code, Some(2), "{said}");
    assert!(said.contains("needs --auth-file"), "{said}");

    // With a credential it goes on to bind -- and fails only because the
    // address is not this machine's.
    let auth = file_holding("auth", "author:s3cret\n");
    let auth_path = auth.0.to_string_lossy().into_owned();
    let (_, said) = run_briefly(
        &stub.url(),
        &["--listen", "192.0.2.1", "--auth-file", &auth_path],
    );
    assert!(said.contains("cannot listen on 192.0.2.1"), "{said}");
}

#[test]
fn a_drive_server_refuses_a_wildcard_listen_even_with_a_credential() {
    let stub = Stub::serving(Vec::new()).expect("loopback");
    let auth = file_holding("auth", "author:s3cret\n");
    let auth_path = auth.0.to_string_lossy().into_owned();
    for wildcard in ["0.0.0.0", "::", "::ffff:0.0.0.0"] {
        let (code, said) = run_briefly(
            &stub.url(),
            &["--listen", wildcard, "--auth-file", &auth_path],
        );
        assert_eq!(code, Some(2), "{wildcard}: {said}");
        assert!(said.contains("is a wildcard"), "{wildcard}: {said}");
    }
}

#[test]
fn a_drive_server_with_an_auth_file_asks_every_request_for_it() {
    let stub = Stub::serving(Vec::new()).expect("loopback");
    let auth = file_holding("auth", "author:s3cret\n");
    let auth_path = auth.0.to_string_lossy().into_owned();
    let served = start(&stub.url(), &["--auth-file", &auth_path]);
    let address = served.listening.clone();
    let refused = exchange(
        &address,
        &format!("GET /events?from=0 HTTP/1.1\r\nHost: {address}\r\n\r\n"),
        |_| false,
    );
    assert_eq!(status(&refused), 401, "{refused}");
    let admitted = exchange(
        &address,
        &format!("GET /events?from=0 HTTP/1.1\r\nHost: {address}\r\n{AUTHOR}\r\n"),
        |read| read.contains("session.start"),
    );
    assert_eq!(status(&admitted), 200, "{admitted}");
}

#[test]
fn a_drive_server_answers_an_allowed_origin_through_its_proxy_host() {
    let stub = Stub::serving(Vec::new()).expect("loopback");
    let served = start(&stub.url(), &["--allow-origin", "http://localhost:5173"]);
    // What a development proxy forwards: the page's host and origin.
    let proxied = exchange(
        &served.listening,
        "GET /events?from=0 HTTP/1.1\r\nHost: localhost:5173\r\n\
         Origin: http://localhost:5173\r\n\r\n",
        |read| read.contains("session.start"),
    );
    assert_eq!(status(&proxied), 200, "{proxied}");
    let elsewhere = exchange(
        &served.listening,
        "GET /events?from=0 HTTP/1.1\r\nHost: localhost:5173\r\n\
         Origin: http://elsewhere.example\r\n\r\n",
        |_| false,
    );
    assert_eq!(status(&elsewhere), 403, "{elsewhere}");

    // An origin that could never match a browser's `Origin` is refused at
    // the command line rather than accepted and ignored.
    for never in ["localhost:5173", "http://localhost:5173/"] {
        let refused = Command::new(DRIVE)
            .args([
                "serve",
                "--endpoint",
                &stub.url(),
                "--model",
                "m",
                "--head",
                "x",
                "--allow-origin",
                never,
            ])
            .output()
            .expect("diet-drive runs");
        assert_eq!(refused.status.code(), Some(2), "{never}");
    }
}

#[test]
fn a_drive_server_sends_the_endpoint_key_from_a_file_never_an_argument() {
    let stub = Stub::serving(vec![Act::Raw(CAPTURED.to_vec())]).expect("loopback");
    let key_file = HeadFile(
        std::env::temp_dir().join(format!("diet-drive-serve-key-{}.txt", std::process::id())),
    );
    std::fs::write(&key_file.0, "k3y-for-the-endpoint\n").expect("the key file is written");
    let key_path = key_file.0.to_string_lossy().into_owned();
    let served = start(&stub.url(), &["--key-file", &key_path]);
    let address = served.listening.clone();
    let reply = post(&address, &address, r#"{"kind":"ask","text":"hi"}"#);
    assert_eq!(status(&reply), 200, "{reply}");
    // Waits for the answer, so the request has reached the stub.
    let _ = exchange(
        &address,
        &format!("GET /events?from=0 HTTP/1.1\r\nHost: {address}\r\n\r\n"),
        |read| read.contains(r#""to":"awaiting""#),
    );
    let heads = stub.heads();
    assert!(
        heads
            .first()
            .is_some_and(|head| head.contains("\r\nAuthorization: Bearer k3y-for-the-endpoint\r\n")),
        "the key file's key was not sent: {heads:?}"
    );

    // There is no flag that takes the key itself.
    let refused = Command::new(DRIVE)
        .args([
            "serve",
            "--endpoint",
            &stub.url(),
            "--model",
            "m",
            "--head",
            "x",
            "--key",
            "k3y",
        ])
        .output()
        .expect("diet-drive runs");
    assert_eq!(refused.status.code(), Some(2));
}

/// A regimen naming substrate `id`, as a file.
fn regimen_naming(id: &str) -> HeadFile {
    file_holding(
        "regimen",
        &format!(
            "arm = \"a\"\ndogma_version = 0\nsubstrate = \"{id}\"\n\
             substrate_reasoning = \"off\"\nsubstrate_hardware = \"{}\"\n\
             [sampler]\nseed = 7\n",
            "a".repeat(64)
        ),
    )
}

#[test]
fn a_drive_server_refuses_to_start_on_a_substrate_the_registry_does_not_resolve() {
    let stub = Stub::serving(Vec::new()).expect("loopback");
    // Not registered at all.
    let regimen = regimen_naming("nowhere-at-all");
    let path = regimen.0.to_string_lossy().into_owned();
    let (code, said) = run_briefly(&stub.url(), &["--regimen", &path]);
    assert_eq!(code, Some(1), "{said}");
    assert!(
        said.contains("`nowhere-at-all` is not a substrate"),
        "{said}"
    );
}

#[test]
fn a_drive_server_announces_the_registered_substrate_its_regimen_names() {
    let stub =
        Stub::serving_with_props(Vec::new(), &diet::drive::canned::build_info()).expect("loopback");
    let dev_loop = format!("{}/drive/dev-loop.toml", env!("CARGO_MANIFEST_DIR"));
    let served = start(&stub.url(), &["--regimen", &dev_loop]);
    assert_eq!(served.substrate.as_deref(), Some("canned-cache-n"));
    assert_eq!(
        served.registry_sha256,
        Some(diet::drive::registry::registry_sha256()),
        "and which registry answered"
    );
    // And without one, nothing is claimed.
    let served = start(&stub.url(), &[]);
    assert_eq!((&served.substrate, &served.registry_sha256), (&None, &None));
}

/// A regimen naming registered substrate `id`, with its equipment's own
/// hardware fingerprint, as a file.
fn regimen_registered(id: &str) -> HeadFile {
    let hardware = diet::drive::registry::identity(diet::drive::registry::REGISTRY, id)
        .expect("registered")
        .hardware_fingerprint;
    file_holding(
        "regimen",
        &format!(
            "arm = \"a\"\ndogma_version = 0\nsubstrate = \"{id}\"\n\
             substrate_reasoning = \"off\"\nsubstrate_hardware = \"{hardware}\"\n\
             [sampler]\nseed = 7\n"
        ),
    )
}

/// A `/props` reply reporting `build_info`.
fn props_saying(build_info: &str) -> Act {
    Act::Answer(format!("{{\"build_info\":\"{build_info}\"}}"))
}

#[test]
fn a_drive_server_starts_only_on_the_engine_the_registry_pins() {
    // A substrate with a registered commit; its binary self-reports build 1.
    let id = "accel24-llamacpp-qwen38-27b-iq3s";
    let commit = diet::drive::registry::identity(diet::drive::registry::REGISTRY, id)
        .expect("registered")
        .engine_commit
        .expect("an engine_commit");
    let regimen = regimen_registered(id);
    let path = regimen.0.to_string_lossy().into_owned();

    let build = format!("b1-{}", &commit[..7]);
    let stub = Stub::serving(vec![props_saying(&build), warm()]).expect("loopback");
    let served = start(&stub.url(), &["--regimen", &path]);
    assert_eq!(
        (
            served.substrate.as_deref(),
            served.engine_build.as_deref(),
            served.engine_identity.as_deref()
        ),
        (Some(id), Some(build.as_str()), Some("checked (commit)")),
        "the substrate, the build the check passed, and how"
    );
    let heads = stub.heads();
    assert!(heads[0].starts_with("GET /props HTTP/1.1\r\n"), "{heads:?}");

    // Another commit -- C0b's build, a production one commit past its pin --
    // and a dirty build of the pinned one: each refused before anything binds.
    for (build_info, names) in [
        // Each refusal's own words, not the build_info every refusal quotes.
        ("b8-e486f80", "the registry pins"),
        (&*format!("{build}-dirty"), "after its commit"),
    ] {
        let stub = Stub::serving(vec![props_saying(build_info)]).expect("loopback");
        let (code, said) = run_briefly(&stub.url(), &["--regimen", &path]);
        assert_eq!(code, Some(1), "{said}");
        assert!(
            said.contains(names) && !said.contains("listening"),
            "{said}"
        );
    }
    // A server that does not answer `/props` is no pass.
    let stub = Stub::serving(vec![Act::Status(404, "no such route".to_owned())]).expect("loopback");
    let (code, said) = run_briefly(&stub.url(), &["--regimen", &path]);
    assert_eq!(code, Some(1), "{said}");
    assert!(said.contains("answered 404"), "{said}");
}

#[test]
fn a_drive_server_refuses_a_substrate_with_no_registered_engine_identity_without_asking_it() {
    let stub = Stub::serving(vec![props_saying("b1-4ceb171")]).expect("loopback");
    let regimen = regimen_registered("all-MiniLM-L6-v2");
    let path = regimen.0.to_string_lossy().into_owned();
    let (code, said) = run_briefly(&stub.url(), &["--regimen", &path]);
    assert_eq!(code, Some(1), "{said}");
    assert!(
        said.contains("neither an `engine_build_info` nor an `engine_commit`"),
        "{said}"
    );
    assert!(stub.heads().is_empty(), "{:?}", stub.heads());
}

#[test]
fn a_drive_server_checks_the_engine_before_it_binds() {
    // On a port already taken, a server that bound first would fail to
    // listen (exit 2, halt); one that checks first refuses the engine (exit 1).
    let regimen = regimen_registered("accel24-llamacpp-qwen38-27b-iq3s");
    let path = regimen.0.to_string_lossy().into_owned();
    let taken = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
    // Through the type: `taken` dot `local` reads as a hostname to hygiene.
    let port = std::net::TcpListener::local_addr(&taken)
        .expect("its address")
        .port()
        .to_string();
    let stub = Stub::serving(vec![props_saying("b8-e486f80")]).expect("loopback");
    let (code, said) = run_briefly(&stub.url(), &["--regimen", &path, "--port", &port]);
    assert_eq!(code, Some(1), "{said}");
    assert!(said.contains("e486f80"), "the engine's refusal: {said}");
}

#[test]
fn a_drive_server_refuses_a_wildcard_before_it_asks_the_engine() {
    // A usage refusal is made before anything touches the network: the
    // endpoint is never sent the bearer for a server that could not start.
    let regimen = regimen_registered("accel24-llamacpp-qwen38-27b-iq3s");
    let path = regimen.0.to_string_lossy().into_owned();
    let stub = Stub::serving(vec![props_saying("b8-e486f80")]).expect("loopback");
    let (code, said) = run_briefly(&stub.url(), &["--regimen", &path, "--listen", "0.0.0.0"]);
    assert_eq!(code, Some(2), "{said}");
    assert!(said.contains("is a wildcard"), "{said}");
    assert!(stub.heads().is_empty(), "{:?}", stub.heads());
}

#[test]
fn a_drive_server_killed_mid_session_leaves_a_log_whole_through_what_it_showed() {
    // #230, ruled: no signal handler. Killed without warning, the log holds
    // every line `/events` showed before the kill, each whole and in order,
    // and nothing after its last line break but one torn line at most, which
    // the reader counts as torn.
    let stub = Stub::serving(vec![Act::Raw(CAPTURED.to_vec())]).expect("loopback");
    let log_file = file_holding("log", "");
    let path = log_file.0.to_string_lossy().into_owned();
    let mut served = start(&stub.url(), &["--log", &path]);
    let address = served.listening.clone();
    let reply = post(&address, &address, r#"{"kind":"ask","text":"hi"}"#);
    assert_eq!(status(&reply), 200, "{reply}");
    let seen = exchange(
        &address,
        &format!("GET /events?from=0 HTTP/1.1\r\nHost: {address}\r\n\r\n"),
        |read| complete_data_events(read) >= 2,
    );
    served.child.kill().expect("the server is killed");
    let _ = served.child.wait();

    let shown: Vec<&str> = seen
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .collect();
    let bytes = std::fs::read(&log_file.0).expect("the log");
    // Split on line breaks as bytes: a kill may cut inside a character.
    let mut segments: Vec<&[u8]> = bytes.split(|byte| *byte == b'\n').collect();
    let tail = segments.pop().unwrap_or_default();
    let lines: Vec<&str> = segments
        .iter()
        .map(|line| std::str::from_utf8(line).expect("every whole line is UTF-8"))
        .collect();
    assert!(
        lines.len() >= shown.len() && lines[..shown.len()] == shown[..],
        "every line shown before the kill is in the log, whole: {lines:?}"
    );
    for line in &lines {
        let _ = log_line_object(line);
    }
    // What follows the last line break is what #259's reader allows and
    // nothing else: nothing; a complete event, read as one; or the start of
    // one, a torn write the reader sets aside and counts (`torn: 1`).
    assert!(
        tail.is_empty() || tail[0] == b'{',
        "only an event's start follows the last line break: {tail:?}"
    );
    let tail_is_an_event = std::str::from_utf8(tail)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .is_some_and(|value| value.is_object());

    // And #259's reader agrees: the log reads through its last complete
    // event, a torn tail set aside and counted.
    let checked = Command::new(DIET)
        .arg("check-log")
        .arg(&log_file.0)
        .output()
        .expect("diet runs");
    let said = String::from_utf8_lossy(&checked.stdout);
    assert_eq!(checked.status.code(), Some(0), "{said}");
    let read = log_line_object(&said);
    assert_eq!(
        (
            read["value"]["events"].as_array().map(Vec::len),
            read["value"]["torn"].as_u64()
        ),
        (
            Some(lines.len() + usize::from(tail_is_an_event)),
            Some(u64::from(!tail.is_empty() && !tail_is_an_event))
        ),
        "{said}"
    );
}

#[test]
fn a_drive_server_exits_0_after_ended_and_ended_is_its_logs_last_line() {
    // #291: no interrupt needed, and nothing logged after `ended`.
    let stub = Stub::serving(vec![Act::Raw(ANSWERED.to_vec())]).expect("loopback");
    let log_file = file_holding("log", "");
    let path = log_file.0.to_string_lossy().into_owned();
    let mut served = start(&stub.url(), &["--log", &path]);
    let address = served.listening.clone();
    let reply = post(&address, &address, r#"{"kind":"ask","text":"hi"}"#);
    assert_eq!(status(&reply), 200, "{reply}");
    let _ = exchange(
        &address,
        &format!("GET /events?from=0 HTTP/1.1\r\nHost: {address}\r\n\r\n"),
        |read| read.contains(r#""reason":"final""#) && read.contains(r#""to":"awaiting""#),
    );
    // A page streaming the log when the session ends sees `ended`, then the
    // stream closes.
    let watcher = {
        let address = address.clone();
        std::thread::spawn(move || {
            exchange(
                &address,
                &format!("GET /events?from=0 HTTP/1.1\r\nHost: {address}\r\n\r\n"),
                |_| false,
            )
        })
    };
    std::thread::sleep(Duration::from_millis(200));
    let reply = post(&address, &address, r#"{"kind":"end"}"#);
    assert_eq!(status(&reply), 200, "{reply}");

    let deadline = Instant::now() + Duration::from_secs(10);
    let exited = loop {
        if let Some(exited) = served.child.try_wait().expect("the child is waited on") {
            break Some(exited);
        }
        if Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(
        exited.and_then(|exited| exited.code()),
        Some(0),
        "serve did not exit on its own"
    );
    let written = std::fs::read_to_string(&log_file.0).expect("the log");
    let last = written.lines().last().expect("a line");
    assert!(
        last.contains(r#""kind":"settlement""#) && last.contains(r#""to":"ended""#),
        "{written}"
    );
    let streamed = watcher.join().expect("the watcher");
    assert!(streamed.contains(r#""to":"ended""#), "{streamed}");
}

#[test]
fn a_drive_servers_log_file_is_the_events_stream_line_for_line() {
    let stub = Stub::serving(vec![Act::Raw(CAPTURED.to_vec())]).expect("loopback");
    let log_file = file_holding("log", "");
    let path = log_file.0.to_string_lossy().into_owned();
    let served = start(&stub.url(), &["--log", &path]);
    assert_eq!(
        served.log,
        Some((path.clone(), false)),
        "the announcement names the log, and nothing was emptied"
    );
    let address = served.listening.clone();
    let reply = post(&address, &address, r#"{"kind":"ask","text":"hi"}"#);
    assert_eq!(status(&reply), 200, "{reply}");
    let settled = exchange(
        &address,
        &format!("GET /events?from=0 HTTP/1.1\r\nHost: {address}\r\n\r\n"),
        |read| read.contains(r#""from":"capture""#) && read.contains(r#""to":"awaiting""#),
    );
    let at_least = settled.matches("data: ").count();

    // The file is written by its own thread: read it once it has caught up.
    let deadline = Instant::now() + Duration::from_secs(10);
    let written = loop {
        let written = std::fs::read_to_string(&log_file.0).unwrap_or_default();
        if written.lines().count() >= at_least || Instant::now() >= deadline {
            break written;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    // And the stream, read again to the file's length, line for line.
    let stream = exchange(
        &address,
        &format!("GET /events?from=0 HTTP/1.1\r\nHost: {address}\r\n\r\n"),
        |read| complete_data_events(read) >= written.lines().count(),
    );
    let streamed = stream
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .fold(String::new(), |mut out, data| {
            out.push_str(data);
            out.push('\n');
            out
        });
    assert!(written.lines().count() >= at_least, "{written}");
    assert_eq!(
        written, streamed,
        "the log file is the stream's data, byte for byte"
    );
}

#[test]
fn a_drive_server_starts_on_a_substrate_of_several_shards() {
    // Two main shards and a draft, which record v1 spells since #211: the
    // substrate resolves, and its server on the registered engine starts.
    let id = "ada48-llamacpp-qwen38flashnext-q20";
    let regimen = regimen_registered(id);
    let path = regimen.0.to_string_lossy().into_owned();
    // The build its registered engine reports, read from the registry rather
    // than written here: the entry moves with its instances (#225).
    let registered = diet::drive::registry::identity(diet::drive::registry::REGISTRY, id)
        .expect("two shards resolve");
    let build = registered.engine_build_info.unwrap_or_else(|| {
        format!(
            "b1-{}",
            &registered.engine_commit.expect("an engine identity")[..7]
        )
    });
    let stub = Stub::serving(vec![props_saying(&build), warm()]).expect("loopback");
    let served = start(&stub.url(), &["--regimen", &path]);
    assert_eq!(
        (served.substrate.as_deref(), served.engine_build.as_deref()),
        (Some(id), Some(build.as_str()))
    );
}

#[test]
fn a_drive_server_starts_on_a_prebuilt_engine_by_its_literal() {
    // The floor: a prebuilt engine whose `/props` names no commit, declared
    // by the literal it reports (#209). Ruled onto this PR by #214.
    let id = "accel24-beellama-qwen27b-q4kxl";
    let regimen = regimen_registered(id);
    let path = regimen.0.to_string_lossy().into_owned();
    let stub = Stub::serving(vec![props_saying("b0-unknown-dirty"), warm()]).expect("loopback");
    let served = start(&stub.url(), &["--regimen", &path]);
    assert_eq!(
        (
            served.substrate.as_deref(),
            served.engine_build.as_deref(),
            served.engine_identity.as_deref()
        ),
        (
            Some(id),
            Some("b0-unknown-dirty"),
            Some("unreported (literal matched)")
        )
    );
    // Any other build is refused, naming the literal it is not.
    let stub = Stub::serving(vec![props_saying("b1-4ceb171")]).expect("loopback");
    let (code, said) = run_briefly(&stub.url(), &["--regimen", &path]);
    assert_eq!(code, Some(1), "{said}");
    assert!(
        said.contains("reports exactly") && said.contains("b0-unknown-dirty"),
        "{said}"
    );
}

/// What `TabbyAPI`'s `GET /v1/model` answered for the 3.8 line's config r2,
/// as the r2 window captured it (Track 4's committed record), with
/// `cache_size` as given: the active template's name and text among it.
fn tabby_model_card(cache_size: u64) -> Act {
    let mut card: serde_json::Value = serde_json::from_str(include_str!(
        "../../substrates/measurements/2026-10-05-accel24-tabbyapi-exl3-27b-r2/window/raw/model.json"
    ))
    .expect("the captured model card");
    card["parameters"]["cache_size"] = serde_json::Value::from(cache_size);
    Act::Answer(card.to_string())
}

#[test]
fn a_drive_server_confirms_a_declared_engines_model_settings_and_draft() {
    // #509: TabbyAPI reports no build, so its entry declares the engine and
    // serve does not ask `/props`; it reads `GET /v1/model` for the model and
    // settings the entry declares, and -- the engine warming itself, a draft
    // declared -- one probe request whose timings show the draft ran.
    let id = "accel24-tabbyapi-exl3-qwen38-27b-3p00";
    let commit = diet::drive::registry::identity(diet::drive::registry::REGISTRY, id)
        .expect("registered")
        .engine_commit
        .expect("an engine_commit");
    let regimen = regimen_registered(id);
    let path = regimen.0.to_string_lossy().into_owned();
    let stub = Stub::serving(vec![tabby_model_card(163_840), warm()]).expect("loopback");
    let log_file = file_holding("log", "");
    let logged = log_file.0.to_string_lossy().into_owned();
    let served = start(&stub.url(), &["--regimen", &path, "--log", &logged]);
    assert_eq!(
        (
            served.substrate.as_deref(),
            served.engine_build.as_deref(),
            served.engine_identity.as_deref()
        ),
        (
            Some(id),
            Some(commit.as_str()),
            Some("declared (the engine not asked)")
        )
    );
    let heads = stub.heads();
    assert!(
        heads[0].starts_with("GET /v1/model HTTP/1.1\r\n"),
        "{heads:?}"
    );
    assert!(
        heads[1].starts_with("POST /v1/chat/completions "),
        "{heads:?}"
    );
    let start_line = first_logged_line(&log_file.0);
    let corroborated = |field: &str, value: &str| {
        serde_json::json!({
            "field": field, "value": value, "provenance": "corroborated", "reported": value,
        })
    };
    assert_eq!(
        start_line["served"],
        serde_json::json!([
            {"field": "engine_commit", "value": commit, "provenance": "declared"},
            corroborated("served_cache_mode", "8,8"),
            corroborated("served_cache_size", "163840"),
            corroborated(
                "served_chat_template_sha256",
                "c3cf9e34abf4f9e36c2d72165aa9c132d3e2a725b6c2586aaa3a8af9d7a81041"
            ),
            corroborated("served_chunk_size", "2048"),
            corroborated("served_max_batch_size", "2"),
            corroborated("served_max_seq_len", "163840"),
            corroborated("served_model", "Qwen3.8-27B-exl3-3.00bpw-img1024"),
            corroborated("served_prompt_template", "chat_template"),
            corroborated("served_use_vision", "true"),
            {
                "field": "served_draft", "value": "true", "provenance": "corroborated",
                "reported": "draft_n 72, draft_n_accepted 44",
            },
        ]),
        "{start_line}"
    );
    assert_eq!(start_line["version"], 7, "{start_line}");
    // And its server speaks TabbyAPI's dialect, by name (#496).
    assert_eq!(start_line["serving"]["dialect"], "tabbyapi", "{start_line}");
}

#[test]
fn a_drive_server_refuses_a_contradicted_setting_and_takes_a_silent_draft_as_declared() {
    let id = "accel24-tabbyapi-exl3-qwen38-27b-3p00";
    let regimen = regimen_registered(id);
    let path = regimen.0.to_string_lossy().into_owned();
    // A cache the entry does not declare: refused before the probe, naming
    // the field and both values.
    let stub = Stub::serving(vec![tabby_model_card(131_072), warm()]).expect("loopback");
    let (code, said) = run_briefly(&stub.url(), &["--regimen", &path]);
    assert_eq!(code, Some(1), "{said}");
    assert!(
        said.contains("`served_cache_size`")
            && said.contains("\\\"163840\\\"")
            && said.contains("\\\"131072\\\""),
        "{said}"
    );
    assert_eq!(stub.heads().len(), 1, "no probe after a contradiction");
    // A declared draft whose probe produced no draft tokens is silence, not
    // a contradiction: the start goes on, the draft declared.
    let stub = Stub::serving(vec![tabby_model_card(163_840), Act::Raw(CAPTURED.to_vec())])
        .expect("loopback");
    let log_file = file_holding("log", "");
    let logged = log_file.0.to_string_lossy().into_owned();
    let served = start(&stub.url(), &["--regimen", &path, "--log", &logged]);
    assert_eq!(served.substrate.as_deref(), Some(id));
    let start_line = first_logged_line(&log_file.0);
    let draft = start_line["served"]
        .as_array()
        .and_then(|fields| fields.iter().find(|field| field["field"] == "served_draft"))
        .cloned();
    assert_eq!(
        draft,
        Some(
            serde_json::json!({"field": "served_draft", "value": "true", "provenance": "declared"})
        ),
        "{start_line}"
    );
    drop(served);
    // An unreachable server is refused as one.
    let gone = Stub::serving(Vec::new()).expect("loopback");
    let url = gone.url();
    drop(gone.received());
    let (code, said) = run_briefly(&url, &["--regimen", &path]);
    assert_eq!(code, Some(1), "{said}");
    assert!(said.contains("not reachable"), "{said}");
}

#[test]
fn a_drive_server_given_a_base_url_asks_its_chat_completions() {
    // #496's live turn: a server's bare base URL was a 404 on the first
    // request. A base URL, or one ending in `/v1`, is completed.
    for suffix in ["", "/", "/v1"] {
        let stub = Stub::serving(vec![Act::Raw(ANSWERED.to_vec())]).expect("loopback");
        let url = stub.url();
        let base = format!(
            "{}{suffix}",
            url.strip_suffix("/v1/chat/completions")
                .expect("the stub's path")
        );
        let served = start(&base, &[]);
        let address = served.listening.clone();
        let reply = post(&address, &address, r#"{"kind":"ask","text":"hi"}"#);
        assert_eq!(status(&reply), 200, "{reply}");
        let read = exchange(
            &address,
            &format!("GET /events?from=0 HTTP/1.1\r\nHost: {address}\r\n\r\n"),
            |read| read.contains("turn.settled"),
        );
        assert!(read.contains("turn.settled"), "{read}");
        drop(served);
        let heads = stub.heads();
        assert!(
            heads
                .iter()
                .any(|head| head.starts_with("POST /v1/chat/completions ")),
            "{base}: {heads:#?}"
        );
    }
}

#[test]
fn a_drive_server_sends_the_substrates_declared_template_kwargs() {
    // The reasoning ruling: Qwen's convention keeps reasoning in history, so
    // the floor's entry declares `preserve_thinking` and serve sends it on
    // every request; with thinking on and no level, the log names the
    // template's default.
    let id = "accel24-tabbyapi-exl3-qwen38-27b-3p00";
    let hardware = diet::drive::registry::identity(diet::drive::registry::REGISTRY, id)
        .expect("registered")
        .hardware_fingerprint;
    let regimen = file_holding(
        "regimen",
        &format!(
            "arm = \"a\"\ndogma_version = 0\nsubstrate = \"{id}\"\n\
             substrate_reasoning = \"on\"\nsubstrate_hardware = \"{hardware}\"\n\
             [sampler]\nseed = 7\n"
        ),
    );
    let path = regimen.0.to_string_lossy().into_owned();
    let stub = Stub::serving(vec![tabby_model_card(163_840), warm()]).expect("loopback");
    let log_file = file_holding("log", "");
    let logged = log_file.0.to_string_lossy().into_owned();
    let _served = start(&stub.url(), &["--regimen", &path, "--log", &logged]);
    let start_line = first_logged_line(&log_file.0);
    assert_eq!(
        start_line["template_kwargs"],
        serde_json::json!({"enable_thinking": true, "preserve_thinking": true}),
        "{start_line}"
    );
    assert_eq!(
        start_line["reasoning_effort_default"], "xhigh",
        "{start_line}"
    );
}

#[test]
fn a_drive_server_says_when_its_log_flag_emptied_a_file() {
    // Ruled on #230: the file is truncated, as the scripted path's output is,
    // and the announcement says so.
    let stub = Stub::serving(Vec::new()).expect("loopback");
    let held = file_holding("log", "an earlier session's log\n");
    let path = held.0.to_string_lossy().into_owned();
    let served = start(&stub.url(), &["--log", &path]);
    assert_eq!(served.log, Some((path, true)));
    // And without the flag, nothing is claimed.
    let served = start(&stub.url(), &[]);
    assert_eq!(served.log, None);
}

/// `diet-drive serve` started through `sh` with `prelude` before it, its
/// first line read and its stdout then closed -- unlike `start`, which keeps
/// reading.
fn start_through_sh(prelude: &str, endpoint: &str, extra: &[&str]) -> (Child, String, HeadFile) {
    let head = file_holding("head", HEAD);
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(format!("{prelude}; exec \"$0\" \"$@\""))
        .arg(DRIVE)
        .args([
            "serve",
            "--endpoint",
            endpoint,
            "--model",
            "a-model",
            "--head",
        ])
        .arg(&head.0)
        .args(extra)
        .stdout(Stdio::piped())
        .spawn()
        .expect("sh starts");
    let mut first = String::new();
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout is piped"));
    let _ = stdout.read_line(&mut first);
    drop(stdout);
    let listening = log_line_object(&first)["listening"]
        .as_str()
        .unwrap_or_else(|| panic!("no announcement: {first}"))
        .to_owned();
    (child, listening, head)
}

#[test]
fn a_drive_server_whose_log_cannot_be_written_stops_even_with_its_stdout_closed() {
    // A file-size limit of one block, its signal ignored, so the log's
    // write fails as a full disk's would; and stdout closed after the
    // announcement, which once turned the failure into a panic that left
    // the server running with its log stopped (#230's review, finding 1).
    let stub = Stub::serving(vec![Act::Raw(CAPTURED.to_vec())]).expect("loopback");
    let log_file = file_holding("log", "");
    let path = log_file.0.to_string_lossy().into_owned();
    let (mut child, address, _head) =
        start_through_sh("ulimit -f 1; trap '' XFSZ", &stub.url(), &["--log", &path]);
    let reply = post(&address, &address, r#"{"kind":"ask","text":"hi"}"#);
    assert_eq!(status(&reply), 200, "{reply}");
    let give_up = Instant::now() + Duration::from_secs(10);
    let exited = loop {
        if let Some(exited) = child.try_wait().expect("the child is waited on") {
            break exited.code();
        }
        if Instant::now() >= give_up {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(exited, Some(3), "a log that stopped stops the server");
}

#[test]
fn a_drive_server_refuses_an_uncreatable_log_file_before_anything_binds() {
    let stub = Stub::serving(Vec::new()).expect("loopback");
    let path = std::env::temp_dir()
        .join(format!("diet-drive-no-such-dir-{}", std::process::id()))
        .join("log.jsonl");
    let (code, said) = run_briefly(&stub.url(), &["--log", &path.to_string_lossy()]);
    assert_eq!(code, Some(3), "{said}");
    assert!(
        said.contains("cannot be written") && !said.contains("listening"),
        "{said}"
    );
}

#[test]
fn a_drive_server_that_fails_to_start_leaves_an_existing_log_file_as_it_was() {
    // Emptied only once the server runs (#230's review, finding 3).
    let held = file_holding("log", "an earlier session's log\n");
    let path = held.0.to_string_lossy().into_owned();
    let taken = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
    let port = std::net::TcpListener::local_addr(&taken)
        .expect("its address")
        .port()
        .to_string();
    let stub = Stub::serving(Vec::new()).expect("loopback");
    let (code, said) = run_briefly(&stub.url(), &["--log", &path, "--port", &port]);
    // Exit 2 is shared with a usage refusal: the reason says it was the bind
    // (#264, round 3).
    assert_eq!(code, Some(2), "the bind fails: {said}");
    assert!(
        said.contains("cannot listen on"),
        "the bind, not usage: {said}"
    );
    assert_eq!(
        std::fs::read_to_string(&held.0).expect("still there"),
        "an earlier session's log\n"
    );
}

/// The dev loop's regimen, which names the canned substrate.
fn dev_loop() -> String {
    format!("{}/drive/dev-loop.toml", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn a_drive_server_starts_a_canned_regimen_only_on_the_canned_server() {
    // #219 item 11: the canned regime is checked like any other, by the
    // literal its server reports (`canned-` and the acts' digest).
    let canned = diet::drive::canned::build_info();
    let stub = Stub::serving_with_props(Vec::new(), &canned).expect("loopback");
    let served = start(&stub.url(), &["--regimen", &dev_loop()]);
    assert_eq!(
        (
            served.substrate.as_deref(),
            served.engine_build.as_deref(),
            served.engine_identity.as_deref()
        ),
        (
            Some("canned-cache-n"),
            Some(canned.as_str()),
            Some("unreported (literal matched)")
        )
    );
}

/// The first line of the log at `path`, once its writer has put one there.
fn first_logged_line(path: &std::path::Path) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let written = std::fs::read_to_string(path).unwrap_or_default();
        if let Some(first) = written.lines().next() {
            return log_line_object(first);
        }
        assert!(Instant::now() < deadline, "nothing was logged within 10 s");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_drive_servers_log_carries_the_substrate_claim_it_announced() {
    // #292: the claim reached stdout only, so a log read later could not say
    // which substrate the session claimed. Both paths of the engine check,
    // so a swapped identity word is seen: the commit path, and the canned
    // regimen's literal.
    let id = "accel24-llamacpp-qwen38-27b-iq3s";
    let commit = diet::drive::registry::identity(diet::drive::registry::REGISTRY, id)
        .expect("registered")
        .engine_commit
        .expect("an engine_commit");
    let regimen = regimen_registered(id);
    let committed = regimen.0.to_string_lossy().into_owned();
    let build = format!("b1-{}", &commit[..7]);
    let canned = diet::drive::canned::build_info();
    for (stub, regimen, announced_as, logged_as) in [
        (
            Stub::serving(vec![props_saying(&build), warm()]).expect("loopback"),
            committed,
            "checked (commit)",
            "engine_commit",
        ),
        (
            Stub::serving_with_props(Vec::new(), &canned).expect("loopback"),
            dev_loop(),
            "unreported (literal matched)",
            "engine_build_info",
        ),
    ] {
        let log_file = file_holding("log", "");
        let path = log_file.0.to_string_lossy().into_owned();
        let served = start(&stub.url(), &["--regimen", &regimen, "--log", &path]);
        assert_eq!(served.engine_identity.as_deref(), Some(announced_as));
        let start_line = first_logged_line(&log_file.0);
        assert_eq!(start_line["kind"], "session.start", "{start_line}");
        let claimed = |key: &str| start_line[key].as_str().map(str::to_owned);
        // v7 (#509): the engine's field, corroborated, and what it reported.
        let field = |key: &str| start_line["served"][0][key].as_str().map(str::to_owned);
        assert_eq!(
            (
                claimed("substrate"),
                claimed("registry_sha256"),
                field("reported"),
                field("field"),
                field("provenance"),
            ),
            (
                served.substrate.clone(),
                served.registry_sha256.clone(),
                served.engine_build.clone(),
                Some(logged_as.to_owned()),
                Some("corroborated".to_owned()),
            ),
            "the log claims what stdout announced: {start_line}"
        );
        assert!(served.substrate.is_some() && served.engine_build.is_some());
        drop(served);

        // And the reader takes it: the claim is the format's, whole.
        let checked = Command::new(DIET)
            .arg("check-log")
            .arg(&log_file.0)
            .output()
            .expect("diet runs");
        let said = String::from_utf8_lossy(&checked.stdout);
        assert_eq!(checked.status.code(), Some(0), "{said}");
        let read = log_line_object(&said);
        assert_eq!(
            read["value"]["events"][0]["served"][0]["field"].as_str(),
            Some(logged_as),
            "{said}"
        );
    }

    // Without a regimen, nothing is claimed.
    let stub = Stub::serving(Vec::new()).expect("loopback");
    let log_file = file_holding("log", "");
    let path = log_file.0.to_string_lossy().into_owned();
    let served = start(&stub.url(), &["--log", &path]);
    let start_line = first_logged_line(&log_file.0);
    drop(served);
    for key in [
        "substrate",
        "registry_sha256",
        "engine_build",
        "engine_identity",
    ] {
        assert!(start_line.get(key).is_none(), "{start_line}");
    }
}

#[test]
fn a_drive_server_refuses_a_canned_regimen_against_a_live_server() {
    // A live llama.cpp under the dev loop's regimen once started and
    // announced a canned identity that was false (#219 item 11).
    let stub = Stub::serving(vec![props_saying("b8-e486f80")]).expect("loopback");
    let (code, said) = run_briefly(&stub.url(), &["--regimen", &dev_loop()]);
    assert_eq!(code, Some(1), "{said}");
    assert!(
        said.contains("b8-e486f80") && said.contains(&diet::drive::canned::build_info()),
        "both values, the registry's literal among them: {said}"
    );
    assert!(
        said.contains(diet::drive::engine::CANNED_SERVER_ONLY),
        "and where a canned substrate is served: {said}"
    );
}

#[test]
fn a_drive_server_refuses_a_canned_regimen_against_a_server_whose_props_has_no_build_info() {
    // The stand-in's case (#219's dry run): `/props` answers with no
    // `build_info` at all, and the refusal still says who serves canned.
    let stub = Stub::serving(vec![Act::Answer("{}".to_owned())]).expect("loopback");
    let (code, said) = run_briefly(&stub.url(), &["--regimen", &dev_loop()]);
    assert_eq!(code, Some(1), "{said}");
    assert!(
        said.contains("no string `build_info`")
            && said.contains(diet::drive::engine::CANNED_SERVER_ONLY),
        "{said}"
    );
}

#[test]
fn a_drive_server_refuses_a_registered_model_substrate_without_the_canned_sentence() {
    let regimen = regimen_registered("accel24-beellama-qwen27b-q4kxl");
    let stub = Stub::serving(vec![props_saying("b8-e486f80")]).expect("loopback");
    let (code, said) = run_briefly(&stub.url(), &["--regimen", &regimen.0.to_string_lossy()]);
    assert_eq!(code, Some(1), "{said}");
    assert!(
        !said.contains(diet::drive::engine::CANNED_SERVER_ONLY),
        "{said}"
    );
}

#[test]
fn a_drive_server_refuses_a_canned_regimen_against_a_server_with_no_props() {
    let stub = Stub::serving(vec![Act::Status(404, "no such route".to_owned())]).expect("loopback");
    let (code, said) = run_briefly(&stub.url(), &["--regimen", &dev_loop()]);
    assert_eq!(code, Some(1), "{said}");
    assert!(said.contains("answered 404"), "{said}");
}

/// `diet`, the checker.
const DIET: &str = env!("CARGO_BIN_EXE_diet");

/// The start names the regimen's bytes as serve read them, and the summary's
/// product is the file beside the record: the dev-loop regimen warrants no
/// fork, so the working memory is empty.
fn the_start_and_summary_name_the_regimen_and_the_product(
    written: &str,
    report: &serde_json::Value,
    path: &str,
) {
    let rows: Vec<serde_json::Value> = written.lines().map(log_line_object).collect();
    assert_eq!(
        rows[0]["regimen_sha256"].as_str(),
        Some(diet::digest::sha256_hex(&std::fs::read(dev_loop()).expect("the regimen")).as_str()),
        "{written}"
    );
    // The dev-loop regimen warrants no fork and runs no commands.
    assert_eq!(
        (
            rows[0]["levers"]["fork_warrant"].as_str(),
            rows[0]["levers"]["approval"].as_str()
        ),
        (Some("none"), Some("undeclared")),
        "{written}"
    );
    // The canned server names no window, so no request was clamped (#588).
    assert!(
        rows[0]["levers"]["step_and_output_limits"]
            .as_str()
            .is_some_and(|word| word.ends_with(":unclamped")),
        "{written}"
    );
    let product = PathBuf::from(format!("{path}.product.txt"));
    assert_eq!(std::fs::read(&product).expect("the product"), b"");
    assert_eq!(
        (
            rows[4]["product_sha256"].as_str(),
            report["product_sha256"].as_str()
        ),
        (
            Some(diet::digest::sha256_hex(b"").as_str()),
            Some(diet::digest::sha256_hex(b"").as_str())
        ),
        "{written}"
    );
    let _ = std::fs::remove_file(&product);
}

#[test]
fn a_drive_server_records_a_two_turn_session_that_check_record_reads() {
    // #157's acceptance as ruled (a): the projection and the unspellable
    // path, nothing about counts -- the canned substrate is not a cited
    // engine, so every turn and response is named, never derived.
    // The canned regimen passes only on the canned server (#243).
    let stub = Stub::serving_with_props(
        vec![Act::Raw(ANSWERED.to_vec()), Act::Raw(ANSWERED.to_vec())],
        &diet::drive::canned::build_info(),
    )
    .expect("loopback");
    let record = file_holding("record", "");
    let path = record.0.to_string_lossy().into_owned();
    let served = start(&stub.url(), &["--regimen", &dev_loop(), "--record", &path]);
    assert_eq!(
        served.record,
        Some((path.clone(), false)),
        "nothing was there to empty"
    );
    let address = served.listening.clone();
    for turn in 1..=2 {
        let reply = post(&address, &address, r#"{"kind":"ask","text":"hi"}"#);
        assert_eq!(status(&reply), 200, "{reply}");
        exchange(
            &address,
            &format!("GET /events?from=0 HTTP/1.1\r\nHost: {address}\r\n\r\n"),
            |read| read.matches(r#""to":"awaiting""#).count() >= turn,
        );
    }
    let reply = post(&address, &address, r#"{"kind":"end"}"#);
    assert_eq!(status(&reply), 200, "{reply}");

    let sidecar = PathBuf::from(format!("{path}.unspellable.json"));
    // The report, written after both files: its digests are theirs.
    let report = log_line_object(
        &served
            .said
            .recv_timeout(Duration::from_secs(10))
            .expect("the record's report"),
    );
    assert_eq!(
        (
            report["record_sha256"].as_str().map(str::to_owned),
            report["sidecar_sha256"].as_str().map(str::to_owned)
        ),
        (
            Some(diet::digest::sha256_hex(
                &std::fs::read(&record.0).expect("the record")
            )),
            Some(diet::digest::sha256_hex(
                &std::fs::read(&sidecar).expect("the sidecar")
            ))
        ),
        "{report}"
    );
    let checked = Command::new(DIET)
        .args(["check-record"])
        .arg(&record.0)
        .output()
        .expect("diet runs");
    assert_eq!(
        checked.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&checked.stdout)
    );
    let written = std::fs::read_to_string(&record.0).expect("the record");
    let kinds: Vec<String> = written
        .lines()
        .map(|line| {
            log_line_object(line)["record"]
                .as_str()
                .expect("a row's kind")
                .to_owned()
        })
        .collect();
    // The second request's head grew by the first turn's ask and answer,
    // rebuilt from the log and named by client::head (ruled on #157).
    assert_eq!(
        kinds,
        ["start", "request", "request", "prefix.changed", "summary"],
        "{written}"
    );
    the_start_and_summary_name_the_regimen_and_the_product(&written, &report, &path);
    assert!(
        !written.contains(r#""reason":"unattributed""#),
        "the head was rebuilt: {written}"
    );
    let named = log_line_object(&std::fs::read_to_string(&sidecar).expect("the sidecar"));
    let items = named["unspellable"].as_array().expect("a list");
    let named_kinds: Vec<&str> = items
        .iter()
        .filter_map(|item| item["kind"].as_str())
        .collect();
    assert_eq!(
        named_kinds,
        ["ask", "response", "ask", "response"],
        "{named}"
    );
    assert!(
        items[1]["text"].is_string(),
        "the answer's text is kept: {named}"
    );
    let _ = std::fs::remove_file(&sidecar);
}

/// #573: the log's `session.start` carries the same `levers` as the
/// record's start row -- one reading of the regimen, written to both, the
/// window clamp's word included.
#[test]
fn a_drive_servers_log_and_record_start_carry_the_same_levers() {
    let stub = Stub::serving_with_props(
        vec![Act::Raw(ANSWERED.to_vec())],
        &diet::drive::canned::build_info(),
    )
    .expect("loopback");
    let record = file_holding("record", "");
    let log = file_holding("log", "");
    let record_path = record.0.to_string_lossy().into_owned();
    let log_path = log.0.to_string_lossy().into_owned();
    let served = start(
        &stub.url(),
        &[
            "--regimen",
            &dev_loop(),
            "--record",
            &record_path,
            "--log",
            &log_path,
        ],
    );
    let address = served.listening.clone();
    let reply = post(&address, &address, r#"{"kind":"ask","text":"hi"}"#);
    assert_eq!(status(&reply), 200, "{reply}");
    exchange(
        &address,
        &format!("GET /events?from=0 HTTP/1.1\r\nHost: {address}\r\n\r\n"),
        |read| read.contains(r#""to":"awaiting""#),
    );
    let reply = post(&address, &address, r#"{"kind":"end"}"#);
    assert_eq!(status(&reply), 200, "{reply}");
    served
        .said
        .recv_timeout(Duration::from_secs(10))
        .expect("the record's report");
    let logged = first_logged_line(&log.0);
    let written = std::fs::read_to_string(&record.0).expect("the record");
    let start_row = log_line_object(written.lines().next().expect("a start row"));
    assert_eq!(logged["kind"], "session.start", "{logged}");
    assert!(
        logged["levers"]["step_and_output_limits"]
            .as_str()
            .is_some_and(|word| word.ends_with(":unclamped")),
        "{logged}"
    );
    assert_eq!(logged["levers"], start_row["levers"], "{logged}\n{written}");
    let _ = std::fs::remove_file(format!("{record_path}.unspellable.json"));
    let _ = std::fs::remove_file(format!("{record_path}.product.txt"));
}

#[test]
fn a_drive_server_empties_an_earlier_record_and_its_sidecar_when_it_starts() {
    // Left in place until the session ends, an earlier run's record would
    // read as this session's if this one never ended (#264's review). An
    // empty record beside a stale sidecar is announced as emptied too.
    for earlier in ["an earlier session's record\n", ""] {
        let stub = Stub::serving_with_props(Vec::new(), &diet::drive::canned::build_info())
            .expect("loopback");
        let record = file_holding("record", earlier);
        let path = record.0.to_string_lossy().into_owned();
        let sidecar = PathBuf::from(format!("{path}.unspellable.json"));
        std::fs::write(&sidecar, "an earlier session's sidecar\n").expect("written");
        let product = PathBuf::from(format!("{path}.product.txt"));
        std::fs::write(&product, "an earlier session's product\n").expect("written");
        let served = start(&stub.url(), &["--regimen", &dev_loop(), "--record", &path]);
        let left = (
            served.record.clone(),
            std::fs::read_to_string(&record.0).expect("the record"),
            sidecar.exists(),
            product.exists(),
        );
        let _ = std::fs::remove_file(&sidecar);
        let _ = std::fs::remove_file(&product);
        assert_eq!(
            left,
            (Some((path, true)), String::new(), false, false),
            "{earlier:?}"
        );
    }
}

#[test]
fn a_drive_server_whose_writers_fail_names_what_they_had_emptied() {
    // A failure after the writers start says what it had already emptied
    // (#264, ruled (i)): here the record is emptied, then its sidecar --
    // a directory -- cannot be removed.
    let record = file_holding("record", "an earlier session's record\n");
    let path = record.0.to_string_lossy().into_owned();
    let sidecar = PathBuf::from(format!("{path}.unspellable.json"));
    std::fs::create_dir_all(sidecar.join("held")).expect("a directory where the sidecar goes");
    let stub =
        Stub::serving_with_props(Vec::new(), &diet::drive::canned::build_info()).expect("loopback");
    let (code, said) = run_briefly(&stub.url(), &["--regimen", &dev_loop(), "--record", &path]);
    let _ = std::fs::remove_dir_all(&sidecar);
    assert_eq!(code, Some(3), "{said}");
    assert!(
        said.contains("cannot be removed")
            && said.contains(&format!(
                "already emptied before this failure: the previous record at {path}"
            )),
        "{said}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_drive_server_whose_log_cannot_be_emptied_names_the_record_it_had_emptied() {
    // Linux refuses `ftruncate` on anything but a regular file, so a log at
    // `/dev/null` opens and then cannot be emptied -- after the record was
    // (#264, ruled (i); round 4). macOS empties `/dev/null`, so this path is
    // reached only here.
    let record = file_holding("record", "an earlier session's record\n");
    let path = record.0.to_string_lossy().into_owned();
    let stub =
        Stub::serving_with_props(Vec::new(), &diet::drive::canned::build_info()).expect("loopback");
    let (code, said) = run_briefly(
        &stub.url(),
        &[
            "--regimen",
            &dev_loop(),
            "--record",
            &path,
            "--log",
            "/dev/null",
        ],
    );
    assert_eq!(code, Some(3), "{said}");
    assert!(
        said.contains("the log cannot be emptied")
            && said.contains(&format!(
                "already emptied before this failure: the previous record at {path}"
            )),
        "{said}"
    );
}

#[test]
fn a_drive_server_that_fails_to_bind_leaves_an_earlier_record_and_sidecar_as_they_were() {
    // Emptied only once the address is bound (#264's review, round 2).
    let record = file_holding("record", "an earlier session's record\n");
    let path = record.0.to_string_lossy().into_owned();
    let sidecar = PathBuf::from(format!("{path}.unspellable.json"));
    std::fs::write(&sidecar, "an earlier session's sidecar\n").expect("written");
    let taken = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
    let port = std::net::TcpListener::local_addr(&taken)
        .expect("its address")
        .port()
        .to_string();
    let stub =
        Stub::serving_with_props(Vec::new(), &diet::drive::canned::build_info()).expect("loopback");
    let (code, said) = run_briefly(
        &stub.url(),
        &["--regimen", &dev_loop(), "--record", &path, "--port", &port],
    );
    let left = (
        std::fs::read_to_string(&record.0).ok(),
        std::fs::read_to_string(&sidecar).ok(),
    );
    let _ = std::fs::remove_file(&sidecar);
    // Exit 2 is shared with a usage refusal: the reason says it was the bind
    // (#264, round 3).
    assert_eq!(code, Some(2), "the bind fails: {said}");
    assert!(
        said.contains("cannot listen on"),
        "the bind, not usage: {said}"
    );
    assert_eq!(
        left,
        (
            Some("an earlier session's record\n".to_owned()),
            Some("an earlier session's sidecar\n".to_owned())
        )
    );
}

/// The bodies one asked turn sends, served with `extra` flags and, when
/// given, the dev loop's regimen with `top` added at its top level.
fn one_turns_bodies(extra: &[&str], top: Option<&str>) -> Vec<String> {
    let stub = Stub::serving_with_props(
        vec![Act::Raw(ANSWERED.to_vec())],
        &diet::drive::canned::build_info(),
    )
    .expect("loopback");
    let regimen = top.map(|top| {
        let whole = std::fs::read_to_string(dev_loop()).expect("the dev loop's regimen");
        let (before, sampler) = whole
            .split_once("\n[sampler]\n")
            .expect("the dev loop declares a sampler table last");
        file_holding("regimen", &format!("{before}\n{top}\n[sampler]\n{sampler}"))
    });
    let path = regimen
        .as_ref()
        .map(|file| file.0.to_string_lossy().into_owned());
    let mut args: Vec<&str> = extra.to_vec();
    if let Some(path) = path.as_deref() {
        args.extend(["--regimen", path]);
    }
    let served = start(&stub.url(), &args);
    let address = served.listening.clone();
    let reply = post(&address, &address, r#"{"kind":"ask","text":"hi"}"#);
    assert_eq!(status(&reply), 200, "{reply}");
    let _ = exchange(
        &address,
        &format!("GET /events?from=0 HTTP/1.1\r\nHost: {address}\r\n\r\n"),
        |read| read.contains(r#""reason":"final""#),
    );
    drop(served);
    stub.received()
}

#[test]
fn a_drive_servers_output_cap_is_the_flag_else_the_regimens_else_the_votes_default() {
    // #569: the flag beats the regimen, which beats the default (64,000, the
    // harness vote); a regimen value that is not a positive integer is unset.
    for (extra, top, cap) in [
        (&[][..], None, 64_000),
        (&[][..], Some("max_output_tokens = 1234"), 1234),
        (
            &["--max-output-tokens", "777"][..],
            Some("max_output_tokens = 1234"),
            777,
        ),
        (&[][..], Some("max_output_tokens = 0"), 64_000),
    ] {
        let sent = one_turns_bodies(extra, top);
        let want = format!(r#""max_tokens":{cap}"#);
        assert!(
            sent.iter().any(|body| body.contains(&want)),
            "{extra:?} {top:?}: {sent:?}"
        );
    }
}

#[test]
fn diet_drive_usage_names_the_serve_form() {
    // The top-level usage listed only the scripted drive (#290).
    let out = Command::new(DRIVE).output().expect("diet-drive runs");
    let said = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{said}");
    assert!(said.contains("diet-drive serve --endpoint"), "{said}");
}

#[test]
fn a_drive_server_refuses_a_record_without_a_regimen() {
    let stub = Stub::serving(Vec::new()).expect("loopback");
    let path = std::env::temp_dir()
        .join(format!(
            "diet-drive-unwritten-record-{}.jsonl",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned();
    let (code, said) = run_briefly(&stub.url(), &["--record", &path]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(code, Some(2), "{said}");
    assert!(said.contains("--record needs --regimen"), "{said}");
}

/// What `diet-drive` exits with, and prints to stdout and stderr, for `args`.
fn asked(args: &[&str]) -> (Option<i32>, String, String) {
    let out = Command::new(DRIVE)
        .args(args)
        .output()
        .expect("diet-drive runs");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn a_drive_help_asked_for_is_an_answer_on_stdout_with_exit_0() {
    // #219 item 8: `--help` exited 2, the code of a usage error, so the first
    // exit code a stranger read said they had done something wrong.
    for args in [
        &["serve", "--help"][..],
        &["serve", "-h"],
        &[
            "serve",
            "--endpoint",
            "http://127.0.0.1:1/v1/chat/completions",
            "--help",
        ],
    ] {
        let (code, stdout, stderr) = asked(args);
        assert_eq!(code, Some(0), "{args:?}: {stderr}");
        assert!(
            stdout.starts_with("usage: diet-drive serve"),
            "{args:?}: {stdout}"
        );
        assert!(stderr.is_empty(), "{args:?}: {stderr}");
    }
    for args in [&["--help"][..], &["-h"]] {
        let (code, stdout, stderr) = asked(args);
        assert_eq!(code, Some(0), "{args:?}: {stderr}");
        assert!(
            stdout.starts_with("usage: diet-drive <regimen>"),
            "{args:?}: {stdout}"
        );
    }
}

#[test]
fn a_drive_usage_error_still_exits_2_and_a_flags_value_is_never_the_question() {
    for args in [
        &["serve"][..],
        &["serve", "--no-such-flag", "x"],
        // `--help` as `--model`'s value is a model named `--help`; the
        // usage is then wrong for want of `--endpoint` and `--head`.
        &["serve", "--model", "--help"],
        &[],
    ] {
        let (code, stdout, stderr) = asked(args);
        assert_eq!(code, Some(2), "{args:?}: {stdout}");
        assert!(stdout.is_empty(), "{args:?}: {stdout}");
        assert!(
            stderr.starts_with("usage: diet-drive"),
            "{args:?}: {stderr}"
        );
    }
}

// ---------------------------------------------------------------------------
// the tool loop's half of serve (#298 point 8, T9)
// ---------------------------------------------------------------------------

/// I0's turn 1: a real llama-server streaming one `bash` call, `ls | wc -l`.
const I0_CALL: &[u8] =
    include_bytes!("../../substrates/measurements/2026-10-02-i0-tool-call-captures/turn1.http");

/// The dev loop's regimen, made to run commands: `top` before it,
/// `isolation` swapped for `isolation_line`, `tail` after it.
fn commands_regimen(top: &str, isolation_line: &str, tail: &str) -> HeadFile {
    let dev = std::fs::read_to_string(dev_loop()).expect("the dev loop's regimen");
    assert!(
        dev.contains("isolation = \"none\""),
        "the dev loop is unconfined"
    );
    let dev = dev.replace("isolation = \"none\"", isolation_line);
    file_holding("regimen", &format!("{top}{dev}\n{tail}"))
}

/// A directory of its own, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new(what: &str) -> Self {
        static MADE: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "diet-drive-serve-{what}-{}-{}",
            std::process::id(),
            MADE.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory");
        Self(dir)
    }

    fn path(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_drive_server_with_a_regimen_that_runs_commands_needs_an_absolute_worktree() {
    let stub = Stub::serving(Vec::new()).expect("loopback");
    let regimen = commands_regimen(
        "allowed_commands = []\n",
        "isolation = \"none\"",
        "[limits]\nmax_steps = 4\n",
    );
    let path = regimen.0.to_string_lossy().into_owned();
    let (code, said) = run_briefly(&stub.url(), &["--regimen", &path]);
    assert_eq!(code, Some(2), "{said}");
    assert!(said.contains("--worktree"), "{said}");
    let (code, said) = run_briefly(
        &stub.url(),
        &["--regimen", &path, "--worktree", "a/relative/tree"],
    );
    assert_eq!(
        code,
        Some(2),
        "a relative worktree is a usage error: {said}"
    );
    assert!(stub.received().is_empty(), "no request reached the server");
}

#[test]
fn a_drive_server_refuses_a_vm_regimen_with_commands_or_without_before_any_request() {
    let tree = Dir::new("vm-tree");
    for top in ["allowed_commands = [\"ls\"]\n", ""] {
        let stub = Stub::serving(Vec::new()).expect("loopback");
        let regimen = commands_regimen(top, "isolation = \"vm\"", "");
        let path = regimen.0.to_string_lossy().into_owned();
        let mut args = vec!["--regimen", path.as_str()];
        let tree_path = tree.path();
        if !top.is_empty() {
            args.extend(["--worktree", tree_path.as_str()]);
        }
        let (code, said) = run_briefly(&stub.url(), &args);
        assert_eq!(code, Some(2), "{top:?}: {said}");
        assert!(said.contains("vm"), "{said}");
        assert!(stub.received().is_empty(), "no request reached the server");
    }
}

#[test]
fn a_drive_server_refuses_unconfined_commands_with_no_step_bound() {
    let tree = Dir::new("unbounded-tree");
    let stub = Stub::serving(Vec::new()).expect("loopback");
    let regimen = commands_regimen("allowed_commands = []\n", "isolation = \"none\"", "");
    let path = regimen.0.to_string_lossy().into_owned();
    let (code, said) = run_briefly(
        &stub.url(),
        &["--regimen", &path, "--worktree", &tree.path()],
    );
    assert_eq!(code, Some(1), "{said}");
    assert!(said.contains("max_steps"), "{said}");
    assert!(stub.received().is_empty(), "no request reached the server");
}

#[test]
fn a_drive_server_opens_the_confinement_at_start_and_a_refusal_comes_before_any_request() {
    // A writable dir inside a secret: no sandbox opens on it, on either
    // backend, and the refusal is the confinement's own.
    let tree = Dir::new("confined-tree");
    let stub = Stub::serving(Vec::new()).expect("loopback");
    let regimen = commands_regimen(
        "allowed_commands = []\nsandbox_writable = [\"~/.aws/inside\"]\n",
        "isolation = \"sandbox\"",
        "",
    );
    let path = regimen.0.to_string_lossy().into_owned();
    let auth = file_holding("auth", "author:s3cret\n");
    let auth_path = auth.0.to_string_lossy().into_owned();
    let (code, said) = run_briefly(
        &stub.url(),
        &[
            "--regimen",
            &path,
            "--worktree",
            &tree.path(),
            "--auth-file",
            &auth_path,
        ],
    );
    assert_eq!(code, Some(2), "{said}");
    assert!(said.contains("lies inside the secret"), "{said}");
    assert!(stub.received().is_empty(), "no request reached the server");
}

/// #559: the worktree's `AGENTS.md`, from it up to its git root, goes into
/// the system prompt in Qwen Code's wrapper, and `session.start` names each by
/// its relative path and digest; `instruction_files = "off"` injects none.
#[test]
fn a_drive_server_puts_the_worktrees_agents_md_in_the_system_prompt() {
    let repo = Dir::new("instructions-repo");
    std::fs::create_dir_all(repo.0.join(".git")).expect("a git root");
    std::fs::write(repo.0.join("AGENTS.md"), "Root rules.\n").expect("write");
    let tree = repo.0.join("app");
    std::fs::create_dir_all(&tree).expect("the worktree");
    std::fs::write(tree.join("AGENTS.md"), "App rules.\n").expect("write");
    let tree_path = tree.to_string_lossy().into_owned();
    let auth = file_holding("auth", "author:s3cret\n");
    let auth_path = auth.0.to_string_lossy().into_owned();
    for (lever, injected) in [("", true), ("instruction_files = \"off\"\n", false)] {
        let regimen = commands_regimen(
            &format!("allowed_commands = []\n{lever}"),
            "isolation = \"none\"",
            "[limits]\nmax_steps = 4\n",
        );
        let path = regimen.0.to_string_lossy().into_owned();
        let stub = Stub::serving_with_props(Vec::new(), &diet::drive::canned::build_info())
            .expect("loopback");
        let log_file = file_holding("log", "");
        let logged = log_file.0.to_string_lossy().into_owned();
        let _served = start(
            &stub.url(),
            &[
                "--regimen",
                &path,
                "--worktree",
                &tree_path,
                "--auth-file",
                &auth_path,
                "--log",
                &logged,
            ],
        );
        let start_line = first_logged_line(&log_file.0);
        let system = start_line["head"][0]["content"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        if injected {
            assert!(
                system.ends_with(
                    "\n\n---\n\n--- Context from: ../AGENTS.md ---\nRoot rules.\n\
                     --- End of Context from: ../AGENTS.md ---\n\n\
                     --- Context from: AGENTS.md ---\nApp rules.\n\
                     --- End of Context from: AGENTS.md ---"
                ),
                "{system}"
            );
            assert_eq!(
                start_line["instruction_files"],
                serde_json::json!([
                    {"path": "../AGENTS.md", "sha256": diet::digest::sha256_hex(b"Root rules.\n")},
                    {"path": "AGENTS.md", "sha256": diet::digest::sha256_hex(b"App rules.\n")},
                ]),
                "{start_line}"
            );
        } else {
            assert!(!system.contains("Context from"), "{system}");
            assert!(
                start_line.get("instruction_files").is_none(),
                "{start_line}"
            );
        }
    }
}

/// A session that runs commands needs a credential, and its file is a
/// secret no command reads (#298 review round 1, finding 1): with none,
/// serve does not start; with one in a path the regimen declares writable,
/// it refuses, as it does the key file.
#[test]
fn a_drive_server_that_runs_commands_demands_a_credential_no_command_can_read() {
    let tree = Dir::new("auth-tree");
    let declared = Dir::new("auth-declared");
    let stub = Stub::serving(Vec::new()).expect("loopback");
    let bounded = commands_regimen(
        "allowed_commands = []\n",
        "isolation = \"none\"",
        "[limits]\nmax_steps = 4\n",
    );
    let path = bounded.0.to_string_lossy().into_owned();
    let (code, said) = run_briefly(
        &stub.url(),
        &["--regimen", &path, "--worktree", &tree.path()],
    );
    assert_eq!(code, Some(2), "{said}");
    assert!(said.contains("--auth-file"), "{said}");

    let auth = declared.0.join("auth");
    std::fs::write(&auth, "author:s3cret\n").expect("an auth file");
    let writable = commands_regimen(
        &format!(
            "allowed_commands = []\nsandbox_writable = [\"{}\"]\n",
            declared.path()
        ),
        "isolation = \"sandbox\"",
        "",
    );
    let path = writable.0.to_string_lossy().into_owned();
    let (code, said) = run_briefly(
        &stub.url(),
        &[
            "--regimen",
            &path,
            "--worktree",
            &tree.path(),
            "--auth-file",
            &auth.to_string_lossy(),
        ],
    );
    assert_eq!(code, Some(1), "{said}");
    assert!(
        said.contains("--auth-file") && said.contains("lies in"),
        "{said}"
    );
    assert!(stub.received().is_empty(), "no request reached the server");
}

/// One `POST` to `path` with the session's credential.
fn post_authed(address: &str, path: &str, json: &str) -> String {
    exchange(
        address,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: {address}\r\n{AUTHOR}Content-Type: application/json\r\n\
             Content-Length: {}\r\n\r\n{json}",
            json.len()
        ),
        |_| false,
    )
}

/// The `/events` stream, with the credential, until `done`.
fn events_authed(address: &str, done: impl Fn(&str) -> bool) -> String {
    exchange(
        address,
        &format!("GET /events?from=0 HTTP/1.1\r\nHost: {address}\r\n{AUTHOR}\r\n"),
        done,
    )
}

/// A streamed reply making one `bash` call, `call-1`, of `command`.
fn a_call(command: &str) -> Act {
    let arguments = serde_json::json!({ "command": command }).to_string();
    let call = serde_json::json!({"choices": [{"index": 0, "finish_reason": null, "delta": {
        "tool_calls": [{"index": 0, "id": "call-1", "type": "function",
            "function": {"name": "bash", "arguments": arguments}}]}}]});
    let done =
        serde_json::json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]});
    Act::Answer(format!("data: {call}\n\ndata: {done}\n\ndata: [DONE]\n\n"))
}

/// What one T1-shaped session ran, and what it left.
struct Drove {
    prompt: serde_json::Value,
    call: serde_json::Value,
    receipt: serde_json::Value,
    log: PathBuf,
    record: PathBuf,
}

/// T1's path through the binary: `serve` with a regimen that runs
/// commands (`top` and `isolation_line` over the dev loop), `--worktree`,
/// `--auth-file`, `--log` and `--record`; one ask, whose call waits and is
/// answered `scope` by the operator; then `end`. Every request carries the
/// credential.
#[allow(clippy::too_many_lines)]
fn drive_t1(
    first: Act,
    (top, isolation_line): (&str, &str),
    (tree, state, auth): (&Dir, &Dir, &HeadFile),
    scope: &str,
    before_the_answer: impl FnOnce(&str),
) -> Drove {
    let stub = Stub::serving_with_props(
        vec![first, Act::Raw(ANSWERED.to_vec())],
        &diet::drive::canned::build_info(),
    )
    .expect("loopback");
    let regimen = commands_regimen(top, isolation_line, "[limits]\nmax_steps = 4\n");
    let path = regimen.0.to_string_lossy().into_owned();
    let auth_path = auth.0.to_string_lossy().into_owned();
    let log = file_holding("log", "");
    let log_path = log.0.to_string_lossy().into_owned();
    let record = file_holding("record", "");
    let record_path = record.0.to_string_lossy().into_owned();
    let state_path = state.path();
    let served = start_with(
        &stub.url(),
        &[
            "--regimen",
            &path,
            "--worktree",
            &tree.path(),
            "--auth-file",
            &auth_path,
            "--log",
            &log_path,
            "--record",
            &record_path,
        ],
        &[("XDG_STATE_HOME", &state_path)],
    );
    let address = served.listening.clone();
    before_the_answer(&address);
    let unauthed = post(&address, &address, r#"{"kind":"ask","text":"hi"}"#);
    assert_eq!(
        status(&unauthed),
        401,
        "every route asks for the credential"
    );
    let reply = post_authed(&address, "/commands", r#"{"kind":"ask","text":"go"}"#);
    assert_eq!(status(&reply), 200, "{reply}");
    let read = events_authed(&address, |read| {
        read.contains("event: waiting\ndata: ") && read.ends_with("\n\n")
    });
    let shown = read
        .split("event: waiting\ndata: ")
        .nth(1)
        .and_then(|rest| rest.split('\n').next())
        .unwrap_or_else(|| panic!("no prompt was shown: {read}"));
    let prompt = log_line_object(shown);
    let id = prompt["id"].as_str().expect("the call's id").to_owned();
    let approve = format!(r#"{{"call":"{id}","scope":"{scope}"}}"#);
    let reply = post_authed(&address, "/approve", &approve);
    assert_eq!(status(&reply), 204, "{reply}");
    events_authed(&address, |read| read.contains(r#""reason":"final""#));
    let reply = post_authed(&address, "/commands", r#"{"kind":"end"}"#);
    assert_eq!(status(&reply), 200, "{reply}");
    let recorded = log_line_object(
        &served
            .said
            .recv_timeout(Duration::from_secs(10))
            .expect("the record's report"),
    );
    assert_eq!(recorded["record"], record_path.as_str(), "{recorded}");
    let report = log_line_object(
        &served
            .said
            .recv_timeout(Duration::from_secs(10))
            .expect("the receipt's report"),
    );
    let receipt_path = format!("{record_path}.receipt.json");
    assert_eq!(report["receipt"], receipt_path.as_str(), "{report}");
    let receipt = log_line_object(&std::fs::read_to_string(&receipt_path).expect("the receipt"));
    let _ = std::fs::remove_file(&receipt_path);
    // The log reads, and the record projected from it, tool turn and all.
    for (check, file) in [("check-log", &log.0), ("check-record", &record.0)] {
        let checked = Command::new(DIET)
            .args([check])
            .arg(file)
            .output()
            .expect("diet runs");
        assert_eq!(
            checked.status.code(),
            Some(0),
            "{check}: {}",
            String::from_utf8_lossy(&checked.stdout)
        );
    }
    let _ = std::fs::remove_file(format!("{record_path}.unspellable.json"));
    let written = std::fs::read_to_string(&log.0).expect("the log");
    let called: Vec<serde_json::Value> = written
        .lines()
        .map(log_line_object)
        .filter(|line| line["kind"] == "tool_call")
        .collect();
    assert_eq!(called.len(), 1, "{written}");
    Drove {
        prompt,
        call: called[0].clone(),
        receipt,
        log: log.0.clone(),
        record: record.0.clone(),
    }
}

/// A git checkout, as the regimen's reference is.
fn a_reference(dir: &Dir) -> String {
    let init = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&dir.0)
        .status()
        .expect("git runs");
    assert!(init.success());
    dir.path()
}

/// The `cwd` a line records for `tree`: under the drive's HOME a `~` path,
/// never expanded; else as given.
fn recorded_cwd(tree: &Dir) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = tree.path();
    match path.strip_prefix(&home) {
        Some(rest) if !home.is_empty() && rest.starts_with('/') => format!("~{rest}"),
        _ => path,
    }
}

/// The receipt's fixed half: what every session writes whatever it ran.
fn receipt_holds(receipt: &serde_json::Value, approvals: &serde_json::Value) {
    assert_eq!(&receipt["approvals"], approvals, "{receipt}");
    assert_eq!(
        receipt["approval_policy"], "approve what stays in the worktree",
        "{receipt}"
    );
    assert_eq!(receipt["lifecycle_scripts"], "unguarded");
    assert_eq!(
        receipt["denylist_sha256"].as_str(),
        Some(diet::drive::shell_gate::denylist_digest().as_str())
    );
    assert_eq!(
        receipt["env_passthrough"],
        serde_json::json!(["PATH", "HOME", "LANG", "TERM", "TMPDIR", "XDG_STATE_HOME"]),
        "the effective list"
    );
}

/// T1's path end to end, unconfined: I0's real streamed call, approved for
/// the session, recorded, and the receipt whole.
#[test]
fn a_drive_server_runs_an_approved_call_records_it_and_writes_its_receipt() {
    let tree = Dir::new("loop-tree");
    let state = Dir::new("loop-state");
    let reference = Dir::new("loop-reference");
    let reference_path = a_reference(&reference);
    let top = format!(
        "allowed_commands = []\nenv_passthrough = [\"XDG_STATE_HOME\"]\n\
         approval_policy = \"approve what stays in the worktree\"\n\
         sandbox_writable = [\"{reference_path}\"]\n"
    );
    let drove = drive_t1(
        Act::Raw(I0_CALL.to_vec()),
        (&top, "isolation = \"none\""),
        (&tree, &state, &file_holding("auth", "author:s3cret\n")),
        "session",
        |_| {},
    );
    // The shape #400's surface reads (#389 5982826097 point 2).
    assert_eq!(
        drove.prompt,
        serde_json::json!({
            "request": drove.prompt["request"].as_u64().expect("a request"),
            "id": "7GJeYs3ux1SaqFVPB5ee2AExFLbsukd7",
            "command": "ls | wc -l",
            "cwd": recorded_cwd(&tree),
            "reason": "not_approved",
            "segments": [
                {"text": "ls", "shape": "ls", "program": "ls", "verdict": "prompt", "why": "not_approved"},
                {"text": "wc -l", "shape": "wc", "program": "wc", "verdict": "prompt", "why": "not_approved"},
            ],
        })
    );
    assert_eq!(drove.call["outcome"], "ran");
    assert_eq!(drove.call["approval"]["scope"], "session");
    assert_eq!(drove.call["argv"][2], "ls | wc -l");
    assert_eq!(drove.call["cwd"], recorded_cwd(&tree).as_str());
    assert_eq!(drove.call["isolation"], "none");
    assert_eq!(
        drove.receipt["allow"],
        serde_json::json!([
            {"shape": "ls", "scope": "session", "origin": "operator"},
            {"shape": "wc", "scope": "session", "origin": "operator"},
        ]),
        "{}",
        drove.receipt
    );
    receipt_holds(
        &drove.receipt,
        &serde_json::json!({"once": 0, "session": 1, "workspace": 0, "preseeded": 0, "declined": 0}),
    );
    assert_eq!(
        drove.receipt["reference_modified"],
        serde_json::json!({ reference_path: "" }),
        "the reference checkout, untouched"
    );
    assert!(
        std::fs::read_dir(&tree.0)
            .expect("the tree")
            .next()
            .is_none(),
        "nothing of the drive's was written into the worktree"
    );
    let _ = (drove.log, drove.record);
}

/// T1's path under the Mac's real sandbox (#298 review round 1, findings
/// 1 and 7): Seatbelt, `network = "host"`, `sandbox_reads = "all"`. The
/// approved command cannot read the credential, and its own `POST
/// /approve` to serve is refused `401`: no approval is forged from inside.
/// It writes the reference checkout, which the receipt names. Runs where
/// `DIET_REQUIRE_SANDBOX` is set on a Mac; elsewhere it says so and passes.
#[test]
fn a_drive_server_runs_t1s_path_under_seatbelt_and_no_command_forges_an_approval() {
    if std::env::var_os("DIET_REQUIRE_SANDBOX").is_none() || !cfg!(target_os = "macos") {
        eprintln!("not run: needs DIET_REQUIRE_SANDBOX on a Mac (Seatbelt)");
        return;
    }
    let tree = Dir::new("seatbelt-tree");
    let state = Dir::new("seatbelt-state");
    let reference = Dir::new("seatbelt-reference");
    let reference_path = a_reference(&reference);
    let top = format!(
        "allowed_commands = []\nenv_passthrough = [\"XDG_STATE_HOME\"]\n\
         approval_policy = \"approve what stays in the worktree\"\n\
         sandbox_writable = [\"{reference_path}\"]\nnetwork = \"host\"\n\
         sandbox_reads = \"all\"\n"
    );
    let auth = file_holding("auth", "author:s3cret\n");
    // What the model asks for: read the credential, answer its own prompt
    // (serve's address is written into the tree before the answer), and
    // touch the reference.
    let command = format!(
        "cat {} ; echo forged=$(curl -s -o /dev/null -w '%{{http_code}}' -X POST \
         -H 'Content-Type: application/json' --data '{{\"call\":\"call-1\",\"scope\":\"session\"}}' \
         http://$(cat address)/approve) ; echo x > {reference_path}/touched",
        auth.0.display()
    );
    let tree_for_address = tree.0.clone();
    let drove = drive_t1(
        a_call(&command),
        (&top, "isolation = \"sandbox\""),
        (&tree, &state, &auth),
        "once",
        move |address| {
            std::fs::write(tree_for_address.join("address"), address).expect("the address");
        },
    );
    assert_eq!(
        drove.prompt["segments"][1]["why"], "dynamic",
        "{}",
        drove.prompt
    );
    assert_eq!(drove.call["outcome"], "ran", "{}", drove.call);
    assert_eq!(drove.call["isolation"], "sandbox");
    assert_eq!(drove.call["network"], "host");
    assert_eq!(drove.call["approval"]["scope"], "once");
    assert_eq!(drove.call["cwd"], recorded_cwd(&tree).as_str());
    assert!(
        drove.call["confined"][0]
            .as_str()
            .is_some_and(|runner| runner.ends_with("sandbox-exec")),
        "{}",
        drove.call
    );
    let stdout = drove.call["stdout"].as_str().unwrap_or_default();
    let stderr = drove.call["stderr"].as_str().unwrap_or_default();
    assert!(
        !stdout.contains("s3cret"),
        "a confined command read the credential: {stdout}"
    );
    assert!(
        stderr.contains("Operation not permitted"),
        "the credential's read was refused: {stderr}"
    );
    assert!(
        stdout.contains("forged=401"),
        "a confined command's own approval was not refused: {stdout} {stderr}"
    );
    assert_eq!(
        drove.receipt["allow"],
        serde_json::json!([]),
        "{}",
        drove.receipt
    );
    receipt_holds(
        &drove.receipt,
        &serde_json::json!({"once": 1, "session": 0, "workspace": 0, "preseeded": 0, "declined": 0}),
    );
    assert_eq!(
        drove.receipt["reference_modified"],
        serde_json::json!({ reference_path: "?? touched\n" }),
        "the reference checkout, as the command left it"
    );
    let _ = (drove.log, drove.record);
}

// ---------------------------------------------------------------------------
// the regimen's sampler, pinned on the wire (#486)
// ---------------------------------------------------------------------------

/// The sampler settings the client can pin, as the wire names them: every
/// other key of a request is not a sampler field.
fn sampler_fields(body: &str) -> serde_json::Map<String, serde_json::Value> {
    let tags: Vec<&str> = diet::client::shape::SamplerSetting::ALL
        .iter()
        .map(|setting| setting.tag())
        .collect();
    log_line_object(body)
        .as_object()
        .expect("a request is an object")
        .iter()
        .filter(|(key, _)| tags.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// The dev loop's regimen with its `[sampler]` table replaced by `sampler`,
/// and `top` added at the top level: one source for the substrate, the
/// engine check and the confinement, so only the sampler differs.
fn dev_loop_sampling(top: &str, sampler: &str) -> HeadFile {
    let whole = std::fs::read_to_string(dev_loop()).expect("the dev loop's regimen");
    let (before, _) = whole
        .split_once("\n[sampler]\n")
        .expect("the dev loop declares a sampler table last");
    file_holding("regimen", &format!("{before}\n{top}\n[sampler]\n{sampler}"))
}

#[test]
fn a_drive_server_pins_the_regimens_sampler_on_the_trunk_and_the_fork_as_its_record_claims() {
    let stub = Stub::serving_with_props(
        vec![Act::Raw(ANSWERED.to_vec()), Act::Raw(ANSWERED.to_vec())],
        &diet::drive::canned::build_info(),
    )
    .expect("loopback");
    let regimen = dev_loop_sampling(
        "interview_warrant = [\"scoping\"]\n",
        "temperature = 0.6\ntop_k = 20\ntop_p = 1.0\nmin_p = 0.0\n",
    );
    let regimen_path = regimen.0.to_string_lossy().into_owned();
    let record = file_holding("record", "");
    let path = record.0.to_string_lossy().into_owned();
    let served = start(
        &stub.url(),
        &["--regimen", &regimen_path, "--record", &path],
    );
    let address = served.listening.clone();
    let reply = post(
        &address,
        &address,
        r#"{"kind":"ask","text":"what are we building?","scoping":true}"#,
    );
    assert_eq!(status(&reply), 200, "{reply}");
    let read = exchange(
        &address,
        &format!("GET /events?from=0 HTTP/1.1\r\nHost: {address}\r\n\r\n"),
        |read| read.contains("fork.settled") && read.contains(r#""to":"awaiting""#),
    );
    assert!(read.contains("fork.settled"), "the fork settled: {read}");
    let reply = post(&address, &address, r#"{"kind":"end"}"#);
    assert_eq!(status(&reply), 200, "{reply}");
    let _report = served
        .said
        .recv_timeout(Duration::from_secs(10))
        .expect("the record's report");

    // The wire: the trunk's request and the fork's, each carrying exactly the
    // four settings the regimen wrote, as the digits it wrote them in.
    let bodies = stub.received();
    let [trunk, fork] = bodies.as_slice() else {
        panic!("one trunk request and one fork request: {bodies:#?}");
    };
    // The fork is the trunk's messages, the answer, and the interview's own
    // question: a request the trunk never sent.
    let messages = |body: &str| log_line_object(body)["messages"].as_array().cloned();
    let (trunk_messages, fork_messages) = (
        messages(trunk).expect("messages"),
        messages(fork).expect("messages"),
    );
    assert_eq!(
        trunk_messages.last().map(|m| m["content"].clone()),
        Some(serde_json::json!("what are we building?")),
        "{trunk}"
    );
    assert_eq!(fork_messages.len(), trunk_messages.len() + 2, "{fork}");
    assert_eq!(
        fork_messages[..trunk_messages.len()],
        trunk_messages[..],
        "{fork}"
    );
    assert_eq!(
        fork_messages.last().map(|m| m["role"].clone()),
        Some(serde_json::json!("user"))
    );
    let pinned = serde_json::json!({
        "temperature": 0.6, "top_k": 20, "top_p": 1.0, "min_p": 0.0,
    });
    for (lane, body) in [("trunk", trunk), ("fork", fork)] {
        assert_eq!(
            serde_json::Value::Object(sampler_fields(body)),
            pinned,
            "the {lane}'s sampler fields: {body}"
        );
        // R1: the regime's reasoning state, on the fork as on the trunk.
        assert_eq!(
            log_line_object(body)["chat_template_kwargs"],
            serde_json::json!({"enable_thinking": false}),
            "the {lane}'s template kwargs: {body}"
        );
        for written in [
            r#""temperature":0.6,"#,
            r#""top_k":20,"#,
            r#""top_p":1.0,"#,
            r#""min_p":0.0,"#,
        ] {
            assert!(body.contains(written), "the {lane} sends {written}: {body}");
        }
    }

    // The record: its `sampler_card` is the same four, in the same digits.
    let written = std::fs::read_to_string(&record.0).expect("the record");
    let start_row = written.lines().next().expect("a start row");
    let card = &log_line_object(start_row)["regime"]["substrates"][0]["sampler_card"];
    assert_eq!(card, &pinned, "{start_row}");
    for digits in [
        r#""temperature":0.6"#,
        r#""top_k":20"#,
        r#""top_p":1.0"#,
        r#""min_p":0.0"#,
    ] {
        assert!(
            start_row.contains(digits),
            "the record holds {digits}: {start_row}"
        );
    }
    let _ = std::fs::remove_file(format!("{path}.unspellable.json"));
}

#[test]
fn a_drive_server_records_and_announces_a_reasoning_budget_it_cannot_send() {
    // R1, duty of care: no chat template has a variable for a budget, so it
    // is not sent -- and nothing refuses for that. The announcement says so,
    // `session.start` records it beside what was sent, and serve starts.
    let stub =
        Stub::serving_with_props(Vec::new(), &diet::drive::canned::build_info()).expect("loopback");
    let whole = std::fs::read_to_string(dev_loop()).expect("the dev loop's regimen");
    let regimen = file_holding(
        "regimen",
        &whole
            .replace(
                "substrate_reasoning = \"off\"",
                "substrate_reasoning = \"on\"",
            )
            .replace(
                "\n[sampler]\n",
                "\n[reasoning]\neffort = \"medium\"\nbudget_tokens = 512\n\n[sampler]\n",
            ),
    );
    let regimen_path = regimen.0.to_string_lossy().into_owned();
    let log_file = file_holding("log", "");
    let logged = log_file.0.to_string_lossy().into_owned();
    let served = start(&stub.url(), &["--regimen", &regimen_path, "--log", &logged]);
    assert_eq!(served.budget_tokens_unsent, Some(512));
    let start_line = first_logged_line(&log_file.0);
    assert_eq!(
        start_line["unsent"],
        serde_json::json!({"budget_tokens": 512}),
        "{start_line}"
    );
    assert_eq!(
        start_line["template_kwargs"],
        serde_json::json!({"enable_thinking": true, "reasoning_effort": "medium"}),
        "{start_line}"
    );
}

#[test]
fn a_drive_server_names_the_fork_delivery_its_regimen_declares() {
    // The fork delivery lever: the regimen's state reaches `session.start`,
    // and a value that is none of the three is refused before it listens.
    let stub =
        Stub::serving_with_props(Vec::new(), &diet::drive::canned::build_info()).expect("loopback");
    let regimen = dev_loop_sampling(
        "interview_warrant = [\"scoping\"]\nfork_delivery = \"advisory\"\n",
        "seed = 7\n",
    );
    let regimen_path = regimen.0.to_string_lossy().into_owned();
    let log_file = file_holding("log", "");
    let logged = log_file.0.to_string_lossy().into_owned();
    let _served = start(&stub.url(), &["--regimen", &regimen_path, "--log", &logged]);
    let start_line = first_logged_line(&log_file.0);
    assert_eq!(start_line["fork_delivery"], "advisory", "{start_line}");
    let refused = dev_loop_sampling(
        "interview_warrant = [\"scoping\"]\nfork_delivery = \"loud\"\n",
        "seed = 7\n",
    );
    let stub =
        Stub::serving_with_props(Vec::new(), &diet::drive::canned::build_info()).expect("loopback");
    let (code, said) = run_briefly(&stub.url(), &["--regimen", &refused.0.to_string_lossy()]);
    assert_ne!(code, None, "it listened: {said}");
    assert!(said.contains("`fork_delivery`"), "{said}");
}

#[test]
fn a_drive_server_refuses_tools_capture_without_a_set_written_for_it() {
    // #610: a fork answers through the capture tools only in an ask set
    // written for them, with self-capture declaring them, and with forks to
    // answer at all; each missing piece is refused before it listens.
    for (top, says) in [
        (
            "capture_modality = \"tools\"\nself_capture = true\n",
            "no `interview_warrant`",
        ),
        (
            "interview_warrant = [\"scoping\"]\ncapture_modality = \"tools\"\n",
            "self-capture off",
        ),
        (
            "interview_warrant = [\"scoping\"]\ncapture_modality = \"tools\"\n\
             self_capture = true\nfork_asks = \"v4\"\n",
            "the ask set `v4`",
        ),
        (
            "interview_warrant = [\"scoping\"]\ncapture_modality = \"both\"\n",
            "`capture_modality` is `fields` or `tools`",
        ),
    ] {
        let refused = dev_loop_sampling(top, "seed = 7\n");
        let stub = Stub::serving_with_props(Vec::new(), &diet::drive::canned::build_info())
            .expect("loopback");
        let (code, said) = run_briefly(&stub.url(), &["--regimen", &refused.0.to_string_lossy()]);
        assert_ne!(code, None, "it listened: {said}");
        assert!(said.contains(says), "{top}: {said}");
    }
}

#[test]
fn a_drive_server_runs_a_regimens_phase_graph() {
    // #563: serve reads `phases` and `phase_transitions` (it refused them,
    // #520), logs the graph and the phase it opens in, and takes a phase on
    // `declare-seam` -- refused here as `nothing-to-seam`, with no turn yet,
    // rather than as a body it cannot read.
    let stub =
        Stub::serving_with_props(Vec::new(), &diet::drive::canned::build_info()).expect("loopback");
    let regimen = dev_loop_sampling(
        "interview_warrant = [\"scoping\"]\nphases = [\"plan\", \"build\"]\n\
         [phase_transitions]\nplan = [\"build\"]\n",
        "seed = 7\n",
    );
    let regimen_path = regimen.0.to_string_lossy().into_owned();
    let log_file = file_holding("log", "");
    let logged = log_file.0.to_string_lossy().into_owned();
    let served = start(&stub.url(), &["--regimen", &regimen_path, "--log", &logged]);
    let start_line = first_logged_line(&log_file.0);
    assert_eq!(
        start_line["phases"],
        serde_json::json!(["plan", "build"]),
        "{start_line}"
    );
    assert_eq!(
        start_line["phase_transitions"],
        serde_json::json!([{"from": "plan", "to": "build"}]),
        "{start_line}"
    );
    assert_eq!(start_line["opening_phase"], "plan", "{start_line}");
    let address = served.listening.clone();
    let reply = post(
        &address,
        &address,
        r#"{"kind":"declare-seam","phase":"build"}"#,
    );
    assert_eq!(status(&reply), 409, "{reply}");
    assert!(reply.contains("nothing-to-seam"), "{reply}");
}

#[test]
fn a_drive_server_refuses_a_sampler_key_it_cannot_pin_before_it_listens() {
    // No acts and no `/props`: a server that got as far as the engine check
    // would be refused with exit 1, and one that listened would not exit.
    let stub = Stub::serving(Vec::new()).expect("loopback");
    let regimen = dev_loop_sampling("", "temperature = 0.6\nmirostat = 2\n");
    let path = regimen.0.to_string_lossy().into_owned();
    let (code, said) = run_briefly(&stub.url(), &["--regimen", &path]);
    assert_eq!(code, Some(2), "{said}");
    assert!(said.contains("`mirostat`"), "the key is named: {said}");
    for tag in diet::client::shape::SamplerSetting::ALL
        .iter()
        .map(|s| s.tag())
    {
        assert!(said.contains(tag), "the accepted set names {tag}: {said}");
    }
    assert!(stub.received().is_empty(), "nothing was asked");
}

#[test]
fn a_drive_server_without_a_regimen_pins_no_sampler_setting() {
    let stub = Stub::serving(vec![Act::Raw(ANSWERED.to_vec())]).expect("loopback");
    let served = start(&stub.url(), &[]);
    let address = served.listening.clone();
    let reply = post(&address, &address, r#"{"kind":"ask","text":"hi"}"#);
    assert_eq!(status(&reply), 200, "{reply}");
    exchange(
        &address,
        &format!("GET /events?from=0 HTTP/1.1\r\nHost: {address}\r\n\r\n"),
        |read| read.contains(r#""to":"awaiting""#),
    );
    drop(served);
    let bodies = stub.received();
    let [body] = bodies.as_slice() else {
        panic!("one request: {bodies:#?}");
    };
    assert!(sampler_fields(body).is_empty(), "{body}");
}

// ---------------------------------------------------------------------------
// the stream-replay substrate (#411)
// ---------------------------------------------------------------------------

/// `diet-drive replay` on a port of the system's choosing: the child, killed
/// when dropped, and the endpoint its first line names.
struct Replaying {
    child: std::process::Child,
    endpoint: String,
}

impl Drop for Replaying {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn replaying(answers: bool) -> Replaying {
    let mode: &[&str] = if answers { &["--answers"] } else { &[] };
    let (substrate, build_info) = if answers {
        ("canned-replay", diet::drive::canned::replay_build_info())
    } else {
        (
            "canned-replay-tools",
            diet::drive::canned::replay_tools_build_info(),
        )
    };
    let mut child = Command::new(DRIVE)
        .arg("replay")
        .args(mode)
        .args(["--port", "0"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("diet-drive replay starts");
    let stdout = child.stdout.take().expect("stdout is piped");
    let (line, lines) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut first = String::new();
        let _ = BufReader::new(stdout).read_line(&mut first);
        let _ = line.send(first);
    });
    let mut replaying = Replaying {
        child,
        endpoint: String::new(),
    };
    let first = lines
        .recv_timeout(Duration::from_secs(10))
        .expect("diet-drive replay announced itself");
    let announced = log_line_object(&first);
    assert_eq!(announced["substrate"], substrate, "{first}");
    assert_eq!(announced["build_info"], build_info.as_str(), "{first}");
    announced["listening"]
        .as_str()
        .expect("the endpoint")
        .clone_into(&mut replaying.endpoint);
    replaying
}

fn replay_regimen() -> String {
    format!("{}/drive/replay.toml", env!("CARGO_MANIFEST_DIR"))
}

/// The rehearsal regimen, naming the answer-only replay instead.
fn answers_regimen() -> HeadFile {
    let tools = std::fs::read_to_string(replay_regimen()).expect("the rehearsal regimen");
    let fingerprint = |acts: &str| diet::drive::canned::hardware_fingerprint(acts);
    let answers = tools
        .replace(
            "substrate = \"canned-replay-tools\"",
            "substrate = \"canned-replay\"",
        )
        .replace(
            &fingerprint(&diet::drive::canned::replay_tools_digest()),
            &fingerprint(&diet::drive::canned::replay_digest()),
        );
    assert_ne!(answers, tools);
    file_holding("regimen", &answers)
}

/// #411's acceptance: a model-less `serve --record` under the rehearsal
/// regimen -- commands allowed, an approval policy, a worktree -- against
/// `diet-drive replay` records real turns. The engine check passes on the
/// replay's literal; each turn settles; `diet check-record` reads the record,
/// and it carries `turn` and `response` rows with the capture's counts
/// (prompt 18 + cached 160, predicted 66), derived because the replay is a
/// cited engine. Nothing is left unspellable but the asks.
#[test]
fn a_drive_server_records_real_turns_against_the_stream_replay_substrate() {
    let replayed = replaying(true);
    let regimen = answers_regimen();
    let regimen_path = regimen.0.to_string_lossy().into_owned();
    let tree = Dir::new("replay-tree");
    let auth = file_holding("auth", "author:s3cret\n");
    let auth_path = auth.0.to_string_lossy().into_owned();
    let record = file_holding("record", "");
    let path = record.0.to_string_lossy().into_owned();
    let served = start(
        &replayed.endpoint,
        &[
            "--regimen",
            &regimen_path,
            "--worktree",
            &tree.path(),
            "--auth-file",
            &auth_path,
            "--record",
            &path,
        ],
    );
    assert_eq!(served.substrate.as_deref(), Some("canned-replay"));
    let address = served.listening.clone();
    for turn in 1..=2 {
        let reply = post_authed(&address, "/commands", r#"{"kind":"ask","text":"hi"}"#);
        assert_eq!(status(&reply), 200, "{reply}");
        let read = events_authed(&address, |read| {
            read.matches(r#""to":"awaiting""#).count() >= turn
        });
        assert_eq!(
            read.matches(r#""reason":"final""#).count(),
            turn,
            "each turn settles final: {read}"
        );
    }
    let reply = post_authed(&address, "/commands", r#"{"kind":"end"}"#);
    assert_eq!(status(&reply), 200, "{reply}");
    let _report = served
        .said
        .recv_timeout(Duration::from_secs(10))
        .expect("the record's report");
    let checked = Command::new(DIET)
        .args(["check-record"])
        .arg(&record.0)
        .output()
        .expect("diet runs");
    assert_eq!(
        checked.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&checked.stdout)
    );
    let written = std::fs::read_to_string(&record.0).expect("the record");
    let rows: Vec<serde_json::Value> = written.lines().map(log_line_object).collect();
    let kinds: Vec<&str> = rows
        .iter()
        .map(|row| row["record"].as_str().expect("a row's kind"))
        .collect();
    assert_eq!(
        kinds.iter().filter(|kind| **kind == "turn").count(),
        2,
        "{written}"
    );
    assert_eq!(
        kinds.iter().filter(|kind| **kind == "response").count(),
        2,
        "{written}"
    );
    for row in &rows {
        match row["record"].as_str() {
            Some("turn") => assert_eq!(row["prefill_tokens"], 178, "{row}"),
            Some("response") => assert_eq!(row["output_tokens"], 66, "{row}"),
            _ => {}
        }
    }
    let sidecar = PathBuf::from(format!("{path}.unspellable.json"));
    let named = log_line_object(&std::fs::read_to_string(&sidecar).expect("the sidecar"));
    let named_kinds: Vec<&str> = named["unspellable"]
        .as_array()
        .expect("a list")
        .iter()
        .filter_map(|item| item["kind"].as_str())
        .collect();
    assert!(
        !named_kinds.contains(&"response"),
        "a response was left unspellable: {named}"
    );
    let _ = std::fs::remove_file(&sidecar);
}

/// The replay answers only a regimen naming it: the canned-cache-n regimen
/// is refused against it (its literal is another digest's), and the
/// rehearsal regimen is refused against the canned server.
#[test]
fn the_stream_replay_and_the_canned_server_each_pass_only_their_own_regimen() {
    let replayed = replaying(false);
    let refused = Command::new(DRIVE)
        .args([
            "serve",
            "--endpoint",
            &replayed.endpoint,
            "--model",
            "m",
            "--head",
        ])
        .arg(&file_holding("head", HEAD).0)
        .args(["--regimen", &dev_loop(), "--port", "0"])
        .output()
        .expect("diet-drive runs");
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let canned =
        Stub::serving_with_props(vec![], &diet::drive::canned::build_info()).expect("loopback");
    let tree = Dir::new("replay-refused-tree");
    let refused = Command::new(DRIVE)
        .args([
            "serve",
            "--endpoint",
            &canned.url(),
            "--model",
            "m",
            "--head",
        ])
        .arg(&file_holding("head", HEAD).0)
        .args([
            "--regimen",
            &replay_regimen(),
            "--worktree",
            &tree.path(),
            "--port",
            "0",
        ])
        .arg("--auth-file")
        .arg(&file_holding("auth", "author:s3cret\n").0)
        .output()
        .expect("diet-drive runs");
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
}

/// The tool-turn replay through the gate (#411's follow-up): each ask is
/// answered with the captured `bash` call of `ls | wc -l`. On turn one `wc`
/// is not in the regimen's allowed set, so the call waits on the operator;
/// approved for the session, it runs in the worktree, its output goes back
/// as a tool message, and the captured answer settles the turn `final`. On
/// turn two the standing approval covers it and nothing prompts. The record
/// reads, with a row for each call.
#[test]
fn the_tool_turn_replay_runs_a_call_through_the_gate_and_its_approval() {
    let replayed = replaying(false);
    let tree = Dir::new("replay-tools-tree");
    let state = Dir::new("replay-tools-state");
    let auth = file_holding("auth", "author:s3cret\n");
    let auth_path = auth.0.to_string_lossy().into_owned();
    let record = file_holding("record", "");
    let path = record.0.to_string_lossy().into_owned();
    let served = start_with(
        &replayed.endpoint,
        &[
            "--regimen",
            &replay_regimen(),
            "--worktree",
            &tree.path(),
            "--auth-file",
            &auth_path,
            "--record",
            &path,
        ],
        &[("XDG_STATE_HOME", &state.path())],
    );
    assert_eq!(served.substrate.as_deref(), Some("canned-replay-tools"));
    let address = served.listening.clone();
    let reply = post_authed(&address, "/commands", r#"{"kind":"ask","text":"count"}"#);
    assert_eq!(status(&reply), 200, "{reply}");
    let read = events_authed(&address, |read| read.contains("event: waiting"));
    let waiting = read
        .split("event: waiting\ndata: ")
        .nth(1)
        .and_then(|rest| rest.split('\n').next())
        .unwrap_or_else(|| panic!("no prompt: {read}"));
    let prompt = log_line_object(waiting);
    assert_eq!(prompt["command"], "ls | wc -l", "{prompt}");
    let approve = format!(
        r#"{{"call":"{}","scope":"session"}}"#,
        prompt["id"].as_str().expect("the call's id")
    );
    let reply = post_authed(&address, "/approve", &approve);
    assert_eq!(status(&reply), 204, "{reply}");
    events_authed(&address, |read| {
        read.matches(r#""reason":"final""#).count() >= 1
    });
    let reply = post_authed(&address, "/commands", r#"{"kind":"ask","text":"again"}"#);
    assert_eq!(status(&reply), 200, "{reply}");
    let read = events_authed(&address, |read| {
        read.matches(r#""reason":"final""#).count() >= 2
    });
    assert_eq!(
        read.matches("event: waiting").count(),
        0,
        "a fresh stream shows no prompt once both turns settled: {read}"
    );
    let reply = post_authed(&address, "/commands", r#"{"kind":"end"}"#);
    assert_eq!(status(&reply), 200, "{reply}");
    let _report = served
        .said
        .recv_timeout(Duration::from_secs(10))
        .expect("the record's report");
    the_tool_turn_record_reads(&record.0);
    let _ = std::fs::remove_file(format!("{path}.unspellable.json"));
}

/// The tool-turn replay's record: `diet check-record` reads it, each turn is
/// one call and then its answer, and turn one's call was approved for the
/// session at its prompt while turn two's ran under that same approval --
/// recorded as it, not as the pre-seed that covered `ls`.
fn the_tool_turn_record_reads(record: &std::path::Path) {
    let checked = Command::new(DIET)
        .args(["check-record"])
        .arg(record)
        .output()
        .expect("diet runs");
    assert_eq!(
        checked.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&checked.stdout)
    );
    let written = std::fs::read_to_string(record).expect("the record");
    let kinds: Vec<String> = written
        .lines()
        .map(|line| {
            log_line_object(line)["record"]
                .as_str()
                .expect("a row's kind")
                .to_owned()
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "start",
            "turn",
            "request",
            "response",
            "tool_call",
            "request",
            "prefix.changed",
            "response",
            "turn",
            "request",
            "prefix.changed",
            "response",
            "tool_call",
            "request",
            "prefix.changed",
            "response",
            "summary",
        ],
        "one call, then its answer, each turn: {written}"
    );
    let calls: Vec<serde_json::Value> = written
        .lines()
        .map(log_line_object)
        .filter(|row| row["record"] == "tool_call")
        .collect();
    assert_eq!(calls[0]["outcome"], "ran", "{written}");
    assert_eq!(calls[0]["approval"]["scope"], "session", "{written}");
    assert_eq!(calls[0]["approval"]["why"], "not_approved", "{written}");
    assert_eq!(calls[1]["outcome"], "ran", "{written}");
    assert_eq!(
        calls[1]["approval"], calls[0]["approval"],
        "turn two's call ran under turn one's session approval, unprompted: {written}"
    );
}

// ---------------------------------------------------------------------------
// the extraction seat, offboard (#570)
// ---------------------------------------------------------------------------

/// The dev loop's regimen, its forks seated offboard on the replay
/// substrate.
fn seated_regimen() -> HeadFile {
    dev_loop_sampling(
        "interview_warrant = [\"scoping\"]\nextraction_seat = \"offboard:canned-replay\"\n",
        "temperature = 0.6\n",
    )
}

#[test]
fn a_drive_servers_offboard_seat_takes_the_fork_and_the_record_names_it() {
    let trunk = Stub::serving_with_props(
        vec![Act::Raw(ANSWERED.to_vec())],
        &diet::drive::canned::build_info(),
    )
    .expect("loopback");
    let seat = Stub::serving_with_props(
        vec![Act::Raw(ANSWERED.to_vec())],
        &diet::drive::canned::replay_build_info(),
    )
    .expect("loopback");
    let regimen = seated_regimen();
    let regimen_path = regimen.0.to_string_lossy().into_owned();
    let record = file_holding("record", "");
    let path = record.0.to_string_lossy().into_owned();
    let log_file = file_holding("log", "");
    let logged = log_file.0.to_string_lossy().into_owned();
    let served = start(
        &trunk.url(),
        &[
            "--regimen",
            &regimen_path,
            "--record",
            &path,
            "--log",
            &logged,
            "--seat-endpoint",
            &seat.url(),
            "--seat-model",
            "a-small-model",
        ],
    );
    let address = served.listening.clone();
    let reply = post(
        &address,
        &address,
        r#"{"kind":"ask","text":"what are we building?","scoping":true}"#,
    );
    assert_eq!(status(&reply), 200, "{reply}");
    let read = exchange(
        &address,
        &format!("GET /events?from=0 HTTP/1.1\r\nHost: {address}\r\n\r\n"),
        |read| read.contains("fork.settled") && read.contains(r#""to":"awaiting""#),
    );
    assert!(read.contains("fork.settled"), "the fork settled: {read}");
    let reply = post(&address, &address, r#"{"kind":"end"}"#);
    assert_eq!(status(&reply), 200, "{reply}");
    let _report = served
        .said
        .recv_timeout(Duration::from_secs(10))
        .expect("the record's report");

    // The wire: the trunk's server saw the turn, the seat's the fork, which
    // named the seat's model and saw the last turn, then its question.
    let [turn] = trunk.received().try_into().expect("one trunk request");
    let [fork] = seat
        .received()
        .try_into()
        .expect("one fork request, to the seat");
    assert_eq!(log_line_object(&turn)["model"], "a-model", "{turn}");
    assert_eq!(log_line_object(&fork)["model"], "a-small-model", "{fork}");
    let messages = |body: &str| log_line_object(body)["messages"].as_array().cloned();
    let (turn_messages, fork_messages) = (
        messages(&turn).expect("messages"),
        messages(&fork).expect("messages"),
    );
    assert_eq!(fork_messages.len(), turn_messages.len() + 2, "{fork}");
    assert_eq!(fork_messages[..turn_messages.len()], turn_messages[..]);

    // The log's fork line names the seat and its model.
    let lines = std::fs::read_to_string(&log_file.0).expect("the log");
    let fork_line = lines
        .lines()
        .map(log_line_object)
        .find(|line| line["kind"] == "fork")
        .expect("a fork line");
    assert_eq!(
        (
            &fork_line["substrate"],
            &fork_line["model"],
            &fork_line["view"]
        ),
        (
            &serde_json::json!("canned-replay"),
            &serde_json::json!("a-small-model"),
            &serde_json::json!("last_turn")
        ),
        "{fork_line}"
    );

    the_fork_settling_names_its_cost(&lines);
    the_record_names_the_seat(&std::fs::read_to_string(&record.0).expect("the record"));
    let _ = std::fs::remove_file(format!("{path}.unspellable.json"));
    let _ = std::fs::remove_file(format!("{path}.product.txt"));
}

/// The fork's settling line: the prompt its server evaluated, as the fork's
/// response reported it, and the call's wall time.
fn the_fork_settling_names_its_cost(lines: &str) {
    let settled_line = lines
        .lines()
        .map(log_line_object)
        .find(|line| line["kind"] == "fork.settled")
        .expect("a fork.settled line");
    let reported = lines
        .lines()
        .map(log_line_object)
        .rev()
        .find(|line| line["kind"] == "response")
        .expect("the fork's response")["timings"]["prompt_n"]
        .clone();
    assert!(reported.is_u64(), "{reported}");
    assert_eq!(settled_line["prompt_tokens"], reported, "{settled_line}");
    assert!(settled_line["wall_ms"].is_u64(), "{settled_line}");
}

/// The record: two substrates, the seat second; the interview lane's rows
/// name the seat, the trunk's the trunk's; the start row's levers say so.
fn the_record_names_the_seat(written: &str) {
    let rows: Vec<serde_json::Value> = written.lines().map(log_line_object).collect();
    let substrates = rows[0]["regime"]["substrates"]
        .as_array()
        .expect("substrates");
    let ids: Vec<&str> = substrates.iter().filter_map(|s| s["id"].as_str()).collect();
    assert_eq!(ids, ["canned-cache-n", "canned-replay"], "{}", rows[0]);
    let named: Vec<(&str, &str)> = rows
        .iter()
        .filter_map(|row| Some((row["lane"].as_str()?, row["substrate"].as_str()?)))
        .collect();
    assert!(named.contains(&("trunk", "canned-cache-n")), "{named:?}");
    assert!(named.contains(&("interview", "canned-replay")), "{named:?}");
    for lever in [
        r#""extraction_seat":"offboard:canned-replay""#,
        r#""fork_input_view":"last_turn:defaulted""#,
    ] {
        assert!(rows[0].to_string().contains(lever), "{lever}: {}", rows[0]);
    }
    assert!(
        named
            .iter()
            .all(|(lane, id)| (*lane == "interview") == (*id == "canned-replay")),
        "{named:?}"
    );
}

#[test]
fn a_drive_server_refuses_an_offboard_seat_it_cannot_reach_as_declared() {
    let regimen = seated_regimen();
    let regimen_path = regimen.0.to_string_lossy().into_owned();
    let trunk = || {
        Stub::serving_with_props(Vec::new(), &diet::drive::canned::build_info()).expect("loopback")
    };

    // Declared offboard, no seat flags.
    let stub = trunk();
    let (code, said) = run_briefly(&stub.url(), &["--regimen", &regimen_path]);
    assert_eq!(code, Some(1), "{said}");
    assert!(
        said.contains("--seat-endpoint and --seat-model") && !said.contains("listening"),
        "{said}"
    );

    // Seat flags, and a warm regimen.
    let warm = dev_loop_sampling("", "temperature = 0.6\n");
    let warm_path = warm.0.to_string_lossy().into_owned();
    let stub = trunk();
    let (code, said) = run_briefly(
        &stub.url(),
        &[
            "--regimen",
            &warm_path,
            "--seat-endpoint",
            "http://127.0.0.1:9",
            "--seat-model",
            "m",
        ],
    );
    assert_eq!(code, Some(1), "{said}");
    assert!(said.contains("the regimen declares none"), "{said}");

    // The trunk's own endpoint.
    let stub = trunk();
    let (code, said) = run_briefly(
        &stub.url(),
        &[
            "--regimen",
            &regimen_path,
            "--seat-endpoint",
            &stub.url(),
            "--seat-model",
            "m",
        ],
    );
    assert_eq!(code, Some(1), "{said}");
    assert!(said.contains("the trunk's own endpoint"), "{said}");

    // A seat whose engine is not the registry's: the trunk's build, not the
    // replay substrate's.
    let stub = trunk();
    let impostor = trunk();
    let (code, said) = run_briefly(
        &stub.url(),
        &[
            "--regimen",
            &regimen_path,
            "--seat-endpoint",
            &impostor.url(),
            "--seat-model",
            "m",
        ],
    );
    assert_eq!(code, Some(1), "{said}");
    assert!(
        said.contains("the extraction seat: ") && !said.contains("listening"),
        "{said}"
    );
}

// ---------------------------------------------------------------------------
// model-elected pruning (#612)
// ---------------------------------------------------------------------------

#[test]
fn a_drive_server_offers_prune_output_only_where_a_prune_can_be_applied() {
    let tree = Dir::new("prune-tree");
    let auth = file_holding("auth", "author:s3cret\n");
    let auth_path = auth.0.to_string_lossy().into_owned();
    let pruning = "model_pruning = \"on\"\n";

    // Commands and a warrant: `prune_output` after the surface's tools.
    let regimen = commands_regimen(
        &format!("allowed_commands = []\ninterview_warrant = [\"scoping\"]\n{pruning}"),
        "isolation = \"none\"",
        "[limits]\nmax_steps = 4\n",
    );
    let path = regimen.0.to_string_lossy().into_owned();
    let stub =
        Stub::serving_with_props(Vec::new(), &diet::drive::canned::build_info()).expect("loopback");
    let log_file = file_holding("log", "");
    let logged = log_file.0.to_string_lossy().into_owned();
    let _served = start(
        &stub.url(),
        &[
            "--regimen",
            &path,
            "--worktree",
            &tree.path(),
            "--auth-file",
            &auth_path,
            "--log",
            &logged,
        ],
    );
    let start_line = first_logged_line(&log_file.0);
    assert_eq!(
        start_line["tools"],
        serde_json::json!(["bash", "prune_output"]),
        "{start_line}"
    );

    // No warrant: nothing fills working memory, so no seam could apply it.
    let regimen = commands_regimen(
        &format!("allowed_commands = []\n{pruning}"),
        "isolation = \"none\"",
        "[limits]\nmax_steps = 4\n",
    );
    let path = regimen.0.to_string_lossy().into_owned();
    let stub =
        Stub::serving_with_props(Vec::new(), &diet::drive::canned::build_info()).expect("loopback");
    let (code, said) = run_briefly(
        &stub.url(),
        &[
            "--regimen",
            &path,
            "--worktree",
            &tree.path(),
            "--auth-file",
            &auth_path,
        ],
    );
    assert_eq!(code, Some(1), "{said}");
    assert!(
        said.contains("no seam could ever replace a pruned result"),
        "{said}"
    );

    // No commands: nothing to prune.
    let regimen = dev_loop_sampling(
        &format!("interview_warrant = [\"scoping\"]\n{pruning}"),
        "temperature = 0.6\n",
    );
    let path = regimen.0.to_string_lossy().into_owned();
    let stub =
        Stub::serving_with_props(Vec::new(), &diet::drive::canned::build_info()).expect("loopback");
    let (code, said) = run_briefly(&stub.url(), &["--regimen", &path]);
    assert_eq!(code, Some(1), "{said}");
    assert!(
        said.contains("needs a session that runs commands"),
        "{said}"
    );
}
