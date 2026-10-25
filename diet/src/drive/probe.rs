//! A run's deny list, and the probe that proves it (#942).
//!
//! Under `sandbox_reads = "all"` a model's commands read everything but the
//! secrets, and a machine that runs experiments holds what encodes them: the
//! private runs, the checkout and its graders, the agents' transcripts and
//! memory, the operator's keys. The operator names those paths in
//! [`SECRETS`], colon-separated and absolute; the runner passes each to
//! serve as `--sandbox-secret`. A live run, or a dry run, of a regimen that
//! runs commands refuses to start with it unset; `none` waives it, for CI
//! and machines that hold nothing.
//!
//! Before such a run, the probe proves the denial from inside serve's own
//! sandbox: a short session on the canned substrate, under the run's regimen
//! with its substrate alone swapped and the same secrets, whose canned
//! replies run the probe's commands. `ls` on each denied path must fail,
//! `cat` of [`PROBE_FILE`] (a file the model must not read) must fail, and a
//! `grep -r` from `$HOME` for [`PROBE_PHRASE`] (text only those files hold)
//! must finish and find nothing. Its verdict names counts, never a path.

use std::fmt::Write as _;
use std::path::Path;

/// The operator's deny list: colon-separated absolute paths, or `none`.
pub const SECRETS: &str = "DIET_SANDBOX_SECRETS";

/// A file the probe must fail to read.
pub const PROBE_FILE: &str = "DIET_SANDBOX_PROBE_FILE";

/// Text the probe's `grep -r` from `$HOME` must not find.
pub const PROBE_PHRASE: &str = "DIET_SANDBOX_PROBE_PHRASE";

/// The largest file the probe's search reads (ruled 2026-10-21): a leaked
/// solution, task note or brief is small text, and the transcripts that grow
/// large sit in a denied `~/.claude`; a home's media would take hours.
pub const MAX_FILESIZE: &str = "64M";

/// How long the probe's `grep -r` from `$HOME` may run, in seconds: a home
/// directory of checkouts takes minutes.
pub const GREP_TIMEOUT_S: u64 = 3_600;

/// How long the probe's turn may take to settle: the grep's time and some.
const GREP_PATIENCE_S: u64 = GREP_TIMEOUT_S + 300;

/// How many forks the probe's session may cut, each answered: a regimen
/// that interviews forks on the ask, on each failed command and on reads.
const PROBE_FORKS: usize = 64;

/// What [`SECRETS`] says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Deny {
    /// Unset.
    Unset,
    /// `none`: waived.
    Waived,
    /// The paths to deny.
    Paths(Vec<String>),
}

/// [`SECRETS`] as `value` gives it.
///
/// # Errors
///
/// When a path is not absolute, or holds a `'`.
pub fn deny_of(value: Option<&str>) -> Result<Deny, String> {
    let Some(value) = value else {
        return Ok(Deny::Unset);
    };
    if value == "none" {
        return Ok(Deny::Waived);
    }
    let paths: Vec<String> = value
        .split(':')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect();
    if paths.is_empty() {
        return Err(format!("{SECRETS} names no path: give paths, or `none`"));
    }
    if let Some(path) = paths
        .iter()
        .find(|path| !Path::new(path).is_absolute() || path.contains('\''))
    {
        return Err(format!(
            "{SECRETS}: `{path}` is not an absolute path without a `'`"
        ));
    }
    Ok(Deny::Paths(paths))
}

/// A path as a single-quoted shell word.
fn quoted(path: &str) -> String {
    format!("'{path}'")
}

/// Where the probe's `grep -r` looks: each top-level entry of `home` but
/// the denied paths and `node_modules`, `Library` and `.Trash` (the app
/// data under `Library` is denied, not searched), then `/private/tmp` and
/// `tmpdir`, which hold other sessions' scratch.
#[must_use]
pub fn grep_roots(home: &Path, denied: &[String], tmpdir: Option<&Path>) -> Vec<String> {
    let skipped = |name: &str| matches!(name, "node_modules" | "Library" | ".Trash");
    let mut roots: Vec<String> = std::fs::read_dir(home)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| !skipped(&entry.file_name().to_string_lossy()))
                .map(|entry| entry.path().display().to_string())
                .filter(|path| !denied.iter().any(|root| root == path))
                .collect()
        })
        .unwrap_or_default();
    roots.sort();
    for extra in [Some(Path::new("/private/tmp")), tmpdir]
        .into_iter()
        .flatten()
    {
        let extra = extra.display().to_string();
        let extra = extra.trim_end_matches('/').to_owned();
        if !roots.contains(&extra) && !denied.contains(&extra) {
            roots.push(extra);
        }
    }
    roots
}

