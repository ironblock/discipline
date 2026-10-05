//! Which of a `bash` call's commands may run: the denylist, the operator's
//! prompt and the approvals that answer it, read from the command line
//! (#298, ruled at 5981578399 points 1-5, 5982466351 and 5983544366; the
//! amendment to #29).
//!
//! This module decides; it runs nothing, stores nothing and asks no one.
//! `serve`'s wait, the approval store and the log line are #298's, which call
//! [`judge`] with the line and the approvals in force and act on the
//! [`Judgement`]. The surface's replay calls [`project`] through `diet/wasm`
//! on a logged call's `argv` and the denylist it ran under (#389, 5982832752),
//! so the page and the drive have one reader.
//!
//! **The line is read by `formats::shell`, the repository's one authorized
//! shell reader**, never by a second one (ruled at 5982466351, question 1). A
//! line it refuses prompts, and so does every shape it admits but this module
//! cannot vouch for:
//!
//! * a subshell `( … )` or a group `{ …; }`;
//! * a here-doc;
//! * a word that is not literal where the gate decides something: the
//!   program, a wrapper's inner program or its options, a subcommand, and
//!   every word of a `git` call (`git reset $F` would hide `--hard`);
//! * a `$( … )` or a backtick anywhere, whose output the shell would splice
//!   in;
//! * `eval`, `source`, `.` and `exec`, and a shell running a file or stdin;
//! * a control-flow keyword in the program position, because the grammar
//!   reads control flow as ordinary words and `if x; then sudo id; fi` would
//!   otherwise yield a command called `then` ([`CONTROL_WORDS`]);
//! * an assignment prefix naming a variable that changes what runs or what it
//!   loads ([`LOADER_VARIABLES`]), on the line or in `env NAME=value`;
//! * `git` with `-c` or any environment prefix, a git subcommand that is not
//!   one of git's own commands (an alias), and an option before a
//!   subcommanded program's subcommand;
//! * an option a wrapper, a shell or `git` does not read.
//!
//! A prompt of that kind has no [`Shape`]: only a `once` approval of the exact
//! line answers it, never a session or workspace one, because a standing
//! approval of `eval` would approve everything.
//!
//! **Refuse, then prompt, then free.** A segment is refused when a program on
//! the [`Denylist`] appears anywhere in it: as its program, inside a literal
//! wrapper (`env`, `timeout`, `sh -c '…'`), as the program `npx` or `xargs`
//! runs, or inside a `$( … )`. A refused segment refuses the whole line.
//! Otherwise a segment is free only when it is one of `git`'s reads
//! ([`git_is_free`]); everything else prompts until an approval covers it,
//! `ls` and `cat` included, which is the amendment's measurement.

use std::collections::BTreeMap;
use std::fmt;

use crate::digest::sha256_hex;
use crate::formats::record::json::{self, Value};
use crate::formats::shell::{self, Command, List, RedirOp, Simple, Target, Word};

/// What no approval can grant, because the host boundary cannot contain it
/// (#29 5981606817). A program by its basename; `git` by its subcommand, and
/// with a flag after it, by that flag too (`reset` only under `--hard`). One
/// edit here changes the list, and [`denylist_digest`] changes with it.
pub const DENYLIST: &[&str] = &[
    "sudo",
    "su",
    "doas",
    "dd",
    "shred",
    "docker",
    "kubectl",
    "systemctl",
    "crontab",
    "at",
    "git push",
    "git reset --hard",
];

/// Words the grammar reads as ordinary words that are the shell's control
/// flow or a compound test. In the program position they prompt.
pub const CONTROL_WORDS: &[&str] = &[
    "if", "then", "elif", "else", "fi", "for", "while", "until", "do", "done", "case", "esac",
    "in", "function", "select", "coproc", "!", "[[", "]]", "((", "))", "{", "}",
];

/// Environment names whose assignment prefix changes which program runs or
/// what it loads (5983544366 (b)): a prefix naming one makes the segment
/// dynamic, whatever the program. Every other prefix is stripped. A name
/// ending `*` covers every name it begins.
pub const LOADER_VARIABLES: &[&str] = &[
    "PATH",
    "LD_*",
    "DYLD_*",
    "GIT_*",
    "NODE_OPTIONS",
    "PYTHONPATH",
    "PYTHONSTARTUP",
    "PERL5LIB",
    "RUBYLIB",
    "CLASSPATH",
];

/// Whether an assignment to `name` is one [`LOADER_VARIABLES`] names.
fn loads(name: &str) -> bool {
    LOADER_VARIABLES
        .iter()
        .any(|pattern| match pattern.strip_suffix('*') {
            Some(prefix) => name.starts_with(prefix),
            None => name == *pattern,
        })
}

/// Programs that run a string or a file as shell: never read as allowed.
pub const EVALUATORS: &[&str] = &["eval", "source", ".", "exec"];

/// Shells, read through when given `-c '…'`.
const SHELLS: &[&str] = &["sh", "bash", "zsh", "dash", "ksh"];
/// A shell's own flags that take no value.
const SHELL_FLAGS: &[&str] = &["-e", "-u", "-x", "--norc", "--noprofile", "-l", "--login"];

/// Literal wrappers: what follows their own options is the command they run.
const WRAPPERS: &[&str] = &[
    "env", "timeout", "nice", "nohup", "time", "command", "builtin",
];

/// The runners: programs that run the program their first word names --
/// `npx X`, pnpm's `pnpx X` and `pnx X`, each `pnpm dlx X` (pnpm 11.21.0's
/// `package.json` `bin`), and `corepack X`, which runs the package manager
/// X names (#484's review round 1, finding 7).
const RUNNERS: &[&str] = &["npx", "pnpx", "pnx", "corepack"];

/// Programs that are a runner under these subcommands: `npm exec X` (and
/// `x`), `pnpm exec X` and `pnpm dlx X`, and the same under `pn`, pnpm
/// 11.21.0's own alias of `pnpm`.
const RUNNER_SUBCOMMANDS: &[(&str, &[&str])] = &[
    ("npm", &["exec", "x"]),
    ("pnpm", &["exec", "dlx"]),
    ("pn", &["exec", "dlx"]),
];

/// `npx`'s and `npm exec`'s flags that take no value.
const RUNNER_FLAGS: &[&str] = &["-y", "--yes", "--no", "--no-install", "-q", "--quiet"];

/// The packages a runner's call is judged as directly (#484's ruling):
/// `npx pnpm@11.20.0 install` and `npm exec pnpm install` are `pnpm
/// install`, shaped by the package as written -- `pnpm@11.20.0 install`,
/// `pnpm install` -- so another pinned version is another approval.
const RUN_THROUGH: &[&str] = &["pnpm"];

/// The runner flags a call [`through`] a runner may carry and still be
/// shaped: npx's `--yes` and `-y`, which answer its install prompt and
/// choose nothing that runs. Any other ([`RUNNER_FLAGS`], `--`) leaves the
/// call never standing, its inner command still read for the denylist.
const RUN_THROUGH_FLAGS: &[&str] = &["-y", "--yes"];

/// A call through a runner to a [`RUN_THROUGH`] package: `npx [-y] pnpm@v
/// …`, `npm exec pnpm …`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Through<'a> {
    /// The package's own name: `pnpm`.
    pub package: &'static str,
    /// The package as written: `pnpm@11.20.0`, or `pnpm`.
    pub spec: &'a str,
    /// The package's words, from the spec on: `pnpm@11.20.0 install`.
    pub words: &'a [Word],
    /// Whether the call can be shaped as the package's own: the runner by
    /// its name, not a path; no runner flag but [`RUN_THROUGH_FLAGS`]; the
    /// package bare or pinned to an exact `x.y.z`.
    pub plain: bool,
}

/// Whether `version` is an exact `x.y.z`: a range or a tag (`11`, `^11.0.0`,
/// `latest`) picks whichever release is newest when it runs.
fn exact_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

/// The words a runner runs, after the runner and its subcommand: `X …` for
/// `npx X …` ([`RUNNERS`]) and `npm exec X …` ([`RUNNER_SUBCOMMANDS`]).
/// `None` for any other program.
fn runner_inner<'a>(program: &str, rest: &'a [Word]) -> Option<&'a [Word]> {
    if RUNNERS.contains(&program) {
        Some(rest)
    } else if RUNNER_SUBCOMMANDS.iter().any(|(runner, subcommands)| {
        *runner == program
            && rest
                .first()
                .is_some_and(|w| subcommands.contains(&w.text.as_str()))
    }) {
        Some(&rest[1..])
    } else {
        None
    }
}

