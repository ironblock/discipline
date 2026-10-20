//! `diet-drive scenario` (#760): a scenario driven end to end on the canned
//! substrate -- `serve` started as a child, driven over its HTTP API, its
//! log checked against the scenario's expectations.

use std::path::{Path, PathBuf};
use std::process::Command;

const DRIVE: &str = env!("CARGO_BIN_EXE_diet-drive");

/// The repository root: where `diet-drive scenario` resolves `scripts/`.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits in the repository")
        .to_path_buf()
}

/// A fresh directory for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "diet-scenario-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after the epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// A scenario file in `dir` whose header names the canned regimen and the
/// floor's head by absolute path, then `body`'s lines.
fn scenario(dir: &Path, name: &str, body: &str) -> PathBuf {
    let root = root();
    let header = serde_json::json!({
        "scenario": name,
        "regimen": root.join("diet/scenarios/canned.toml"),
        "head": root.join("diet/drive/heads/floor.md"),
    });
    let path = dir.join(format!("{name}.jsonl"));
    std::fs::write(&path, format!("{header}\n{body}")).expect("the scenario");
    path
}

/// Run `diet-drive scenario <file> --out <out> --no-assemble`: its exit
/// code and its last stdout line, as JSON.
fn run(file: &Path, out: &Path) -> (Option<i32>, serde_json::Value) {
    let ran = Command::new(DRIVE)
        .current_dir(root())
        .args(["scenario"])
        .arg(file)
        .arg("--out")
        .arg(out)
        .arg("--no-assemble")
        .output()
        .expect("diet-drive runs");
    let stdout = String::from_utf8_lossy(&ran.stdout);
    let last = stdout.lines().last().unwrap_or("");
    let said = serde_json::from_str(last).unwrap_or_else(|_| {
        panic!(
            "not JSON: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&ran.stderr)
        )
    });
    (ran.status.code(), said)
}

