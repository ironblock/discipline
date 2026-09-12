//! `diet-replay` as a program, which is the only place the adapter is a
//! product rather than a function.
//!
//! #28 asks for a replay that "produces a working object and a census with no
//! model calls". Both halves of that are properties of the PROGRAM: a unit
//! test can prove `ClaudeCode::adapt` maps a row, but only running the binary
//! shows that a census and an object come out of one invocation, that a log
//! the adapter refuses exits 2 and writes no object, and that nothing on the
//! path needs an endpoint.
//!
//! **Every test here begins with `adapters_`, and that is load-bearing.**
//! #28's own acceptance row is `cargo test -p discipline-diet -- adapters`,
//! and that is a substring filter over test names. A library test carries its
//! module path, so `adapters::claude_code::tests::…` matches it for free; an
//! integration test is named by its function alone and matches nothing unless
//! the filter is in the name. Without the prefix the issue's command would
//! exit 0 having run none of the tests that run the program -- a pass that
//! proves the opposite of what it looks like.
//!
//! The `drive` lane shipped two seeded faults recorded as catching nothing
//! for exactly this reason: its filter never reached the tests that caught
//! them. This file is written after that lesson rather than before it, and
//! the lane's `gate.toml` declares the same filter the issue does.

use std::path::{Path, PathBuf};
use std::process::Command;

const REPLAY: &str = env!("CARGO_BIN_EXE_diet-replay");

/// The regimen this lane ships beside its corpus.
fn regimen() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters/replayed.toml")
}

/// One committed fixture, by name.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("adapters/fixtures/claude-code")
        .join(format!("{name}.jsonl"))
}