/// `words`, a simple command's, as a call through a runner to a
/// [`RUN_THROUGH`] package, if it is one.
#[must_use]
pub fn through(words: &[Word]) -> Option<Through<'_>> {
    let first = words.first().filter(|w| w.literal)?;
    let program = basename(&first.text);
    let inner = runner_inner(program, &words[1..])?;
    let mut plain = program == first.text;
    let mut i = 0;
    while let Some(word) = inner.get(i).filter(|w| w.literal) {
        if RUNNER_FLAGS.contains(&word.text.as_str()) {
            plain &= RUN_THROUGH_FLAGS.contains(&word.text.as_str());
            i += 1;
        } else {
            if word.text == "--" {
                plain = false;
                i += 1;
            }
            break;
        }
    }
    let spec = inner.get(i).filter(|w| w.literal)?;
    let (name, version) = match spec.text.split_once('@') {
        Some((name, version)) => (name, Some(version)),
        None => (spec.text.as_str(), None),
    };
    let package = RUN_THROUGH.iter().find(|p| **p == name)?;
    plain &= version.is_none_or(exact_version);
    Some(Through {
        package,
        spec: &spec.text,
        words: &inner[i..],
        plain,
    })
}

/// The package a shape's program names: `pnpm` for `pnpm@11.20.0`, which a
/// call [`through`] a runner is shaped as; `program` itself otherwise.
#[must_use]
pub fn package_of(program: &str) -> &str {
    match program.split_once('@') {
        Some((name, version)) if RUN_THROUGH.contains(&name) && exact_version(version) => name,
        _ => program,
    }
}

/// `xargs`'s flags without a value, then with one.
const XARGS_FLAGS: &[&str] = &[
    "-0",
    "--null",
    "-r",
    "--no-run-if-empty",
    "-t",
    "--verbose",
    "-x",
    "--exit",
];
const XARGS_VALUED: &[&str] = &["-n", "-L", "-P", "-I", "-d", "-s", "-E", "-a", "--arg-file"];

/// The `find` actions that run a command.
const FIND_EXEC: &[&str] = &["-exec", "-execdir", "-ok", "-okdir"];

/// git's global options, with a value, without one, and as `--name=value`.
const GIT_VALUED: &[&str] = &["-C", "--git-dir", "--work-tree", "--namespace"];
const GIT_FLAGS: &[&str] = &[
    "--no-pager",
    "-P",
    "--bare",
    "--no-replace-objects",
    "--literal-pathspecs",
];
const GIT_PREFIXED: &[&str] = &["--git-dir=", "--work-tree=", "--namespace="];

/// git's own commands (`git --list-cmds=main`, its internal `--` helpers
/// aside). A subcommand outside it is an alias or an external `git-*`
/// program this cannot expand, so it is dynamic (5983544366 (f)); expanding
/// an alias is the integration's job.
const GIT_COMMANDS: &[&str] = &[
    "add",
    "am",
    "annotate",
    "apply",
    "archive",
    "backfill",
    "bisect",
    "blame",
    "branch",
    "bugreport",
    "bundle",
    "cat-file",
    "check-attr",
    "check-ignore",
    "check-mailmap",
    "check-ref-format",
    "checkout",
    "checkout-index",
    "cherry",
    "cherry-pick",
    "clean",
    "clone",
    "column",
    "commit",
    "commit-graph",
    "commit-tree",
    "config",
    "count-objects",
    "credential",
    "credential-cache",
    "credential-store",
    "daemon",
    "describe",
    "diagnose",
    "diff",
    "diff-files",
    "diff-index",
    "diff-pairs",
    "diff-tree",
    "difftool",
    "fast-export",
    "fast-import",
    "fetch",
    "fetch-pack",
    "filter-branch",
    "fmt-merge-msg",
    "for-each-ref",
    "for-each-repo",
    "format-patch",
    "fsck",
    "fsck-objects",
    "gc",
    "get-tar-commit-id",
    "grep",
    "hash-object",
    "help",
    "hook",
    "http-backend",
    "http-fetch",
    "http-push",
    "imap-send",
    "index-pack",
    "init",
    "init-db",
    "interpret-trailers",
    "log",
    "ls-files",
    "ls-remote",
    "ls-tree",
    "mailinfo",
    "mailsplit",
    "maintenance",
    "merge",
    "merge-base",
    "merge-file",
    "merge-index",
    "merge-octopus",
    "merge-one-file",
    "merge-ours",
    "merge-recursive",
    "merge-recursive-ours",
    "merge-recursive-theirs",
    "merge-resolve",
    "merge-subtree",
    "merge-tree",
    "mergetool",
    "mktag",
    "mktree",
    "multi-pack-index",
    "mv",
    "name-rev",
    "notes",
    "pack-objects",
    "pack-redundant",
    "pack-refs",
    "patch-id",
    "pickaxe",
    "prune",
    "prune-packed",
    "pull",
    "push",
    "quiltimport",
    "range-diff",
    "read-tree",
    "rebase",
    "receive-pack",
    "reflog",
    "refs",
    "remote",
    "remote-ext",
    "remote-fd",
    "remote-ftp",
    "remote-ftps",
    "remote-http",
    "remote-https",
    "repack",
    "replace",
    "replay",
    "request-pull",
    "rerere",
    "reset",
    "restore",
    "rev-list",
    "rev-parse",
    "revert",
    "rm",
    "send-email",
    "send-pack",
    "shell",
    "shortlog",
    "show",
    "show-branch",
    "show-index",
    "show-ref",
    "sparse-checkout",
    "stage",
    "stash",
    "status",
    "stripspace",
    "submodule",
    "subtree",
    "switch",
    "symbolic-ref",
    "tag",
    "unpack-file",
    "unpack-objects",
    "update-index",
    "update-ref",
    "update-server-info",
    "upload-archive",
    "upload-pack",
    "var",
    "verify-commit",
    "verify-pack",
    "verify-tag",
    "version",
    "whatchanged",
    "worktree",
    "write-tree",
];

/// git's free reads (point 4), `branch` aside.
const GIT_READS: &[&str] = &["status", "log", "diff", "show"];

/// The programs whose subcommand is part of an approval's shape (point 5).
const SUBCOMMANDED: &[&str] = &[
    "git", "npm", "npx", "pnpm", "pn", "pnpx", "pnx", "yarn", "cargo", "pip", "pip3",
];

/// How deep `sh -c '…'` and `$( … )` are read before the call prompts instead.
const MAX_DEPTH: usize = 4;

/// The sha256 of the denylist's canonical text: its entries sorted, each
/// followed by a newline, UTF-8. The receipt's denylist digest. Sorted, so
/// the list is a set and reordering it is not a change (5982466351).
#[must_use]
pub fn denylist_digest() -> String {
    Denylist::standard().digest()
}

/// A denylist, read from entries spelled as [`DENYLIST`]'s are: `program`, or
/// `git <subcommand>`, or `git <subcommand> <flag>`. The drive judges by
/// [`Denylist::standard`]; a replay judges by the list the call ran under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denylist {
    /// Each entry as written, sorted.
    entries: Vec<String>,
}

impl Denylist {
    /// The drive's own list, [`DENYLIST`].
    #[must_use]
    pub fn standard() -> Self {
        Self::new(DENYLIST).unwrap_or_else(|_| Self {
            entries: Vec::new(),
        })
    }

    /// A list from its entries.
    ///
    /// # Errors
    ///
    /// An entry that is empty, carries more than one word outside a `git`
    /// entry, or names `git` with no subcommand or more than one flag.
    pub fn new<S: AsRef<str>>(entries: &[S]) -> Result<Self, String> {
        let mut kept = Vec::new();
        for entry in entries {
            let entry = entry.as_ref();
            let words: Vec<&str> = entry.split(' ').collect();
            let valid = if words.first() == Some(&"git") {
                (2..=3).contains(&words.len()) && words.iter().all(|w| !w.is_empty())
            } else {
                words.len() == 1 && !entry.is_empty()
            };
            if !valid {
                return Err(format!(
                    "`{entry}` is not a denylist entry: a program, or `git <subcommand> [<flag>]`"
                ));
            }
            kept.push(entry.to_owned());
        }
        kept.sort_unstable();
        kept.dedup();
        Ok(Self { entries: kept })
    }

    /// The sha256 of the sorted canonical text, each entry plus `\n`.
    #[must_use]
    pub fn digest(&self) -> String {
        let text: String = self
            .entries
            .iter()
            .flat_map(|e| [e.as_str(), "\n"])
            .collect();
        sha256_hex(text.as_bytes())
    }

    /// The entry a program alone matches.
    fn program(&self, program: &str) -> Option<&str> {
        self.entries
            .iter()
            .map(String::as_str)
            .find(|entry| !entry.contains(' ') && *entry == program)
    }

    /// The entry a git subcommand and its arguments match.
    fn git(&self, subcommand: &str, args: &[Word]) -> Option<&str> {
        self.entries.iter().map(String::as_str).find(|entry| {
            let words: Vec<&str> = entry.split(' ').collect();
            words.first() == Some(&"git")
                && words.get(1) == Some(&subcommand)
                && words
                    .get(2)
                    .is_none_or(|flag| args.iter().any(|w| w.text == *flag))
        })
    }
}

/// What an approval of a segment covers: its program, plus its subcommand for
/// the [`SUBCOMMANDED`] programs and `python -m`. Arguments are not part of
/// it, so approving `cat a.txt` for the session covers `cat b.txt`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Shape {
    /// The program's basename: `/bin/cat` is `cat`.
    pub program: String,
    /// `install` for `npm install`; `-m http.server` for `python -m http.server`.
    pub subcommand: Option<String>,
}

