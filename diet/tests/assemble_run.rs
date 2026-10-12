//! `scripts/assemble-run.py` (#757): one command assembles a run directory
//! from serve's outputs -- scrubbed with the hygiene check's own patterns,
//! checked, counted through `diet`, digested -- and reproduces a run
//! assembled by hand.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const RUN_C: &str = "runs/2026-10-11-levers-run-c-agent-driven";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the repository")
        .to_path_buf()
}

/// A scratch directory of this test's own, emptied first.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("diet-run-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// Levers run C's committed files, staged under the names serve writes, the
/// log edited by `edit`.
fn staged(name: &str, edit: impl Fn(String) -> String) -> PathBuf {
    let run = root().join(RUN_C);
    let stage = scratch(name);
    for (from, to) in [
        ("record.jsonl", "record.jsonl"),
        ("product.txt", "record.jsonl.product.txt"),
        ("unspellable.json", "record.jsonl.unspellable.json"),
        ("receipt.json", "record.jsonl.receipt.json"),
        ("regimen.toml", "regimen.toml"),
    ] {
        std::fs::copy(run.join(from), stage.join(to)).expect("staged");
    }
    let log = std::fs::read_to_string(run.join("session.log")).expect("the log");
    std::fs::write(stage.join("session.log"), edit(log)).expect("the log staged");
    stage
}

fn assemble(stage: &Path, out: &Path, home: &str) -> Output {
    assemble_with(stage, out, home, &[])
}

fn assemble_with(stage: &Path, out: &Path, home: &str, more: &[&str]) -> Output {
    Command::new("python3")
        .current_dir(root())
        .arg("scripts/assemble-run.py")
        .args(["--log", &stage.join("session.log").to_string_lossy()])
        .args(["--record", &stage.join("record.jsonl").to_string_lossy()])
        .args(["--regimen", &stage.join("regimen.toml").to_string_lossy()])
        .args(["--out", &out.to_string_lossy()])
        .args(["--home", home])
        .args(more)
        .output()
        .expect("python3 runs the assembler")
}

/// A home directory under the account `name`, built here rather than
/// written out: the tree's own hygiene check refuses the literal shape.
fn home_of(name: &str) -> String {
    format!("/{}/{name}", ["Us", "ers"].concat())
}

/// A LAN address, built from its octets for the same reason.
fn lan_address() -> String {
    [192, 168, 1, 20]
        .map(|octet: u8| octet.to_string())
        .join(".")
}