/// Run the program and return `(exit code, stdout, stderr)`.
///
/// The environment is stripped of every proxy and endpoint variable a
/// transport could pick a destination out of. It cannot prove the program
/// makes no call -- nothing short of a network namespace does -- but it does
/// mean a future edit that reached for one could not be handed a destination
/// by the test that is supposed to be showing it does not.
fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(REPLAY)
        .args(args)
        .env_remove("HTTP_PROXY")
        .env_remove("HTTPS_PROXY")
        .env_remove("http_proxy")
        .env_remove("https_proxy")
        .env_remove("DIET_ENDPOINT")
        .env_remove("OPENAI_BASE_URL")
        .output()
        .expect("diet-replay runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The two lines of JSON before the object's dump: the census and the tally.
fn heading(stdout: &str) -> (&str, &str) {
    let mut lines = stdout.lines();
    let census = lines.next().expect("a census line");
    let tally = lines.next().expect("a tally line");
    (census, tally)
}

#[test]
fn adapters_an_adapted_session_replays_to_a_census_and_a_working_object() {
    let (code, out, err) = run(&[
        "--adapter",
        "claude-code",
        "--regimen",
        &regimen().to_string_lossy(),
        &fixture("session").to_string_lossy(),
    ]);
    assert_eq!(code, 0, "{err}");

    let (census, tally) = heading(&out);
    assert!(
        census.contains("\"adapter\":\"claude-code\"")
            && census.contains("\"rows\":9")
            && census.contains("\"mapped_rows\":6")
            && census.contains("\"unmapped_rows\":3"),
        "the census is the first line and it counts the corpus: {census}"
    );
    assert!(
        tally.contains("\"entries\":2") && tally.contains("\"events\":6"),
        "and the second line says what the lane derived: {tally}"
    );

    // The object, which is the other half of #28's acceptance line. Its dump
    // opens with the regime -- the one the operator declared, not one the
    // program invented from a log that carries none.
    assert!(
        out.contains("\"object\":\"dump\"") && out.contains("\"arm\":\"replay-fixture\""),
        "the object is dumped under the declared regime: {out}"
    );
    assert!(
        out.contains("the working directory is /srv/project")
            && out.contains("mechanical/file:/srv/project/parser.rs"),
        "with the facts the deterministic lane derived: {out}"
    );
}

#[test]
fn adapters_a_replay_needs_no_endpoint_and_is_given_nowhere_to_dial() {
    // The whole claim of replay mode: work that already happened is read, not
    // redone. The command line has no endpoint to give it, and an object still
    // comes out -- which is what makes the census a free observation rather
    // than a second run of the session.
    let (code, out, _) = run(&[
        "--adapter",
        "claude-code",
        "--regimen",
        &regimen().to_string_lossy(),
        &fixture("session").to_string_lossy(),
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("\"object_version\":2"), "{out}");

    // And no argument position exists for one: a fourth token is a usage
    // error rather than a destination.
    let (code, _, _) = run(&[
        "--adapter",
        "claude-code",
        "--regimen",
        &regimen().to_string_lossy(),
        &fixture("session").to_string_lossy(),
        "http://127.0.0.1:1/v1",
    ]);
    assert_eq!(code, 1, "a second positional is refused, not dialled");
}

#[test]
fn adapters_a_kind_never_seen_is_counted_and_does_not_stop_the_replay() {
    let (code, out, err) = run(&[
        "--adapter",
        "claude-code",
        "--regimen",
        &regimen().to_string_lossy(),
        &fixture("an-unmapped-kind").to_string_lossy(),
    ]);
    assert_eq!(
        code, 0,
        "an unmapped kind is a census entry, not a refusal: {err}"
    );

    let (census, _) = heading(&out);
    assert!(
        census.contains("\"a-kind-this-adapter-has-never-seen\":1")
            && census.contains("\"unmapped_rows\":1"),
        "and it is named in the census rather than silently skipped: {census}"
    );
    assert!(
        census.contains("\"rows\":3") && census.contains("\"mapped_rows\":2"),
        "rows still equals mapped plus unmapped: {census}"
    );
}

#[test]
fn adapters_a_renamed_field_is_refused_with_its_own_exit_code_and_no_object() {
    let (code, out, err) = run(&[
        "--adapter",
        "claude-code",
        "--regimen",
        &regimen().to_string_lossy(),
        &fixture("a-renamed-field").to_string_lossy(),
    ]);

    // 2 is pinned by #28: "the adapter exits 2 (refuses) rather than mapping
    // the wrong field". Distinct from 1, which is everything about the
    // invocation, because "your command line is wrong" and "the harness
    // changed its format" are different things to wake up to.
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("row 2") && err.contains("`assistant`") && err.contains("`message`"),
        "the refusal names the row, the kind and the field: {err}"
    );
    assert!(
        out.is_empty(),
        "and nothing is written: a partial census of a log the adapter does \
         not understand is worse than none. Got: {out}"
    );
}

#[test]
fn adapters_every_refusal_that_is_not_drift_exits_one() {
    let regimen = regimen().to_string_lossy().into_owned();
    let log = fixture("session").to_string_lossy().into_owned();
    let missing = fixture("no-such-fixture").to_string_lossy().into_owned();

    for (why, args) in [
        ("no arguments at all", vec![]),
        (
            "no log",
            vec!["--adapter", "claude-code", "--regimen", &regimen],
        ),
        ("no adapter", vec!["--regimen", &regimen, &log]),
        ("no regimen", vec!["--adapter", "claude-code", &log]),
        (
            "an adapter this build does not have",
            vec!["--adapter", "opencode", "--regimen", &regimen, &log],
        ),
        (
            "a flag nobody defined",
            vec![
                "--adapter",
                "claude-code",
                "--regimen",
                &regimen,
                "--quiet",
                &log,
            ],
        ),
        (
            "a log that is not there",
            vec!["--adapter", "claude-code", "--regimen", &regimen, &missing],
        ),
        (
            "a regimen that is not one",
            vec!["--adapter", "claude-code", "--regimen", &log, &log],
        ),
    ] {
        let (code, out, err) = run(&args);
        assert_eq!(code, 1, "{why}: exits 1, not {code}. {err}");
        assert!(!err.is_empty(), "{why}: and says why");
        assert!(out.is_empty(), "{why}: and prints no census");
    }
}

#[test]
fn adapters_a_replay_is_the_same_bytes_every_time_it_is_run() {
    // The gym reads this output back. A census whose key order or counts
    // depended on a hash seed would make two runs of the same log
    // incomparable, which is the one thing the census exists to allow.
    let regimen = regimen().to_string_lossy().into_owned();
    let log = fixture("session").to_string_lossy().into_owned();
    let args = ["--adapter", "claude-code", "--regimen", &regimen, &log];

    let (first_code, first, _) = run(&args);
    let (again_code, again, _) = run(&args);
    assert_eq!(first_code, 0);
    assert_eq!(again_code, 0);
    assert_eq!(first, again, "two replays of one log differ");
}

#[test]
fn adapters_a_path_that_resolves_to_nothing_makes_no_fact() {
    // Distilled from row 5,955 of the log this adapter was written against: a
    // `Bash` call whose pipeline the capture lane read a file operand out of
    // and could not resolve, with no `cd` before it to resolve against. The
    // lane hands back a touch whose path is the empty string, and the first
    // version of this program turned it into
    // `{"content":"read ","id":"mechanical/file/"}` -- an entry in the working
    // object asserting a read of nothing, which is a fact that is not one.
    let (code, out, err) = run(&[
        "--adapter",
        "claude-code",
        "--regimen",
        &regimen().to_string_lossy(),
        &fixture("a-path-that-resolves-to-nothing").to_string_lossy(),
    ]);
    assert_eq!(code, 0, "{err}");

    let (_, tally) = heading(&out);
    assert!(
        tally.contains("\"entries\":1"),
        "the resolvable operand is the only entry: {tally}"
    );
    assert!(
        out.contains("mechanical/file:tools/gate/AGENTS.md"),
        "and it is the one the lane could resolve: {out}"
    );
    assert!(
        !out.contains("\"id\":\"mechanical/file:\""),
        "while the one it could not resolve is dropped rather than written \
         as a read of the empty string: {out}"
    );
}

#[test]
fn adapters_a_reader_that_stops_reading_is_not_a_failed_replay() {
    // `diet-replay | head` closes the pipe partway through, and `println!`
    // panics when a write fails: the acid test against a 14 MB log printed a
    // correct census and exited 101. A program whose contract is an exit code
    // cannot have one that depends on whether somebody piped it.
    //
    // The log is GENERATED rather than committed because the defect needs
    // more output than a pipe buffer holds -- roughly 64 KiB on Linux. A
    // fixture small enough to be readable would sit inside the buffer, every
    // write would succeed, and the case would pass whether or not the bug was
    // there. A case that cannot go red is not a case.
    use std::fmt::Write as _;

    /// Enough calls that the object's dump is larger than a pipe buffer.
    const CALLS: usize = 1_500;

    let mut log = String::from(
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"touch many files\"},\
         \"sessionId\":\"s\"}\n",
    );
    for at in 0..CALLS {
        let _ = writeln!(
            log,
            "{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":\
             [{{\"type\":\"tool_use\",\"id\":\"t{at}\",\"name\":\"Read\",\"input\":\
             {{\"file_path\":\"/srv/project/a-file-with-a-long-enough-name-{at}.rs\"}}}}],\
             \"usage\":{{\"output_tokens\":1}}}},\"sessionId\":\"s\"}}"
        );
    }
    let dir = std::env::temp_dir().join(format!(
        "diet-replay-pipe-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::create_dir_all(&dir).expect("a directory for the generated log");
    let path = dir.join("many.jsonl");
    std::fs::write(&path, &log).expect("the generated log");

    let mut child = Command::new(REPLAY)
        .args([
            "--adapter",
            "claude-code",
            "--regimen",
            &regimen().to_string_lossy(),
            &path.to_string_lossy(),
        ])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("diet-replay runs");

    // Read one line, the way `head -1` does, then close the pipe.
    let mut stdout = child.stdout.take().expect("a pipe");
    let mut census = String::new();
    {
        use std::io::BufRead as _;
        let mut reader = std::io::BufReader::new(&mut stdout);
        reader.read_line(&mut census).expect("the census line");
    }
    drop(stdout);

    let status = child.wait().expect("it finishes");
    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        census.contains(&format!("\"mapped_rows\":{}", CALLS + 1)),
        "the census was written before the reader left: {census}"
    );
    assert_eq!(
        status.code(),
        Some(0),
        "a reader that stopped reading ends the run at 0, not at a panic's 101"
    );
}
