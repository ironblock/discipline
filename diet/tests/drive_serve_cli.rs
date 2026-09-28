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
    let head = HeadFile(std::env::temp_dir().join(format!(
        "diet-drive-serve-{}-{}.txt",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default()
    )));
    std::fs::write(&head.0, HEAD).expect("the head file is written");
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
    let (line, announced) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut first = String::new();
        let _ = BufReader::new(stdout).read_line(&mut first);
        let _ = line.send(first);
    });
    let Ok(first) = announced.recv_timeout(Duration::from_secs(10)) else {
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
    let mut read = String::new();
    let mut chunk = [0_u8; 8192];
    while !done(&read) && Instant::now() < give_up {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => read.push_str(&String::from_utf8_lossy(&chunk[..count])),
            Err(_) => {}
        }
    }
    read
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
    log::parse(&document).expect("what the binary streams is a v0 log");

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
fn a_drive_server_binds_loopback_and_has_no_listen_flag() {
    let stub = Stub::serving(Vec::new()).expect("loopback");
    let served = start(&stub.url(), &[]);
    assert!(
        served.listening.starts_with("127.0.0.1:"),
        "{}",
        served.listening
    );

    let refused = Command::new(DRIVE)
        .args([
            "serve",
            "--endpoint",
            &stub.url(),
            "--model",
            "m",
            "--head",
            "x",
            "--listen",
            "0.0.0.0:0",
        ])
        .output()
        .expect("diet-drive runs");
    assert_eq!(
        refused.status.code(),
        Some(2),
        "`--listen` is I7's, with auth"
    );
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