impl fmt::Display for Shape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.subcommand {
            Some(sub) => write!(f, "{} {sub}", self.program),
            None => f.write_str(&self.program),
        }
    }
}

/// An approval's reach, as #388's log records it. `once` is the exact line,
/// once; the rest cover a [`Shape`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scope {
    /// This line, this time.
    Once,
    /// The shape, for the rest of the session.
    Session,
    /// The shape, for this worktree, across sessions.
    Workspace,
    /// The shape, granted before the session by the regimen's
    /// `allowed_commands`.
    Preseeded,
}

/// A decision in force: the line it was given for, or the shape it covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Approval {
    /// `once`: the exact command line, byte for byte, every segment of it.
    Once {
        /// The tool call's command line.
        line: String,
    },
    /// `session`, `workspace` or `preseeded`: a shape.
    Standing {
        /// Which of the three. [`Scope::Once`] here covers nothing.
        scope: Scope,
        /// What it covers.
        shape: Shape,
    },
}

impl Approval {
    /// The scope the log records.
    #[must_use]
    pub fn scope(&self) -> Scope {
        match self {
            Self::Once { .. } => Scope::Once,
            Self::Standing { scope, .. } => *scope,
        }
    }

    /// Whether this approval covers a prompting segment of `line`.
    #[must_use]
    pub fn covers(&self, line: &str, segment: &Segment) -> bool {
        match self {
            Self::Once { line: approved } => approved == line,
            Self::Standing { scope, shape } => {
                *scope != Scope::Once && segment.shape.as_ref() == Some(shape)
            }
        }
    }
}

/// Why a segment prompts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// `formats::shell` does not read the line.
    Unparsable(String),
    /// Syntax whose effect the gate cannot see: an expansion, a subshell, a
    /// keyword, an evaluator, a wrapper whose inner command is not literal.
    /// Only a `once` approval answers it.
    Dynamic(String),
    /// A readable command no approval covers yet.
    NotApproved,
}

/// What became of one segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// A program on the denylist, named by its entry.
    Refused(String),
    /// It needs the operator.
    Prompt(Why),
    /// One of git's reads: no approval needed.
    Free,
    /// An approval in force covers it.
    Approved(Scope),
}

/// One simple command of the line, after its wrappers are read through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// What an approval of it would cover. `None` when it is dynamic or
    /// unreadable, which no standing approval covers.
    pub shape: Option<Shape>,
    /// What became of it.
    pub verdict: Verdict,
}

/// What the whole line comes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// A segment is refused: the call does not run (`refused { denylist }`).
    Refused,
    /// A segment needs the operator.
    Prompt,
    /// Every segment is free or approved.
    Run,
}

/// The gate's reading of one command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judgement {
    /// Each segment, in the order written, wrappers read through.
    pub segments: Vec<Segment>,
}

impl Judgement {
    /// Refused if any segment is; else a prompt if any segment prompts; else
    /// it runs.
    #[must_use]
    pub fn outcome(&self) -> Outcome {
        if self.refused_by().is_some() {
            Outcome::Refused
        } else if self
            .segments
            .iter()
            .any(|segment| matches!(segment.verdict, Verdict::Prompt(_)))
        {
            Outcome::Prompt
        } else {
            Outcome::Run
        }
    }

    /// The first denylist entry that refused a segment.
    #[must_use]
    pub fn refused_by(&self) -> Option<&str> {
        self.segments
            .iter()
            .find_map(|segment| match &segment.verdict {
                Verdict::Refused(entry) => Some(entry.as_str()),
                _ => None,
            })
    }

    fn approve(mut self, line: &str, approvals: &[Approval]) -> Self {
        for segment in &mut self.segments {
            if matches!(segment.verdict, Verdict::Prompt(_))
                && let Some(approval) = approvals.iter().find(|a| a.covers(line, segment))
            {
                segment.verdict = Verdict::Approved(approval.scope());
            }
        }
        self
    }
}

/// Read `line` and judge each segment against [`DENYLIST`] and `approvals`.
#[must_use]
pub fn judge(line: &str, approvals: &[Approval]) -> Judgement {
    let gate = Gate {
        denylist: &Denylist::standard(),
    };
    Judgement {
        segments: gate.read(line, 0),
    }
    .approve(line, approvals)
}

/// Judge a call by its `argv` -- the form a log's `tool_call` keeps --
/// against `denylist`, with no approvals: what a replay re-derives (#389).
/// `["sh", "-c", "ls; sudo id"]` is read through the shell like any wrapper.
#[must_use]
pub fn judge_argv(argv: &[String], denylist: &Denylist) -> Judgement {
    let words: Vec<Word> = argv
        .iter()
        .map(|text| Word {
            text: text.clone(),
            literal: true,
            substitutions: Vec::new(),
        })
        .collect();
    let gate = Gate { denylist };
    Judgement {
        segments: gate.read_words(&words, &Simple::default(), 0, false),
    }
}

/// The browser-callable judgement (#389): `input` is one JSON object,
/// `{"argv": [...], "denylist": [...]}`, and the value is the [`Judgement`]:
/// `{"outcome": ..., "segments": [{"verdict": ..., "shape"?: ..., "entry"?:
/// ..., "why"?: ..., "reason"?: ...}]}`. Read with the record's own JSON
/// reader and rendered in its value space, so native and wasm can be compared
/// byte for byte.
///
/// # Errors
///
/// Input that is not such an object, or a denylist entry [`Denylist::new`]
/// refuses.
pub fn project(input: &str) -> Result<Value, String> {
    let object = json::line(input).map_err(|err| format!("not a judgement request: {err}"))?;
    let strings = |key: &str| -> Result<Vec<String>, String> {
        let Some(Value::Array(items)) = object.get(key) else {
            return Err(format!("`{key}` is not a list"));
        };
        items
            .iter()
            .map(|item| match item {
                Value::String(text) => Ok(text.clone()),
                _ => Err(format!("`{key}` holds something that is not a string")),
            })
            .collect()
    };
    let unknown: Vec<&String> = object
        .keys()
        .filter(|k| *k != "argv" && *k != "denylist")
        .collect();
    if let Some(key) = unknown.first() {
        return Err(format!("`{key}` is not a key of a judgement request"));
    }
    let argv = strings("argv")?;
    if argv.is_empty() {
        return Err("`argv` is empty".to_owned());
    }
    let denylist = Denylist::new(&strings("denylist")?)?;
    Ok(judgement_value(&judge_argv(&argv, &denylist)))
}

fn judgement_value(judgement: &Judgement) -> Value {
    let text = |s: &str| Value::String(s.to_owned());
    let outcome = match judgement.outcome() {
        Outcome::Refused => "refused",
        Outcome::Prompt => "prompt",
        Outcome::Run => "run",
    };
    let segments = judgement
        .segments
        .iter()
        .map(|segment| {
            let mut fields = BTreeMap::new();
            if let Some(shape) = &segment.shape {
                fields.insert("shape".to_owned(), text(&shape.to_string()));
            }
            let verdict = match &segment.verdict {
                Verdict::Refused(entry) => {
                    fields.insert("entry".to_owned(), text(entry));
                    "refused"
                }
                Verdict::Prompt(why) => {
                    let (kind, reason) = match why {
                        Why::Unparsable(reason) => ("unparsable", Some(reason)),
                        Why::Dynamic(reason) => ("dynamic", Some(reason)),
                        Why::NotApproved => ("not_approved", None),
                    };
                    fields.insert("why".to_owned(), text(kind));
                    if let Some(reason) = reason {
                        fields.insert("reason".to_owned(), text(reason));
                    }
                    "prompt"
                }
                Verdict::Free => "free",
                Verdict::Approved(_) => "approved",
            };
            fields.insert("verdict".to_owned(), text(verdict));
            Value::Object(fields)
        })
        .collect();
    Value::Object(BTreeMap::from([
        ("outcome".to_owned(), text(outcome)),
        ("segments".to_owned(), Value::Array(segments)),
    ]))
}

fn prompt(shape: Option<Shape>, why: Why) -> Segment {
    Segment {
        shape,
        verdict: Verdict::Prompt(why),
    }
}

fn dynamic(why: impl Into<String>) -> Segment {
    prompt(None, Why::Dynamic(why.into()))
}

fn refused(entry: &str) -> Segment {
    Segment {
        shape: None,
        verdict: Verdict::Refused(entry.to_owned()),
    }
}

fn is_refused(segment: &Segment) -> bool {
    matches!(segment.verdict, Verdict::Refused(_))
}

fn basename(text: &str) -> &str {
    text.rsplit('/').next().unwrap_or(text)
}

/// The reader, carrying the denylist it judges by.
struct Gate<'a> {
    denylist: &'a Denylist,
}

