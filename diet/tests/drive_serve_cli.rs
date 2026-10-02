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

/// llama-server `4df29be`'s reply to one streamed ask (R2b's fixture).
const CAPTURED: &[u8] = include_bytes!("../client/fixtures/llama-server-4df29be-stream.http");

const HEAD: &str = "you are the trunk, served\n";

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
    let head = file_holding("head", HEAD);
    let mut child = Command::new(DRIVE)
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
fn a_served_drive_streams_a_real_servers_answer_over_sse() {
    let stub = Stub::serving(vec![Act::Raw(CAPTURED.to_vec())]).expect("loopback");
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

    let log::Event::SessionStart { opened, head, .. } = &lines[0].event else {
        panic!("the stream does not begin with the session: {stream}");
    };
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
    let stub = Stub::serving(vec![props_saying(&build)]).expect("loopback");
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
    let stub = Stub::serving(vec![props_saying(&build)]).expect("loopback");
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
    let stub = Stub::serving(vec![props_saying("b0-unknown-dirty")]).expect("loopback");
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
    assert_eq!(code, Some(2), "the bind fails: {said}");
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

#[test]
fn a_drive_server_records_a_two_turn_session_that_check_record_reads() {
    // #157's acceptance as ruled (a): the projection and the unspellable
    // path, nothing about counts -- the canned substrate is not a cited
    // engine, so every turn and response is named, never derived.
    // The canned regimen passes only on the canned server (#243).
    let stub = Stub::serving_with_props(
        vec![Act::Raw(CAPTURED.to_vec()), Act::Raw(CAPTURED.to_vec())],
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
        ["start", "request", "request", "prefix.changed"],
        "{written}"
    );
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
        let served = start(&stub.url(), &["--regimen", &dev_loop(), "--record", &path]);
        let left = (
            served.record.clone(),
            std::fs::read_to_string(&record.0).expect("the record"),
            sidecar.exists(),
        );
        let _ = std::fs::remove_file(&sidecar);
        assert_eq!(
            left,
            (Some((path, true)), String::new(), false),
            "{earlier:?}"
        );
    }
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
    assert_eq!(code, Some(2), "the bind fails: {said}");
    assert_eq!(
        left,
        (
            Some("an earlier session's record\n".to_owned()),
            Some("an earlier session's sidecar\n".to_owned())
        )
    );
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
