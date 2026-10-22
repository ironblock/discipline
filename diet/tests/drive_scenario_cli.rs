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
         max_output_tokens = 512\nmeditation_at_context_fraction = 0.1276\n\
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
        r#"{"expect":"meditation","where":{"reason":"window","at_turn":"3","window":"37615"},"after":"budget"}"#,
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
            for key in ["t", "seq", "opened", "cwd", "latency_ms", "wait_ms"] {
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

/// #555 step 4c: a dry run rehearses a script on the canned hosted
/// substrate under the regimen the live run would take -- its substrate
/// alone swapped -- and prints the plan: the rung and its model, the
/// regimen's limits, the requests by lane and by fork kind, their sized
/// prompts summed, their output at `max_tokens`, the retries, whether the
/// cap was met, and the worst case, labelled a bound. Nothing leaves
/// loopback.
#[test]
fn a_dry_run_prints_the_plan_a_live_run_would_spend() {
    let dir = scratch("dry-run");
    let temp = dir.join("temp");
    std::fs::create_dir_all(&temp).expect("a scratch directory");
    // The tree's Sonnet 5.5 regimen, its model's commands run as the script's
    // turn needs: what a live run of hosted.jsonl would take.
    let live = dir.join("live.toml");
    let sonnet = std::fs::read_to_string(root().join("diet/drive/sonnet-5-5.toml"))
        .expect("the Sonnet regimen");
    let runs = "isolation = \"none\"\napproval = \"none\"\napproval_policy = \"approvals are \
                off: every command runs without a gate decision or a prompt\"\n\n\
                [limits]\nmax_steps = 4\n\n";
    let at = sonnet.find("[reasoning]").expect("a reasoning table");
    std::fs::write(&live, format!("{}{runs}{}", &sonnet[..at], &sonnet[at..]))
        .expect("the live regimen");
    let ran = Command::new(DRIVE)
        .current_dir(root())
        .arg("scenario")
        .arg(root().join("diet/scenarios/hosted.jsonl"))
        .arg("--dry-run")
        .arg("--regimen")
        .arg(&live)
        .arg("--out")
        .arg(dir.join("out"))
        .arg("--no-assemble")
        .env_remove("ANTHROPIC_API_KEY")
        .env("TMPDIR", &temp)
        .output()
        .expect("diet-drive runs");
    let said: serde_json::Value = serde_json::from_slice(&ran.stdout).expect("the report");
    assert_eq!(ran.status.code(), Some(0), "{said}");
    assert_eq!(
        rehearsals_in(&temp),
        0,
        "a rehearsal's regimen was left behind"
    );
    let plan = &said["plan"];
    assert_eq!(plan["rung"], "anthropic-claude-sonnet-5-5", "{plan}");
    assert_eq!(plan["model"], "claude-sonnet-5-5", "{plan}");
    assert_eq!(plan["rehearsed_on"], "canned-scenario-hosted", "{plan}");
    assert_eq!(
        (
            &plan["limits"]["max_api_requests"],
            &plan["limits"]["api_retries"]
        ),
        (&serde_json::json!(40), &serde_json::json!(2)),
        "{plan}"
    );
    let rehearsal = &plan["rehearsal"];
    // hosted.jsonl: turn 1 asks twice (the call, then its answer), turn 2
    // once and retries once, turn 3 once and is refused.
    assert_eq!(rehearsal["requests"], 4, "{plan}");
    assert_eq!(rehearsal["retries"], 1, "{plan}");
    assert_eq!(rehearsal["attempts"], 5, "{plan}");
    assert_eq!(rehearsal["by_lane"]["trunk"]["requests"], 4, "{plan}");
    assert_eq!(
        rehearsal["by_lane"]["trunk"]["input_sized"], rehearsal["sized_tokens"],
        "{plan}"
    );
    assert_eq!(
        rehearsal["input_split"]["uncached"],
        serde_json::Value::Null,
        "{plan}"
    );
    assert_eq!(rehearsal["api_cap_met"], false, "{plan}");
    assert!(
        rehearsal["sized_tokens"].as_u64().is_some_and(|n| n > 0),
        "{plan}"
    );
    assert_eq!(
        rehearsal["output_tokens_at_max_tokens"]
            .as_u64()
            .map(|n| n % 4),
        Some(0)
    );
    let bound = &plan["bound"];
    assert_eq!(bound["tokens"], 40 * (1_000_000 + 128_000), "{plan}");
    assert!(
        bound["is"]
            .as_str()
            .is_some_and(|text| text.contains("bound")),
        "{plan}"
    );
    assert_eq!(
        plan["api_unchecked"],
        serde_json::json!(["api_temperature_with_top_p"])
    );
    // #926: a drive that ran to its end plans completely, and its log's and
    // record's starts name the rung it ran under the declarations of.
    assert_eq!(plan["complete"], true, "{plan}");
    for start in [
        first_line(&dir.join("out/session.log")),
        first_line(&dir.join("out/session.record.jsonl")),
    ] {
        assert_eq!(
            start["rehearsing_as"], "anthropic-claude-sonnet-5-5",
            "{start}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The rehearsal regimens a dry run left in `temp`, the temporary directory
/// it was given: its own, so another worktree's dry run, live at the same
/// moment in the shared one, is not counted.
fn rehearsals_in(temp: &Path) -> usize {
    std::fs::read_dir(temp)
        .expect("the temporary directory")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("diet-dry-run-")
        })
        .count()
}

/// #555 step 4c: a dry run plans a hosted rung's spend, so a regimen on any
/// other substrate is refused; and no rehearsal's copy of its regimen is
/// left in the temporary directory, however a run ended.
#[test]
fn a_dry_run_refuses_a_rung_that_is_not_hosted_and_leaves_no_copy() {
    let dir = scratch("dry-run-refused");
    let temp = dir.join("temp");
    std::fs::create_dir_all(&temp).expect("a scratch directory");
    // A dry run plans a hosted rung's spend: another regimen is refused.
    let refused = Command::new(DRIVE)
        .current_dir(root())
        .arg("scenario")
        .arg(root().join("diet/scenarios/hosted.jsonl"))
        .arg("--dry-run")
        .arg("--regimen")
        .arg(root().join("diet/scenarios/canned.toml"))
        .arg("--out")
        .arg(dir.join("refused"))
        .arg("--no-assemble")
        .env("TMPDIR", &temp)
        .output()
        .expect("diet-drive runs");
    let said: serde_json::Value = serde_json::from_slice(&refused.stdout).expect("the report");
    assert_eq!(refused.status.code(), Some(1), "{said}");
    assert!(
        said["failure"]
            .as_str()
            .is_some_and(|why| why.contains("server_kind")),
        "{said}"
    );
    // And no rehearsal's regimen copy is left, however the run ended.
    assert_eq!(
        rehearsals_in(&temp),
        0,
        "a rehearsal's regimen was left behind"
    );
    let _ = std::fs::remove_dir_all(&dir);
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

/// `diet-drive script` (#908) over `log`: its exit code, stdout and stderr.
fn script(log: &Path, out: &Path, canned: Option<&Path>) -> (Option<i32>, String, String) {
    let mut command = Command::new(DRIVE);
    command
        .current_dir(root())
        .arg("script")
        .arg("--log")
        .arg(log)
        .arg("--regimen")
        .arg(root().join("diet/scenarios/kitchen-sink.toml"))
        .arg("--out")
        .arg(out);
    if let Some(canned) = canned {
        command.arg("--canned").arg(canned);
    }
    let ran = command.output().expect("diet-drive runs");
    (
        ran.status.code(),
        String::from_utf8_lossy(&ran.stdout).into_owned(),
        String::from_utf8_lossy(&ran.stderr).into_owned(),
    )
}

/// #908: the round trip. The kitchen sink's log -- every step a scenario
/// holds -- is derived into an operator script, which replays on the canned
/// substrate with the kitchen sink's replies and seed; the replay's operator
/// lines are the original's, line for line, apart from `seq` and `t`, and
/// its trunk starts from the same head.
#[test]
fn an_operator_script_derived_from_a_log_replays_its_operator_lines() {
    let dir = scratch("operator-script");
    let original = dir.join("original");
    let kitchen = root().join("diet/scenarios/kitchen-sink.jsonl");
    let (code, said) = run(&kitchen, &original);
    assert_eq!(code, Some(0), "{said}");
    let (code, stdout, stderr) = script(
        &original.join("session.log"),
        &dir.join("script"),
        Some(&kitchen),
    );
    assert_eq!(code, Some(0), "{stdout}{stderr}");
    let report: serde_json::Value = serde_json::from_str(stdout.trim()).expect("the report");
    assert_eq!(report["fixed_asks"], serde_json::json!([0]), "{report}");
    let replayed = dir.join("replayed");
    let (code, said) = run(&dir.join("script/script.jsonl"), &replayed);
    assert_eq!(code, Some(0), "{said}");
    let read = |path: &Path| std::fs::read_to_string(path).expect("a log");
    let (before, after) = (
        read(&original.join("session.log")),
        read(&replayed.join("session.log")),
    );
    let lines = |log: &str| diet::drive::operator_script::operator_lines(log).expect("a log");
    let (before_lines, after_lines) = (lines(&before), lines(&after));
    assert!(
        before_lines.len() > 30,
        "the kitchen sink's moves: {}",
        before_lines.len()
    );
    for (a, b) in before_lines.iter().zip(&after_lines) {
        assert_eq!(a, b, "an operator line differs");
    }
    assert_eq!(before_lines.len(), after_lines.len());
    let head = |log: &str| {
        let start: serde_json::Value =
            serde_json::from_str(log.lines().next().expect("a start")).expect("an object");
        start["head"].clone()
    };
    assert_eq!(
        head(&before),
        head(&after),
        "the trunk starts from the same head"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// #908: a log holding a move the format cannot express -- here a stop --
/// is refused, exit 1, the refusal naming its line; no script is written.
#[test]
fn an_operator_script_refuses_a_move_it_cannot_express_naming_its_line() {
    let dir = scratch("operator-script-refused");
    let (code, stdout, stderr) = script(
        &root().join("diet/formats/log/fixtures/valid/a-cancelled-turn.jsonl"),
        &dir.join("script"),
        None,
    );
    assert_eq!(code, Some(1), "{stdout}{stderr}");
    let refusal: serde_json::Value = serde_json::from_str(stdout.trim()).expect("the refusal");
    assert!(
        refusal["error"]
            .as_str()
            .is_some_and(|why| why.contains("line 5 (`stop.asked`)")),
        "{refusal}{stderr}"
    );
    assert!(!dir.join("script/script.jsonl").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// One asked-and-answered turn, met by its one expectation, or by
/// `expect` in its place.
fn one_turn(dir: &Path, name: &str, expect: &str) -> PathBuf {
    scenario(
        dir,
        name,
        &format!(
            "{}\n{}\n{}\n{expect}\n",
            r#"{"reply":{"when":"hello there","text":"Hello."}}"#,
            r#"{"step":"ask","text":"hello there"}"#,
            r#"{"step":"end"}"#,
        ),
    )
}

/// `diet-drive scenario FILE --out OUT --repeat N` and `extra`: its exit
/// code and every stdout line, as JSON.
fn repeated(file: &Path, out: &Path, extra: &[&str]) -> (Option<i32>, Vec<serde_json::Value>) {
    let ran = Command::new(DRIVE)
        .current_dir(root())
        .arg("scenario")
        .arg(file)
        .arg("--out")
        .arg(out)
        .args(extra)
        .output()
        .expect("diet-drive runs");
    let stdout = String::from_utf8_lossy(&ran.stdout);
    let lines = stdout
        .lines()
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|_| {
                panic!(
                    "not JSON: {line}\nstderr: {}",
                    String::from_utf8_lossy(&ran.stderr)
                )
            })
        })
        .collect();
    (ran.status.code(), lines)
}

/// A file's first line, as JSON.
fn first_line(path: &Path) -> serde_json::Value {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|why| panic!("{}: {why}", path.display()));
    serde_json::from_str(text.lines().next().unwrap_or("")).expect("a JSON line")
}

/// #910: `--repeat 3` runs three sessions one after another, each in its
/// own `repeat-NN/`, each log's `session.start` (v17) and record's start
/// (v3) naming its script's digest, its repeat and the count; one line per
/// repeat, then the batch's.
#[test]
fn repeats_run_one_after_another_each_naming_its_repeat() {
    let dir = scratch("repeats");
    let file = one_turn(
        &dir,
        "repeats",
        r#"{"expect":"turn.settled","where":{"reason":"final"}}"#,
    );
    let digest = diet::digest::sha256_hex(&std::fs::read(&file).expect("the scenario"));
    let out = dir.join("out");
    let (code, lines) = repeated(&file, &out, &["--no-assemble", "--repeat", "3"]);
    assert_eq!(code, Some(0), "{lines:?}");
    assert_eq!(lines.len(), 4, "{lines:?}");
    let mut ended_before = 0;
    for (k, line) in (1..=3).zip(&lines) {
        assert_eq!(line["repeat"], k, "{line}");
        assert_eq!(line["of"], 3, "{line}");
        assert_eq!(line["attempt"], 1, "{line}");
        assert_eq!(line["void"], serde_json::Value::Null, "{line}");
        assert_eq!(line["ok"], true, "{line}");
        assert_eq!(line["turns"], 1, "{line}");
        assert_eq!(line["settles"], serde_json::json!({"final": 1}), "{line}");
        assert_eq!(line["unmet"], serde_json::json!([]), "{line}");
        assert!(line["tokens"].as_u64().is_some_and(|n| n > 0), "{line}");
        let repeat = out.join(format!("repeat-{k:02}"));
        assert_eq!(
            line["record"],
            repeat.join("session.record.jsonl").display().to_string(),
            "{line}"
        );
        let run = serde_json::json!({"script_sha256": digest, "repeat": k, "of": 3});
        let start = first_line(&repeat.join("session.log"));
        assert_eq!(start["version"], diet::formats::log::VERSION, "{start}");
        assert_eq!(start["run"], run, "{start}");
        let record = first_line(&repeat.join("session.record.jsonl"));
        assert_eq!(
            record["version"],
            diet::formats::record::VERSION,
            "{record}"
        );
        assert_eq!(record["run"], run, "{record}");
        // Repeat k ended before repeat k+1 opened.
        let log = std::fs::read_to_string(repeat.join("session.log")).expect("the log");
        let last: serde_json::Value =
            serde_json::from_str(log.lines().last().unwrap_or("")).expect("a JSON line");
        let opened = start["opened"].as_u64().expect("opened");
        assert!(
            opened >= ended_before,
            "repeat {k} opened before the last ended"
        );
        ended_before = opened + last["t"].as_u64().expect("t");
    }
    assert_eq!(lines[3]["ok"], true, "{}", lines[3]);
    assert_eq!(lines[3]["ran"], 3, "{}", lines[3]);
    // #914: the batch keeps its per-repeat lines beside its runs.
    let kept = std::fs::read_to_string(out.join("runs.jsonl")).expect("runs.jsonl");
    let kept: Vec<serde_json::Value> = kept
        .lines()
        .map(|line| serde_json::from_str(line).expect("a JSON line"))
        .collect();
    assert_eq!(kept, lines[..3], "runs.jsonl holds the repeat lines");
    assert_eq!(lines[3]["script_sha256"], digest, "{}", lines[3]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// #910: a repeat whose expectation is unmet is data: its line names it,
/// it is assembled all the same, and the batch passes.
#[test]
fn an_unmet_expectation_in_a_repeat_is_data_and_the_repeat_is_assembled() {
    let dir = scratch("repeat-unmet");
    let file = one_turn(&dir, "unmet", r#"{"expect":"meditation.held"}"#);
    let out = dir.join("out");
    let (code, lines) = repeated(&file, &out, &["--repeat", "2"]);
    assert_eq!(code, Some(0), "{lines:?}");
    assert_eq!(lines.len(), 3, "{lines:?}");
    for (k, line) in (1..=2).zip(&lines) {
        assert_eq!(line["ok"], false, "{line}");
        assert_eq!(line["void"], serde_json::Value::Null, "{line}");
        assert!(
            line["unmet"].to_string().contains("meditation.held"),
            "{line}"
        );
        let repeat = out.join(format!("repeat-{k:02}"));
        assert_eq!(
            line["record"],
            repeat.join("record.jsonl").display().to_string(),
            "{line}"
        );
        assert_eq!(
            first_line(&repeat.join("record.jsonl"))["run"]["repeat"],
            k,
            "{line}"
        );
    }
    assert_eq!(lines[2]["ok"], true, "{}", lines[2]);
    assert_eq!(lines[2]["ran"], 2, "{}", lines[2]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// #910: the stop file, `STOP` in `--out` unless `--stop-file` names
/// another, stops the batch before its next repeat starts, and the batch's
/// line says at which.
#[test]
fn the_stop_file_stops_the_batch_before_its_next_repeat() {
    let dir = scratch("repeat-stop");
    let file = one_turn(
        &dir,
        "stops",
        r#"{"expect":"turn.settled","where":{"reason":"final"}}"#,
    );
    let out = dir.join("out");
    std::fs::create_dir_all(&out).expect("the out directory");
    std::fs::write(out.join("STOP"), "").expect("the stop file");
    let (code, lines) = repeated(&file, &out, &["--no-assemble", "--repeat", "2"]);
    assert_eq!(code, Some(0), "{lines:?}");
    assert_eq!(lines.len(), 1, "no repeat ran: {lines:?}");
    assert_eq!(lines[0]["stopped_at"], 1, "{}", lines[0]);
    assert_eq!(lines[0]["ran"], 0, "{}", lines[0]);
    assert!(!out.join("repeat-01").exists());
    let elsewhere = dir.join("halt");
    std::fs::write(&elsewhere, "").expect("another stop file");
    let other = dir.join("other");
    let (code, lines) = repeated(
        &file,
        &other,
        &[
            "--no-assemble",
            "--repeat",
            "2",
            "--stop-file",
            &elsewhere.display().to_string(),
        ],
    );
    assert_eq!(code, Some(0), "{lines:?}");
    assert_eq!(lines[0]["stopped_at"], 1, "{}", lines[0]);
    assert_eq!(
        lines[0]["stopped_by"],
        elsewhere.display().to_string(),
        "{}",
        lines[0]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// #910: a repeat serve cannot run to its end for a reason outside the
/// model -- here, an endpoint that refuses the connection, so serve never
/// starts -- is void: re-run at its slot, each attempt's outputs kept
/// beside it, until `--void-cap` re-runs have voided too; then the batch
/// stops, naming the slot, and fails. Nothing leaves loopback.
#[test]
fn a_void_repeat_is_rerun_at_its_slot_until_the_cap_then_the_batch_stops_naming_it() {
    let dir = scratch("repeat-void");
    let file = one_turn(
        &dir,
        "voids",
        r#"{"expect":"turn.settled","where":{"reason":"final"}}"#,
    );
    let regimen = root().join("diet/scenarios/canned.toml");
    let (code, lines) = repeated(
        &file,
        &dir.join("out"),
        &[
            "--no-assemble",
            "--repeat",
            "2",
            "--void-cap",
            "1",
            "--endpoint",
            "http://127.0.0.1:1",
            "--model",
            "unreachable",
            "--regimen",
            &regimen.display().to_string(),
        ],
    );
    assert_eq!(code, Some(1), "{lines:?}");
    assert_eq!(lines.len(), 3, "two attempts, then the batch: {lines:?}");
    for (attempt, line) in (1..=2).zip(&lines) {
        assert_eq!(line["repeat"], 1, "{line}");
        assert_eq!(line["attempt"], attempt, "{line}");
        assert_eq!(line["void"], "serve", "{line}");
        assert!(
            line["failure"].to_string().contains("serve did not start"),
            "{line}"
        );
    }
    assert_eq!(lines[2]["ok"], false, "{}", lines[2]);
    assert_eq!(lines[2]["ran"], 0, "{}", lines[2]);
    assert_eq!(
        lines[2]["void_cap_reached"],
        serde_json::json!({"repeat": 1, "voids": 2}),
        "{}",
        lines[2]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// #910: `--repeat` gives each repeat a fresh worktree, so it takes no
/// `--worktree`; `--stop-file` and `--void-cap` are its alone.
#[test]
fn repeat_refuses_a_worktree_and_its_flags_refuse_to_go_without_it() {
    let dir = scratch("repeat-usage");
    let file = one_turn(&dir, "usage", r#"{"expect":"turn.settled"}"#);
    for extra in [
        vec!["--repeat", "2", "--worktree", "/tmp"],
        vec!["--void-cap", "1"],
        vec!["--stop-file", "/tmp/stop"],
        vec!["--repeat", "0"],
    ] {
        let ran = Command::new(DRIVE)
            .current_dir(root())
            .arg("scenario")
            .arg(&file)
            .arg("--out")
            .arg(dir.join("out"))
            .args(&extra)
            .output()
            .expect("diet-drive runs");
        assert_eq!(ran.status.code(), Some(2), "{extra:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The canned regimen with `extra` bound at its top level, written as
/// `name` in `dir`.
fn canned_with(dir: &Path, name: &str, extra: &str) -> PathBuf {
    let canned = std::fs::read_to_string(root().join("diet/scenarios/canned.toml"))
        .expect("the canned regimen");
    let (top, sampler) = canned.split_once("[sampler]").expect("its sampler table");
    let path = dir.join(name);
    std::fs::write(&path, format!("{top}{extra}\n[sampler]{sampler}")).expect("the regimen");
    path
}

/// #912: `pair` runs the scenario under each regimen twice, interleaved
/// ABBA, each arm's repeats in `out/a/` and `out/b/`; every log and record
/// start names its arm, the pair (the digest of the two regimens' digests),
/// the script's digest and its repeat of its arm; the batch names the one
/// lever.
#[test]
fn a_pair_runs_abba_and_every_record_names_its_arm_and_pair() {
    let dir = scratch("pair");
    let file = one_turn(
        &dir,
        "pair",
        r#"{"expect":"turn.settled","where":{"reason":"final"}}"#,
    );
    let a = canned_with(&dir, "a.toml", "");
    let b = canned_with(&dir, "b.toml", "max_output_tokens = 512");
    let digest = |path: &Path| diet::digest::sha256_hex(&std::fs::read(path).expect("bytes"));
    let pair = diet::digest::sha256_hex(format!("{}{}", digest(&a), digest(&b)).as_bytes());
    let out = dir.join("out");
    let ran = Command::new(DRIVE)
        .current_dir(root())
        .arg("pair")
        .arg(&file)
        .arg("--a")
        .arg(&a)
        .arg("--b")
        .arg(&b)
        .args(["--repeat", "2", "--no-assemble", "--out"])
        .arg(&out)
        .output()
        .expect("diet-drive runs");
    let stdout = String::from_utf8_lossy(&ran.stdout);
    let lines: Vec<serde_json::Value> = stdout
        .lines()
        .map(|line| serde_json::from_str(line).expect("a JSON line"))
        .collect();
    assert_eq!(ran.status.code(), Some(0), "{stdout}");
    assert_eq!(lines.len(), 5, "{stdout}");
    let order: Vec<(&str, u64)> = lines[..4]
        .iter()
        .map(|line| {
            (
                line["arm"].as_str().unwrap_or(""),
                line["repeat"].as_u64().unwrap_or(0),
            )
        })
        .collect();
    assert_eq!(order, [("a", 1), ("b", 1), ("b", 2), ("a", 2)], "{stdout}");
    let script = digest(&file);
    let mut opened = Vec::new();
    for (arm, k) in order {
        let repeat = out.join(arm).join(format!("repeat-{k:02}"));
        let run = serde_json::json!({
            "script_sha256": script, "repeat": k, "of": 2, "arm": arm, "arms": pair,
        });
        let start = first_line(&repeat.join("session.log"));
        assert_eq!(start["run"], run, "{start}");
        assert_eq!(first_line(&repeat.join("session.record.jsonl"))["run"], run);
        opened.push(start["opened"].as_u64().expect("opened"));
    }
    assert!(opened.is_sorted(), "the runs ran in ABBA order: {opened:?}");
    let batch = &lines[4];
    assert_eq!(batch["ok"], true, "{batch}");
    assert_eq!(batch["pair"], pair, "{batch}");
    assert_eq!(batch["lever"]["name"], "step_and_output_limits", "{batch}");
    assert_eq!(batch["ran"], serde_json::json!({"a": 2, "b": 2}), "{batch}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// #932: `arms` runs the scenario under each of a manifest's regimens, in
/// a Williams order: three arms in name order (limits, native, window),
/// two blocks, rows [0, 1, 2] and [1, 2, 0]. Every log and record start
/// names its arm by name and the batch's digest (of the regimens' digests,
/// in name order); the batch names what each arm differs from the baseline
/// in, says two blocks of a six-row cycle do not balance, and keeps its line
/// as `batch.json`, from which `table` reads the baseline: one delta per
/// other arm.
#[test]
#[allow(clippy::too_many_lines)]
fn arms_run_in_a_williams_order_and_table_against_the_baseline() {
    let dir = scratch("arms");
    let file = one_turn(
        &dir,
        "arms",
        r#"{"expect":"turn.settled","where":{"reason":"final"}}"#,
    );
    let native = canned_with(&dir, "native.toml", "");
    let limits = canned_with(&dir, "limits.toml", "max_output_tokens = 512");
    let window = canned_with(&dir, "window.toml", "meditation_window = \"off\"");
    let manifest = dir.join("ARMS.toml");
    std::fs::write(
        &manifest,
        "baseline = \"native\"\n[arm.native]\nregimen = \"native.toml\"\n\
         [arm.limits]\nregimen = \"limits.toml\"\n[arm.window]\nregimen = \"window.toml\"\n",
    )
    .expect("the manifest");
    let digest = |path: &Path| diet::digest::sha256_hex(&std::fs::read(path).expect("bytes"));
    let batch_digest = diet::digest::sha256_hex(
        format!("{}{}{}", digest(&limits), digest(&native), digest(&window)).as_bytes(),
    );
    let out = dir.join("out");
    let ran = Command::new(DRIVE)
        .current_dir(root())
        .arg("arms")
        .arg(&file)
        .arg("--arms")
        .arg(&manifest)
        .args(["--repeat", "2", "--no-assemble", "--out"])
        .arg(&out)
        .output()
        .expect("diet-drive runs");
    let stdout = String::from_utf8_lossy(&ran.stdout);
    assert_eq!(
        ran.status.code(),
        Some(0),
        "{stdout}{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let lines: Vec<serde_json::Value> = stdout
        .lines()
        .map(|line| serde_json::from_str(line).expect("a JSON line"))
        .collect();
    assert_eq!(lines.len(), 7, "{stdout}");
    let order: Vec<(&str, u64)> = lines[..6]
        .iter()
        .map(|line| {
            (
                line["arm"].as_str().unwrap_or(""),
                line["repeat"].as_u64().unwrap_or(0),
            )
        })
        .collect();
    assert_eq!(
        order,
        [
            ("limits", 1),
            ("native", 1),
            ("window", 1),
            ("native", 2),
            ("window", 2),
            ("limits", 2)
        ],
        "{stdout}"
    );
    let script = digest(&file);
    let mut opened = Vec::new();
    for (arm, k) in order {
        let repeat = out.join(arm).join(format!("repeat-{k:02}"));
        let run = serde_json::json!({
            "script_sha256": script, "repeat": k, "of": 2, "arm": arm, "arms": batch_digest,
        });
        let start = first_line(&repeat.join("session.log"));
        assert_eq!(start["run"], run, "{start}");
        assert_eq!(first_line(&repeat.join("session.record.jsonl"))["run"], run);
        opened.push(start["opened"].as_u64().expect("opened"));
    }
    assert!(
        opened.is_sorted(),
        "the runs ran in the Williams order: {opened:?}"
    );
    let batch = &lines[6];
    assert_eq!(batch["ok"], true, "{batch}");
    assert_eq!(batch["arms"], batch_digest, "{batch}");
    assert_eq!(batch["baseline"], "native", "{batch}");
    assert_eq!(
        batch["against"]["limits"]["lever"]["name"], "step_and_output_limits",
        "{batch}"
    );
    assert!(
        batch["against"]["window"]["lever"]["name"].is_string(),
        "{batch}"
    );
    assert_eq!(
        batch["ran"],
        serde_json::json!({"limits": 2, "native": 2, "window": 2}),
        "{batch}"
    );
    assert_eq!(batch["order"]["design"], "williams", "{batch}");
    assert_eq!(batch["order"]["rows"], 6, "{batch}");
    assert_eq!(batch["order"]["balanced"], false, "{batch}");
    let kept: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("batch.json")).expect("batch.json"))
            .expect("JSON");
    assert_eq!(&kept, batch, "batch.json is the batch's line");
    let tabled = Command::new(DRIVE)
        .arg("table")
        .arg(&out)
        .output()
        .expect("diet-drive runs");
    assert_eq!(
        tabled.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&tabled.stderr)
    );
    let table: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("table.json")).expect("table.json"))
            .expect("JSON");
    assert_eq!(table["baseline"], "native", "{table}");
    assert_eq!(table["arms"], batch_digest, "{table}");
    for row in table["rows"].as_array().expect("rows") {
        let deltas: Vec<&String> = row["deltas"].as_object().expect("deltas").keys().collect();
        assert_eq!(deltas, ["limits", "window"], "{row}");
        let by_arm: Vec<&String> = row["by_arm"].as_object().expect("by_arm").keys().collect();
        assert_eq!(by_arm, ["limits", "native", "window"], "{row}");
    }
    let markdown = std::fs::read_to_string(out.join("table.md")).expect("table.md");
    assert!(
        markdown.contains(
            "| row | limits median [min, max] | native median [min, max] | window median [min, max] \
             | limits − native median [95%] | limits − native mean [95%] \
             | window − native median [95%] | window − native mean [95%] | source |"
        ),
        "{markdown}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// #932: an arm more than one lever from the baseline is refused before
/// any run, the refusal naming the arm, its levers and its keys.
#[test]
fn an_arm_more_than_one_lever_from_the_baseline_is_refused_naming_it() {
    let dir = scratch("arms-refused");
    let file = one_turn(&dir, "arms", r#"{"expect":"turn.settled"}"#);
    canned_with(&dir, "native.toml", "");
    canned_with(&dir, "limits.toml", "max_output_tokens = 512");
    canned_with(
        &dir,
        "both.toml",
        "max_output_tokens = 512\nmeditation_window = \"off\"",
    );
    let manifest = dir.join("ARMS.toml");
    std::fs::write(
        &manifest,
        "baseline = \"native\"\n[arm.native]\nregimen = \"native.toml\"\n\
         [arm.limits]\nregimen = \"limits.toml\"\n[arm.both]\nregimen = \"both.toml\"\n",
    )
    .expect("the manifest");
    let out = dir.join("out");
    let ran = Command::new(DRIVE)
        .current_dir(root())
        .arg("arms")
        .arg(&file)
        .arg("--arms")
        .arg(&manifest)
        .args(["--repeat", "2", "--no-assemble", "--out"])
        .arg(&out)
        .output()
        .expect("diet-drive runs");
    let stdout = String::from_utf8_lossy(&ran.stdout);
    assert_eq!(ran.status.code(), Some(1), "{stdout}");
    let said: serde_json::Value = serde_json::from_str(stdout.trim()).expect("one JSON line");
    assert_eq!(said["arm"], "both", "{said}");
    assert_eq!(
        said["levers"].as_object().map(serde_json::Map::len),
        Some(2),
        "{said}"
    );
    assert!(!out.exists(), "nothing ran: {}", out.display());
    let _ = std::fs::remove_dir_all(&dir);
}

/// #912: two regimens more than one lever apart are refused before any
/// run, the refusal naming the levers and the keys.
#[test]
fn a_pair_more_than_one_lever_apart_is_refused_naming_the_levers_and_keys() {
    let dir = scratch("pair-refused");
    let file = one_turn(&dir, "pair", r#"{"expect":"turn.settled"}"#);
    let a = canned_with(&dir, "a.toml", "");
    let b = canned_with(
        &dir,
        "b.toml",
        "max_output_tokens = 512\nmeditation_window = \"off\"",
    );
    let out = dir.join("out");
    let ran = Command::new(DRIVE)
        .current_dir(root())
        .arg("pair")
        .arg(&file)
        .arg("--a")
        .arg(&a)
        .arg("--b")
        .arg(&b)
        .args(["--repeat", "2", "--no-assemble", "--out"])
        .arg(&out)
        .output()
        .expect("diet-drive runs");
    let stdout = String::from_utf8_lossy(&ran.stdout);
    assert_eq!(ran.status.code(), Some(1), "{stdout}");
    let said: serde_json::Value =
        serde_json::from_str(stdout.lines().last().unwrap_or("")).expect("a JSON line");
    assert_eq!(
        said["keys"],
        serde_json::json!(["max_output_tokens", "meditation_window"]),
        "{said}"
    );
    assert_eq!(
        said["levers"].as_object().map(serde_json::Map::len),
        Some(2),
        "{said}"
    );
    let why = said["refused"].as_str().unwrap_or("");
    assert!(
        why.contains("max_output_tokens") && why.contains("meditation_window"),
        "{said}"
    );
    assert!(!out.join("a").exists(), "nothing ran");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `diet-drive pair` on `file` under `a` and `b`, once each, with `extra`:
/// its exit code and every stdout line.
fn paired(
    file: &Path,
    (a, b): (&Path, &Path),
    out: &Path,
    extra: &[&str],
) -> (Option<i32>, Vec<serde_json::Value>) {
    let ran = Command::new(DRIVE)
        .current_dir(root())
        .arg("pair")
        .arg(file)
        .arg("--a")
        .arg(a)
        .arg("--b")
        .arg(b)
        .args(["--repeat", "1", "--no-assemble", "--out"])
        .arg(out)
        .args(extra)
        .output()
        .expect("diet-drive runs");
    let stdout = String::from_utf8_lossy(&ran.stdout);
    let lines = stdout
        .lines()
        .map(|line| serde_json::from_str(line).expect("a JSON line"))
        .collect();
    (ran.status.code(), lines)
}

/// #912: `--contrast` declares what a pair differs in; it runs when the
/// declared set is exactly what differs, and the batch names it; a member
/// missing or extra is refused before any run, naming it.
#[test]
fn a_declared_contrast_runs_when_exact_and_is_refused_naming_a_missing_or_extra_member() {
    let dir = scratch("contrast");
    let file = one_turn(
        &dir,
        "contrast",
        r#"{"expect":"turn.settled","where":{"reason":"final"}}"#,
    );
    let a = canned_with(&dir, "a.toml", "");
    let b = canned_with(
        &dir,
        "b.toml",
        "max_output_tokens = 512\nmeditation_window = \"off\"",
    );
    let (code, lines) = paired(
        &file,
        (&a, &b),
        &dir.join("ran"),
        &["--contrast", "max_output_tokens,meditation_window"],
    );
    assert_eq!(code, Some(0), "{lines:?}");
    assert_eq!(lines.len(), 3, "{lines:?}");
    let batch = &lines[2];
    assert_eq!(
        batch["contrast"]["declared"],
        serde_json::json!(["max_output_tokens", "meditation_window"]),
        "{batch}"
    );
    assert_eq!(
        batch["contrast"]["levers"]
            .as_object()
            .map(serde_json::Map::len),
        Some(2),
        "{batch}"
    );
    for (contrast, missing, extra) in [
        (
            "max_output_tokens",
            serde_json::json!(["meditation_trigger"]),
            serde_json::json!([]),
        ),
        (
            "max_output_tokens,meditation_window,note_dedupe",
            serde_json::json!([]),
            serde_json::json!(["note_dedupe"]),
        ),
    ] {
        let out = dir.join(format!("refused-{}", contrast.len()));
        let (code, lines) = paired(&file, (&a, &b), &out, &["--contrast", contrast]);
        assert_eq!(code, Some(1), "{contrast}: {lines:?}");
        assert_eq!(lines.len(), 1, "{contrast}: nothing ran: {lines:?}");
        assert_eq!(lines[0]["missing"], missing, "{contrast}: {}", lines[0]);
        assert_eq!(lines[0]["extra"], extra, "{contrast}: {}", lines[0]);
        assert!(!out.join("a").exists(), "{contrast}: nothing ran");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// #914: `table` over a canned `pair` batch whose numbers the scenario
/// sets. Arm A (`max_steps = 4`) asks, runs `exit 3` and is answered: two
/// responses reporting 1000 and 1500 prompt tokens, 7 output tokens each
/// (the canned server's count), one failed command, settled `final`. Arm B
/// (`max_steps = 1`) stops before the call runs: one response, no failed
/// command, settled `max_steps`, its `final` expectation unmet. Runs alike
/// within an arm leave nothing to resample, so every interval is its delta.
#[test]
#[allow(clippy::too_many_lines)]
fn a_pair_batchs_table_holds_the_numbers_its_scenario_set() {
    let dir = scratch("table");
    let regimen = |name: &str, max_steps: u32| {
        let path = dir.join(name);
        std::fs::write(
            &path,
            format!(
                "arm = \"table\"\ndogma_version = 0\nsubstrate = \"canned-scenario\"\n\
                 substrate_reasoning = \"off\"\n\
                 substrate_hardware = \"b6bc80e9e8e4bc2342219f064dab89a8d17c02535600b32b84895fc0b9ba6513\"\n\
                 isolation = \"none\"\napproval = \"none\"\n\
                 approval_policy = \"approvals are off: every command runs without a gate decision or a prompt\"\n\
                 tool_surface = \"standard\"\n[limits]\nmax_steps = {max_steps}\n\
                 [sampler]\ntemperature = 0.6\nseed = 7\n"
            ),
        )
        .expect("the regimen");
        path
    };
    let (a, b) = (regimen("a.toml", 4), regimen("b.toml", 1));
    let header = serde_json::json!({
        "scenario": "table",
        "regimen": "a.toml",
        "head": root().join("diet/drive/heads/floor.md"),
    });
    let file = dir.join("table.jsonl");
    std::fs::write(
        &file,
        format!(
            "{header}\n{}",
            concat!(
                r#"{"reply":{"when":"Check the build","text":"Running it.","prompt_tokens":1000,"calls":[{"id":"call-x","name":"bash","arguments":"{\"command\": \"exit 3\"}"}]}}"#,
                "\n",
                r#"{"reply":{"text":"It failed with 3.","prompt_tokens":1500}}"#,
                "\n",
                r#"{"step":"ask","text":"Check the build."}"#,
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
    let (code, lines) = paired(&file, (&a, &b), &out, &["--repeat", "2"]);
    assert_eq!(code, Some(0), "{lines:?}");
    let kept = std::fs::read_to_string(out.join("runs.jsonl")).expect("runs.jsonl");
    assert_eq!(kept.lines().count(), 4, "one line per run: {kept}");
    let ran = Command::new(DRIVE)
        .arg("table")
        .arg(&out)
        .output()
        .expect("diet-drive runs");
    let said = String::from_utf8_lossy(&ran.stdout);
    assert_eq!(
        ran.status.code(),
        Some(0),
        "{said}{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert!(said.contains(r#""seed":0"#), "{said}");
    let table: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("table.json")).expect("table.json"))
            .expect("JSON");
    assert_eq!(
        table["runs"],
        serde_json::json!({"a": 2, "b": 2}),
        "{table}"
    );
    assert_eq!(
        table["primary_measure"],
        "primary measure (#932): not declared"
    );
    let row = |name: &str| {
        table["rows"]
            .as_array()
            .and_then(|rows| rows.iter().find(|row| row["row"] == name))
            .cloned()
            .unwrap_or_else(|| panic!("no row {name}: {table}"))
    };
    for (name, a, b) in [
        ("failed commands", 1.0, 0.0),
        ("prompt tokens per run", 2500.0, 1000.0),
        ("prompt tokens per turn", 2500.0, 1000.0),
        ("output tokens per run", 14.0, 7.0),
        ("turns", 1.0, 1.0),
        ("forks", 0.0, 0.0),
        ("captures", 0.0, 0.0),
        ("refusals", 0.0, 0.0),
        ("settles: final", 1.0, 0.0),
        ("settles: max_steps", 0.0, 1.0),
        ("unmet expectations", 0.0, 1.0),
    ] {
        let row = row(name);
        assert_eq!(
            row["by_arm"]["a"]["runs"],
            serde_json::json!([a, a]),
            "{row}"
        );
        assert_eq!(
            row["by_arm"]["b"]["runs"],
            serde_json::json!([b, b]),
            "{row}"
        );
        let delta = serde_json::json!({"value": b - a, "low": b - a, "high": b - a});
        assert_eq!(row["deltas"]["b"]["delta_median"], delta, "{row}");
        assert_eq!(row["deltas"]["b"]["delta_mean"], delta, "{row}");
    }
    let markdown = std::fs::read_to_string(out.join("table.md")).expect("table.md");
    assert!(
        markdown.contains("| primary measure (#932): not declared |"),
        "{markdown}"
    );
    assert!(markdown.contains("seed 0"), "{markdown}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A canned regimen that runs commands, with at most `max_steps` steps a
/// turn, written as `name` in `dir`.
fn commanding(dir: &Path, name: &str, max_steps: u32) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(
        &path,
        format!(
            "arm = \"grade\"\ndogma_version = 0\nsubstrate = \"canned-scenario\"\n\
             substrate_reasoning = \"off\"\n\
             substrate_hardware = \"b6bc80e9e8e4bc2342219f064dab89a8d17c02535600b32b84895fc0b9ba6513\"\n\
             isolation = \"none\"\napproval = \"none\"\n\
             approval_policy = \"approvals are off: every command runs without a gate decision or a prompt\"\n\
             tool_surface = \"standard\"\n[limits]\nmax_steps = {max_steps}\n\
             [sampler]\ntemperature = 0.6\nseed = 7\n"
        ),
    )
    .expect("the regimen");
    path
}

/// The reference stdout check, expecting `main`.
fn stdout_check() -> String {
    format!(
        "python3 -I \"{}\" main",
        root()
            .join("diet/scenarios/checks/stdout-equals.py")
            .display()
    )
}

/// A graded task (#942): its ask prints `main` and exits 1, which the
/// reference check grades wrong, as a stdout task is right only from a call
/// that exited 0; the first `Graded: wrong` feedback's turn prints `main`
/// and exits 0, so a check expecting `main` is wrong once and then right.
/// `grade` is the step's extra keys.
fn graded(dir: &Path, name: &str, grade: &serde_json::Value) -> PathBuf {
    let call = |id: &str, command: &str| serde_json::json!([{"id": id, "name": "bash", "arguments": serde_json::json!({"command": command}).to_string()}]);
    let mut step = serde_json::json!({
        "step": "grade",
        "task": "T1",
        "attempts": 3,
        "wrong": "Graded: wrong. Expected: {expected}. Got: {got}. Try again in ./{task}.",
        "last": "Graded: wrong. Expected: {expected}. Got: {got}. No attempts left; moving on.",
    });
    for (key, value) in grade.as_object().into_iter().flatten() {
        step[key] = value.clone();
    }
    let lines = [
        serde_json::json!({"scenario": name, "regimen": "a.toml", "head": root().join("diet/drive/heads/floor.md")}),
        serde_json::json!({"reply": {"when": "Print the branch name", "text": "On it.", "calls": call("c1", "echo main; exit 1")}}),
        serde_json::json!({"reply": {"text": "Printed it."}}),
        serde_json::json!({"reply": {"when": "Graded: wrong", "text": "Again.", "calls": call("c2", "echo main")}}),
        serde_json::json!({"reply": {"text": "Printed it again."}}),
        serde_json::json!({"reply": {"when": "Graded: wrong", "text": "Again.", "calls": call("c3", "echo main")}}),
        serde_json::json!({"reply": {"when": "Graded: wrong", "text": "Moving on."}}),
        serde_json::json!({"reply": {"when": "Graded: right", "text": "Thanks."}}),
        serde_json::json!({"step": "ask", "text": "Print the branch name."}),
        step,
        serde_json::json!({"step": "end"}),
    ];
    let path = dir.join(format!("{name}.jsonl"));
    let text: String = lines.iter().map(|line| line.to_string() + "\n").collect();
    std::fs::write(&path, text).expect("the scenario");
    path
}

/// The log's lines of `kind`, as JSON.
fn logged(log: &Path, kind: &str) -> Vec<serde_json::Value> {
    std::fs::read_to_string(log)
        .expect("the log")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("a JSON line"))
        .filter(|line| line["kind"] == kind)
        .collect()
}

/// #942: a scenario's services run beside serve, outside the sandbox. A
/// foreground server waits on its reserved port; a daemon that detaches
/// writes its PIDs and a value, and its `stop` leaves its PID alive, which
/// the runner then ends. The ask is filled from both, the check is told
/// both and keeps a file in its own directory, and serve is given the
/// daemon's socket, which its receipt names. After the run nothing it
/// started is alive.
#[test]
#[allow(clippy::too_many_lines)]
fn services_start_before_serve_fill_the_run_and_stop_after_it() {
    let dir = scratch("services");
    commanding(&dir, "a.toml", 4);
    // The model's commands are passed the web port: serve was given it.
    let regimen = std::fs::read_to_string(dir.join("a.toml")).expect("the regimen");
    std::fs::write(
        dir.join("a.toml"),
        regimen.replace(
            "[limits]",
            "env_passthrough = [\"DIET_PORT_WEB\"]\n[limits]",
        ),
    )
    .expect("the regimen");
    // A socket's address holds at most 103 bytes: the scratch dir's is too long.
    let socket = PathBuf::from(format!("/private/tmp/diet-svc-{}.sock", std::process::id()));
    let daemon_start = format!(
        "bash -c 'trap \"echo term > $DIET_SERVICE_DIR/termed; exit 0\" TERM; while true; do sleep 0.1; done' \
         > /dev/null 2>&1 & echo $! > \"$DIET_SERVICE_DIR/pids\"; \
         printf '{{\"token\": \"t-42\", \"socket\": \"{}\"}}' > \"$DIET_SERVICE_DIR/provides.json\"",
        socket.display()
    );
    let check = "python3 -c 'import json, os, urllib.request\n\
        port = os.environ[\"DIET_PORT_WEB\"]\n\
        body = urllib.request.urlopen(\"http://127.0.0.1:%s/\" % port).status\n\
        echoed = json.load(open(os.environ[\"DIET_GRADE_TURN\"]))[\"calls\"][0][\"stdout\"][\"text\"].strip()\n\
        open(os.path.join(os.environ[\"DIET_GRADE_OUT\"], \"seen.txt\"), \"w\").write(os.environ[\"DIET_SERVICE_DAEMON_TOKEN\"])\n\
        print(json.dumps({\"right\": body == 200 and echoed == port, \"expected\": port, \"got\": echoed}))'";
    let call = serde_json::json!([{"id": "c1", "name": "bash", "arguments": serde_json::json!({"command": "echo \"$DIET_PORT_WEB\""}).to_string()}]);
    let lines = [
        serde_json::json!({
            "scenario": "services",
            "regimen": "a.toml",
            "head": root().join("diet/drive/heads/floor.md"),
            "ports": ["web"],
            "services": [
                {"name": "web", "run": "exec python3 -m http.server --bind 127.0.0.1 \"$DIET_PORT_WEB\"", "ready": {"tcp": "web"}},
                {"name": "daemon", "start": daemon_start, "stop": "true", "ready": {"command": "test -s \"$DIET_SERVICE_DIR/provides.json\""}},
            ],
            "sandbox_unix_sockets": ["service.daemon.socket"],
        }),
        serde_json::json!({"reply": {"when": "Open port", "text": "On it.", "calls": call}}),
        serde_json::json!({"reply": {"text": "Done."}}),
        serde_json::json!({"reply": {"when": "Graded: right", "text": "Thanks."}}),
        serde_json::json!({"step": "ask", "text": "Open port {port.web} with token {service.daemon.token}."}),
        serde_json::json!({"step": "grade", "task": "T1", "check": check, "wrong": "Graded: wrong.", "right": "Graded: right on {port.web}."}),
        serde_json::json!({"step": "end"}),
    ];
    let file = dir.join("services.jsonl");
    let text: String = lines.iter().map(|line| line.to_string() + "\n").collect();
    std::fs::write(&file, text).expect("the scenario");
    let out = dir.join("out");
    let (code, said) = run(&file, &out);
    assert_eq!(code, Some(0), "{said}");
    assert!(said["services"]["web"]["ready_ms"].is_u64(), "{said}");
    assert_eq!(
        said["services"]["web"]["stopped"]["killed"], false,
        "{said}"
    );
    assert_eq!(
        said["services"]["daemon"]["stopped"]["stop_exit"], 0,
        "{said}"
    );
    let left = said["services"]["daemon"]["stopped"]["pids_left_after_stop"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(left.len(), 1, "the daemon's stop left its sleep: {said}");
    let pid = left[0].as_u64().expect("a pid");
    let alive = Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .expect("kill runs")
        .success();
    assert!(!alive, "the runner ended pid {pid}");
    assert!(
        out.join("services/daemon/termed").exists(),
        "the runner asked it to end, TERM, before any KILL"
    );
    let asks = logged(&out.join("session.log"), "ask");
    let first = asks[0]["text"].as_str().expect("text");
    assert!(
        first.starts_with("Open port ") && first.ends_with(" with token t-42."),
        "{first}"
    );
    let port: u16 = first["Open port ".len()..]
        .split(' ')
        .next()
        .and_then(|port| port.parse().ok())
        .expect("a port");
    assert!(
        std::net::TcpStream::connect(("127.0.0.1", port)).is_err(),
        "the web service stopped with the run"
    );
    assert_eq!(
        asks[1]["text"].as_str(),
        Some(format!("Graded: right on {port}.").as_str()),
        "{asks:?}"
    );
    let grades = logged(&out.join("session.log"), "grade");
    assert_eq!(grades[0]["right"], true, "{grades:?}");
    assert_eq!(
        std::fs::read_to_string(out.join("grades/T1/attempt-1/seen.txt")).expect("kept"),
        "t-42"
    );
    let receipt: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(out.join("session.record.jsonl.receipt.json")).expect("receipt"),
    )
    .expect("JSON");
    assert_eq!(
        receipt["unix_sockets"],
        serde_json::json!([socket.display().to_string()]),
        "{receipt}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// #942: a service that never becomes ready voids the run as `services`
/// before serve starts, and what it started is stopped.
#[test]
fn a_service_not_ready_in_time_voids_the_run() {
    let dir = scratch("services-late");
    commanding(&dir, "a.toml", 4);
    let lines = [
        serde_json::json!({
            "scenario": "late",
            "regimen": "a.toml",
            "head": root().join("diet/drive/heads/floor.md"),
            "services": [{"name": "never", "run": "exec sleep 300", "ready": {"command": "false"}, "ready_timeout_s": 1}],
        }),
        serde_json::json!({"reply": {"when": "hello", "text": "Hello."}}),
        serde_json::json!({"step": "ask", "text": "hello"}),
        serde_json::json!({"step": "end"}),
    ];
    let file = dir.join("late.jsonl");
    let text: String = lines.iter().map(|line| line.to_string() + "\n").collect();
    std::fs::write(&file, text).expect("the scenario");
    let out = dir.join("out");
    let (code, said) = run(&file, &out);
    assert_eq!(code, Some(1), "{said}");
    assert_eq!(said["void"], "services", "{said}");
    assert!(
        said["failure"]
            .as_str()
            .unwrap_or("")
            .contains("not ready in 1 s"),
        "{said}"
    );
    assert_eq!(
        said["services"]["never"]["stopped"]["killed"], false,
        "{said}"
    );
    assert!(!out.join("session.log").exists(), "serve never started");
    let _ = std::fs::remove_dir_all(&dir);
}

/// #942: a placeholder naming a port or a service the header does not
/// declare is refused when the scenario is read.
#[test]
fn a_placeholder_naming_nothing_declared_is_refused() {
    let dir = scratch("services-undeclared");
    let file = scenario(
        &dir,
        "undeclared",
        concat!(
            r#"{"reply":{"when":"x","text":"y"}}"#,
            "\n",
            r#"{"step":"ask","text":"open {port.web}"}"#,
            "\n",
            r#"{"step":"end"}"#,
            "\n",
        ),
    );
    let (code, said) = run(&file, &dir.join("out"));
    assert_eq!(code, Some(1), "{said}");
    assert!(
        said["failure"]
            .as_str()
            .unwrap_or("")
            .contains("`{port.web}` names a port or a service"),
        "{said}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// #942: a check that fails once and then passes logs two `grade` lines,
/// sends the `wrong` ask and then the `right` one, then moves on; with
/// `right` omitted, only the `wrong` ask. Every ask names its task.
#[test]
fn a_check_wrong_once_then_right_logs_two_grades_and_sends_its_feedback() {
    let dir = scratch("grade");
    commanding(&dir, "a.toml", 4);
    for (name, right, asks) in [
        ("with-right", Some("Graded: right."), 3),
        ("without-right", None, 2),
    ] {
        let mut grade = serde_json::json!({"check": stdout_check()});
        if let Some(right) = right {
            grade["right"] = serde_json::json!(right);
        }
        let file = graded(&dir, name, &grade);
        let out = dir.join(name);
        let (code, said) = run(&file, &out);
        assert_eq!(code, Some(0), "{name}: {said}");
        let grades = logged(&out.join("session.log"), "grade");
        let verdicts: Vec<_> = grades
            .iter()
            .map(|grade| {
                (
                    grade["attempt"].clone(),
                    grade["right"].clone(),
                    grade["attempts"].clone(),
                )
            })
            .collect();
        assert_eq!(
            verdicts,
            [
                (
                    serde_json::json!(1),
                    serde_json::json!(false),
                    serde_json::json!(3)
                ),
                (
                    serde_json::json!(2),
                    serde_json::json!(true),
                    serde_json::json!(3)
                ),
            ],
            "{name}"
        );
        let digest = diet::digest::sha256_hex(stdout_check().as_bytes());
        assert!(
            grades
                .iter()
                .all(|grade| grade["check_sha256"] == digest.as_str() && grade["task"] == "T1")
        );
        let asked = logged(&out.join("session.log"), "ask");
        assert_eq!(asked.len(), asks, "{name}: {asked:?}");
        assert!(
            asked.iter().all(|ask| ask["task"] == "T1"),
            "{name}: {asked:?}"
        );
        assert_eq!(
            asked[1]["text"], "Graded: wrong. Expected: main. Got: main. Try again in ./T1.",
            "{name}"
        );
        if let Some(right) = right {
            assert_eq!(asked[2]["text"], right, "{name}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// #942: a check that prints no verdict, or changes the worktree, fails
/// the run void as `grader`, the second naming the path it changed.
#[test]
fn a_check_that_crashes_or_changes_the_worktree_voids_the_run() {
    let dir = scratch("grade-void");
    commanding(&dir, "a.toml", 4);
    for (name, check, names) in [
        ("crashes", "echo not-a-verdict", "no JSON object"),
        (
            "writes",
            r#"touch marker; echo '{"right": true, "expected": "x", "got": "x"}'"#,
            "marker",
        ),
    ] {
        let file = graded(&dir, name, &serde_json::json!({"check": check}));
        let (code, said) = run(&file, &dir.join(name));
        assert_eq!(code, Some(1), "{name}: {said}");
        assert_eq!(said["void"], "grader", "{name}: {said}");
        assert!(
            said["failure"].to_string().contains(names),
            "{name}: {said}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// #942: a graded `pair`'s table gains each task's rows. Arm A (four steps)
/// runs its commands and is right at attempt 2; arm B (one step) never runs
/// one, so it is never right in its 3 attempts.
#[test]
fn a_graded_pairs_table_shows_attempts_to_right_and_never_right() {
    let dir = scratch("grade-table");
    let a = commanding(&dir, "a.toml", 4);
    let b = commanding(&dir, "b.toml", 1);
    let file = graded(
        &dir,
        "graded",
        &serde_json::json!({"check": stdout_check(), "right": "Graded: right."}),
    );
    let out = dir.join("out");
    let (code, lines) = paired(&file, (&a, &b), &out, &[]);
    assert_eq!(code, Some(0), "{lines:?}");
    let ran = Command::new(DRIVE)
        .arg("table")
        .arg(&out)
        .output()
        .expect("diet-drive runs");
    assert_eq!(
        ran.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let table: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("table.json")).expect("table.json"))
            .expect("JSON");
    let row = |name: &str| {
        table["rows"]
            .as_array()
            .and_then(|rows| rows.iter().find(|row| row["row"] == name))
            .cloned()
            .unwrap_or_else(|| panic!("no row {name}: {table}"))
    };
    let to_right = row("attempts to right: T1");
    assert_eq!(
        to_right["by_arm"]["a"]["runs"],
        serde_json::json!([2.0]),
        "{to_right}"
    );
    assert_eq!(
        to_right["by_arm"]["b"]["runs"],
        serde_json::json!([null]),
        "{to_right}"
    );
    let never = row("never right: T1");
    assert_eq!(
        never["by_arm"]["a"]["runs"],
        serde_json::json!([0.0]),
        "{never}"
    );
    assert_eq!(
        never["by_arm"]["b"]["runs"],
        serde_json::json!([1.0]),
        "{never}"
    );
    assert_eq!(
        never["deltas"]["b"]["delta_median"]["value"],
        serde_json::json!(1.0),
        "{never}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// #942: `script` derives a graded run's grade step back -- its task, its
/// attempts and the feedback texts the run sent -- and no ask step of the
/// feedback asks, which the step sends; the check, which the log keeps
/// only the digest of, is written to fail until it is supplied.
#[test]
fn a_graded_runs_script_derives_its_grade_step() {
    let dir = scratch("grade-script");
    commanding(&dir, "a.toml", 4);
    let file = graded(
        &dir,
        "graded",
        &serde_json::json!({"check": stdout_check(), "right": "Graded: right."}),
    );
    let out = dir.join("run");
    let (code, said) = run(&file, &out);
    assert_eq!(code, Some(0), "{said}");
    let derived = dir.join("derived");
    let (code, stdout, stderr) = script(&out.join("session.log"), &derived, None);
    assert_eq!(code, Some(0), "{stdout}{stderr}");
    let steps: Vec<serde_json::Value> = std::fs::read_to_string(derived.join("script.jsonl"))
        .expect("the script")
        .lines()
        .skip(1)
        .map(|line| serde_json::from_str(line).expect("a JSON line"))
        .filter(|line: &serde_json::Value| line.get("step").is_some())
        .collect();
    let kinds: Vec<_> = steps
        .iter()
        .map(|step| step["step"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(kinds, ["ask", "grade", "end"], "{steps:?}");
    assert_eq!(steps[0]["task"], "T1");
    let grade = &steps[1];
    assert_eq!(grade["task"], "T1");
    assert_eq!(grade["attempts"], 3);
    assert_eq!(
        grade["wrong"],
        "Graded: wrong. Expected: main. Got: main. Try again in ./T1."
    );
    assert_eq!(grade["right"], "Graded: right.");
    let digest = diet::digest::sha256_hex(stdout_check().as_bytes());
    assert!(
        grade["check"]
            .as_str()
            .is_some_and(|check| check.contains(&digest) && check.ends_with("exit 1")),
        "{grade}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The tree's Sonnet 5.5 regimen, its commands run as hosted.jsonl's turn
/// needs, its rung swapped to `rung` (on `hardware`) when given, and
/// `extra` bound at its top level: what a live run would take.
fn hosted_regimen(dir: &Path, name: &str, rung: Option<(&str, &str)>, extra: &str) -> PathBuf {
    let sonnet = std::fs::read_to_string(root().join("diet/drive/sonnet-5-5.toml"))
        .expect("the Sonnet regimen");
    let sonnet = match rung {
        Some((rung, hardware)) => sonnet
            .replace("substrate = \"anthropic-claude-sonnet-5-5\"", &format!("substrate = \"{rung}\""))
            .replace(
                "substrate_hardware = \"24a1117b0a8112dbcd56ec38eb21cb44bf8124400d6233af927ecadc1db1b13e\"",
                &format!("substrate_hardware = \"{hardware}\""),
            ),
        None => sonnet,
    };
    let commands = format!(
        "isolation = \"none\"\napproval = \"none\"\napproval_policy = \"approvals are \
         off: every command runs without a gate decision or a prompt\"\n{extra}\n\n\
         [limits]\nmax_steps = 4\n\n"
    );
    let at = sonnet.find("[reasoning]").expect("a reasoning table");
    let path = dir.join(name);
    std::fs::write(
        &path,
        format!("{}{commands}{}", &sonnet[..at], &sonnet[at..]),
    )
    .expect("the live regimen");
    path
}

/// A dry run of `file` under `regimen`: its exit code and its report.
fn dry_run(file: &Path, regimen: &Path, out: &Path) -> (Option<i32>, serde_json::Value) {
    let ran = Command::new(DRIVE)
        .current_dir(root())
        .arg("scenario")
        .arg(file)
        .arg("--dry-run")
        .arg("--regimen")
        .arg(regimen)
        .arg("--out")
        .arg(out)
        .arg("--no-assemble")
        .env_remove("ANTHROPIC_API_KEY")
        .output()
        .expect("diet-drive runs");
    let stdout = String::from_utf8_lossy(&ran.stdout);
    let said = serde_json::from_str(stdout.lines().last().unwrap_or(""))
        .unwrap_or_else(|_| panic!("not JSON: {stdout}{}", String::from_utf8_lossy(&ran.stderr)));
    (ran.status.code(), said)
}

/// #926: a rehearsal takes its rung's window. A window meditation's share
/// is admitted rehearsing for a rung that declares a window (Sonnet 5.5's
/// 1,000,000), and refused at start rehearsing for one that declares none
/// (`stub-hosted-api`), as a live run on each would be.
#[test]
fn a_rehearsal_is_refused_at_start_where_its_rung_would_be() {
    let dir = scratch("rehearse-window");
    // A fraction trigger needs something to fill working memory: a scoping
    // warrant, which hosted.jsonl's asks never fire.
    let fraction = "interview_warrant = [\"scoping\"]\nmeditation_at_context_fraction = 0.5";
    let declares = hosted_regimen(&dir, "declares.toml", None, fraction);
    let file = root().join("diet/scenarios/hosted.jsonl");
    let (code, said) = dry_run(&file, &declares, &dir.join("declares"));
    assert_eq!(code, Some(0), "{said}");
    assert_eq!(said["plan"]["complete"], true, "{said}");
    let none = hosted_regimen(
        &dir,
        "none.toml",
        Some((
            "stub-hosted-api",
            "e2f4ca82432821d2f721a5dc9ece9a6bebeb16292c0f45da9ced80613626fbf0",
        )),
        fraction,
    );
    let (code, said) = dry_run(&file, &none, &dir.join("none"));
    assert_eq!(code, Some(1), "{said}");
    assert!(
        said["failure"].to_string().contains("serve did not start")
            && said["failure"].to_string().contains("serving_context"),
        "{said}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// #926: a drive that fails partway still plans from what its log holds,
/// marked incomplete, with the failure beside it.
#[test]
fn a_dry_run_whose_drive_fails_plans_what_its_log_holds() {
    let dir = scratch("rehearse-failed");
    let regimen = hosted_regimen(&dir, "live.toml", None, "");
    let head = root()
        .join("diet/drive/heads/floor.md")
        .display()
        .to_string();
    let text = std::fs::read_to_string(root().join("diet/scenarios/hosted.jsonl"))
        .expect("hosted.jsonl")
        .replace("\"../drive/heads/floor.md\"", &format!("\"{head}\""));
    assert!(text.contains(&head), "the head named by its whole path");
    // A last step that cannot be met: no `grade` line is ever logged.
    let file = dir.join("fails.jsonl");
    std::fs::write(
        &file,
        format!(
            "{text}{}\n",
            r#"{"step":"wait","until":"grade","timeout":1}"#
        ),
    )
    .expect("the scenario");
    let (code, said) = dry_run(&file, &regimen, &dir.join("out"));
    assert_eq!(code, Some(1), "{said}");
    let plan = &said["plan"];
    assert_eq!(plan["complete"], false, "{said}");
    assert!(
        plan["stopped_by"]
            .as_str()
            .is_some_and(|why| why.contains("grade")),
        "{said}"
    );
    assert_eq!(
        plan["rehearsal"]["requests"], 4,
        "every request before it: {said}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// #926: a rehearsal takes its rung's vision. An ask attaching a picture is
/// admitted rehearsing for a rung that takes images (Sonnet 5.5, declared
/// from the provider's docs) and refused `no-vision` rehearsing for one
/// that declares none (`stub-hosted-api`).
#[test]
fn a_rehearsal_takes_an_image_where_its_rung_would() {
    let dir = scratch("rehearse-vision");
    std::fs::write(dir.join("shot.png"), b"\x89PNG\r\n\x1a\n0000").expect("the picture");
    let header = serde_json::json!({
        "scenario": "vision",
        "regimen": "unused.toml",
        "head": root().join("diet/drive/heads/floor.md"),
        "seed": {"shot.png": "shot.png"},
    });
    let file = dir.join("vision.jsonl");
    std::fs::write(
        &file,
        format!(
            "{header}\n{}",
            concat!(
                r#"{"reply":{"text":"A PNG signature."}}"#,
                "\n",
                r#"{"step":"ask","text":"What is in the picture?","attach":["shot.png"]}"#,
                "\n",
                r#"{"step":"end"}"#,
                "\n",
            )
        ),
    )
    .expect("the scenario");
    let takes = hosted_regimen(&dir, "takes.toml", None, "");
    let (code, said) = dry_run(&file, &takes, &dir.join("takes"));
    assert_eq!(code, Some(0), "{said}");
    let blind = hosted_regimen(
        &dir,
        "blind.toml",
        Some((
            "stub-hosted-api",
            "e2f4ca82432821d2f721a5dc9ece9a6bebeb16292c0f45da9ced80613626fbf0",
        )),
        "",
    );
    let (code, said) = dry_run(&file, &blind, &dir.join("blind"));
    assert_eq!(code, Some(1), "{said}");
    assert!(said["failure"].to_string().contains("no-vision"), "{said}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// #932: under `head_nonce = "per_run"` each run's head opens with a nonce
/// of its own, drawn from its run, which `session.start` names; the
/// table's cached prefill is then labelled carryover-free; `script` takes
/// the nonce off the head it derives; and a value that is not a state is
/// refused at start.
#[test]
fn a_nonced_pairs_heads_are_each_their_own_and_its_table_says_so() {
    let dir = scratch("nonce");
    let file = one_turn(
        &dir,
        "nonce",
        r#"{"expect":"turn.settled","where":{"reason":"final"}}"#,
    );
    let a = canned_with(&dir, "a.toml", "head_nonce = \"per_run\"");
    let b = canned_with(
        &dir,
        "b.toml",
        "head_nonce = \"per_run\"\nmax_output_tokens = 512",
    );
    let out = dir.join("out");
    let (code, lines) = paired(&file, (&a, &b), &out, &["--repeat", "2"]);
    assert_eq!(code, Some(0), "{lines:?}");
    let floor =
        std::fs::read_to_string(root().join("diet/drive/heads/floor.md")).expect("the head");
    let mut nonces = std::collections::BTreeSet::new();
    for (arm, k) in [("a", 1), ("a", 2), ("b", 1), ("b", 2)] {
        let start = first_line(
            &out.join(arm)
                .join(format!("repeat-{k:02}"))
                .join("session.log"),
        );
        let nonce = start["head_nonce"].as_str().expect("a nonce").to_owned();
        assert_eq!(nonce.len(), 16, "{start}");
        let head = start["head"][0]["content"].as_str().expect("the head");
        assert!(
            head.starts_with(&format!("Session nonce: {nonce}\n")),
            "{head}"
        );
        assert!(head.contains(floor.trim()), "the head after it");
        assert_eq!(start["levers"]["head_nonce"], "per_run", "{start}");
        nonces.insert(nonce);
    }
    assert_eq!(nonces.len(), 4, "every run's nonce its own: {nonces:?}");
    let ran = Command::new(DRIVE)
        .arg("table")
        .arg(&out)
        .output()
        .expect("diet-drive runs");
    assert_eq!(
        ran.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let table: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("table.json")).expect("table.json"))
            .expect("JSON");
    let cached = table["rows"]
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row["row"] == "cached prefill: trunk")
        })
        .cloned()
        .unwrap_or_else(|| panic!("no trunk prefill row: {table}"));
    assert!(
        cached["source"]
            .as_str()
            .is_some_and(|source| source.contains("carryover-free")),
        "{cached}"
    );
    let derived = dir.join("derived");
    let (code, stdout, stderr) = script(&out.join("a/repeat-01/session.log"), &derived, None);
    assert_eq!(code, Some(0), "{stdout}{stderr}");
    let head = std::fs::read_to_string(derived.join("head.md")).expect("the derived head");
    assert!(!head.contains("Session nonce"), "{head}");
    let bad = canned_with(&dir, "bad.toml", "head_nonce = \"sometimes\"");
    let ran = Command::new(DRIVE)
        .current_dir(root())
        .arg("scenario")
        .arg(&file)
        .args(["--no-assemble", "--regimen"])
        .arg(&bad)
        .arg("--out")
        .arg(dir.join("refused"))
        .output()
        .expect("diet-drive runs");
    let said = String::from_utf8_lossy(&ran.stdout);
    assert_eq!(ran.status.code(), Some(1), "{said}");
    assert!(said.contains("head_nonce"), "{said}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// #932: compaction depth `none`, the golden arm's state, never meditates.
/// A declared meditation is admitted and logged `meditation.skipped`, not
/// refused, though working memory is empty; no meditation line or record
/// row is written, so nothing was refilled; the next ask is sent on the
/// trunk as it stood; and the start names the lever `none`. Without it, the same declaration over an
/// empty working memory is refused `nothing-to-meditate`.
#[test]
fn a_session_that_never_meditates_skips_a_declared_meditation_and_never_refills() {
    let dir = scratch("never");
    let body = format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n",
        r#"{"reply":{"when":"what are we building","text":"A tracker for the team's bugs."}}"#,
        r#"{"reply":{"when":"who files them","text":"Anyone on the team."}}"#,
        r#"{"step":"ask","text":"what are we building"}"#,
        r#"{"step":"declare-meditation"}"#,
        r#"{"step":"ask","text":"who files them"}"#,
        r#"{"step":"end"}"#,
    );
    let never = canned_with(&dir, "never.toml", "meditation_tail_tokens = \"none\"");
    let file = scenario(&dir, "never", &body);
    let out = dir.join("never");
    let ran = Command::new(DRIVE)
        .current_dir(root())
        .arg("scenario")
        .arg(&file)
        .args(["--no-assemble", "--regimen"])
        .arg(&never)
        .arg("--out")
        .arg(&out)
        .output()
        .expect("diet-drive runs");
    let said = String::from_utf8_lossy(&ran.stdout);
    assert_eq!(
        ran.status.code(),
        Some(0),
        "{said}{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let log = out.join("session.log");
    let skipped = logged(&log, "meditation.skipped");
    assert_eq!(skipped.len(), 1, "{skipped:?}");
    assert_eq!(
        (&skipped[0]["at_turn"], &skipped[0]["reason"]),
        (&serde_json::json!(1), &serde_json::json!("operator"))
    );
    assert!(logged(&log, "meditation").is_empty(), "no meditation line");
    assert_eq!(first_line(&log)["levers"]["compaction_depth"], "none");
    let record = std::fs::read_to_string(out.join("session.record.jsonl")).expect("the record");
    assert!(
        !record.contains(r#""record":"meditation""#),
        "no meditation row"
    );
    // The second turn is asked and sent, on no refill.
    let requests = logged(&log, "request");
    assert_eq!(requests.len(), 2, "{requests:?}");
    // Under total compaction the same declaration over an empty working
    // memory is refused.
    let total = canned_with(&dir, "total.toml", "");
    let ran = Command::new(DRIVE)
        .current_dir(root())
        .arg("scenario")
        .arg(&file)
        .args(["--no-assemble", "--regimen"])
        .arg(&total)
        .arg("--out")
        .arg(dir.join("total"))
        .output()
        .expect("diet-drive runs");
    let said = String::from_utf8_lossy(&ran.stdout);
    assert_eq!(ran.status.code(), Some(1), "{said}");
    assert!(said.contains("nothing-to-meditate"), "{said}");
    let _ = std::fs::remove_dir_all(&dir);
}