impl Gate<'_> {
    /// Every segment of `line`, at `depth` levels of `sh -c` inside the call.
    fn read(&self, line: &str, depth: usize) -> Vec<Segment> {
        match shell::parse(line) {
            Ok(list) if list.is_empty() => vec![dynamic("an empty command line")],
            Ok(list) => {
                let mut segments = Vec::new();
                self.read_list(&list, depth, false, &mut segments);
                segments
            }
            Err(err) => vec![prompt(None, Why::Unparsable(err.to_string()))],
        }
    }

    /// `inside` is whether the list is a subshell's or a group's: its commands
    /// are still checked against the denylist, and prompt whatever they are.
    fn read_list(&self, list: &List, depth: usize, inside: bool, into: &mut Vec<Segment>) {
        for and_or in &list.items {
            for link in &and_or.chain {
                for command in &link.pipeline {
                    match command {
                        Command::Simple(simple) => {
                            let mut found = self.read_simple(simple, depth);
                            if inside {
                                for segment in &mut found {
                                    if !is_refused(segment) {
                                        *segment = dynamic("a subshell or group");
                                    }
                                }
                            }
                            into.extend(found);
                        }
                        Command::Subshell(inner) | Command::Group(inner) => {
                            self.read_list(inner, depth, true, into);
                        }
                    }
                }
            }
        }
    }

    /// The segments one simple command comes to: usually one, more when the
    /// command is `sh -c '…'`.
    fn read_simple(&self, simple: &Simple, depth: usize) -> Vec<Segment> {
        // What the shell runs before the command: every `$( … )` in a word, an
        // assignment or a redirection. Read for the denylist, and the command
        // prompts whatever they hold.
        let mut spliced = Vec::new();
        let mut backtick = false;
        let assigned = simple.assignments.iter().filter_map(|a| a.value.as_ref());
        let redirected = simple.redirections.iter().filter_map(|r| match &r.target {
            Target::File(word) => Some(word),
            _ => None,
        });
        for word in simple.words.iter().chain(assigned).chain(redirected) {
            spliced.extend(word.substitutions.iter().cloned());
            backtick |= !word.literal && word.text.contains('`');
        }
        let mut inner = Vec::new();
        for text in &spliced {
            if depth >= MAX_DEPTH {
                inner.push(dynamic("a command substitution nested too deep to read"));
            } else {
                inner.extend(self.read(text, depth + 1));
            }
        }
        if let Some(refusal) = inner.into_iter().find(is_refused) {
            return vec![refusal];
        }
        let loader = simple.assignments.iter().any(|a| loads(&a.name));
        let heredoc = simple
            .redirections
            .iter()
            .any(|r| matches!(r.op, RedirOp::Heredoc | RedirOp::HeredocStrip));
        let mut segments = self.read_words(&simple.words, simple, depth, false);
        if segments.iter().any(is_refused) {
            return segments;
        }
        let why = if !spliced.is_empty() {
            Some("a command substitution")
        } else if loader {
            Some("an environment prefix that changes what runs or what it loads")
        } else if backtick {
            Some("a backtick substitution")
        } else if heredoc {
            Some("a here-doc")
        } else {
            None
        };
        if let Some(why) = why {
            for segment in &mut segments {
                *segment = dynamic(why);
            }
        }
        segments
    }

    /// Read `words` as a command: the program, through any wrapper, to a
    /// verdict. `simple` is the command they came from, for its assignments
    /// and redirections; `prefixed` is whether a wrapper above set an
    /// environment variable (`env NAME=value …`).
    fn read_words(
        &self,
        words: &[Word],
        simple: &Simple,
        depth: usize,
        prefixed: bool,
    ) -> Vec<Segment> {
        let Some(first) = words.first() else {
            // Assignments alone run no program.
            return vec![Segment {
                shape: None,
                verdict: Verdict::Free,
            }];
        };
        if !first.literal {
            return vec![dynamic("a program word that is not literal")];
        }
        let program = basename(&first.text);
        if let Some(entry) = self.denylist.program(program) {
            return vec![refused(entry)];
        }
        if CONTROL_WORDS.contains(&program) {
            return vec![dynamic(format!("the control-flow word `{program}`"))];
        }
        if EVALUATORS.contains(&program) {
            return vec![dynamic(format!(
                "`{program}` runs a string or a file as shell"
            ))];
        }
        let rest = &words[1..];
        if program == "git" {
            vec![self.read_git(rest, simple, prefixed)]
        } else if SHELLS.contains(&program) {
            self.read_shell(program, rest, depth)
        } else if WRAPPERS.contains(&program) {
            match unwrap(program, rest) {
                Ok([]) => vec![dynamic(format!("`{program}` with no command"))],
                // `env NAME=value …` is an environment prefix on what it
                // runs, as `NAME=value …` is: carried down to `read_git`,
                // which tests it after the denylist (5983544366 (c); #402's
                // round-2 review, B1).
                Ok(inner) => {
                    let assigned = program == "env"
                        && rest[..rest.len() - inner.len()]
                            .iter()
                            .any(|w| !w.text.starts_with('-') && w.text.contains('='));
                    self.read_words(inner, simple, depth, prefixed || assigned)
                }
                Err(why) => vec![dynamic(why)],
            }
        } else if let Some(through) = through(words) {
            // `npx pnpm@v …` is `pnpm …` (#484's ruling).
            self.read_through(&through, simple, depth, prefixed)
        } else if let Some(inner) = runner_inner(program, rest) {
            // `npx X` and `npm exec X` run the program X names (point 2): it
            // meets the denylist, and the call is otherwise shaped as written.
            // pnpm's runners are read the same way (#482).
            self.runner(program, inner, rest)
        } else if program == "xargs" {
            self.named_inner(program, xargs_inner(rest), simple, depth)
        } else if program == "find" && find_inner(rest).is_some() {
            self.named_inner(program, find_inner(rest), simple, depth)
        } else {
            vec![ordinary(program, rest)]
        }
    }

    /// A call [`through`] a runner, read as the package's own words: a
    /// refusal among them refuses it; a plain one is shaped as the package
    /// as written, any other is dynamic.
    fn read_through(
        &self,
        through: &Through<'_>,
        simple: &Simple,
        depth: usize,
        prefixed: bool,
    ) -> Vec<Segment> {
        let mut words = through.words.to_vec();
        through.package.clone_into(&mut words[0].text);
        let mut segments = self.read_words(&words, simple, depth, prefixed);
        if let Some(refusal) = segments.iter().find(|s| is_refused(s)) {
            return vec![refusal.clone()];
        }
        if !through.plain {
            return vec![dynamic(format!(
                "`{}` through a runner with a flag, a path or a version that is not exact",
                through.spec
            ))];
        }
        for segment in &mut segments {
            if let Some(shape) = segment
                .shape
                .as_mut()
                .filter(|s| s.program == through.package)
            {
                through.spec.clone_into(&mut shape.program);
            }
        }
        segments
    }

    /// `npx …` or `npm exec …` ([`RUNNERS`], [`RUNNER_SUBCOMMANDS`]): the
    /// program it runs is the first word past its flags that take no value
    /// (and past `--`). A denylisted one is refused; otherwise the call is
    /// shaped from `shaped`, the words after the program.
    fn runner(&self, program: &str, inner: &[Word], shaped: &[Word]) -> Vec<Segment> {
        let mut i = 0;
        while let Some(word) = inner.get(i) {
            if RUNNER_FLAGS.contains(&word.text.as_str()) {
                i += 1;
            } else {
                if word.text == "--" {
                    i += 1;
                }
                break;
            }
        }
        if let Some(word) = inner
            .get(i)
            .filter(|w| w.literal && !w.text.starts_with('-'))
            && let Some(entry) = self.denylist.program(basename(&word.text))
        {
            return vec![refused(entry)];
        }
        vec![ordinary(program, shaped)]
    }

    /// A non-literal wrapper (`xargs`, `find -exec`): its inner command is
    /// read through as far as it is literal, so a denylisted program there is
    /// refused (`xargs env sudo`); otherwise the call prompts, because the
    /// arguments it runs on are built at run time. `None` is an inner command
    /// this cannot locate.
    fn named_inner(
        &self,
        wrapper: &str,
        inner: Option<&[Word]>,
        simple: &Simple,
        depth: usize,
    ) -> Vec<Segment> {
        let Some(words) = inner.filter(|words| !words.is_empty()) else {
            return vec![dynamic(format!(
                "`{wrapper}` runs a command this cannot read"
            ))];
        };
        let found = self.read_words(words, simple, depth, false);
        if let Some(refusal) = found.into_iter().find(is_refused) {
            return vec![refusal];
        }
        vec![dynamic(format!(
            "`{wrapper}` runs a command on arguments it builds"
        ))]
    }

    /// `sh -c '…'`: the string is read as a command line of its own, to
    /// [`MAX_DEPTH`]. Without `-c` the shell runs a script file, or stdin, that
    /// this cannot see, and that is dynamic.
    fn read_shell(&self, program: &str, rest: &[Word], depth: usize) -> Vec<Segment> {
        let mut i = 0;
        while let Some(word) = rest.get(i) {
            if !word.literal {
                return vec![dynamic(format!(
                    "`{program}` with a word that is not literal"
                ))];
            }
            let text = word.text.as_str();
            if text == "-c" {
                return match rest.get(i + 1) {
                    Some(script) if script.literal && depth < MAX_DEPTH => {
                        self.read(&script.text, depth + 1)
                    }
                    Some(script) if script.literal => {
                        vec![dynamic(format!("`{program} -c` nested too deep to read"))]
                    }
                    _ => vec![dynamic(format!(
                        "`{program} -c` with a command that is not literal"
                    ))],
                };
            }
            if SHELL_FLAGS.contains(&text) {
                i += 1;
            } else if text == "-o" || text == "+o" {
                // `-o pipefail`: an option name follows.
                i += 2;
            } else if text.starts_with('-') {
                return vec![dynamic(format!(
                    "`{program}` with an option this does not read: `{text}`"
                ))];
            } else {
                // A script file, which the gate cannot read: `sh x.sh` is
                // `source x.sh` in a child, so it prompts as `source` does, and
                // no standing approval of `sh` runs whatever script comes next.
                return vec![dynamic(format!(
                    "`{program}` runs a script file this cannot read"
                ))];
            }
        }
        // No `-c` and no file: the shell reads its commands from stdin.
        vec![dynamic(format!(
            "`{program}` reads commands this cannot see"
        ))]
    }

    /// `git`: its global options, then its subcommand. Every word must be
    /// literal, because a `$F` could be `--hard`.
    fn read_git(&self, rest: &[Word], simple: &Simple, prefixed: bool) -> Segment {
        if rest.iter().any(|w| !w.literal) {
            return dynamic("a `git` call with a word that is not literal");
        }
        let mut i = 0;
        let mut configured = false;
        while let Some(word) = rest.get(i) {
            let text = word.text.as_str();
            if text == "-c" {
                configured = true;
                i += 2;
            } else if GIT_VALUED.contains(&text) {
                i += 2;
            } else if GIT_FLAGS.contains(&text) || GIT_PREFIXED.iter().any(|p| text.starts_with(p))
            {
                i += 1;
            } else if text.starts_with('-') {
                return dynamic(format!(
                    "`git` with a global option this does not read: `{text}`"
                ));
            } else {
                break;
            }
        }
        let Some(subcommand) = rest.get(i).map(|w| w.text.as_str()) else {
            return prompt(
                Some(Shape {
                    program: "git".to_owned(),
                    subcommand: None,
                }),
                Why::NotApproved,
            );
        };
        let args = &rest[i + 1..];
        if let Some(entry) = self.denylist.git(subcommand, args) {
            return refused(entry);
        }
        if !GIT_COMMANDS.contains(&subcommand) {
            return dynamic(format!(
                "`git {subcommand}` is not one of git's own commands: an alias or an external program"
            ));
        }
        let shape = Shape {
            program: "git".to_owned(),
            subcommand: Some(subcommand.to_owned()),
        };
        // `-c` can name a pager, an external diff or an ssh command, and an
        // environment prefix can do the same through a name `LOADER_VARIABLES`
        // does not list (`PAGER`, `EDITOR`): either makes the call run a
        // program its shape does not name, so it is dynamic -- never free, and
        // no standing approval of `git <subcommand>` reaches it (5983544366
        // (c)).
        if configured || prefixed || !simple.assignments.is_empty() {
            return dynamic(
                "`git` with `-c` or an environment prefix, which can name a program to run",
            );
        }
        if git_is_free(subcommand, args) && !writes_a_file(simple) {
            return Segment {
                shape: Some(shape),
                verdict: Verdict::Free,
            };
        }
        prompt(Some(shape), Why::NotApproved)
    }
}