/// A scenario that asks once and expects the turn to settle `final`
/// passes: exit 0, `ok`, the replies' digest named, and the log and the
/// record written under `--out`.
#[test]
fn a_scenario_on_the_canned_substrate_passes_and_names_its_replies() {
    let dir = scratch("passes");
    let file = scenario(
        &dir,
        "smoke",
        concat!(
            r#"{"reply":{"when":"hello there","text":"Hello."}}"#,
            "\n",
            r#"{"step":"ask","text":"hello there"}"#,
            "\n",
            r#"{"step":"end"}"#,
            "\n",
            r#"{"expect":"turn.settled","where":{"reason":"final"}}"#,
            "\n",
        ),
    );
    let out = dir.join("out");
    let (code, said) = run(&file, &out);
    assert_eq!(code, Some(0), "{said}");
    assert_eq!(said["ok"], true, "{said}");
    assert_eq!(said["scenario"], "smoke");
    assert_eq!(said["substrate"], "canned-scenario");
    assert_eq!(said["replies_sha256"].as_str().map(str::len), Some(64));
    assert!(out.join("session.log").is_file(), "{said}");
    assert!(out.join("session.record.jsonl").is_file(), "{said}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// An expectation the log does not meet fails the run, naming it.
#[test]
fn an_unmet_expectation_fails_the_run_naming_it() {
    let dir = scratch("unmet");
    let file = scenario(
        &dir,
        "unmet",
        concat!(
            r#"{"reply":{"text":"Hello."}}"#,
            "\n",
            r#"{"step":"ask","text":"hi"}"#,
            "\n",
            r#"{"step":"end"}"#,
            "\n",
            r#"{"expect":"meditation.held"}"#,
            "\n",
        ),
    );
    let (code, said) = run(&file, &dir.join("out"));
    assert_eq!(code, Some(1), "{said}");
    assert_eq!(said["ok"], false);
    assert!(
        said["unmet"].to_string().contains("meditation.held"),
        "{said}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A request no reply answers fails the run, naming the request's last
/// message -- never answered with something generic.
#[test]
fn a_request_no_reply_answers_fails_the_run_naming_it() {
    let dir = scratch("unmatched");
    let file = scenario(
        &dir,
        "unmatched",
        concat!(
            r#"{"reply":{"when":"something else","text":"Hello."}}"#,
            "\n",
            r#"{"step":"ask","text":"an ask nobody wrote a reply for"}"#,
            "\n",
            r#"{"step":"end"}"#,
            "\n",
        ),
    );
    let (code, said) = run(&file, &dir.join("out"));
    assert_eq!(code, Some(1), "{said}");
    assert!(
        said["unmatched"]
            .to_string()
            .contains("an ask nobody wrote a reply for"),
        "{said}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// #798: on `canned-scenario-windowed`, whose registry entry declares a
/// context window, a reply reporting a large prompt fires the budget
/// meditation at its turn's end, and a large tool result fires the window
/// meditation inside a later turn, before the trunk's next request -- once
/// the trunk holds a turn it can drop.
#[test]
fn a_windowed_canned_substrate_fires_the_budget_and_the_window_meditations() {
    let dir = scratch("windowed");
    std::fs::write(
        dir.join("windowed.toml"),
        "arm = \"rehearsal\"\ndogma_version = 0\n\
         substrate = \"canned-scenario-windowed\"\nsubstrate_reasoning = \"off\"\n\
         substrate_hardware = \"b6bc80e9e8e4bc2342219f064dab89a8d17c02535600b32b84895fc0b9ba6513\"\n\
         isolation = \"none\"\napproval = \"none\"\n\
         approval_policy = \"approvals are off: every command runs without a gate decision or a prompt\"\n\
         interview_warrant = [\"scoping\"]\ncapture_modality = \"fields\"\n\
         max_output_tokens = 512\nmeditation_at_context_fraction = 0.2\n\
         [limits]\nmax_steps = 4\n[sampler]\ntemperature = 0.6\n",
    )
    .expect("the regimen");
    let header = serde_json::json!({
        "scenario": "windowed",
        "regimen": "windowed.toml",
        "head": root().join("diet/drive/heads/floor.md"),
    });
    let body = [
        r#"{"reply":{"when":"Scope it","text":"Scoped: one tracker.","prompt_tokens":5000}}"#,
        r#"{"reply":{"when":"The turn has ended","text":"DECISION: one tracker\nPLAN: NONE\nSUPERSEDE: NONE"}}"#,
        r#"{"reply":{"when":"Name it","text":"It is the tracker."}}"#,
        r#"{"reply":{"when":"Print it","text":"Printing.","calls":[{"id":"call-b","name":"bash","arguments":"{\"command\": \"python3 -c \\\"print('x' * 20000)\\\"\"}"}]}}"#,
        r#"{"reply":{"when":"xxxxxxxxxx","text":"Printed."}}"#,
        r#"{"reply":{"text":"."}}"#,
        r#"{"step":"ask","text":"Scope it: one tracker.","scoping":true}"#,
        r#"{"step":"ask","text":"Name it."}"#,
        r#"{"step":"ask","text":"Print it: a long line."}"#,
        r#"{"step":"end"}"#,
        r#"{"expect":"meditation","name":"budget","where":{"reason":"budget","at_turn":"1"}}"#,
        r#"{"expect":"meditation","where":{"reason":"window","at_turn":"3","window":"24000"},"after":"budget"}"#,
    ]
    .join("\n");
    let file = dir.join("windowed.jsonl");
    std::fs::write(&file, format!("{header}\n{body}\n")).expect("the scenario");
    let (code, said) = run(&file, &dir.join("out"));
    assert_eq!(code, Some(0), "{said}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file an ask attaches is kept in the assembled run (#372's layout,
/// `files/<sha256>`), as serve's own recording keeps it: the work
/// directory that held the copy goes once the run is assembled, and a
/// replay fetches the file from the run.
#[test]
fn an_assembled_run_keeps_the_files_its_asks_attached() {
    let dir = scratch("keeps-files");
    let png: &[u8] = b"\x89PNG\r\n\x1a\n0000";
    let sha256 = diet::digest::sha256_hex(png);
    std::fs::write(dir.join("shot.png"), png).expect("the picture");
    std::fs::write(
        dir.join("tools.toml"),
        concat!(
            "arm = \"rehearsal\"\n",
            "dogma_version = 0\n",
            "substrate = \"canned-scenario-windowed\"\n",
            "substrate_reasoning = \"off\"\n",
            "substrate_hardware = \"b6bc80e9e8e4bc2342219f064dab89a8d17c02535600b32b84895fc0b9ba6513\"\n",
            "isolation = \"none\"\n",
            "approval = \"none\"\n",
            "approval_policy = \"approvals are off: every command runs without a gate decision or a prompt\"\n",
            "tool_surface = \"standard\"\n",
            "[limits]\nmax_steps = 2\n",
            "[sampler]\ntemperature = 0.6\ntop_p = 0.95\nseed = 7\n",
        ),
    )
    .expect("the regimen");
    let header = serde_json::json!({
        "scenario": "attaches",
        "regimen": "tools.toml",
        "head": root().join("diet/drive/heads/floor.md"),
        "seed": {"shot.png": "shot.png"},
    });
    let file = dir.join("attaches.jsonl");
    std::fs::write(
        &file,
        format!(
            "{header}\n{}",
            concat!(
                r#"{"reply":{"text":"A PNG signature and four bytes."}}"#,
                "\n",
                r#"{"step":"ask","text":"What is in the picture?","attach":["shot.png"]}"#,
                "\n",
                r#"{"step":"end"}"#,
                "\n",
                r#"{"expect":"turn.settled","where":{"reason":"final"}}"#,
                "\n",
            )
        ),
    )
    .expect("the scenario");
    let out = dir.join("out");
    let ran = Command::new(DRIVE)
        .current_dir(root())
        .arg("scenario")
        .arg(&file)
        .arg("--out")
        .arg(&out)
        .output()
        .expect("diet-drive runs");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(ran.status.code(), Some(0), "{said}");
    let log = std::fs::read_to_string(out.join("session.log")).expect("the assembled log");
    assert!(
        log.contains(&format!("files/{sha256}")),
        "the log names the copy: {said}"
    );
    assert_eq!(
        std::fs::read(out.join("files").join(&sha256))
            .ok()
            .as_deref(),
        Some(png),
        "the copy is in the run under the digest the log names: {said}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A run's log with what may differ between two runs of one scenario
/// taken out -- when each line was written (`t`, `opened`, and a warrant
/// gate's `latency_ms`), its place (`seq`: a background job's end lands
/// where its timing puts it, and a gate's call or an audit's chunks beside
/// each other interleave as they finish, #862), so each reference to a line
/// by its `seq` is read as that line's kind, where a command really ran
/// (`cwd`), and the real roots `shown_roots` declares -- as a sorted list of
/// lines, and its kept outputs by name and bytes.
/// Everything else -- every message, result, notice, size and digest the
/// model's requests were made of -- stays.
fn run_of(out: &Path) -> (Vec<String>, std::collections::BTreeMap<String, Vec<u8>>) {
    // The fields that name another line by its `seq`, as the exercise's
    // lane check reads them (`exercise/src/drive/scenario-log.ts`).
    const REFERENCES: [&str; 8] = [
        "request",
        "to_request",
        "fork",
        "at",
        "cut",
        "after",
        "last_seq",
        "opened_by",
    ];
    let log = std::fs::read_to_string(out.join("session.log")).expect("the log");
    let events: Vec<serde_json::Value> = log
        .lines()
        .map(|line| serde_json::from_str(line).expect("a log line"))
        .collect();
    let kinds: std::collections::BTreeMap<u64, String> = events
        .iter()
        .filter_map(|event| Some((event["seq"].as_u64()?, event["kind"].as_str()?.to_owned())))
        .collect();
    let kind_of = |seq: u64| format!("<{}>", kinds.get(&seq).map_or("?", String::as_str));
    let mut lines: Vec<String> = events
        .into_iter()
        .map(|mut event| {
            let object = event.as_object_mut().expect("an object");
            for key in ["t", "seq", "opened", "cwd", "latency_ms"] {
                object.remove(key);
            }
            for key in REFERENCES {
                if let Some(seq) = object.get(key).and_then(serde_json::Value::as_u64) {
                    object.insert(key.to_owned(), serde_json::Value::from(kind_of(seq)));
                }
            }
            // A fork's trigger names its line after its word: `open:102`.
            if let Some((word, seq)) = object
                .get("trigger")
                .and_then(serde_json::Value::as_str)
                .and_then(|trigger| trigger.split_once(':'))
                .and_then(|(word, seq)| Some((word.to_owned(), seq.parse::<u64>().ok()?)))
            {
                object.insert(
                    "trigger".to_owned(),
                    serde_json::Value::from(format!("{word}:{}", kind_of(seq))),
                );
            }
            if let Some(roots) = object
                .get_mut("shown_roots")
                .and_then(serde_json::Value::as_object_mut)
            {
                for real in roots.values_mut() {
                    *real = serde_json::Value::from("<real>");
                }
            }
            event.to_string()
        })
        .collect();
    lines.sort();
    let kept = std::fs::read_dir(out.join("files"))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| {
                    (
                        entry.file_name().to_string_lossy().into_owned(),
                        std::fs::read(entry.path()).expect("a kept output"),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    (lines, kept)
}

/// A background job that outlasts its turn ends at one place in the log
/// only if the next ask waits for it: `wait` with `where` waits past the
/// lines of its kind that do not match. With the wait, the next ask's
/// notice reports the job completed; without it, the job has not ended by
/// then, and the run fails -- every time, not only under load. What made
/// two kitchen sinks run side by side show their model different notices.
#[test]
fn a_wait_with_where_holds_the_next_ask_for_the_job_it_names() {
    let scenario = root().join("diet/scenarios/background-wait.jsonl");
    let written = std::fs::read_to_string(&scenario).expect("the scenario");
    let dir = scratch("background-wait");
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let unwaited = dir.join("unwaited.jsonl");
    let lines: Vec<&str> = written
        .lines()
        .filter(|line| !line.starts_with(r#"{"step":"wait""#))
        .collect();
    assert_eq!(lines.len() + 1, written.lines().count(), "one wait step");
    let head = root().join("diet/drive/heads/floor.md");
    let regimen = root().join("diet/scenarios/background-wait.toml");
    std::fs::write(
        &unwaited,
        lines
            .join("\n")
            .replace(
                "\"../drive/heads/floor.md\"",
                &format!("{:?}", head.display().to_string()),
            )
            .replace(
                "\"background-wait.toml\"",
                &format!("{:?}", regimen.display().to_string()),
            )
            + "\n",
    )
    .expect("the unwaited copy");
    let run = |file: &Path, out: &str| {
        let ran = Command::new(DRIVE)
            .current_dir(root())
            .arg("scenario")
            .arg(file)
            .arg("--out")
            .arg(dir.join(out))
            .arg("--no-assemble")
            .output()
            .expect("diet-drive runs");
        let said: serde_json::Value =
            serde_json::from_slice(&ran.stdout).expect("the run's report");
        (ran.status.code(), said)
    };
    let (code, said) = run(&scenario, "waited");
    assert_eq!(
        (code, &said["ok"]),
        (Some(0), &serde_json::json!(true)),
        "{said}"
    );
    let (code, said) = run(&unwaited, "unwaited");
    assert_eq!(code, Some(1), "{said}");
    let unmet = said["unmet"].to_string();
    assert!(
        unmet.contains("kind=notice") && unmet.contains("completed"),
        "{said}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A scenario shows its model the same bytes wherever it runs (path-free
/// scenario outputs): its worktree and recording are shown as fixed
/// placeholders, its background jobs are named from their count and shown
/// pid 0, so two runs at once under roots of different lengths and parents
/// -- /var/folders here, /tmp on CI -- write the same log but for when and
/// where, and keep the same outputs, byte for byte, under the same digests.
#[test]
fn a_scenario_shows_its_model_the_same_bytes_under_any_roots() {
    let dir = scratch("path-free");
    // The two runs at once: each is a minute of the kitchen sink's waits.
    let children: Vec<(PathBuf, std::process::Child)> =
        ["a", "a-much-longer-parent-directory/nested"]
            .iter()
            .map(|parent| {
                let out = dir.join(parent).join("out");
                let child = Command::new(DRIVE)
                    .current_dir(root())
                    .arg("scenario")
                    .arg(root().join("diet/scenarios/kitchen-sink.jsonl"))
                    .arg("--out")
                    .arg(&out)
                    .arg("--worktree")
                    .arg(dir.join(parent).join("wt"))
                    .arg("--no-assemble")
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .expect("diet-drive runs");
                (out, child)
            })
            .collect();
    let mut runs = Vec::new();
    for (out, child) in children {
        let ran = child.wait_with_output().expect("diet-drive ends");
        assert_eq!(
            ran.status.code(),
            Some(0),
            "{}{}",
            String::from_utf8_lossy(&ran.stdout),
            String::from_utf8_lossy(&ran.stderr)
        );
        runs.push(run_of(&out));
    }
    let (first, second) = (&runs[0], &runs[1]);
    assert!(
        first.0.iter().any(|line| line.contains("\"shown_roots\"")),
        "the start declares its roots"
    );
    for (a, b) in first.0.iter().zip(&second.0) {
        assert_eq!(a, b, "the two runs' logs differ");
    }
    assert_eq!(first.0.len(), second.0.len());
    assert_eq!(
        first.1.keys().collect::<Vec<_>>(),
        second.1.keys().collect::<Vec<_>>(),
        "the kept outputs' digests differ"
    );
    assert_eq!(first.1, second.1, "the kept outputs' bytes differ");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The heads in `out`'s record sidecar that could not be rebuilt from the
/// log.
fn unrebuilt(out: &Path) -> Vec<serde_json::Value> {
    let sidecar: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(out.join("session.record.jsonl.unspellable.json"))
            .expect("the sidecar"),
    )
    .expect("JSON");
    sidecar["unspellable"]
        .as_array()
        .expect("unspellable")
        .iter()
        .filter(|item| {
            item["why"]
                .as_str()
                .is_some_and(|why| why.contains("could not be rebuilt"))
        })
        .cloned()
        .collect()
}

/// #882: a phased session declares the phase tool, and its record rebuilds
/// every request's head from the log as any other session's does: the
/// sidecar of the shipped two-phase scenario names no head it could not
/// rebuild.
#[test]
fn a_phased_sessions_request_heads_are_rebuilt_from_its_log() {
    let dir = scratch("phased-heads");
    let out = dir.join("out");
    let (code, said) = run(&root().join("diet/scenarios/two-phases.jsonl"), &out);
    assert_eq!(code, Some(0), "{said}");
    let log = std::fs::read_to_string(out.join("session.log")).expect("the log");
    let start: serde_json::Value =
        serde_json::from_str(log.lines().next().expect("a start")).expect("JSON");
    assert!(
        start["tools"]
            .as_array()
            .is_some_and(|tools| tools.iter().any(|tool| tool == "propose_phase_transition")),
        "the session declared the phase tool: {start}"
    );
    let unrebuilt = unrebuilt(&out);
    assert!(unrebuilt.is_empty(), "{unrebuilt:#?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// #884: a window meditation that splits a running turn (#813) refills the
/// trunk mid-turn, and the record rebuilds every request's head after it:
/// the sidecar of the shipped split-turn scenario names none it could not.
#[test]
fn a_split_turns_request_heads_are_rebuilt_from_its_log() {
    let dir = scratch("split-heads");
    let out = dir.join("out");
    let (code, said) = run(&root().join("diet/scenarios/split-turn.jsonl"), &out);
    assert_eq!(code, Some(0), "{said}");
    let log = std::fs::read_to_string(out.join("session.log")).expect("the log");
    assert!(
        log.lines()
            .any(|line| line.contains(r#""kind":"meditation""#)
                && line.contains(r#""turn_steps_kept""#)),
        "the session split a turn"
    );
    let unrebuilt = unrebuilt(&out);
    assert!(unrebuilt.is_empty(), "{unrebuilt:#?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// #886: a window meditation's compaction of a running turn persists, as
/// in `OpenCode` 2 and Pi: the steps it compacted do not come back when the
/// turn settles, so the next ask -- its trunk the refill, the split turn's
/// ask and the steps it kept -- has nothing to drop and fires no meditation.
#[test]
fn a_split_turns_compaction_persists_past_its_settle() {
    let dir = scratch("split-persists");
    let out = dir.join("out");
    let (code, said) = run(&root().join("diet/scenarios/split-turn.jsonl"), &out);
    assert_eq!(code, Some(0), "{said}");
    let log = std::fs::read_to_string(out.join("session.log")).expect("the log");
    let lines: Vec<serde_json::Value> = log
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON"))
        .collect();
    assert_eq!(lines[0]["compaction_persists"], true, "{}", lines[0]);
    let second = lines
        .iter()
        .position(|line| line["kind"] == "ask" && line["turn"] == 2)
        .expect("a second ask");
    let refired: Vec<&serde_json::Value> = lines[second..]
        .iter()
        .filter(|line| line["kind"] == "meditation")
        .collect();
    assert!(refired.is_empty(), "{refired:#?}");
    assert!(unrebuilt(&out).is_empty(), "{:#?}", unrebuilt(&out));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A step's `patience` is how long it may take to settle: a reply held back
/// longer than the ask's patience fails the drive, naming it, and the same
/// ask under a longer patience passes.
#[test]
fn a_step_that_outlives_its_patience_fails_the_drive_and_a_longer_patience_passes() {
    let body = |patience: u64| {
        format!(
            "{}\n{}\n{}\n",
            r#"{"reply":{"when":"take your time","text":"Done.","delay_ms":4000}}"#,
            serde_json::json!({"step": "ask", "text": "take your time", "patience": patience}),
            r#"{"step":"end"}"#,
        )
    };
    let dir = scratch("patience");
    let short = scenario(&dir, "short", &body(1));
    let (code, said) = run(&short, &dir.join("short-out"));
    assert_eq!(code, Some(1), "{said}");
    assert!(said.to_string().contains("not idle after 1s"), "{said}");
    let long = scenario(&dir, "long", &body(60));
    let (code, said) = run(&long, &dir.join("long-out"));
    assert_eq!(code, Some(0), "{said}");
    assert_eq!(said["ok"], true, "{said}");
    let _ = std::fs::remove_dir_all(&dir);
}