/// `text` as a shell word that prints it: hex escapes through `printf`, so
/// the probe's own files (its scenario, its log) never hold the phrase it
/// greps for.
fn printed(text: &str) -> String {
    let escaped = text.bytes().fold(String::new(), |mut escaped, byte| {
        let _ = write!(escaped, "\\x{byte:02x}");
        escaped
    });
    format!("\"$(printf '{escaped}')\"")
}

/// The probe's scenario: one ask, answered by a reply whose calls run the
/// probe's commands, then a reply that ends the turn.
#[must_use]
pub fn scenario(
    regimen: &Path,
    head: &Path,
    roots: &[String],
    (files, phrase, grep_in): (&[String], &str, &[String]),
    (canary, sealed): (&str, Option<&crate::drive::scenario::Sealed>),
    tools: &[String],
) -> String {
    let bash = |id: String, command: String, timeout: u64| {
        serde_json::json!({
            "id": id,
            "name": "bash",
            "arguments": serde_json::json!({"command": command, "timeout": timeout}).to_string(),
        })
    };
    let mut calls: Vec<serde_json::Value> = roots
        .iter()
        .enumerate()
        .map(|(at, root)| {
            bash(
                format!("probe-ls-{at}"),
                format!("ls -- {}", quoted(root)),
                30_000,
            )
        })
        .collect();
    calls.extend(files.iter().enumerate().map(|(at, file)| {
        bash(
            format!("probe-cat-{at}"),
            format!("cat -- {}", quoted(file)),
            30_000,
        )
    }));
    // A sealed session's legs (#948): its unsealed parts' files must read,
    // and its sealed files must not come out of the checkout's git objects.
    if let Some(sealed) = sealed {
        calls.extend(sealed.readable.iter().enumerate().map(|(at, file)| {
            bash(
                format!("probe-read-{at}"),
                format!("cat -- {} > /dev/null", quoted(file)),
                30_000,
            )
        }));
        if let Some((root, under)) = &sealed.git {
            calls.extend(under.iter().enumerate().map(|(at, rel)| {
                bash(
                    format!("probe-git-{at}"),
                    format!(
                        "git -C {} show {}",
                        quoted(root),
                        quoted(&format!("HEAD:{rel}"))
                    ),
                    30_000,
                )
            }));
        }
    }
    // The tools the scenario needs its model to run (#976): each found on
    // the sandbox's PATH, its file readable and executable, and answering.
    calls.extend(tools.iter().enumerate().map(|(at, tool)| {
        bash(
            format!("probe-tool-{at}"),
            format!(
                "p=$(command -v -- {tool}) && test -r \"$p\" && test -x \"$p\" && {tool} --version"
            ),
            30_000,
        )
    }));
    // The sweep for a stray copy of the sealed source (#948): one line only
    // that source holds, searched beside the deny list's phrase.
    let seal_phrase = sealed
        .and_then(|sealed| sealed.phrase.as_deref())
        .map(|phrase| format!(" -e {}", printed(phrase)))
        .unwrap_or_default();
    calls.push(bash(
        "probe-rg-version".to_owned(),
        "rg --version".to_owned(),
        30_000,
    ));
    // ripgrep (#942): parallel, ignoring no ignore file and no dotfile, the
    // phrase a fixed string. It exits 2 whenever a file is unreadable, as
    // the denied ones are, so its exit says nothing: what it lists does. A
    // canary only this run's own file holds proves it searched at all.
    calls.push(bash(
        "probe-grep".to_owned(),
        format!(
            "rg --no-ignore --hidden --fixed-strings --files-with-matches --no-messages \
             --max-filesize {MAX_FILESIZE} --glob '!node_modules' --glob '!Library' --glob '!.Trash' \
             -e {}{seal_phrase} -e {} -- {}",
            printed(phrase),
            printed(canary),
            grep_in
                .iter()
                .map(|root| quoted(root))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        GREP_TIMEOUT_S * 1_000,
    ));
    // The run's regimen may interview. A refutation fork is cut as the ask
    // arrives and quotes it, so its answers, routed on the refutation
    // question's own words, come first, and the commands reach the trunk.
    // Any other request, a fork's or the trunk's last, is answered in text.
    let refuted = serde_json::json!({"reply": {
        "when": "show that something you believed was wrong",
        "text": "",
        "calls": [{"id": "probe-refuted", "name": "refute_beliefs", "arguments": "{\"refuted\":[]}"}],
    }});
    let answered = serde_json::json!({"reply": {"text": "Probed."}});
    let mut lines =
        vec![serde_json::json!({"scenario": "sandbox-probe", "regimen": regimen, "head": head})];
    lines.extend(std::iter::repeat_n(refuted, PROBE_FORKS));
    lines.push(serde_json::json!({"reply": {"when": "Probe the sandbox", "text": "Probing.", "calls": calls}}));
    lines.extend(std::iter::repeat_n(answered, PROBE_FORKS));
    // The search may take as long as its own timeout: the step waits for it.
    lines.push(serde_json::json!({"step": "ask", "text": "Probe the sandbox.", "patience": GREP_PATIENCE_S}));
    lines.push(serde_json::json!({"step": "end"}));
    lines.iter().fold(String::new(), |mut text, line| {
        text.push_str(&line.to_string());
        text.push('\n');
        text
    })
}

/// The probe's verdict, read from its session's log: how many denied paths
/// `ls` failed on, whether `cat` failed, whether `grep` finished finding
/// nothing, and under a seal (#948) whether every unsealed file read and
/// every sealed one stayed out of `git show`, and (#976) whether every
/// tool the scenario names ran; `ok` when all hold.
#[must_use]
pub fn verdict(
    log: &str,
    (roots, files): (usize, usize),
    (canary_file, sealed): (&str, Option<&crate::drive::scenario::Sealed>),
    tools: &[String],
) -> serde_json::Value {
    let calls: Vec<serde_json::Value> = log
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|line| line["kind"] == "tool_call")
        .collect();
    let call = |id: &str| calls.iter().find(|call| call["id"] == id);
    let failed = |id: &str| {
        call(id).is_some_and(|call| {
            call["outcome"] == "ran" && call["exit"].as_i64().is_some_and(|exit| exit != 0)
        })
    };
    let denied = (0..roots)
        .filter(|at| failed(&format!("probe-ls-{at}")))
        .count();
    let file_denied = files > 0 && (0..files).all(|at| failed(&format!("probe-cat-{at}")));
    let grep = call("probe-grep");
    let grep_finished = grep.is_some_and(|call| call["outcome"] == "ran" && call["exit"].is_i64());
    let listed: Vec<&str> = grep
        .and_then(|call| call["stdout"].as_str())
        .unwrap_or("")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let canary_seen = listed.contains(&canary_file);
    let found = listed.iter().filter(|line| **line != canary_file).count();
    let succeeded = |id: &str| {
        call(id).is_some_and(|call| call["outcome"] == "ran" && call["exit"].as_i64() == Some(0))
    };
    let readable = sealed.map_or(0, |sealed| sealed.readable.len());
    let read = (0..readable)
        .filter(|at| succeeded(&format!("probe-read-{at}")))
        .count();
    let gits = sealed
        .and_then(|sealed| sealed.git.as_ref())
        .map_or(0, |(_, under)| under.len());
    let git_denied = (0..gits)
        .filter(|at| failed(&format!("probe-git-{at}")))
        .count();
    let version = call("probe-rg-version")
        .and_then(|call| call["stdout"].as_str())
        .and_then(|text| text.lines().next())
        .unwrap_or("")
        .to_owned();
    let ran: serde_json::Map<String, serde_json::Value> = tools
        .iter()
        .enumerate()
        .map(|(at, tool)| {
            let id = format!("probe-tool-{at}");
            let version = call(&id)
                .and_then(|call| call["stdout"].as_str())
                .and_then(|text| text.lines().next())
                .unwrap_or("")
                .to_owned();
            (
                tool.clone(),
                serde_json::json!({"ok": succeeded(&id), "version": version}),
            )
        })
        .collect();
    let tools_ran = ran.values().all(|tool| tool["ok"] == true);
    let mut said = serde_json::json!({
        "ok": roots > 0 && denied == roots && file_denied && grep_finished && canary_seen && found == 0
            && read == readable && git_denied == gits && tools_ran,
        "paths": roots,
        "paths_denied": denied,
        "file_denied": file_denied,
        "grep_finished": grep_finished,
        "canary_seen": canary_seen,
        "phrase_found": found > 0,
        "files_with_phrase": found,
        "rg": version,
        "max_filesize": MAX_FILESIZE,
    });
    if !tools.is_empty() {
        said["tools"] = serde_json::Value::Object(ran);
    }
    if let Some(sealed) = sealed {
        said["sealed"] = serde_json::json!({
            "parts": sealed.parts,
            "readable": readable,
            "readable_read": read,
            "git": gits,
            "git_denied": git_denied,
            "phrase_searched": sealed.phrase.is_some(),
        });
    }
    said
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #942: the deny list is unset, waived, or absolute paths.
    #[test]
    fn the_deny_list_is_unset_waived_or_absolute_paths() {
        assert_eq!(deny_of(None), Ok(Deny::Unset));
        assert_eq!(deny_of(Some("none")), Ok(Deny::Waived));
        assert_eq!(
            deny_of(Some("/a:/b/c:")),
            Ok(Deny::Paths(vec!["/a".to_owned(), "/b/c".to_owned()]))
        );
        assert!(deny_of(Some("/a:relative")).is_err());
        assert!(deny_of(Some("/it's")).is_err());
        assert!(deny_of(Some(":")).is_err());
    }

    /// #942: the grep looks in the home directory's top level but the denied
    /// paths and `Library`, then `/private/tmp` and the temporary directory,
    /// and the phrase is printed, never written.
    #[test]
    fn the_grep_looks_beside_the_denied_paths_and_in_the_scratch() {
        let home = std::env::temp_dir().join(format!("diet-probe-roots-{}", std::process::id()));
        for name in ["git", "private", "Library", ".Trash", "notes"] {
            std::fs::create_dir_all(home.join(name)).expect("a directory");
        }
        let private = home.join("private").display().to_string();
        let roots = grep_roots(
            &home,
            std::slice::from_ref(&private),
            Some(Path::new("/var/scratch/")),
        );
        let at = |name: &str| home.join(name).display().to_string();
        assert_eq!(
            roots,
            [
                at("git"),
                at("notes"),
                "/private/tmp".to_owned(),
                "/var/scratch".to_owned()
            ]
        );
        let text = scenario(
            Path::new("/r.toml"),
            Path::new("/h.md"),
            &[private],
            (&["/f".to_owned()], "trap phrase", &roots),
            ("canary words", None),
            &[],
        );
        assert!(
            !text.contains("trap phrase") && !text.contains("canary words"),
            "{text}"
        );
        assert!(
            text.contains("rg --no-ignore --hidden --fixed-strings")
                && text.contains("--max-filesize 64M"),
            "{text}"
        );
        assert!(
            !text.contains(" rg -r") && !text.contains(" -r "),
            "rg's -r is --replace: {text}"
        );
        assert!(
            text.contains("printf") && text.contains("x74"),
            "the phrase goes hex-escaped through printf: {text}"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    /// #976: a scenario's tools are probed inside the sandbox, each found,
    /// readable, executable and answering, and the verdict holds only when
    /// every one ran.
    #[test]
    fn the_verdict_holds_only_when_every_tool_ran() {
        let tools = ["ast-grep".to_owned(), "missing-tool".to_owned()];
        let text = scenario(
            Path::new("/r.toml"),
            Path::new("/h.md"),
            &["/denied".to_owned()],
            (&["/f".to_owned()], "phrase", &["/home".to_owned()]),
            ("canary", None),
            &tools,
        );
        assert!(
            text.contains("probe-tool-0") && text.contains("probe-tool-1"),
            "{text}"
        );
        for part in [
            "command -v -- ast-grep",
            "test -r ",
            "test -x ",
            "&& ast-grep --version",
        ] {
            assert!(text.contains(part), "{part}: {text}");
        }
        let line = |id: &str, exit: i64, stdout: &str| {
            serde_json::json!({"kind": "tool_call", "id": id, "outcome": "ran", "exit": exit, "stdout": stdout})
                .to_string()
        };
        let base = [
            line("probe-ls-0", 1, ""),
            line("probe-cat-0", 1, ""),
            line("probe-grep", 2, "/t/canary.txt\n"),
        ]
        .join("\n");
        let both = format!(
            "{base}\n{}\n{}",
            line("probe-tool-0", 0, "ast-grep 0.44.0\n"),
            line("probe-tool-1", 0, "missing-tool 1\n")
        );
        let held = verdict(&both, (1, 1), ("/t/canary.txt", None), &tools);
        assert_eq!(held["ok"], true, "{held}");
        assert_eq!(
            held["tools"]["ast-grep"]["version"], "ast-grep 0.44.0",
            "{held}"
        );
        let one_missing = format!(
            "{base}\n{}\n{}",
            line("probe-tool-0", 0, "ast-grep 0.44.0\n"),
            line("probe-tool-1", 1, "")
        );
        let refused = verdict(&one_missing, (1, 1), ("/t/canary.txt", None), &tools);
        assert_eq!(refused["ok"], false, "{refused}");
        assert_eq!(refused["tools"]["missing-tool"]["ok"], false, "{refused}");
        // Never run at all: not proven.
        let unrun = verdict(&base, (1, 1), ("/t/canary.txt", None), &tools);
        assert_eq!(unrun["ok"], false, "{unrun}");
        // No tools named: no tools leg, and the rest decides.
        let none = verdict(&base, (1, 1), ("/t/canary.txt", None), &[]);
        assert_eq!(
            (none["ok"].clone(), none.get("tools").is_none()),
            (serde_json::json!(true), true),
            "{none}"
        );
    }

    /// #942: the verdict holds only when every probe failed to read, and
    /// the grep finished finding nothing.
    #[test]
    fn the_verdict_holds_only_when_nothing_was_read() {
        let line = |id: &str, exit: Option<i64>, stdout: &str| {
            let mut call = serde_json::json!({"kind": "tool_call", "id": id, "outcome": "ran", "stdout": stdout});
            if let Some(exit) = exit {
                call["exit"] = serde_json::json!(exit);
            }
            call.to_string()
        };
        let canary = "/t/probe/canary.txt";
        let log = |ls1: i64, cat: i64, grep: (Option<i64>, &str)| {
            [
                line("probe-ls-0", Some(1), ""),
                line("probe-ls-1", Some(ls1), "x"),
                line("probe-cat-0", Some(cat), ""),
                line("probe-rg-version", Some(0), "ripgrep 15.1.0\n"),
                line("probe-grep", grep.0, grep.1),
            ]
            .join("\n")
        };
        let seen = "/t/probe/canary.txt\n";
        // rg exits 2 where a denied file is unreadable: what it lists decides.
        let held = verdict(&log(1, 1, (Some(2), seen)), (2, 1), (canary, None), &[]);
        assert_eq!(held["ok"], true, "{held}");
        assert_eq!(held["rg"], "ripgrep 15.1.0", "{held}");
        assert_eq!(
            held["max_filesize"], "64M",
            "the line says what the search skipped: {held}"
        );
        let read = verdict(&log(0, 1, (Some(2), seen)), (2, 1), (canary, None), &[]);
        assert_eq!(
            (read["ok"].clone(), read["paths_denied"].clone()),
            (serde_json::json!(false), serde_json::json!(1))
        );
        assert_eq!(
            verdict(&log(1, 0, (Some(2), seen)), (2, 1), (canary, None), &[])["ok"],
            false
        );
        let leaked = verdict(
            &log(1, 1, (Some(0), "/x/TASKS.md\n/t/probe/canary.txt\n")),
            (2, 1),
            (canary, None),
            &[],
        );
        assert_eq!(
            (leaked["ok"].clone(), leaked["files_with_phrase"].clone()),
            (serde_json::json!(false), serde_json::json!(1))
        );
        let blind = verdict(&log(1, 1, (Some(2), "")), (2, 1), (canary, None), &[]);
        assert_eq!(
            (blind["ok"].clone(), blind["canary_seen"].clone()),
            (serde_json::json!(false), serde_json::json!(false)),
            "a search that found not even the canary searched nothing"
        );
        assert_eq!(
            verdict(&log(1, 1, (None, seen)), (2, 1), (canary, None), &[])["ok"],
            false,
            "a search killed before it finished proves nothing"
        );
        assert_eq!(verdict("", (2, 1), (canary, None), &[])["ok"], false);
    }
}