fn xargs_inner(rest: &[Word]) -> Option<&[Word]> {
    let mut i = 0;
    while let Some(word) = rest.get(i) {
        let text = word.text.as_str();
        if XARGS_FLAGS.contains(&text) {
            i += 1;
        } else if XARGS_VALUED.contains(&text) {
            i += 2;
        } else if text == "--" {
            return rest.get(i + 1..);
        } else if text.starts_with('-') {
            return None;
        } else {
            return Some(&rest[i..]);
        }
    }
    None
}

/// The words after `-exec` (or `-execdir`, `-ok`, `-okdir`), up to its `;`
/// or `+`.
fn find_inner(rest: &[Word]) -> Option<&[Word]> {
    let at = rest
        .iter()
        .position(|w| FIND_EXEC.contains(&w.text.as_str()))?;
    let words = &rest[at + 1..];
    let end = words
        .iter()
        .position(|w| w.text == ";" || w.text == "+")
        .unwrap_or(words.len());
    Some(&words[..end])
}

/// How a literal wrapper's word is read: skip it (and its value), or the
/// command starts here, or after `n` more words.
enum Step {
    Skip(usize),
    Command,
    CommandAfter(usize),
}

/// One word of a wrapper's own options. An option this does not know is an
/// `Err`, because guessing past it could misplace the inner program.
fn wrapper_step(wrapper: &str, text: &str) -> Result<Step, ()> {
    if text == "--" {
        return Ok(Step::CommandAfter(1));
    }
    let numeric =
        text.len() > 1 && text.starts_with('-') && text[1..].chars().all(|c| c.is_ascii_digit());
    let step = if wrapper == "env" {
        if ["-i", "--ignore-environment", "-"].contains(&text) {
            Step::Skip(1)
        } else if ["-u", "--unset"].contains(&text) {
            Step::Skip(2)
        } else if text.starts_with('-') || text.split_once('=').is_some_and(|(name, _)| loads(name))
        {
            // An option this does not read, or a loader variable
            // (5983544366 (b)).
            return Err(());
        } else if text.contains('=') {
            Step::Skip(1)
        } else {
            Step::Command
        }
    } else if wrapper == "timeout" {
        if ["--preserve-status", "--foreground", "-v", "--verbose"].contains(&text)
            || text.starts_with("--signal=")
            || text.starts_with("--kill-after=")
        {
            Step::Skip(1)
        } else if ["-s", "--signal", "-k", "--kill-after"].contains(&text) {
            Step::Skip(2)
        } else if text.starts_with('-') {
            return Err(());
        } else {
            // The duration, then the command.
            Step::CommandAfter(1)
        }
    } else if wrapper == "nice" {
        if ["-n", "--adjustment"].contains(&text) {
            Step::Skip(2)
        } else if text.starts_with("--adjustment=") || numeric {
            Step::Skip(1)
        } else if text.starts_with('-') {
            return Err(());
        } else {
            Step::Command
        }
    } else if ["time", "command", "builtin"].contains(&wrapper) {
        if text == "-p" {
            Step::Skip(1)
        } else if text.starts_with('-') {
            return Err(());
        } else {
            Step::Command
        }
    } else if text.starts_with('-') {
        // `nohup` takes no options of its own.
        return Err(());
    } else {
        Step::Command
    };
    Ok(step)
}

/// The words a literal wrapper runs, past its own options.
fn unwrap<'a>(wrapper: &str, rest: &'a [Word]) -> Result<&'a [Word], String> {
    let mut i = 0;
    while let Some(word) = rest.get(i) {
        if !word.literal {
            return Err(format!("`{wrapper}` with a word that is not literal"));
        }
        match wrapper_step(wrapper, &word.text) {
            Ok(Step::Skip(n)) => i += n,
            Ok(Step::Command) => return Ok(&rest[i..]),
            Ok(Step::CommandAfter(n)) => return Ok(rest.get(i + n..).unwrap_or(&[])),
            Err(()) => {
                return Err(format!(
                    "`{wrapper}` with an option this does not read: `{}`",
                    word.text
                ));
            }
        }
    }
    Ok(&[])
}

/// Whether a redirection writes a file (`/dev/null` aside).
fn writes_a_file(simple: &Simple) -> bool {
    simple.redirections.iter().any(|r| {
        r.op.writes_file()
            && !matches!(&r.target, Target::File(word) if word.literal && word.text == "/dev/null")
    })
}

/// A program that is not git, a shell or a wrapper: it prompts until an
/// approval covers its shape.
fn ordinary(program: &str, rest: &[Word]) -> Segment {
    match shape_of(program, rest) {
        Ok(shape) => prompt(Some(shape), Why::NotApproved),
        Err(why) => dynamic(why),
    }
}