fn said(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "one JSON line: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// The README's file table rows: what a hand-assembled run pinned.
fn file_rows(readme: &str) -> Vec<String> {
    readme
        .lines()
        .filter(|line| {
            [
                "session.log",
                "record.jsonl",
                "product.txt",
                "receipt.json",
                "unspellable.json",
                "regimen.toml",
            ]
            .iter()
            .any(|name| line.starts_with(&format!("| `{name}` |")))
        })
        .map(str::to_owned)
        .collect()
}

#[test]
fn assembling_levers_run_c_reproduces_its_counts_and_digests() {
    let stage = staged("c", |log| log);
    let runs = scratch("c-runs");
    let out = runs.join("2026-10-11-levers-run-c");
    let driven = "scripted: scenario levers-run-c";
    let assembled = assemble_with(
        &stage,
        &out,
        &home_of("nobody-in-particular"),
        &["--driven-by", driven],
    );
    assert!(assembled.status.success(), "{}", said(&assembled));
    let counts = &said(&assembled)["counts"];
    // As its README counted them: 2 asks, 24 requests, 13 tool calls, 11
    // forks (9 interview, 2 audit), 35 patches, 1 flag, 1 meditation held
    // then refilled, 1 refusal.
    for (kind, n) in [
        ("ask", 2),
        ("request", 24),
        ("tool_call", 13),
        ("fork", 11),
        ("patch", 35),
        ("flag", 1),
        ("meditation.held", 1),
        ("meditation", 1),
        ("refused", 1),
    ] {
        assert_eq!(counts["kinds"][kind], n, "{kind}: {counts}");
    }
    assert_eq!(counts["lanes"]["fork"]["interview"], 9, "{counts}");
    assert_eq!(counts["lanes"]["fork"]["audit"], 2, "{counts}");
    let committed =
        std::fs::read_to_string(root().join(RUN_C).join("README.md")).expect("its README");
    let skeleton = std::fs::read_to_string(out.join("README.md")).expect("a skeleton");
    assert_eq!(
        file_rows(&skeleton),
        file_rows(&committed),
        "the digests table"
    );
    assert!(
        skeleton.contains(&format!("**Who drove it:** {driven}\n")),
        "--driven-by fills the first line: {skeleton}"
    );
    let _ = std::fs::remove_dir_all(&stage);
    let _ = std::fs::remove_dir_all(&runs);
}

#[test]
fn a_planted_home_path_is_scrubbed_and_a_planted_lan_address_refused() {
    let home = home_of("someone-planted");
    let plant = |what: String| {
        // Inside the first ask's text, where an operator might paste it.
        move |log: String| log.replacen(r#""text":""#, &format!(r#""text":"see {what} -- "#), 1)
    };
    let stage = staged("home", plant(format!("{home}/work/notes.md")));
    let runs = scratch("home-runs");
    let out = runs.join("2026-10-11-planted");
    let assembled = assemble(&stage, &out, &home);
    assert!(assembled.status.success(), "{}", said(&assembled));
    assert_eq!(
        said(&assembled)["scrubbed"]["session.log"]["personal-home-path"],
        1,
        "{}",
        said(&assembled)
    );
    let skeleton = std::fs::read_to_string(out.join("README.md")).expect("a skeleton");
    assert!(
        skeleton.contains("**Who drove it:** [unwritten: who drove it"),
        "without --driven-by the line is left for whoever drove it: {skeleton}"
    );
    let log = std::fs::read_to_string(out.join("session.log")).expect("the log");
    assert!(log.contains("~/work/notes.md") && !log.contains("someone-planted"));
    let stage_ip = staged("ip", plant(lan_address()));
    let out_ip = runs.join("2026-10-11-planted-ip");
    let refused = assemble(&stage_ip, &out_ip, &home);
    assert_eq!(refused.status.code(), Some(1));
    assert!(
        said(&refused)["refused"]
            .as_str()
            .is_some_and(|why| why.contains("private-ipv4")),
        "{}",
        said(&refused)
    );
    assert!(!out_ip.exists(), "a refusal leaves nothing");
    let _ = std::fs::remove_dir_all(&stage);
    let _ = std::fs::remove_dir_all(&stage_ip);
    let _ = std::fs::remove_dir_all(&runs);
}

/// A host-shaped name under `.local`, built here: the tree's own hygiene
/// check refuses the literal shape.
fn local_host(name: &str) -> String {
    format!("{name}.{}", ["lo", "cal"].concat())
}

/// T1 on the floor (2026-10-12): the dotenv file name `.env.local`, in a
/// source file the model read, is no host, and its run assembles with the
/// text as the model read it. A host-shaped `.local` name is still
/// refused, and so is one dressed as the dotenv name's suffix.
#[test]
fn the_dotenv_name_assembles_and_a_local_host_is_still_refused() {
    let home = home_of("someone-planted");
    let plant = |what: String| {
        move |log: String| log.replacen(r#""text":""#, &format!(r#""text":"{what} -- "#), 1)
    };
    let dotenv = format!("// Load .{} (not checked in)", local_host("env"));
    let runs = scratch("dotenv-runs");
    let stage = staged("dotenv", plant(dotenv));
    let out = runs.join("2026-10-12-dotenv");
    let assembled = assemble(&stage, &out, &home);
    assert!(assembled.status.success(), "{}", said(&assembled));
    let log = std::fs::read_to_string(out.join("session.log")).expect("the log");
    assert!(
        log.contains(&format!(".{} (not checked in)", local_host("env"))),
        "kept as read"
    );
    for (name, host) in [
        ("host", local_host("build")),
        ("dressed", local_host("staging.env")),
    ] {
        let stage = staged(name, plant(format!("ssh {host} -- ")));
        let out = runs.join(format!("2026-10-12-{name}"));
        let refused = assemble(&stage, &out, &home);
        assert_eq!(refused.status.code(), Some(1), "{host}: {}", said(&refused));
        assert!(
            said(&refused)["refused"]
                .as_str()
                .is_some_and(|why| why.contains("internal-hostname")),
            "{host}: {}",
            said(&refused)
        );
        let _ = std::fs::remove_dir_all(&stage);
    }
    let _ = std::fs::remove_dir_all(&stage);
    let _ = std::fs::remove_dir_all(&runs);
}

/// The same carve-out in the tree's own gate: `hygiene.sh --tree` passes
/// the dotenv name and still fails a host-shaped `.local` name.
#[test]
fn the_hygiene_gate_passes_the_dotenv_name_and_fails_a_local_host() {
    let gate = |name: &str, line: &str| {
        let tree = scratch(name);
        std::fs::write(tree.join("note.txt"), format!("{line}\n")).expect("a file");
        let ran = Command::new("bash")
            .current_dir(root())
            .args(["scripts/hygiene.sh", "--tree", &tree.to_string_lossy()])
            .output()
            .expect("bash runs the gate");
        let _ = std::fs::remove_dir_all(&tree);
        ran.status.code()
    };
    assert_eq!(
        gate(
            "gate-dotenv",
            &format!("// Load .{} (not checked in)", local_host("env"))
        ),
        Some(0)
    );
    assert_ne!(
        gate("gate-host", &format!("ssh {}", local_host("build"))),
        Some(0)
    );
    assert_ne!(
        gate(
            "gate-dressed",
            &format!("ssh {}", local_host("staging.env"))
        ),
        Some(0)
    );
}