/// The shape of `program rest…`. An option before the subcommand of a
/// subcommanded program is an `Err`: `npm --prefix install run x` runs `run`,
/// and reading `install` as its subcommand would let an approval of
/// `npm install` cover it.
fn shape_of(program: &str, rest: &[Word]) -> Result<Shape, String> {
    let python = program == "python"
        || program
            .strip_prefix("python")
            .is_some_and(|v| v.chars().all(|c| c.is_ascii_digit() || c == '.'));
    if python {
        let mut subcommand = None;
        if rest.first().is_some_and(|w| w.literal && w.text == "-m") {
            match rest.get(1) {
                Some(module) if module.literal => subcommand = Some(format!("-m {}", module.text)),
                _ => return Err(format!("`{program} -m` with a module that is not literal")),
            }
        }
        // `python3.12` and `python3` are one shape (5983544366 (h)).
        return Ok(Shape {
            program: "python".to_owned(),
            subcommand,
        });
    }
    if !SUBCOMMANDED.contains(&program) {
        return Ok(Shape {
            program: program.to_owned(),
            subcommand: None,
        });
    }
    let mut words = rest.iter();
    let mut next = words.next();
    // `cargo +nightly build`: the toolchain is not an option.
    if program == "cargo" && next.is_some_and(|w| w.literal && w.text.starts_with('+')) {
        next = words.next();
    }
    match next {
        None => Ok(Shape {
            program: program.to_owned(),
            subcommand: None,
        }),
        Some(word) if !word.literal => {
            Err(format!("`{program}` with a subcommand that is not literal"))
        }
        Some(word) if word.text.starts_with('-') => Err(format!(
            "`{program}` with an option before its subcommand: `{}`",
            word.text
        )),
        Some(word) => Ok(Shape {
            program: program.to_owned(),
            subcommand: Some(word.text.clone()),
        }),
    }
}

/// The options with which a git read runs a program its repository's
/// config names -- `diff.external`, a textconv driver, `gpg.program` -- or
/// reads outside the worktree (`--no-index`), so the read is not free
/// (#298 5983544366 (e), "so a free read is a read"; review rounds 1 and 2).
const GIT_RUNS_A_PROGRAM: &[&str] = &["--ext-diff", "--textconv", "--show-signature", "--no-index"];

/// The option prefixes that reach into submodules, whose own configuration
/// and attributes a free read cannot vouch for: not free in any form.
const GIT_SUBMODULES: &[&str] = &["--submodule", "--ignore-submodules", "--recurse-submodules"];

/// The pretty formats git builds in: a `--pretty`/`--format` value that is
/// none of these, and is no `format:`/`tformat:` string, names a `pretty.*`
/// alias from the repository's config.
const GIT_PRETTY: &[&str] = &[
    "oneline",
    "short",
    "medium",
    "full",
    "fuller",
    "reference",
    "email",
    "mboxrd",
    "raw",
];

/// Whether a `--pretty=`/`--format=` word is one a free read may carry:
/// a built-in format or a format string, with no signature placeholder
/// (`%G…` in any case, `%(signature…)`), which runs `gpg.program` (review
/// round 2, finding 1(b)). Any other word is not a format option.
fn plain_format(text: &str) -> bool {
    if text == "--format" {
        // `--format` takes its value with `=`: alone, git refuses it.
        return false;
    }
    let Some(value) = text
        .strip_prefix("--pretty=")
        .or_else(|| text.strip_prefix("--format="))
    else {
        return true;
    };
    let lowered = value.to_ascii_lowercase();
    !lowered.contains("%g")
        && !lowered.contains("signature")
        && (GIT_PRETTY.contains(&value)
            || value.starts_with("format:")
            || value.starts_with("tformat:")
            || value.contains('%'))
}

/// `git status`'s long options a free read may carry, exactly. `status`
/// parses its options with parse-options, which takes any unique
/// abbreviation (`--ignore-sub=none`, `--verb`), so no prefix denylist is
/// sound there (review round 3, finding 3): a long option not listed here
/// or in [`GIT_STATUS_LONG_VALUED`] is not free.
const GIT_STATUS_LONG: &[&str] = &[
    "--",
    "--short",
    "--branch",
    "--show-stash",
    "--porcelain",
    "--long",
    "--untracked-files",
    "--ignored",
    "--null",
    "--column",
    "--no-column",
    "--ahead-behind",
    "--no-ahead-behind",
    "--renames",
    "--no-renames",
    "--find-renames",
];

/// `git status`'s long options a free read may carry with a value: each
/// prefix, any value.
const GIT_STATUS_LONG_VALUED: &[&str] = &[
    "--porcelain=",
    "--untracked-files=",
    "--ignored=",
    "--column=",
    "--find-renames=",
];

/// Whether `text` is a `git status` long option outside
/// [`GIT_STATUS_LONG`] and [`GIT_STATUS_LONG_VALUED`].
fn status_long_unlisted(text: &str) -> bool {
    text.starts_with("--")
        && !GIT_STATUS_LONG.contains(&text)
        && !GIT_STATUS_LONG_VALUED.iter().any(|p| text.starts_with(p))
}

/// The keys `git branch --sort=` may name in a free read, each optionally
/// after `-`: none reads a signature. A `signature` atom verifies one,
/// which runs `gpg.program` (review round 3, finding 2), so every key not
/// listed prompts.
const GIT_BRANCH_SORT_KEYS: &[&str] = &[
    "refname",
    "committerdate",
    "creatordate",
    "authordate",
    "objectname",
    "version:refname",
    "v:refname",
];

/// Whether a `git branch` word `--sort=<key>` names a key in
/// [`GIT_BRANCH_SORT_KEYS`]; `None` when it is no `--sort=` word.
fn branch_sort_listed(text: &str) -> Option<bool> {
    let key = text.strip_prefix("--sort=")?;
    let key = key.strip_prefix('-').unwrap_or(key);
    Some(GIT_BRANCH_SORT_KEYS.contains(&key))
}

/// Whether a `git status` option makes it run a diff, which runs the
/// repository's textconv and diff drivers: `-v`, `--verbose`, and any
/// bundle of short options holding a `v` (review round 2, finding 1(c)).
fn status_shows_a_diff(text: &str) -> bool {
    text == "--verbose" || (text.starts_with('-') && !text.starts_with("--") && text.contains('v'))
}

/// git's free reads (point 4): `status`, `diff`, `log`, `show`, and `branch`
/// in its listing forms. `--output` writes a file, so it prompts; and so do
/// [`GIT_RUNS_A_PROGRAM`]'s options, a submodule option, a format naming a
/// signature or an alias, and `status -v`, which run one or reach past
/// what the read can vouch for.
#[must_use]
pub fn git_is_free(subcommand: &str, args: &[Word]) -> bool {
    let output = args.iter().any(|w| {
        w.text == "--output"
            || w.text.starts_with("--output=")
            || GIT_RUNS_A_PROGRAM.contains(&w.text.as_str())
            || GIT_SUBMODULES.iter().any(|p| w.text.starts_with(p))
            || !plain_format(&w.text)
            || (subcommand == "status" && status_shows_a_diff(&w.text))
            || (subcommand == "status" && status_long_unlisted(&w.text))
    });
    if GIT_READS.contains(&subcommand) {
        !output
    } else if subcommand == "branch" {
        !output && branch_lists(args)
    } else {
        false
    }
}

/// `git branch` only lists: no positional name (which would create a
/// branch) unless `--list` is given, and no option outside the listing set.
fn branch_lists(args: &[Word]) -> bool {
    const LISTING: &[&str] = &[
        "-a",
        "--all",
        "-r",
        "--remotes",
        "-l",
        "--list",
        "-v",
        "-vv",
        "--verbose",
        "--show-current",
        "--no-color",
        "--color",
        "--column",
        "--no-column",
        "-i",
        "--ignore-case",
    ];
    const LISTING_PREFIX: &[&str] = &[
        "--merged",
        "--no-merged",
        "--contains",
        "--no-contains",
        "--points-at",
        "--format=",
        "--color=",
        "--column=",
        "--abbrev=",
    ];
    let listing = args.iter().any(|w| w.text == "-l" || w.text == "--list");
    args.iter().all(|w| {
        let text = w.text.as_str();
        if let Some(listed) = branch_sort_listed(text) {
            listed
        } else if text.starts_with('-') {
            LISTING.contains(&text) || LISTING_PREFIX.iter().any(|p| text.starts_with(p))
        } else {
            listing
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(line: &str) -> Outcome {
        judge(line, &[]).outcome()
    }

    fn refused_by(line: &str) -> Option<String> {
        judge(line, &[]).refused_by().map(str::to_owned)
    }

    fn session(program: &str, subcommand: Option<&str>) -> Approval {
        Approval::Standing {
            scope: Scope::Session,
            shape: Shape {
                program: program.to_owned(),
                subcommand: subcommand.map(str::to_owned),
            },
        }
    }

    #[test]
    fn a_denylisted_program_in_any_segment_is_refused() {
        for line in [
            "sudo id",
            "/usr/bin/sudo id",
            "FOO=1 sudo id",
            "ls; sudo id",
            "ls && sudo id",
            "ls || sudo id",
            "ls | sudo tee x",
            "ls & sudo id",
            "echo a\nsudo id",
            "cat a; ls; dd if=/dev/zero of=x",
        ] {
            assert_eq!(outcome(line), Outcome::Refused, "{line:?}");
        }
        assert_eq!(refused_by("ls; shred x").as_deref(), Some("shred"));
    }

    #[test]
    fn git_is_refused_by_its_subcommand_after_its_global_options() {
        for (line, entry) in [
            ("git push", "git push"),
            ("git push --force origin main", "git push"),
            ("git -C sub push", "git push"),
            ("git -c user.name=x --no-pager push", "git push"),
            ("git --git-dir=.git push", "git push"),
            ("git reset --hard", "git reset --hard"),
            ("git reset --hard HEAD~1", "git reset --hard"),
            ("git -C x reset --hard", "git reset --hard"),
            ("env FOO=1 git push", "git push"),
            ("env FOO=1 git reset --hard", "git reset --hard"),
            ("env FOO=1 nice git push", "git push"),
        ] {
            assert_eq!(refused_by(line).as_deref(), Some(entry), "{line:?}");
        }
        assert_eq!(outcome("git reset HEAD~1"), Outcome::Prompt);
        assert_eq!(outcome("git clean -fd"), Outcome::Prompt);
        assert_eq!(outcome("git reset $FLAG"), Outcome::Prompt);
        assert_eq!(outcome("git --frobnicate push"), Outcome::Prompt);
    }

    #[test]
    fn a_wrapper_does_not_hide_a_denylisted_program() {
        for line in [
            "env sudo id",
            "env -i FOO=1 sudo id",
            "env -u HOME sudo id",
            "timeout 5 sudo id",
            "timeout -s KILL 5 sudo id",
            "nice -n 10 sudo id",
            "nice -5 sudo id",
            "nohup sudo id",
            "time sudo id",
            "command sudo id",
            "builtin sudo id",
            "sh -c 'sudo id'",
            "bash -c 'ls; sudo id'",
            "sh -c \"sh -c 'sudo id'\"",
            "timeout 5 env nice sudo id",
            "xargs sudo id",
            "xargs -0 -n 1 sudo",
            "xargs env sudo id",
            "find . -exec sudo rm {} ;",
            "echo $(sudo id)",
            "X=$(sudo id) ls",
            "(sudo id)",
            "{ sudo id; }",
            "env git push",
            "timeout 9 git reset --hard",
            "npx docker ps",
            "npx -y docker ps",
            "npx -- kubectl get pods",
            "npm exec docker ps",
            "npm exec -- sudo id",
            "npm x crontab -l",
        ] {
            assert_eq!(outcome(line), Outcome::Refused, "{line:?}");
        }
    }

    /// #482: pnpm's runners -- `pnpm dlx`, `pnpm exec`, `pnpx` and `pnx`
    /// (pnpm 11.21.0's alias of `pnpm dlx`), and `pn` (its alias of `pnpm`)
    /// -- run the program their first word names, so it meets the denylist
    /// as it does under `npx` and `npm exec`.
    #[test]
    fn pnpms_runners_do_not_hide_a_denylisted_program() {
        for line in [
            "pnpx docker ps",
            "pnpx -y docker ps",
            "pnpx -- kubectl get pods",
            "pnx sudo id",
            "pnpm dlx docker ps",
            "pnpm dlx -- sudo id",
            "pnpm exec docker ps",
            "pnpm exec -- sudo id",
            "pn dlx crontab -l",
            "pn exec sudo id",
        ] {
            assert_eq!(outcome(line), Outcome::Refused, "{line:?}");
        }
    }

    #[test]
    fn dynamic_syntax_prompts_and_is_never_read_as_allowed() {
        for line in [
            "$CMD x",
            "\"$HOME/bin/tool\"",
            "~/bin/tool",
            "echo `id`",
            "echo $(id)",
            "eval ls",
            "source x.sh",
            ". x.sh",
            "exec ls",
            "(git status)",
            "{ git status; }",
            "if true; then ls; fi",
            "for f in a; do ls; done",
            "while true; do ls; done",
            "xargs ls",
            "xargs grep sudo",
            "find . -exec ls {} ;",
            "sh -c \"$X\"",
            "bash script.sh",
            "env --frobnicate ls",
            "cat <<EOF\nx\nEOF\n",
            "git -c core.pager=less log",
            "GIT_PAGER=x git log",
            "npm --prefix x install",
            "unterminated 'quote",
            "git reset $FLAG",
            "git $SUB",
        ] {
            let judged = judge(line, &[]);
            assert_ne!(
                judged.outcome(),
                Outcome::Run,
                "{line:?} was read as allowed"
            );
            // Approve, for the session, every shape the line shows. A dynamic
            // segment has none, so it still prompts: no standing approval
            // reaches what the gate cannot read.
            let everything: Vec<Approval> = judged
                .segments
                .iter()
                .filter_map(|segment| segment.shape.clone())
                .map(|shape| Approval::Standing {
                    scope: Scope::Session,
                    shape,
                })
                .collect();
            assert_ne!(
                judge(line, &everything).outcome(),
                Outcome::Run,
                "{line:?} ran under standing approvals of its own shapes"
            );
        }
        // ...and only a `once` of the exact line answers a dynamic segment.
        let line = "eval ls";
        let shaped = judge(line, &[session("eval", None)]);
        assert_eq!(
            shaped.outcome(),
            Outcome::Prompt,
            "a standing approval answered `eval`"
        );
        let once = judge(
            line,
            &[Approval::Once {
                line: line.to_owned(),
            }],
        );
        assert_eq!(once.outcome(), Outcome::Run);
    }

    #[test]
    fn gits_reads_are_free_and_nothing_else_is() {
        for line in [
            "git status",
            "git -C sub status --short",
            "git diff HEAD~1",
            "git log --oneline -5",
            "git show HEAD:README.md",
            "git branch",
            "git branch -a -v",
            "git branch --list 'feat/*'",
            "git --no-pager log",
            "git log 2>&1",
            "git status > /dev/null",
            "git status && git diff",
        ] {
            assert_eq!(outcome(line), Outcome::Run, "{line:?}");
        }
        for line in [
            "ls",
            "cat a.txt",
            "git branch new",
            "git branch -d old",
            "git branch -D old",
            "git branch -m a b",
            "git branch --delete x",
            "git diff --output=x.patch",
            "git log > out.txt",
            "git log >> out.txt",
            "git show --output x",
            "git commit -m x",
            "git checkout x",
            "git status; ls",
        ] {
            assert_eq!(outcome(line), Outcome::Prompt, "{line:?}");
        }
    }

    #[test]
    fn a_session_approval_covers_its_shape_and_no_other() {
        let approvals = [session("cat", None), session("npm", Some("install"))];
        assert_eq!(judge("cat a.txt", &approvals).outcome(), Outcome::Run);
        assert_eq!(
            judge("/bin/cat b.txt c.txt", &approvals).outcome(),
            Outcome::Run
        );
        assert_eq!(
            judge("npm install left-pad", &approvals).outcome(),
            Outcome::Run
        );
        for line in [
            "ls",
            "catt a",
            "npm run build",
            "npm",
            "npx install",
            "cat a; ls",
        ] {
            assert_eq!(
                judge(line, &approvals).outcome(),
                Outcome::Prompt,
                "{line:?}"
            );
        }
        // A standing approval never covers a refusal.
        let sudo = [session("sudo", None)];
        assert_eq!(judge("sudo id", &sudo).outcome(), Outcome::Refused);
        // `Once` is the exact line and nothing else.
        let once = [Approval::Once {
            line: "rm -r build".to_owned(),
        }];
        assert_eq!(judge("rm -r build", &once).outcome(), Outcome::Run);
        assert_eq!(judge("rm -r build ", &once).outcome(), Outcome::Prompt);
        assert_eq!(judge("rm -r src", &once).outcome(), Outcome::Prompt);
        // A `Standing` carrying `Once` covers nothing.
        let odd = [Approval::Standing {
            scope: Scope::Once,
            shape: Shape {
                program: "cat".to_owned(),
                subcommand: None,
            },
        }];
        assert_eq!(judge("cat a", &odd).outcome(), Outcome::Prompt);
    }

    #[test]
    fn shapes_are_program_plus_subcommand_and_never_a_guess() {
        let shape = |line: &str| {
            judge(line, &[]).segments[0]
                .shape
                .clone()
                .map(|s| s.to_string())
        };
        assert_eq!(shape("cat a.txt").as_deref(), Some("cat"));
        assert_eq!(shape("/usr/bin/cat a.txt").as_deref(), Some("cat"));
        assert_eq!(shape("npm install x").as_deref(), Some("npm install"));
        assert_eq!(shape("npm run dev").as_deref(), Some("npm run"));
        assert_eq!(
            shape("cargo +nightly build").as_deref(),
            Some("cargo build")
        );
        assert_eq!(
            shape("python -m http.server").as_deref(),
            Some("python -m http.server")
        );
        assert_eq!(
            shape("python3.12 -m pytest -q").as_deref(),
            Some("python -m pytest")
        );
        assert_eq!(shape("python3 script.py").as_deref(), Some("python"));
        assert_eq!(shape("python3 -m pytest"), shape("python3.12 -m pytest -x"));
        assert_eq!(shape("git commit -m x").as_deref(), Some("git commit"));
        assert_eq!(shape("timeout 5 cargo test").as_deref(), Some("cargo test"));
        assert_eq!(shape("npm --prefix x install"), None);
        assert_eq!(shape("npx prisma generate").as_deref(), Some("npx prisma"));
        assert_eq!(shape("npm exec prisma").as_deref(), Some("npm exec"));
        assert_eq!(
            shape("pnpx prisma generate").as_deref(),
            Some("pnpx prisma")
        );
        assert_eq!(shape("pnx prisma generate").as_deref(), Some("pnx prisma"));
        assert_eq!(shape("pnpm dlx prisma").as_deref(), Some("pnpm dlx"));
        assert_eq!(shape("pnpm exec prisma").as_deref(), Some("pnpm exec"));
        assert_eq!(shape("npm $SUB"), None);
    }

    #[test]
    fn a_chain_prompts_per_unapproved_segment() {
        let judged = judge("cat a | grep x && ls", &[session("cat", None)]);
        let verdicts: Vec<_> = judged.segments.iter().map(|s| s.verdict.clone()).collect();
        assert_eq!(
            verdicts,
            vec![
                Verdict::Approved(Scope::Session),
                Verdict::Prompt(Why::NotApproved),
                Verdict::Prompt(Why::NotApproved),
            ]
        );
        assert_eq!(judged.outcome(), Outcome::Prompt);
    }

    #[test]
    fn the_denylist_digest_is_over_its_sorted_canonical_text() {
        let mut entries = DENYLIST.to_vec();
        entries.sort_unstable();
        let text: String = entries.iter().flat_map(|e| [*e, "\n"]).collect();
        let want = sha256_hex(text.as_bytes());
        assert_eq!(denylist_digest(), want);
        assert_eq!(denylist_digest().len(), 64);
        assert!(text.starts_with("at\ncrontab\n"), "sorted: {text:?}");
    }

    #[test]
    fn a_denylist_reads_its_entries_and_refuses_what_is_not_one() {
        assert!(Denylist::new(&["sudo", "git push", "git reset --hard"]).is_ok());
        for bad in [
            "",
            "two words",
            "git",
            "git reset --hard --mixed",
            "git  push",
        ] {
            assert!(
                Denylist::new(&[bad]).is_err(),
                "{bad:?} was read as an entry"
            );
        }
        // The digest is the set's: order and repetition do not change it.
        let a = Denylist::new(&["sudo", "dd", "git push"]).map(|d| d.digest());
        let b = Denylist::new(&["git push", "sudo", "dd", "sudo"]).map(|d| d.digest());
        assert_eq!(a, b);
        assert_eq!(Denylist::standard().digest(), denylist_digest());
    }

    #[test]
    fn a_replay_judges_by_the_denylist_the_call_ran_under() {
        let argv = |words: &[&str]| words.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
        let standard = Denylist::standard();
        let narrow = Denylist::new(&["dd"]).expect("an entry");
        assert_eq!(
            judge_argv(&argv(&["sudo", "id"]), &standard).outcome(),
            Outcome::Refused
        );
        assert_eq!(
            judge_argv(&argv(&["sudo", "id"]), &narrow).outcome(),
            Outcome::Prompt
        );
        assert_eq!(
            judge_argv(&argv(&["dd", "if=x"]), &narrow).outcome(),
            Outcome::Refused
        );
        // The argv a `bash` call keeps is read through its shell.
        let shelled = argv(&["sh", "-c", "ls; sudo id"]);
        assert_eq!(judge_argv(&shelled, &standard).refused_by(), Some("sudo"));
        assert_eq!(
            judge_argv(&argv(&["git", "push"]), &standard).outcome(),
            Outcome::Refused
        );
        assert_eq!(
            judge_argv(&argv(&["git", "status"]), &standard).outcome(),
            Outcome::Run
        );
        // An argv word is literal: `$X` is the text `$X`, not an expansion.
        assert_eq!(
            judge_argv(&argv(&["cat", "$X"]), &standard).segments[0].shape,
            Some(Shape {
                program: "cat".to_owned(),
                subcommand: None
            })
        );
    }

    #[test]
    fn the_projection_renders_segments_and_refuses_what_is_not_a_request() {
        let render = |input: &str| {
            project(input).map(|value| {
                let mut out = String::new();
                json::render(&value, &mut out);
                out
            })
        };
        assert_eq!(
            render(
                r#"{"argv":["sh","-c","git status; npm install; sudo id"],"denylist":["sudo"]}"#
            ),
            Ok(concat!(
                r#"{"outcome":"refused","segments":["#,
                r#"{"shape":"git status","verdict":"free"},"#,
                r#"{"shape":"npm install","verdict":"prompt","why":"not_approved"},"#,
                r#"{"entry":"sudo","verdict":"refused"}]}"#
            )
            .to_owned())
        );
        assert_eq!(
            render(r#"{"argv":["eval","ls"],"denylist":[]}"#),
            Ok(concat!(
                r#"{"outcome":"prompt","segments":[{"reason":"`eval` runs a string or a file as shell","#,
                r#""verdict":"prompt","why":"dynamic"}]}"#
            )
            .to_owned())
        );
        for bad in [
            "not json",
            r#"{"argv":["ls"]}"#,
            r#"{"denylist":[]}"#,
            r#"{"argv":[],"denylist":[]}"#,
            r#"{"argv":["ls",1],"denylist":[]}"#,
            r#"{"argv":["ls"],"denylist":["two words"]}"#,
            r#"{"argv":["ls"],"denylist":[],"extra":1}"#,
        ] {
            assert!(render(bad).is_err(), "{bad:?} was read as a request");
        }
    }

    #[test]
    fn a_loader_prefix_is_dynamic_and_any_other_is_stripped() {
        for line in [
            "LD_PRELOAD=x.so cat a",
            "DYLD_INSERT_LIBRARIES=x cat a",
            "PATH=. ls",
            "GIT_DIR=x git status",
            "NODE_OPTIONS=--require=x node a.js",
            "PYTHONPATH=. python -m pytest",
            "env LD_PRELOAD=x.so cat a",
            "env -i PATH=. ls",
        ] {
            let judged = judge(line, &[session("cat", None), session("ls", None)]);
            assert!(
                judged.segments.iter().all(|s| s.shape.is_none()),
                "{line:?} kept a shape"
            );
            assert_ne!(
                judged.outcome(),
                Outcome::Run,
                "{line:?} ran under a standing approval"
            );
        }
        let judged = judge("FOO=1 LANG=C cat a", &[session("cat", None)]);
        assert_eq!(judged.outcome(), Outcome::Run);
    }

    #[test]
    fn an_unknown_git_subcommand_is_dynamic() {
        for line in ["git lg", "git co main", "git my-push-alias", "git lfs pull"] {
            let judged = judge(line, &[]);
            assert_eq!(judged.segments[0].shape, None, "{line:?} kept a shape");
            assert_eq!(judged.outcome(), Outcome::Prompt, "{line:?}");
        }
        assert_eq!(outcome("git log"), Outcome::Run);
        assert_eq!(outcome("git commit -m x"), Outcome::Prompt);
    }

    #[test]
    fn the_standard_denylist_is_every_entry_of_the_constant() {
        // A bad edit to `DENYLIST` must fail here, never leave a list that
        // refuses nothing.
        assert_eq!(Denylist::standard().entries.len(), DENYLIST.len());
        for entry in DENYLIST {
            assert!(Denylist::new(&[*entry]).is_ok(), "{entry:?}");
        }
    }

    #[test]
    fn a_denylisted_program_is_refused_however_it_is_spelled() {
        for line in [
            "\"sudo\" id",
            "'su''do' id",
            "s\\udo id",
            "./sudo id",
            "/usr/bin/../bin/sudo id",
            "xargs -a list sudo",
            "xargs --arg-file list sudo",
            "time -p sudo id",
            "nice -n 5 timeout 9 sudo id",
        ] {
            assert_eq!(outcome(line), Outcome::Refused, "{line:?}");
        }
        assert_ne!(outcome("coproc sudo id"), Outcome::Run);
    }

    #[test]
    fn the_shapes_ruled_as_built_hold() {
        // (a) `|&` is the pipe it abbreviates: `2>&1 |`.
        let piped = judge("ls |& grep x", &[]);
        let shapes: Vec<_> = piped
            .segments
            .iter()
            .map(|s| s.shape.as_ref().map(ToString::to_string))
            .collect();
        assert_eq!(shapes, vec![Some("ls".to_owned()), Some("grep".to_owned())]);
        assert_eq!(outcome("git log |& cat"), Outcome::Prompt);
        // (d) Assignments alone run nothing.
        assert_eq!(outcome("FOO=1"), Outcome::Run);
        assert_eq!(outcome("FOO=1 BAR=2"), Outcome::Run);
        // (c) Any environment prefix on git, on the line or through `env`.
        for line in [
            "FOO=1 git status",
            "PAGER=cat git log",
            "env FOO=1 git status",
            "env PAGER=cat git log",
            "env FOO=1 nice git status",
            "env FOO=1 timeout 5 git log",
        ] {
            let judged = judge(line, &[]);
            assert_eq!(judged.segments[0].shape, None, "{line:?} kept a shape");
            assert_eq!(judged.outcome(), Outcome::Prompt, "{line:?}");
        }
        // A shell option with a value is read past.
        assert_eq!(
            refused_by("bash -o pipefail -c 'sudo id'").as_deref(),
            Some("sudo")
        );
    }
}
