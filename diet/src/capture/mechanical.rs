//! The mechanical capture lane.
//!
//! Some facts about a session are derivable from the tool calls themselves:
//! the working directory, which files were read and written, which commands
//! ran and what they returned. In the research phase the working directory
//! was, at one point, an **interview question** -- and the model got it wrong
//! in exactly the way one would expect, because it was reconstructing state
//! it had no reason to track. Asking for a mechanical fact spends a fork to
//! get a worse answer than the record already holds.
//!
//! So this lane owns every fact code can derive, and the model is only ever
//! asked about things only the model knows. Three consequences are in the
//! types rather than in a habit:
//!
//! * **The command line is read as the shell reads it.** `cd a; (cd b; ls);
//!   pwd` ends in `a`, because the subshell ran against its own copy of the
//!   state. The lane parses through [`crate::formats::shell`], the one
//!   authorized reader, and carries no pattern of its own for `cd`.
//! * **A derived entry grounds itself by naming the tool call it came from.**
//!   Its id is built from that event's id, its content names it, and it never
//!   passes through the groundedness gate: the gate checks a lane's output
//!   against what the lane was told to work from, and this lane's contract
//!   input is the record row itself. See [`Lane::apply_to`].
//! * **A mechanical fact is not an interview question.** The lint over ask
//!   templates fails on any question about a working directory, a file the
//!   model touched or a command it ran, so the mistake that motivated the
//!   lane cannot be written back into a template.
//!
//! The lane cannot run commands. Whether a `cd` failed is read from the
//! shell's own report in the call's output, or from the call's exit status
//! when the `cd` was the last thing on the line; otherwise a command is taken
//! to have succeeded. Every reading here prefers a miss to a false claim: an
//! unresolvable path is recorded as written and marked unresolved, a word the
//! shell would expand loses the working directory rather than naming a
//! directory that never existed, and a command the lane does not know derives
//! nothing from the shape of its arguments.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use crate::formats::record::Event;
use crate::formats::record::json::Value;
use crate::formats::shell::{self, AndOr, Command, Join, Link, List, Simple, Target, Word};
use crate::object::{Applied, EntryId, ObjectError, Patch, Provenance, WorkingObject};

/// The lane's name, as every entry's provenance spells it.
pub const LANE: &str = "mechanical";

/// The argument a shell tool carries its command line in.
pub const ARG_COMMAND: &str = "command";

/// The argument a path tool carries its file in.
pub const ARG_PATH: &str = "path";

/// The argument any tool may carry the directory it ran in.
pub const ARG_CWD: &str = "cwd";

/// What the lane says when `popd` had nothing to pop and the shell said
/// nothing about it. The lane knows this failure without being told, because
/// it is the one shell failure that follows from state the lane maintains.
const EMPTY_STACK: &str = "popd: directory stack empty";

/// What the lane says when `pushd` had no argument and no other directory.
const NO_OTHER_DIRECTORY: &str = "pushd: no other directory";

// ---------------------------------------------------------------------------
// tools
// ---------------------------------------------------------------------------

/// What a tool does to the state this lane tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ToolKind {
    /// Runs a command line: its `command` argument is read as the shell would.
    Shell,
    /// Reads the file at its `path` argument.
    Read,
    /// Writes the file at its `path` argument.
    Edit,
}

impl ToolKind {
    /// Every kind, so a table can be checked against the vocabulary.
    pub const ALL: &'static [Self] = &[Self::Shell, Self::Read, Self::Edit];

    /// A stable name, for records and fixtures.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Read => "read",
            Self::Edit => "edit",
        }
    }
}

/// The tools the lane knows, under the names harnesses give them.
///
/// A table rather than a match: a name absent from it is an unknown tool,
/// which is recorded as a command run and derives nothing -- never guessed at
/// from the shape of its arguments. A harness that names its tools otherwise
/// adds rows here, in one place, rather than teaching the walk a second
/// vocabulary.
pub const TOOLS: &[(&str, ToolKind)] = &[
    ("bash", ToolKind::Shell),
    ("sh", ToolKind::Shell),
    ("shell", ToolKind::Shell),
    ("run_command", ToolKind::Shell),
    ("execute", ToolKind::Shell),
    ("read_file", ToolKind::Read),
    ("cat_file", ToolKind::Read),
    ("view_file", ToolKind::Read),
    ("edit_file", ToolKind::Edit),
    ("write_file", ToolKind::Edit),
    ("create_file", ToolKind::Edit),
];

/// The kind of the tool called `name`, if the lane knows it.
#[must_use]
pub fn tool_kind(name: &str) -> Option<ToolKind> {
    TOOLS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, kind)| *kind)
}

// ---------------------------------------------------------------------------
// the working directory
// ---------------------------------------------------------------------------

/// The tracked working directory.
///
/// `Home` is a symbolic value and not a path: `cd` with no argument goes
/// there, and the lane cannot say where that is until the shell reports it.
/// Rendering it as a path would be the guess this lane exists to refuse.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Cwd {
    /// Not established yet, or lost to a `cd` whose target the shell would
    /// have expanded.
    #[default]
    Unknown,
    /// The home directory, path not reported.
    Home,
    /// An absolute path.
    Path(PathBuf),
}

/// Which of the three the working directory is, as a closed vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CwdKind {
    /// [`Cwd::Unknown`].
    Unknown,
    /// [`Cwd::Home`].
    Home,
    /// [`Cwd::Path`].
    Path,
}

impl CwdKind {
    /// Every kind.
    pub const ALL: &'static [Self] = &[Self::Unknown, Self::Home, Self::Path];

    /// A stable name, for fixtures.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Home => "home",
            Self::Path => "path",
        }
    }
}

impl Cwd {
    /// Which kind this is.
    #[must_use]
    pub fn kind(&self) -> CwdKind {
        match self {
            Self::Unknown => CwdKind::Unknown,
            Self::Home => CwdKind::Home,
            Self::Path(_) => CwdKind::Path,
        }
    }

    /// The path, when the working directory is one.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Path(path) => Some(path),
            Self::Unknown | Self::Home => None,
        }
    }

    /// This working directory, as the record's value space.
    #[must_use]
    pub fn value(&self) -> Value {
        let mut members = BTreeMap::new();
        members.insert(
            KEY_KIND.to_owned(),
            Value::String(self.kind().tag().to_owned()),
        );
        if let Some(path) = self.path() {
            members.insert(
                KEY_PATH.to_owned(),
                Value::String(path.display().to_string()),
            );
        }
        Value::Object(members)
    }
}

impl fmt::Display for Cwd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown => f.write_str("unknown"),
            Self::Home => f.write_str("the home directory, path not reported"),
            Self::Path(path) => write!(f, "{}", path.display()),
        }
    }
}

// ---------------------------------------------------------------------------
// files, commands, failures
// ---------------------------------------------------------------------------

/// What happened to a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TouchKind {
    /// Read, by a read tool or a shell reader.
    Read,
    /// Written, by an edit tool, a redirection or a shell writer.
    Written,
    /// Deleted, by `rm`, `rmdir`, or as the source of a `mv`.
    Deleted,
}

impl TouchKind {
    /// Every kind.
    pub const ALL: &'static [Self] = &[Self::Read, Self::Written, Self::Deleted];

    /// A stable name, for entries and fixtures.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Written => "written",
            Self::Deleted => "deleted",
        }
    }

    /// The verb an entry's content uses.
    #[must_use]
    pub fn verb(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Written => "wrote",
            Self::Deleted => "deleted",
        }
    }
}

/// The last thing that happened to a file, and which tool call did it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Touch {
    /// What happened.
    pub kind: TouchKind,
    /// The turn it happened in.
    pub turn: u32,
    /// The tool-call event that did it.
    pub event: String,
    /// Whether the path was resolved against a known working directory. A
    /// relative path touched while the cwd was unknown is kept as written,
    /// and this says so rather than letting it read as a path from the root.
    pub resolved: bool,
}

/// One tool call, as a command that ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRun {
    /// The tool-call event.
    pub event: String,
    /// The turn it ran in.
    pub turn: u32,
    /// The command line for a shell tool; the tool's name otherwise.
    pub line: String,
    /// The exit status, when the record kept it.
    pub exit: Option<i64>,
}

/// A `cd`, `pushd` or `popd` the lane knows did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The tool-call event.
    pub event: String,
    /// The command as written.
    pub command: String,
    /// The shell's own report, or the lane's reason when the shell said
    /// nothing and the lane could tell anyway.
    pub report: String,
}

// ---------------------------------------------------------------------------
// shell builtins the lane models
// ---------------------------------------------------------------------------

/// The three builtins that move the working directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Builtin {
    /// `cd`
    Cd,
    /// `pushd`
    Pushd,
    /// `popd`
    Popd,
}

impl Builtin {
    /// Every builtin, so a report reader cannot forget one.
    pub const ALL: &'static [Self] = &[Self::Cd, Self::Pushd, Self::Popd];

    /// The word it is written as.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Cd => "cd",
            Self::Pushd => "pushd",
            Self::Popd => "popd",
        }
    }

    /// The builtin `word` names, if it names one.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|found| found.tag() == word)
    }
}

// ---------------------------------------------------------------------------
// the file-touching commands
// ---------------------------------------------------------------------------

/// What a command does to the file operands it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Operands {
    /// Every file operand is read.
    Read,
    /// Every file operand is written.
    Write,
    /// Every file operand is deleted.
    Delete,
    /// The last operand is written and the ones before it are deleted: `mv`
    /// moves a file, so its source stops existing under the name it had.
    Move,
    /// The last operand is written and the ones before it are left alone.
    /// A copy's source survives untouched under its own name, and this lane
    /// records what the turn did to a file rather than every descriptor a
    /// syscall opened.
    Copy,
}

impl Operands {
    /// Every disposition.
    pub const ALL: &'static [Self] = &[
        Self::Read,
        Self::Write,
        Self::Delete,
        Self::Move,
        Self::Copy,
    ];

    /// A stable name, for fixtures.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Delete => "delete",
            Self::Move => "move",
            Self::Copy => "copy",
        }
    }
}

/// One shell command whose operands name files, and how to tell which.
///
/// A table rather than a chain of conditions, for the reason the tool table
/// is one: what the lane claims to understand is written down where it can be
/// read, and a command absent from it touches nothing rather than being
/// guessed at.
#[derive(Debug, Clone, Copy)]
pub struct FileCommand {
    /// The command word, as a basename.
    pub word: &'static str,
    /// What it does to the operands that are files.
    pub operands: Operands,
    /// Flags that consume the word after them, so that word is not a file.
    pub value_flags: &'static [&'static str],
    /// Whether the first operand is the command's pattern or script rather
    /// than a file.
    pub first_operand_is_pattern: bool,
    /// Flags that supply that pattern, so the first operand is a file after
    /// all.
    pub pattern_flags: &'static [&'static str],
    /// A flag without which this command names no file this lane will claim.
    /// `sed -n '80,160p' f` prints part of a file; `sed 's/a/b/' f` is an edit
    /// whose result went to standard output, and calling both a read would
    /// make the read a word that meant nothing.
    pub required_flag: Option<&'static str>,
}

impl FileCommand {
    /// A command whose operands are all files.
    const fn plain(
        word: &'static str,
        operands: Operands,
        value_flags: &'static [&'static str],
    ) -> Self {
        Self {
            word,
            operands,
            value_flags,
            first_operand_is_pattern: false,
            pattern_flags: &[],
            required_flag: None,
        }
    }
}

/// The shell commands the lane reads a file operand out of.
pub const FILE_COMMANDS: &[FileCommand] = &[
    FileCommand::plain("cat", Operands::Read, &[]),
    FileCommand::plain("head", Operands::Read, &["-n", "-c"]),
    FileCommand::plain("tail", Operands::Read, &["-n", "-c"]),
    FileCommand::plain("less", Operands::Read, &[]),
    FileCommand::plain("more", Operands::Read, &[]),
    FileCommand::plain("wc", Operands::Read, &[]),
    FileCommand {
        word: "sed",
        operands: Operands::Read,
        value_flags: &["-e", "-f", "-i"],
        first_operand_is_pattern: true,
        pattern_flags: &["-e", "-f"],
        required_flag: Some("-n"),
    },
    FileCommand {
        word: "grep",
        operands: Operands::Read,
        value_flags: &["-A", "-B", "-C", "-m", "-e", "-f"],
        first_operand_is_pattern: true,
        pattern_flags: &["-e", "-f"],
        required_flag: None,
    },
    FileCommand::plain("tee", Operands::Write, &[]),
    FileCommand::plain("touch", Operands::Write, &["-d", "-r", "-t"]),
    FileCommand::plain("mkdir", Operands::Write, &["-m"]),
    FileCommand::plain("cp", Operands::Copy, &[]),
    FileCommand::plain("mv", Operands::Move, &[]),
    FileCommand::plain("rm", Operands::Delete, &[]),
    FileCommand::plain("rmdir", Operands::Delete, &[]),
];

/// The command `word` names, if the lane reads files out of it.
#[must_use]
pub fn file_command(word: &str) -> Option<&'static FileCommand> {
    FILE_COMMANDS.iter().find(|known| known.word == word)
}

/// Roots whose contents are devices rather than files a turn touched.
///
/// `2>/dev/null` is a way of saying "discard this", not a file the session
/// wrote, and an entry claiming the turn wrote `/dev/null` would be a fact
/// about the shell's plumbing offered as a fact about the work.
pub const NOT_FILES: &[&str] = &["/dev/", "/proc/", "/sys/"];

/// The word a command uses for standard input or output rather than a file.
const DASH: &str = "-";

// ---------------------------------------------------------------------------
// stated-fact holes
// ---------------------------------------------------------------------------

/// The facts a template may state instead of asking for.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Facts {
    /// The working directory, when it is a path. `None` while it is unknown
    /// or symbolic: a template that cannot state it drops the line.
    pub cwd: Option<String>,
    /// The file most recently written.
    pub last_edited: Option<String>,
    /// The command line most recently run.
    pub last_command: Option<String>,
}

/// A hole a template may carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Hole {
    /// `{cwd}`
    Cwd,
    /// `{last_edited}`
    LastEdited,
    /// `{last_command}`
    LastCommand,
}

impl Hole {
    /// Every hole, so a test can require a fixture for each.
    pub const ALL: &'static [Self] = &[Self::Cwd, Self::LastEdited, Self::LastCommand];

    /// The name between the braces.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Cwd => "cwd",
            Self::LastEdited => "last_edited",
            Self::LastCommand => "last_command",
        }
    }

    /// The hole `name` spells, if it is one of this lane's.
    #[must_use]
    pub fn from_tag(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|hole| hole.tag() == name)
    }
}

impl Facts {
    /// The fact `hole` asks for, when it is known.
    #[must_use]
    pub fn get(&self, hole: Hole) -> Option<&str> {
        match hole {
            Hole::Cwd => self.cwd.as_deref(),
            Hole::LastEdited => self.last_edited.as_deref(),
            Hole::LastCommand => self.last_command.as_deref(),
        }
    }
}

/// Fill a template's mechanical holes from `facts`.
///
/// A line containing a hole whose fact is unknown is dropped whole, so that
/// "You are in {cwd}." vanishes rather than reading "You are in .". The
/// half-filled sentence is worse than the missing one: it reads as a stated
/// fact, and the fact it states is that the session is nowhere.
///
/// A hole this lane does not own -- `{intent}`, filled by the router -- is
/// left as written, so one pass over a template does not consume another
/// lane's holes.
#[must_use]
pub fn fill(template: &str, facts: &Facts) -> String {
    let mut out = String::new();
    for piece in template.split_inclusive('\n') {
        let (body, ending) = piece
            .strip_suffix('\n')
            .map_or((piece, ""), |body| (body, "\n"));
        if let Some(filled) = fill_line(body, facts) {
            out.push_str(&filled);
            out.push_str(ending);
        }
    }
    out
}

/// One line, filled, or `None` when a hole it carries has no fact.
fn fill_line(line: &str, facts: &Facts) -> Option<String> {
    let mut out = String::new();
    let mut rest = line;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}').map(|offset| open + offset) else {
            break;
        };
        out.push_str(&rest[..open]);
        match Hole::from_tag(&rest[open + 1..close]) {
            Some(hole) => out.push_str(facts.get(hole)?),
            None => out.push_str(&rest[open..=close]),
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    Some(out)
}

// ---------------------------------------------------------------------------
// the lint
// ---------------------------------------------------------------------------

/// Phrases that make a question a question about a mechanical fact.
///
/// Compared against the lowercased line, longest-standing first: a table, not
/// a pattern, so what is forbidden is written down where it can be read and
/// extended. The list is what the mechanical lane derives, which is exactly
/// the list of things a fork must not be spent on.
pub const MECHANICAL_NOUNS: &[&str] = &[
    "working directory",
    "current directory",
    "cwd",
    "which directory",
    "what directory",
    "files did you",
    "files you edited",
    "files you changed",
    "files you read",
    "files you touched",
    "commands did you run",
    "command did you run",
    "exit code",
];

/// How a question opens, when it does not end in a question mark.
///
/// A statement that mentions the working directory is a template stating a
/// fact, which is the whole point of the lane; only an interrogative is an
/// offence.
pub const QUESTION_OPENERS: &[&str] = &["what ", "which ", "where ", "how many "];

/// The mechanical noun `line` asks about, if it is a question about one.
#[must_use]
pub fn asks_for_a_mechanical_fact(line: &str) -> Option<&'static str> {
    let lowered = line.trim().to_lowercase();
    let interrogative = lowered.ends_with('?')
        || QUESTION_OPENERS
            .iter()
            .any(|opener| lowered.starts_with(opener));
    if !interrogative {
        return None;
    }
    MECHANICAL_NOUNS
        .iter()
        .copied()
        .find(|noun| lowered.contains(noun))
}

/// One line of a template that asks for what the lane already knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offence {
    /// The template's file name.
    pub template: String,
    /// The line, from 1.
    pub line: usize,
    /// The noun that made it an offence.
    pub noun: &'static str,
    /// The line as written.
    pub text: String,
}

/// Every offending line of one template.
#[must_use]
pub fn lint_template(template: &str, text: &str) -> Vec<Offence> {
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            asks_for_a_mechanical_fact(line).map(|noun| Offence {
                template: template.to_owned(),
                line: index + 1,
                noun,
                text: line.to_owned(),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// the value space the corpus pins
// ---------------------------------------------------------------------------

const KEY_CWD: &str = "cwd";
const KEY_FILES: &str = "files";
const KEY_FAILURES: &str = "failures";
const KEY_KIND: &str = "kind";
const KEY_PATH: &str = "path";
const KEY_EVENT: &str = "event";
const KEY_TURN: &str = "turn";
const KEY_RESOLVED: &str = "resolved";
const KEY_COMMAND: &str = "command";
const KEY_REPORT: &str = "report";

// ---------------------------------------------------------------------------
// the lane
// ---------------------------------------------------------------------------

/// The lane: the derived state, and the log it emits entries from.
#[derive(Debug, Clone, Default)]
pub struct Lane {
    state: State,
    files: BTreeMap<PathBuf, Touch>,
    commands: Vec<CommandRun>,
    failures: Vec<Failure>,
    facts: Vec<Fact>,
}

/// The shell's state: what a subshell copies and a group shares.
#[derive(Debug, Clone, Default)]
struct State {
    cwd: Cwd,
    stack: Vec<Cwd>,
}

/// One fact derived from one tool call, in the order it was derived.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Fact {
    turn: u32,
    event: String,
    derived: Derived,
}

/// What a fact says.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Derived {
    /// The working directory after the call, recorded because the call moved
    /// it. A call that left it alone establishes nothing new.
    Cwd(Cwd),
    /// A file touched.
    Touch { path: PathBuf, touch: Touch },
    /// A command run. `parsed` is false when the line was outside the shell
    /// subset, so the entry can say the lane derived nothing from it.
    Ran {
        line: String,
        exit: Option<i64>,
        parsed: bool,
    },
}

/// One tool call, while it is being read.
#[derive(Debug)]
struct Call<'a> {
    event: String,
    turn: u32,
    exit: Option<i64>,
    reports: Vec<Report>,
    /// The last simple command the line would run in the live shell, if the
    /// exit status belongs to one. The status of a backgrounded list, a
    /// pipeline or a subshell is not the status of a `cd` inside it.
    trailing: Option<&'a Simple>,
}

/// A failure the shell reported in its own words.
#[derive(Debug, Clone)]
struct Report {
    builtin: Builtin,
    argument: Option<String>,
    text: String,
    used: bool,
}

impl Call<'_> {
    /// The reason this builtin did not happen, if the lane has one.
    ///
    /// The shell's own report first, because it names the argument and so
    /// says WHICH `cd` failed; the exit status only when this command was the
    /// last thing the line ran, because otherwise the status belongs to
    /// something else -- `cd diet && cargo test` exiting 101 is a failing
    /// test, not a directory that is not there.
    fn refusal(
        &mut self,
        builtin: Builtin,
        argument: Option<&str>,
        simple: &Simple,
    ) -> Option<String> {
        let found = self.reports.iter().position(|report| {
            !report.used
                && report.builtin == builtin
                && report
                    .argument
                    .as_deref()
                    .is_none_or(|named| Some(named) == argument)
        });
        if let Some(index) = found {
            self.reports[index].used = true;
            return Some(self.reports[index].text.clone());
        }
        let code = self.exit?;
        if code != 0 && self.trailing.is_some_and(|last| std::ptr::eq(last, simple)) {
            return Some(format!("exit {code}"));
        }
        None
    }
}

impl Lane {
    /// Consume one event of the record, in order.
    ///
    /// Only a tool call moves the state. Every other kind is listed so that a
    /// kind added to the record has to be considered here rather than
    /// silently ignored.
    pub fn observe(&mut self, event: &Event) {
        match event {
            Event::ToolCall {
                id,
                at_turn,
                tool,
                args,
                exit,
                output,
            } => self.tool_call(id, *at_turn, tool, args.as_ref(), *exit, output.as_deref()),
            Event::Start { .. }
            | Event::Turn { .. }
            | Event::Request { .. }
            | Event::Response { .. }
            | Event::Fork { .. }
            | Event::Capture { .. }
            | Event::Seam { .. }
            | Event::Rejected { .. }
            | Event::Claim { .. }
            | Event::Summary { .. } => {}
        }
    }

    /// The tracked working directory.
    #[must_use]
    pub fn cwd(&self) -> &Cwd {
        &self.state.cwd
    }

    /// The directory stack, bottom first.
    #[must_use]
    pub fn stack(&self) -> &[Cwd] {
        &self.state.stack
    }

    /// Every file touched, with the last thing that happened to it.
    #[must_use]
    pub fn files(&self) -> &BTreeMap<PathBuf, Touch> {
        &self.files
    }

    /// Every command run, in order.
    #[must_use]
    pub fn commands(&self) -> &[CommandRun] {
        &self.commands
    }

    /// Every `cd`, `pushd` or `popd` the lane knows did not happen.
    #[must_use]
    pub fn failures(&self) -> &[Failure] {
        &self.failures
    }

    /// The facts a template may state instead of asking for.
    #[must_use]
    pub fn facts(&self) -> Facts {
        let last_edited = self
            .facts
            .iter()
            .rev()
            .find_map(|fact| match &fact.derived {
                Derived::Touch { path, touch } if touch.kind == TouchKind::Written => {
                    Some(path.display().to_string())
                }
                _ => None,
            });
        Facts {
            cwd: self.state.cwd.path().map(|path| path.display().to_string()),
            last_edited,
            last_command: self.commands.last().map(|run| run.line.clone()),
        }
    }

    /// The entries derived in `turn`, as patches.
    ///
    /// Ids are built from the tool-call event's id -- `<event>/cwd`,
    /// `<event>/read:<path>`, `<event>/ran` -- and never minted from a
    /// counter, so the same record read twice produces the same object.
    /// Each content names the event it was derived from, which is what makes
    /// the entry auditable without a second lookup.
    ///
    /// # Panics
    ///
    /// Never in practice: every id is a non-empty string built from a
    /// non-empty event id, which is what [`EntryId::new`] refuses.
    #[must_use]
    pub fn patches(&self, turn: u32) -> Vec<Patch> {
        let mut patches = Vec::new();
        for fact in self.facts.iter().filter(|fact| fact.turn == turn) {
            let (suffix, content) = match &fact.derived {
                Derived::Cwd(cwd) => (KEY_CWD.to_owned(), cwd_content(&fact.event, cwd)),
                Derived::Touch { path, touch } => (
                    format!("{}:{}", touch.kind.tag(), path.display()),
                    touch_content(&fact.event, path, touch),
                ),
                Derived::Ran { line, exit, parsed } => (
                    RAN.to_owned(),
                    ran_content(&fact.event, line, *exit, *parsed),
                ),
            };
            let id = EntryId::new(&format!("{}/{suffix}", fact.event))
                .expect("an event id is not blank, so neither is an id built from it");
            let index = u32::try_from(patches.len()).unwrap_or(u32::MAX);
            patches.push(Patch::Add {
                id,
                content,
                provenance: Provenance {
                    turn,
                    lane: LANE.to_owned(),
                    fork: None,
                    index,
                },
            });
        }
        patches
    }

    /// Apply the entries derived in `turn` to `object`.
    ///
    /// Not through the groundedness gate, and there is no [`LaneReport`] to
    /// hand back. The gate checks a lane's output against the input the lane
    /// was told to work from; this lane's input is the record row itself, and
    /// every entry names that row. A check that looked for the entry's text
    /// in the row would reject "the working directory is /work/a" because the
    /// row says `cd a` -- a derivation is not a quotation, and rejecting one
    /// for failing to be the other would drop exactly the facts that are free
    /// and exact.
    ///
    /// [`LaneReport`]: crate::capture::grounded::LaneReport
    ///
    /// # Errors
    ///
    /// Whatever [`WorkingObject::apply_turn`] refuses.
    pub fn apply_to(
        &self,
        object: &mut WorkingObject,
        turn: u32,
    ) -> Result<Vec<Applied>, ObjectError> {
        let patches = self.patches(turn);
        object.apply_turn(&patches)
    }

    /// The outcome the conformance corpus pins: the working directory, the
    /// files map and the failures list, as the record's value space.
    #[must_use]
    pub fn value(&self) -> Value {
        let files = self
            .files
            .iter()
            .map(|(path, touch)| {
                (
                    path.display().to_string(),
                    Value::Object(BTreeMap::from([
                        (KEY_EVENT.to_owned(), Value::String(touch.event.clone())),
                        (
                            KEY_KIND.to_owned(),
                            Value::String(touch.kind.tag().to_owned()),
                        ),
                        (KEY_RESOLVED.to_owned(), Value::Boolean(touch.resolved)),
                        (KEY_TURN.to_owned(), Value::Integer(i64::from(touch.turn))),
                    ])),
                )
            })
            .collect();
        let failures = self
            .failures
            .iter()
            .map(|failure| {
                Value::Object(BTreeMap::from([
                    (
                        KEY_COMMAND.to_owned(),
                        Value::String(failure.command.clone()),
                    ),
                    (KEY_EVENT.to_owned(), Value::String(failure.event.clone())),
                    (KEY_REPORT.to_owned(), Value::String(failure.report.clone())),
                ]))
            })
            .collect();
        Value::Object(BTreeMap::from([
            (KEY_CWD.to_owned(), self.state.cwd.value()),
            (KEY_FAILURES.to_owned(), Value::Array(failures)),
            (KEY_FILES.to_owned(), Value::Object(files)),
        ]))
    }

    // -- reading one call ---------------------------------------------------

    fn tool_call(
        &mut self,
        id: &str,
        turn: u32,
        tool: &str,
        args: Option<&BTreeMap<String, Value>>,
        exit: Option<i64>,
        output: Option<&str>,
    ) {
        let before = self.state.cwd.clone();
        if let Some(reported) = string_arg(args, ARG_CWD)
            && let Some(path) = absolute(reported)
        {
            self.state.cwd = Cwd::Path(path);
        }

        match tool_kind(tool) {
            Some(ToolKind::Shell) => self.shell_call(id, turn, args, exit, output),
            // A path tool derives the file it touched, and that is the whole
            // of what it did. It is not recorded as a command run: `commands`
            // is what the session ran, and a reader who wants "what did this
            // turn do to that file" is answered by the touch, exactly.
            Some(kind @ (ToolKind::Read | ToolKind::Edit)) => {
                if let Some(path) = string_arg(args, ARG_PATH) {
                    let touch = match kind {
                        ToolKind::Read => TouchKind::Read,
                        ToolKind::Edit | ToolKind::Shell => TouchKind::Written,
                    };
                    let cwd = self.state.cwd.clone();
                    self.record_touch(id, turn, path, touch, &cwd);
                }
            }
            // A tool the table does not name derives nothing. Its arguments
            // are not read for a path, because a `path` key in an unknown
            // tool's arguments is a guess about a contract nobody wrote down.
            None => self.record_run(id, turn, tool, exit, true),
        }

        if self.state.cwd != before {
            let cwd = self.state.cwd.clone();
            self.facts.push(Fact {
                turn,
                event: id.to_owned(),
                derived: Derived::Cwd(cwd),
            });
        }
    }

    fn shell_call(
        &mut self,
        id: &str,
        turn: u32,
        args: Option<&BTreeMap<String, Value>>,
        exit: Option<i64>,
        output: Option<&str>,
    ) {
        let Some(line) = string_arg(args, ARG_COMMAND) else {
            // A shell tool call the record kept no command line for is still
            // a call that happened; saying so beats saying nothing.
            self.record_run(id, turn, SHELL_WITHOUT_A_LINE, exit, true);
            return;
        };
        let Ok(list) = shell::parse(line) else {
            self.record_run(id, turn, line, exit, false);
            return;
        };
        self.record_run(id, turn, line, exit, true);
        let mut call = Call {
            event: id.to_owned(),
            turn,
            exit,
            reports: reports_in(output.unwrap_or_default()),
            trailing: trailing_simple(&list),
        };
        let mut state = std::mem::take(&mut self.state);
        self.run_list(&list, &mut state, &mut call);
        self.state = state;
        if trailing_simple(&list)
            .and_then(Simple::command)
            .filter(|word| word.literal)
            .and_then(|word| basename(&word.text))
            == Some(PWD)
            && let Some(path) = reported_path(output.unwrap_or_default())
        {
            self.state.cwd = Cwd::Path(path);
        }
    }

    // -- walking the command line -------------------------------------------

    fn run_list<'a>(&mut self, list: &'a List, state: &mut State, call: &mut Call<'a>) {
        for item in &list.items {
            if item.background {
                // `&` puts the list in a subshell of its own; nothing it does
                // to the working directory reaches the shell that spawned it.
                let mut copy = state.clone();
                self.run_and_or(item, &mut copy, call);
            } else {
                self.run_and_or(item, state, call);
            }
        }
    }

    fn run_and_or<'a>(&mut self, item: &'a AndOr, state: &mut State, call: &mut Call<'a>) {
        // Whether the lane KNOWS the pipeline before this one failed. It only
        // ever knows that of a `cd`, `pushd` or `popd` the shell reported on,
        // so everything else is taken to have succeeded and `||` after it is
        // skipped -- a miss, not a false claim.
        let mut failed = false;
        for link in &item.chain {
            let skip = match link.join {
                None => false,
                Some(Join::And) => failed,
                Some(Join::Or) => !failed,
            };
            if skip {
                continue;
            }
            failed = self.run_pipeline(link, state, call);
        }
    }

    fn run_pipeline<'a>(&mut self, link: &'a Link, state: &mut State, call: &mut Call<'a>) -> bool {
        if link.pipeline.len() > 1 {
            for command in &link.pipeline {
                let mut copy = state.clone();
                self.run_command(command, &mut copy, call);
            }
            return false;
        }
        match link.pipeline.first() {
            Some(command) => self.run_command(command, state, call),
            None => false,
        }
    }

    fn run_command<'a>(
        &mut self,
        command: &'a Command,
        state: &mut State,
        call: &mut Call<'a>,
    ) -> bool {
        match command {
            Command::Subshell(inner) => {
                let mut copy = state.clone();
                self.run_list(inner, &mut copy, call);
                false
            }
            Command::Group(inner) => {
                self.run_list(inner, state, call);
                false
            }
            Command::Simple(simple) => self.run_simple(simple, state, call),
        }
    }

    fn run_simple<'a>(
        &mut self,
        simple: &'a Simple,
        state: &mut State,
        call: &mut Call<'a>,
    ) -> bool {
        self.run_substitutions(simple, state, call);
        self.run_redirections(simple, state, call);
        let Some(word) = simple.command() else {
            return false;
        };
        if !word.literal {
            return false;
        }
        let Some(name) = basename(&word.text) else {
            return false;
        };
        if let Some(builtin) = Builtin::from_word(name) {
            return self.run_builtin(builtin, simple, state, call);
        }
        if let Some(known) = file_command(name) {
            self.run_file_command(known, simple, state, call);
        }
        false
    }

    /// `$( … )` runs against a copy of the state, exactly as `( … )` does.
    fn run_substitutions(&mut self, simple: &Simple, state: &State, call: &mut Call<'_>) {
        // A literal word is one the shell passes through whole -- it was
        // single-quoted, and `$(` inside it is three characters, not a
        // command. `Word.text` is the text after unquoting, so it looks
        // exactly like a substitution to a scanner that does not ask. The
        // grammar already decided; asking it is the difference between
        // recording a file that was read and inventing one that was not.
        fn unquoted(word: &Word) -> Option<&str> {
            (!word.literal).then_some(word.text.as_str())
        }
        let mut texts: Vec<&str> = simple.words.iter().filter_map(unquoted).collect();
        texts.extend(
            simple
                .assignments
                .iter()
                .filter_map(|assignment| assignment.value.as_ref())
                .filter_map(unquoted),
        );
        texts.extend(simple.redirections.iter().filter_map(
            |redirection| match &redirection.target {
                Target::File(word) => unquoted(word),
                Target::Descriptor(_) | Target::Heredoc { .. } => None,
            },
        ));
        for text in texts {
            for inner in substitutions(text) {
                let Ok(list) = shell::parse(inner) else {
                    continue;
                };
                let mut copy = state.clone();
                let mut nested = Call {
                    event: call.event.clone(),
                    turn: call.turn,
                    // A substitution's own status is not the call's status,
                    // and the shell reports on it under the same names, so
                    // neither the exit code nor an unconsumed report may be
                    // charged to a command inside one.
                    exit: None,
                    reports: Vec::new(),
                    trailing: None,
                };
                self.run_list(&list, &mut copy, &mut nested);
            }
        }
    }

    fn run_redirections(&mut self, simple: &Simple, state: &State, call: &Call<'_>) {
        for redirection in &simple.redirections {
            let Target::File(word) = &redirection.target else {
                continue;
            };
            let kind = if redirection.op.writes_file() {
                TouchKind::Written
            } else if redirection.op.reads_file() {
                TouchKind::Read
            } else {
                continue;
            };
            if !word.literal {
                continue;
            }
            let event = call.event.clone();
            let cwd = state.cwd.clone();
            self.record_touch(&event, call.turn, &word.text, kind, &cwd);
        }
    }

    fn run_builtin(
        &mut self,
        builtin: Builtin,
        simple: &Simple,
        state: &mut State,
        call: &mut Call<'_>,
    ) -> bool {
        let argument = simple.operands().first();
        let written = argument.map(|word| word.text.clone());
        let refused = call.refusal(builtin, written.as_deref(), simple);
        if let Some(report) = refused {
            // A builtin the lane knows did not happen leaves the state where
            // it was. Applying it anyway is the failure this branch exists
            // for: the cwd then tracks a directory the shell never entered,
            // and every relative path after it resolves against a lie.
            self.record_failure(call, simple, report);
            return true;
        }
        match builtin {
            Builtin::Cd => {
                state.cwd = target_of(argument, state);
                false
            }
            Builtin::Pushd => self.push_directory(argument, simple, state, call),
            Builtin::Popd => self.pop_directory(simple, state, call),
        }
    }

    fn push_directory(
        &mut self,
        argument: Option<&Word>,
        simple: &Simple,
        state: &mut State,
        call: &Call<'_>,
    ) -> bool {
        if argument.is_some() {
            state.stack.push(state.cwd.clone());
            state.cwd = target_of(argument, state);
            return false;
        }
        // `pushd` with no argument exchanges the top two directories.
        if let Some(other) = state.stack.pop() {
            let here = std::mem::replace(&mut state.cwd, other);
            state.stack.push(here);
            return false;
        }
        self.record_failure(call, simple, NO_OTHER_DIRECTORY.to_owned());
        true
    }

    fn pop_directory(&mut self, simple: &Simple, state: &mut State, call: &Call<'_>) -> bool {
        if let Some(previous) = state.stack.pop() {
            state.cwd = previous;
            return false;
        }
        // The one shell failure the lane knows without being told: the stack
        // it maintains is empty, so there was nowhere to go back to. Passing
        // over it silently would leave the cwd right and the record wrong.
        self.record_failure(call, simple, EMPTY_STACK.to_owned());
        true
    }

    fn run_file_command(
        &mut self,
        known: &FileCommand,
        simple: &Simple,
        state: &State,
        call: &Call<'_>,
    ) {
        if let Some(required) = known.required_flag
            && !simple
                .operands()
                .iter()
                .any(|word| word.literal && word.text == required)
        {
            return;
        }
        let operands = file_operands(known, simple);
        let event = call.event.clone();
        let cwd = state.cwd.clone();
        match known.operands {
            Operands::Read | Operands::Write | Operands::Delete => {
                let kind = match known.operands {
                    Operands::Read => TouchKind::Read,
                    Operands::Write => TouchKind::Written,
                    Operands::Delete | Operands::Move | Operands::Copy => TouchKind::Deleted,
                };
                for operand in operands {
                    self.record_touch(&event, call.turn, operand, kind, &cwd);
                }
            }
            Operands::Move | Operands::Copy => {
                let Some((destination, sources)) = operands.split_last() else {
                    return;
                };
                if known.operands == Operands::Move {
                    for source in sources {
                        self.record_touch(&event, call.turn, source, TouchKind::Deleted, &cwd);
                    }
                }
                self.record_touch(&event, call.turn, destination, TouchKind::Written, &cwd);
            }
        }
    }

    // -- bookkeeping ---------------------------------------------------------

    fn record_run(&mut self, event: &str, turn: u32, line: &str, exit: Option<i64>, parsed: bool) {
        self.commands.push(CommandRun {
            event: event.to_owned(),
            turn,
            line: line.to_owned(),
            exit,
        });
        self.facts.push(Fact {
            turn,
            event: event.to_owned(),
            derived: Derived::Ran {
                line: line.to_owned(),
                exit,
                parsed,
            },
        });
    }

    fn record_failure(&mut self, call: &Call<'_>, simple: &Simple, report: String) {
        self.failures.push(Failure {
            event: call.event.clone(),
            command: rendered(simple),
            report,
        });
    }

    fn record_touch(&mut self, event: &str, turn: u32, raw: &str, kind: TouchKind, cwd: &Cwd) {
        if raw == DASH || NOT_FILES.iter().any(|root| raw.starts_with(root)) {
            return;
        }
        let (path, resolved) = resolve(raw, cwd);
        let touch = Touch {
            kind,
            turn,
            event: event.to_owned(),
            resolved,
        };
        self.files.insert(path.clone(), touch.clone());
        // One entry per (call, path): a line that names the same file twice
        // touched it once as far as a later reader is concerned, and two
        // entries would collide on the id built from the pair.
        let already = self.facts.iter().position(|fact| {
            fact.event == event
                && matches!(&fact.derived, Derived::Touch { path: held, .. } if *held == path)
        });
        let fact = Fact {
            turn,
            event: event.to_owned(),
            derived: Derived::Touch { path, touch },
        };
        match already {
            Some(index) => self.facts[index] = fact,
            None => self.facts.push(fact),
        }
    }
}

/// The suffix a `ran` entry's id carries.
const RAN: &str = "ran";

/// What `pwd` is written as.
const PWD: &str = "pwd";

/// What a shell call with no command line is recorded as having run.
const SHELL_WITHOUT_A_LINE: &str = "a shell tool call the record kept no command line for";

// ---------------------------------------------------------------------------
// entry contents
// ---------------------------------------------------------------------------

fn cwd_content(event: &str, cwd: &Cwd) -> String {
    match cwd {
        Cwd::Unknown => {
            format!("the working directory is no longer known after tool call {event}")
        }
        Cwd::Home => format!(
            "the working directory is the home directory after tool call {event}; \
             the shell has not reported its path"
        ),
        Cwd::Path(path) => format!(
            "the working directory is {} after tool call {event}",
            path.display()
        ),
    }
}

fn touch_content(event: &str, path: &Path, touch: &Touch) -> String {
    let verb = touch.kind.verb();
    if touch.resolved {
        format!("tool call {event} {verb} {}", path.display())
    } else {
        format!("tool call {event} {verb} {}, cwd unknown", path.display())
    }
}

fn ran_content(event: &str, line: &str, exit: Option<i64>, parsed: bool) -> String {
    let status = exit.map_or_else(String::new, |code| format!(", exit {code}"));
    if parsed {
        format!("tool call {event} ran `{line}`{status}")
    } else {
        format!(
            "tool call {event} ran `{line}`{status}; the command line is outside the \
             shell subset v0 reads, so nothing was derived from it"
        )
    }
}

// ---------------------------------------------------------------------------
// paths
// ---------------------------------------------------------------------------

/// A path with `.` and `..` resolved lexically.
///
/// Lexically and not on disk: this lane never touches a filesystem, and a
/// reader that resolved symlinks would produce a different answer on every
/// machine that replayed the record.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => out.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if out.as_os_str().is_empty() || out.ends_with(Component::ParentDir.as_os_str()) {
                    out.push(Component::ParentDir.as_os_str());
                } else {
                    // `/..` is `/`, so a root that cannot pop stays the root.
                    let _ = out.pop();
                }
            }
            Component::Normal(part) => out.push(part),
        }
    }
    out
}

/// `raw` as an absolute path, if it is one.
fn absolute(raw: &str) -> Option<PathBuf> {
    let path = Path::new(raw);
    path.is_absolute().then(|| normalize(path))
}

/// `raw` against `cwd`, and whether the working directory was known.
fn resolve(raw: &str, cwd: &Cwd) -> (PathBuf, bool) {
    if let Some(path) = absolute(raw) {
        return (path, true);
    }
    match cwd.path() {
        Some(base) => (normalize(&base.join(raw)), true),
        None => (normalize(Path::new(raw)), false),
    }
}

/// Where a `cd` or `pushd` with this argument goes.
fn target_of(argument: Option<&Word>, state: &State) -> Cwd {
    let Some(word) = argument else {
        return Cwd::Home;
    };
    if word.text == TILDE {
        return Cwd::Home;
    }
    if !word.literal || word.text == DASH {
        // `cd -` goes to the previous directory, which this lane does not
        // keep, and an expanding word names a directory it cannot know.
        return Cwd::Unknown;
    }
    if let Some(path) = absolute(&word.text) {
        return Cwd::Path(path);
    }
    match state.cwd.path() {
        Some(base) => Cwd::Path(normalize(&base.join(&word.text))),
        None => Cwd::Unknown,
    }
}

/// The home directory, written.
const TILDE: &str = "~";

/// The last element of a path-like word: `./target/debug/diet` is `diet`.
fn basename(word: &str) -> Option<&str> {
    Path::new(word).file_name().and_then(|name| name.to_str())
}

// ---------------------------------------------------------------------------
// reading the command line
// ---------------------------------------------------------------------------

/// The last simple command the list would run in the live shell.
///
/// `None` when the exit status of the call does not belong to a command this
/// lane models: a backgrounded list returns immediately, a pipeline's members
/// each run in a subshell, and a subshell's `cd` never touched the parent in
/// the first place.
fn trailing_simple(list: &List) -> Option<&Simple> {
    let item = list.items.last()?;
    if item.background {
        return None;
    }
    let link = item.chain.last()?;
    if link.pipeline.len() > 1 {
        return None;
    }
    match link.pipeline.last()? {
        Command::Simple(simple) => Some(simple),
        Command::Group(inner) => trailing_simple(inner),
        Command::Subshell(_) => None,
    }
}

/// The command as written, for a failure the record has to name.
fn rendered(simple: &Simple) -> String {
    simple
        .words
        .iter()
        .map(|word| word.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The file operands of one command, flags and patterns removed.
fn file_operands<'a>(known: &FileCommand, simple: &'a Simple) -> Vec<&'a str> {
    let has_pattern_flag = simple
        .operands()
        .iter()
        .any(|word| word.literal && known.pattern_flags.contains(&word.text.as_str()));
    let mut want_pattern = known.first_operand_is_pattern && !has_pattern_flag;
    let mut found = Vec::new();
    let mut skip_next = false;
    for word in simple.operands() {
        if skip_next {
            skip_next = false;
            continue;
        }
        if is_flag(&word.text) {
            skip_next = known.value_flags.contains(&word.text.as_str());
            continue;
        }
        if want_pattern {
            want_pattern = false;
            continue;
        }
        if word.literal {
            found.push(word.text.as_str());
        }
    }
    found
}

/// Whether a word is an option rather than an operand. A lone `-` is standard
/// input, which is an operand and not a file.
fn is_flag(word: &str) -> bool {
    word.starts_with('-') && word != DASH
}

/// Every `$( … )` inside a word, as the text between the brackets.
fn substitutions(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] != b'$' || bytes[index + 1] != b'(' {
            index += 1;
            continue;
        }
        let start = index + 2;
        let mut depth = 1_u32;
        let mut cursor = start;
        while cursor < bytes.len() {
            match bytes[cursor] {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            cursor += 1;
        }
        if depth != 0 {
            break;
        }
        found.push(&text[start..cursor]);
        index = cursor + 1;
    }
    found
}

/// The working directory the shell printed, if it printed one.
///
/// Only a line that is nothing but an absolute path counts. A listing that
/// happens to mention a path is not the shell reporting where it is, and
/// reading one as a report is how a lane that exists to be exact starts
/// guessing.
fn reported_path(output: &str) -> Option<PathBuf> {
    output
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| line.starts_with('/') && !line.contains(char::is_whitespace))
        .map(|line| normalize(Path::new(line)))
}

/// Every failure the shell reported in the call's output.
///
/// The shapes are the ones the common shells write: `cd: x: …`, `bash: cd: x:
/// …`, `bash: line 1: cd: x: …`, `-bash: cd: x: …`, `sh: 1: cd: can't cd to
/// x`, and the `pushd`/`popd` equivalents. The builtin's name is a whole
/// colon-separated field, and the argument -- when the report names one -- is
/// the field after it, so a report can be charged to the `cd` it is about
/// rather than to the first one on the line.
fn reports_in(output: &str) -> Vec<Report> {
    let mut found = Vec::new();
    for line in output.lines() {
        let fields: Vec<&str> = line.split(": ").collect();
        let Some(at) = fields
            .iter()
            .position(|field| Builtin::from_word(field.trim()).is_some())
        else {
            continue;
        };
        let Some(builtin) = Builtin::from_word(fields[at].trim()) else {
            continue;
        };
        let rest = &fields[at + 1..];
        let Some(first) = rest.first() else {
            continue;
        };
        let argument = if builtin == Builtin::Popd {
            None
        } else if let Some(named) = first.strip_prefix(CANT_CD_TO) {
            Some(named.trim().to_owned())
        } else if rest.len() >= 2 {
            Some((*first).to_owned())
        } else {
            None
        };
        found.push(Report {
            builtin,
            argument,
            text: line.to_owned(),
            used: false,
        });
    }
    found
}

/// How the leanest shells spell a `cd` that did not happen.
const CANT_CD_TO: &str = "can't cd to ";

/// One string argument of a tool call.
fn string_arg<'a>(args: Option<&'a BTreeMap<String, Value>>, key: &str) -> Option<&'a str> {
    match args?.get(key) {
        Some(Value::String(text)) => Some(text.as_str()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{
        Builtin, Cwd, CwdKind, Facts, Failure, Hole, Lane, MECHANICAL_NOUNS, Operands,
        QUESTION_OPENERS, TOOLS, ToolKind, TouchKind, asks_for_a_mechanical_fact, fill,
        lint_template,
    };
    use crate::formats::record::json::{self, Value};
    use crate::formats::record::{self, Event, Regime};
    use crate::object::{EntryId, Patch, WorkingObject};

    const START: &str = r#"{"record":"start","regime":{"arm":"baseline","dogma_version":0,"substrate":{"name":"local","model":"m","quantization":"q","sampler":{"seed":0},"reasoning":"on","hardware":"h"}}}"#;

    fn regime() -> Regime {
        record::parse(START).expect("a record").regime().clone()
    }

    /// A tool call with string arguments.
    fn call(
        tool: &str,
        id: &str,
        turn: u32,
        args: &[(&str, &str)],
        exit: Option<i64>,
        output: Option<&str>,
    ) -> Event {
        Event::ToolCall {
            id: id.to_owned(),
            at_turn: turn,
            tool: tool.to_owned(),
            args: Some(
                args.iter()
                    .map(|(key, value)| ((*key).to_owned(), Value::String((*value).to_owned())))
                    .collect(),
            ),
            exit,
            output: output.map(str::to_owned),
        }
    }

    /// A shell call that starts in `/work`, succeeded, and printed nothing
    /// the lane would read.
    fn shell(id: &str, command: &str) -> Event {
        call(
            "bash",
            id,
            1,
            &[("cwd", "/work"), ("command", command)],
            Some(0),
            None,
        )
    }

    /// A shell call that starts in `/work` and reported what it did.
    fn shell_said(id: &str, command: &str, exit: i64, output: &str) -> Event {
        call(
            "bash",
            id,
            1,
            &[("cwd", "/work"), ("command", command)],
            Some(exit),
            Some(output),
        )
    }

    /// A shell call with no starting directory.
    fn shell_nowhere(id: &str, command: &str, output: Option<&str>) -> Event {
        call("bash", id, 1, &[("command", command)], Some(0), output)
    }

    fn lane_after(events: &[Event]) -> Lane {
        let mut lane = Lane::default();
        for event in events {
            lane.observe(event);
        }
        lane
    }

    fn at(path: &str) -> Cwd {
        Cwd::Path(PathBuf::from(path))
    }

    fn touched(lane: &Lane, path: &str) -> Option<(TouchKind, bool)> {
        lane.files()
            .get(Path::new(path))
            .map(|touch| (touch.kind, touch.resolved))
    }

    const NO_SUCH: &str = "bash: cd: nowhere: No such file or directory";

    // Acceptance: `cd a; (cd b; ls); pwd` tracks `a`. The subshell ran
    // against its own copy of the state, and the parent never saw `b`.
    #[test]
    fn a_subshells_cd_does_not_leak_into_the_parent() {
        let lane = lane_after(&[shell("t1", "cd a; (cd b; ls); pwd")]);
        assert_eq!(
            lane.cwd(),
            &at("/work/a"),
            "the subshell cd leaked into the parent"
        );
    }

    // Acceptance: a failed `cd` leaves the cwd where it was, and the failure
    // is in the record rather than in nobody's memory.
    #[test]
    fn a_failed_cd_leaves_the_cwd_where_it_was_and_is_recorded() {
        let lane = lane_after(&[shell_said("t1", "cd nowhere", 1, NO_SUCH)]);
        assert_eq!(
            lane.cwd(),
            &at("/work"),
            "a failed cd moved the working directory"
        );
        assert_eq!(
            lane.failures(),
            &[Failure {
                event: "t1".to_owned(),
                command: "cd nowhere".to_owned(),
                report: NO_SUCH.to_owned(),
            }],
            "a failed cd was not recorded"
        );
    }

    #[test]
    fn a_cd_that_fails_by_exit_status_alone_is_recorded_only_when_it_ran_last() {
        let lane = lane_after(&[shell_said("t1", "ls && cd b", 1, "")]);
        assert_eq!(lane.cwd(), &at("/work"), "a trailing cd with exit 1 stood");
        assert_eq!(lane.failures().len(), 1);
        assert_eq!(lane.failures()[0].command, "cd b");
        assert_eq!(lane.failures()[0].report, "exit 1");

        // The exit belongs to the last command, and here that is not the cd.
        let lane = lane_after(&[shell_said("t1", "cd diet && cargo test", 101, "")]);
        assert_eq!(
            lane.cwd(),
            &at("/work/diet"),
            "a cd before a failing test was undone"
        );
        assert!(lane.failures().is_empty());
    }

    #[test]
    fn popd_on_an_empty_stack_is_a_recorded_failure() {
        let lane = lane_after(&[shell("t1", "popd")]);
        assert_eq!(lane.cwd(), &at("/work"));
        assert_eq!(
            lane.failures().len(),
            1,
            "popd on an empty stack was silently ignored"
        );
        assert_eq!(lane.failures()[0].command, "popd");
        assert!(lane.failures()[0].report.contains("stack empty"));
    }

    #[test]
    fn pushd_and_popd_walk_the_stack() {
        let mut lane = lane_after(&[shell("t1", "pushd diet; pushd src; popd; ls")]);
        assert_eq!(lane.cwd(), &at("/work/diet"));
        assert_eq!(lane.stack(), &[at("/work")]);
        // pushd without popd stays pushed across calls.
        lane.observe(&call(
            "bash",
            "t2",
            1,
            &[("command", "popd")],
            Some(0),
            None,
        ));
        assert_eq!(lane.cwd(), &at("/work"));
        assert!(lane.stack().is_empty());
    }

    #[test]
    fn a_group_runs_against_the_live_state_and_a_subshell_against_a_copy() {
        assert_eq!(
            lane_after(&[shell("t1", "{ cd diet; }; pwd")]).cwd(),
            &at("/work/diet"),
            "a group cd did not reach the parent"
        );
        assert_eq!(
            lane_after(&[shell("t1", "(cd diet); pwd")]).cwd(),
            &at("/work")
        );
    }

    #[test]
    fn a_command_substitution_runs_against_a_discarded_copy() {
        let lane = lane_after(&[shell(
            "t1",
            "x=$(cd diet; cat Cargo.toml); echo $(cat notes.txt)",
        )]);
        assert_eq!(lane.cwd(), &at("/work"), "a substitution cd leaked");
        assert_eq!(
            touched(&lane, "/work/diet/Cargo.toml"),
            Some((TouchKind::Read, true)),
            "a read inside a substitution was not recorded"
        );
        assert_eq!(
            touched(&lane, "/work/notes.txt"),
            Some((TouchKind::Read, true))
        );
    }

    #[test]
    fn and_after_a_known_failure_short_circuits_and_or_runs_instead() {
        let lane = lane_after(&[shell_said("t1", "cd nowhere && cat secret.txt", 1, NO_SUCH)]);
        assert!(
            lane.files().is_empty(),
            "a command after `&&` ran although the cd before it failed"
        );
        assert_eq!(lane.cwd(), &at("/work"));
        let lane = lane_after(&[shell_said("t1", "cd nowhere || cd diet", 0, NO_SUCH)]);
        assert_eq!(
            lane.cwd(),
            &at("/work/diet"),
            "the `||` branch did not run after a known failure"
        );
        // Nothing is known about `ls`, so it is taken to have succeeded and
        // the `||` branch is not claimed to have run.
        let lane = lane_after(&[shell("t1", "ls || cat secret.txt")]);
        assert!(lane.files().is_empty());
    }

    #[test]
    fn a_semicolon_after_a_failure_does_not_short_circuit() {
        let lane = lane_after(&[shell_said("t1", "cd nowhere; cat notes.txt", 0, NO_SUCH)]);
        assert_eq!(lane.cwd(), &at("/work"));
        assert_eq!(
            touched(&lane, "/work/notes.txt"),
            Some((TouchKind::Read, true))
        );
    }

    #[test]
    fn a_pipeline_and_a_background_chain_run_apart_from_the_parent() {
        assert_eq!(
            lane_after(&[shell("t1", "cd diet | cat; pwd")]).cwd(),
            &at("/work"),
            "a cd inside a pipeline moved the parent"
        );
        assert_eq!(
            lane_after(&[shell("t1", "cd diet & pwd")]).cwd(),
            &at("/work"),
            "a backgrounded cd moved the parent"
        );
    }

    #[test]
    fn the_shells_own_pwd_report_establishes_an_unknown_cwd() {
        let lane = lane_after(&[
            shell_nowhere("t1", "pwd", Some("/work/diet\n")),
            shell_nowhere("t2", "cat Cargo.toml", None),
        ]);
        assert_eq!(lane.cwd(), &at("/work/diet"));
        assert_eq!(
            touched(&lane, "/work/diet/Cargo.toml"),
            Some((TouchKind::Read, true)),
            "a path after the report was not resolved against it"
        );
        // The shell's report outranks what was tracked.
        let lane = lane_after(&[shell_said("t1", "cd a; pwd", 0, "/elsewhere/a\n")]);
        assert_eq!(lane.cwd(), &at("/elsewhere/a"));
        // Only a bare absolute path is a report; a listing is not.
        let lane = lane_after(&[shell_nowhere("t1", "ls -la", Some("/work/x.txt is here\n"))]);
        assert_eq!(lane.cwd(), &Cwd::Unknown);
        // And only when `pwd` was the last thing the line ran.
        let lane = lane_after(&[shell_nowhere("t1", "(cd b; pwd)", Some("/work/b\n"))]);
        assert_eq!(lane.cwd(), &Cwd::Unknown);
    }

    #[test]
    fn a_cwd_argument_establishes_the_cwd_before_the_command_runs() {
        let lane = lane_after(&[call(
            "read_file",
            "t1",
            1,
            &[("cwd", "/work"), ("path", "notes.txt")],
            None,
            None,
        )]);
        assert_eq!(lane.cwd(), &at("/work"));
        assert_eq!(
            touched(&lane, "/work/notes.txt"),
            Some((TouchKind::Read, true))
        );
    }

    #[test]
    fn cd_with_no_argument_goes_home_and_home_is_not_a_path() {
        let lane = lane_after(&[shell("t1", "cd")]);
        assert_eq!(lane.cwd(), &Cwd::Home);
        assert_eq!(lane.facts().cwd, None, "home was rendered as a path");
        let lane = lane_after(&[shell("t1", "cd; cd diet")]);
        assert_eq!(
            lane.cwd(),
            &Cwd::Unknown,
            "a relative cd from home was resolved"
        );
    }

    #[test]
    fn a_cd_to_a_word_the_shell_would_expand_loses_the_cwd() {
        assert_eq!(lane_after(&[shell("t1", "cd $DIR")]).cwd(), &Cwd::Unknown);
        assert_eq!(lane_after(&[shell("t1", "cd ~")]).cwd(), &Cwd::Home);
        assert_eq!(lane_after(&[shell("t1", "cd -")]).cwd(), &Cwd::Unknown);
        assert_eq!(
            lane_after(&[shell("t1", "cd /abs/../x/./y")]).cwd(),
            &at("/x/y")
        );
    }

    #[test]
    fn a_relative_path_resolves_against_the_tracked_cwd_or_is_marked_unresolved() {
        let lane = lane_after(&[shell("t1", "cat a/../b.txt /work/x.txt")]);
        assert_eq!(touched(&lane, "/work/b.txt"), Some((TouchKind::Read, true)));
        assert_eq!(touched(&lane, "/work/x.txt"), Some((TouchKind::Read, true)));
        let lane = lane_after(&[shell_nowhere("t1", "cat b.txt", None)]);
        assert_eq!(
            touched(&lane, "b.txt"),
            Some((TouchKind::Read, false)),
            "a path under an unknown cwd was not marked unresolved"
        );
        assert!(lane.patches(1).iter().any(|patch| matches!(
            patch,
            Patch::Add { content, .. } if content.contains("cwd unknown")
        )));
    }

    #[test]
    fn shell_readers_touch_their_file_operands_and_nothing_else() {
        let cases: &[(&str, &[&str], &[&str])] = &[
            ("cat notes.txt", &["/work/notes.txt"], &[]),
            ("head -n 20 a.rs", &["/work/a.rs"], &["/work/20"]),
            ("tail -60 log.txt", &["/work/log.txt"], &["/work/-60"]),
            ("less a.txt", &["/work/a.txt"], &[]),
            ("more a.txt", &["/work/a.txt"], &[]),
            ("wc -l notes.txt", &["/work/notes.txt"], &[]),
            (
                "sed -n '80,160p' diet/src/object.rs",
                &["/work/diet/src/object.rs"],
                &["/work/80,160p"],
            ),
            ("sed 's/a/b/' f.txt", &[], &["/work/f.txt"]),
            (
                "grep -n -A 3 'fn parse' diet/src/lib.rs",
                &["/work/diet/src/lib.rs"],
                &["/work/3", "/work/fn parse"],
            ),
            ("grep -rn foo diet/src", &["/work/diet/src"], &["/work/foo"]),
            ("cat -", &[], &["/work/-"]),
            ("cat $FILE", &[], &["/work/$FILE"]),
        ];
        for (command, read, untouched) in cases {
            let lane = lane_after(&[shell("t1", command)]);
            for path in *read {
                assert_eq!(
                    touched(&lane, path),
                    Some((TouchKind::Read, true)),
                    "{command}: {path} was not read"
                );
            }
            for path in *untouched {
                assert_eq!(touched(&lane, path), None, "{command}: {path} was touched");
            }
        }
    }

    #[test]
    fn shell_writers_and_deleters_touch_the_files_they_name() {
        let cases: &[(&str, &[(&str, TouchKind)])] = &[
            ("tee out.txt", &[("/work/out.txt", TouchKind::Written)]),
            ("touch new.txt", &[("/work/new.txt", TouchKind::Written)]),
            ("mkdir -p build/x", &[("/work/build/x", TouchKind::Written)]),
            ("cp a.txt b.txt", &[("/work/b.txt", TouchKind::Written)]),
            ("rm -rf build", &[("/work/build", TouchKind::Deleted)]),
            ("rmdir empty", &[("/work/empty", TouchKind::Deleted)]),
            (
                "mv old.txt new.txt",
                &[
                    ("/work/old.txt", TouchKind::Deleted),
                    ("/work/new.txt", TouchKind::Written),
                ],
            ),
            (
                "mv a.txt b.txt dir/",
                &[
                    ("/work/a.txt", TouchKind::Deleted),
                    ("/work/b.txt", TouchKind::Deleted),
                    ("/work/dir", TouchKind::Written),
                ],
            ),
        ];
        for (command, expected) in cases {
            let lane = lane_after(&[shell("t1", command)]);
            for (path, kind) in *expected {
                assert_eq!(
                    touched(&lane, path),
                    Some((*kind, true)),
                    "{command}: {path} was not {}",
                    kind.tag()
                );
            }
            assert_eq!(lane.files().len(), expected.len(), "{command} touched more");
        }
        assert_eq!(
            touched(&lane_after(&[shell("t1", "cp a.txt b.txt")]), "/work/a.txt"),
            None,
            "the source of a cp was recorded as touched"
        );
    }

    #[test]
    fn redirections_touch_their_targets_and_descriptors_are_not_files() {
        let lane = lane_after(&[shell("t1", "cargo test 2>&1 > out.txt")]);
        assert_eq!(
            touched(&lane, "/work/out.txt"),
            Some((TouchKind::Written, true))
        );
        assert_eq!(lane.files().len(), 1, "a descriptor was recorded as a file");
        let lane = lane_after(&[shell("t1", "sort < names.txt >> sorted.txt 2>/dev/null")]);
        assert_eq!(
            touched(&lane, "/work/names.txt"),
            Some((TouchKind::Read, true))
        );
        assert_eq!(
            touched(&lane, "/work/sorted.txt"),
            Some((TouchKind::Written, true))
        );
        assert_eq!(lane.files().len(), 2, "/dev/null was recorded as a file");
    }

    #[test]
    fn read_and_edit_tools_touch_the_path_in_their_arguments() {
        for (name, kind) in TOOLS {
            let expected = match kind {
                ToolKind::Read => Some((TouchKind::Read, true)),
                ToolKind::Edit => Some((TouchKind::Written, true)),
                ToolKind::Shell => None,
            };
            let lane = lane_after(&[call(
                name,
                "t1",
                1,
                &[("cwd", "/work"), ("path", "diet/src/lib.rs")],
                None,
                None,
            )]);
            assert_eq!(
                touched(&lane, "/work/diet/src/lib.rs"),
                expected,
                "{name} ({})",
                kind.tag()
            );
        }
    }

    #[test]
    fn an_unknown_tool_is_recorded_as_a_command_run_with_no_derived_facts() {
        let lane = lane_after(&[call("xyzzy", "t1", 1, &[], Some(127), Some("not found"))]);
        assert_eq!(lane.commands().len(), 1);
        assert_eq!(lane.commands()[0].line, "xyzzy");
        assert_eq!(lane.commands()[0].exit, Some(127));
        assert!(lane.files().is_empty());
        assert_eq!(lane.cwd(), &Cwd::Unknown);
        let ids: Vec<String> = lane
            .patches(1)
            .iter()
            .map(|patch| match patch {
                Patch::Add { id, .. } | Patch::Supersede { id, .. } => id.to_string(),
                Patch::Resolve { target, .. } | Patch::Retire { target, .. } => target.to_string(),
            })
            .collect();
        assert_eq!(ids, vec!["t1/ran"], "an unknown tool derived something");
        // A shell tool that carries no command line is recorded the same way.
        let lane = lane_after(&[call("bash", "t2", 1, &[], None, None)]);
        assert_eq!(lane.commands().len(), 1);
        assert!(lane.commands()[0].line.contains("no command line"));
    }

    #[test]
    fn a_command_line_outside_the_shell_subset_is_still_recorded_as_run() {
        let lane = lane_after(&[shell_nowhere("t1", "ls &&", None)]);
        assert_eq!(lane.commands()[0].line, "ls &&");
        let patches = lane.patches(1);
        assert_eq!(patches.len(), 1);
        assert!(matches!(
            &patches[0],
            Patch::Add { content, .. } if content.contains("outside the shell subset")
        ));
    }

    #[test]
    fn every_kind_has_a_distinct_spelling() {
        let tools: Vec<&str> = ToolKind::ALL.iter().map(|kind| kind.tag()).collect();
        assert_eq!(tools, vec!["shell", "read", "edit"]);
        let touches: Vec<&str> = TouchKind::ALL.iter().map(|kind| kind.tag()).collect();
        assert_eq!(touches, vec!["read", "written", "deleted"]);
        let cwds: Vec<&str> = CwdKind::ALL.iter().map(|kind| kind.tag()).collect();
        assert_eq!(cwds, vec!["unknown", "home", "path"]);
        let builtins: Vec<&str> = Builtin::ALL.iter().map(|kind| kind.tag()).collect();
        assert_eq!(builtins, vec!["cd", "pushd", "popd"]);
        let operands: Vec<&str> = Operands::ALL.iter().map(|kind| kind.tag()).collect();
        assert_eq!(operands, vec!["read", "write", "delete", "move", "copy"]);
        for (name, kind) in TOOLS {
            assert_eq!(super::tool_kind(name), Some(*kind));
        }
        assert_eq!(super::tool_kind("xyzzy"), None);
        for builtin in Builtin::ALL {
            assert_eq!(Builtin::from_word(builtin.tag()), Some(*builtin));
        }
        assert_eq!(Builtin::from_word("ls"), None);
        assert_eq!(at("/work").kind(), CwdKind::Path);
        assert_eq!(
            Cwd::Home.to_string(),
            "the home directory, path not reported"
        );
        assert_eq!(Cwd::Unknown.to_string(), "unknown");
        // Every command the table knows is reachable by its own word.
        for known in super::FILE_COMMANDS {
            assert_eq!(
                super::file_command(known.word).map(|found| found.operands),
                Some(known.operands)
            );
        }
    }

    #[test]
    fn entries_are_named_by_the_event_they_derive_from() {
        let lane = lane_after(&[
            shell("t1", "cd diet; cat Cargo.toml"),
            call("edit_file", "t2", 1, &[("path", "src/lib.rs")], None, None),
            call("bash", "t3", 2, &[("command", "ls")], Some(0), None),
        ]);
        let patches = lane.patches(1);
        let mut ids = Vec::new();
        for (index, patch) in patches.iter().enumerate() {
            let Patch::Add {
                id,
                content,
                provenance,
            } = patch
            else {
                panic!("a mechanical entry is always an Add: {patch:?}")
            };
            ids.push(id.to_string());
            let event = id.as_str().split('/').next().expect("an id has an event");
            assert!(
                content.contains(event),
                "`{id}` does not name its event in its content: {content}"
            );
            assert_eq!(provenance.turn, 1);
            assert_eq!(provenance.lane, super::LANE);
            assert_eq!(provenance.fork, None);
            assert_eq!(
                provenance.index as usize, index,
                "positions are the order derived"
            );
        }
        ids.sort();
        assert_eq!(
            ids,
            vec![
                "t1/cwd",
                "t1/ran",
                "t1/read:/work/diet/Cargo.toml",
                "t2/written:/work/diet/src/lib.rs",
            ]
        );
        assert_eq!(lane.patches(2).len(), 1, "turn 2 has its own entries");
        assert!(lane.patches(3).is_empty());
    }

    // Acceptance for the doc comment: the entries land in the object without
    // a LaneReport, because their grounding is the row they name.
    #[test]
    fn mechanical_entries_are_applied_without_a_lane_report() {
        let lane = lane_after(&[shell("t1", "cd diet; cat Cargo.toml")]);
        let mut object = WorkingObject::open(regime());
        lane.apply_to(&mut object, 1).expect("the turn applies");
        for patch in lane.patches(1) {
            let Patch::Add { id, .. } = patch else {
                panic!("an Add")
            };
            assert!(
                object.entry(&id).is_some(),
                "a mechanical entry was dropped as if it needed grounding: {id}"
            );
        }
        assert_eq!(object.live().count(), 3);
    }

    #[test]
    fn facts_come_from_the_lane() {
        let lane = lane_after(&[shell("t1", "cd diet; echo x > out.txt")]);
        assert_eq!(
            lane.facts(),
            Facts {
                cwd: Some("/work/diet".to_owned()),
                last_edited: Some("/work/diet/out.txt".to_owned()),
                last_command: Some("cd diet; echo x > out.txt".to_owned()),
            }
        );
        assert_eq!(Lane::default().facts(), Facts::default());
    }

    /// One template line per hole, with what it fills to.
    const HOLE_FIXTURES: &[(Hole, &str, &str)] = &[
        (Hole::Cwd, "You are in {cwd}.", "You are in /work/diet."),
        (
            Hole::LastEdited,
            "You last edited {last_edited}.",
            "You last edited /work/diet/src/lib.rs.",
        ),
        (
            Hole::LastCommand,
            "You last ran `{last_command}`.",
            "You last ran `cargo test`.",
        ),
    ];

    fn known() -> Facts {
        Facts {
            cwd: Some("/work/diet".to_owned()),
            last_edited: Some("/work/diet/src/lib.rs".to_owned()),
            last_command: Some("cargo test".to_owned()),
        }
    }

    #[test]
    fn every_hole_has_a_fixture() {
        for hole in Hole::ALL {
            let (_, template, filled) = HOLE_FIXTURES
                .iter()
                .find(|(fixture, _, _)| fixture == hole)
                .unwrap_or_else(|| panic!("hole `{}` has no fixture", hole.tag()));
            assert_eq!(fill(template, &known()), *filled, "{{{}}}", hole.tag());
            assert_eq!(
                fill(template, &Facts::default()),
                "",
                "a line with an unknown {{{}}} was not dropped whole",
                hole.tag()
            );
            assert_eq!(Hole::from_tag(hole.tag()), Some(*hole));
        }
        assert_eq!(Hole::from_tag("intent"), None);
    }

    #[test]
    fn a_line_with_an_unknown_fact_is_dropped_whole_and_the_others_stay() {
        let template =
            "You are in {cwd}.\nAnswer from this turn alone.\nYou last ran `{last_command}`.\n";
        let facts = Facts {
            cwd: None,
            last_edited: None,
            last_command: Some("cargo test".to_owned()),
        };
        assert_eq!(
            fill(template, &facts),
            "Answer from this turn alone.\nYou last ran `cargo test`.\n"
        );
        assert_eq!(fill("no holes", &facts), "no holes");
    }

    #[test]
    fn a_hole_that_is_not_mechanical_is_left_for_its_own_lane() {
        assert_eq!(
            fill("{intent} in {cwd} {", &known()),
            "{intent} in /work/diet {"
        );
    }

    /// Where the ask templates live. Found through the manifest directory so
    /// that the test reads the tree it was built from and not a path nobody
    /// else has.
    fn asks_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/capture/router/asks")
    }

    fn templates() -> Vec<PathBuf> {
        let dir = asks_dir();
        let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap_or_else(|err| panic!("{}: {err}", dir.display()))
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "txt"))
            .collect();
        found.sort();
        assert!(
            !found.is_empty(),
            "{}: no templates, so the lint would hold vacuously",
            dir.display()
        );
        found
    }

    // Acceptance: a template that asks the model for the working directory
    // fails the lint. Mechanical facts are not interview questions.
    #[test]
    fn no_ask_template_asks_for_a_mechanical_fact() {
        let mut offences = Vec::new();
        for path in templates() {
            let name = path
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .expect("a template has a name")
                .to_owned();
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            offences.extend(lint_template(&name, &text));
        }
        let listed: Vec<String> = offences
            .iter()
            .map(|offence| {
                format!(
                    "{}:{}: `{}`: {}",
                    offence.template, offence.line, offence.noun, offence.text
                )
            })
            .collect();
        assert!(
            offences.is_empty(),
            "{} ask template line(s) ask the model for a mechanical fact:\n  {}",
            offences.len(),
            listed.join("\n  ")
        );
    }

    /// Questions about mechanical facts, and the noun each is caught by.
    const ASKED: &[(&str, &str)] = &[
        ("Which directory are you in?", "which directory"),
        (
            "What is your current working directory?",
            "working directory",
        ),
        ("What is the current directory", "current directory"),
        ("Where is your cwd now?", "cwd"),
        ("what directory did the tests run in?", "what directory"),
        ("Which files did you edit this turn?", "files did you"),
        ("Are the files you edited under test?", "files you edited"),
        ("Do the files you changed compile?", "files you changed"),
        ("How many files you read were docs?", "files you read"),
        ("Were the files you touched tests?", "files you touched"),
        ("Which commands did you run?", "commands did you run"),
        ("Which command did you run last?", "command did you run"),
        ("What exit code did the build return?", "exit code"),
    ];

    /// Lines the lint must leave alone: statements, and questions about what
    /// only the model knows.
    const NOT_ASKED: &[&str] = &[
        "What did this turn establish that a later turn would need?",
        "You are in {cwd}.",
        "You last ran `{last_command}`; what did it show?",
        "List the files you edited.",
        "Why did you choose that approach?",
    ];

    #[test]
    fn the_lint_catches_every_shape_of_mechanical_question() {
        for (line, noun) in ASKED {
            assert_eq!(
                asks_for_a_mechanical_fact(line),
                Some(*noun),
                "a question about a mechanical fact went unflagged: {line}"
            );
        }
        for line in NOT_ASKED {
            assert_eq!(
                asks_for_a_mechanical_fact(line),
                None,
                "a line that asks for nothing mechanical was flagged: {line}"
            );
        }
        for noun in MECHANICAL_NOUNS {
            assert!(
                ASKED.iter().any(|(_, caught)| caught == noun),
                "noun `{noun}` in the table has no fixture that it catches"
            );
        }
        let offences = lint_template("seeded.txt", "fine\nWhich directory are you in?\n");
        assert_eq!(offences.len(), 1);
        assert_eq!((offences[0].line, offences[0].noun), (2, "which directory"));
    }

    #[test]
    fn every_failure_report_shape_is_read() {
        let shapes: &[(&str, &str)] = &[
            ("cd x", "bash: cd: x: No such file or directory"),
            ("cd x", "cd: x: No such file or directory"),
            ("cd x", "sh: 1: cd: can't cd to x"),
            ("cd x", "bash: line 1: cd: x: Not a directory"),
            ("cd x", "-bash: cd: x: Permission denied"),
            ("pushd x", "bash: pushd: x: No such file or directory"),
            ("popd", "bash: popd: directory stack empty"),
        ];
        for (command, output) in shapes {
            let lane = lane_after(&[shell_said("t1", command, 1, output)]);
            assert_eq!(lane.cwd(), &at("/work"), "{output}: the cwd moved");
            assert_eq!(
                lane.failures().len(),
                1,
                "{output}: recorded {:?}",
                lane.failures()
            );
            assert_eq!(lane.failures()[0].report, *output);
            assert_eq!(lane.failures()[0].command, *command);
        }
        // A report names its argument, so it fails the cd it is about.
        let lane = lane_after(&[shell_said(
            "t1",
            "cd a; cd b",
            1,
            "bash: cd: b: No such file or directory",
        )]);
        assert_eq!(lane.cwd(), &at("/work/a"));
        assert_eq!(lane.failures().len(), 1);
        assert_eq!(lane.failures()[0].command, "cd b");
    }

    // -- the corpus ---------------------------------------------------------

    fn corpus_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("capture/mechanical/corpus")
    }

    /// Every case file, sorted; red when there are none.
    fn corpus_cases() -> Vec<PathBuf> {
        let dir = corpus_dir();
        let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap_or_else(|err| panic!("{}: {err}", dir.display()))
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
            .collect();
        found.sort();
        assert!(
            !found.is_empty(),
            "{}: no cases, so every assertion over the corpus would hold vacuously",
            dir.display()
        );
        found
    }

    /// The cases the lane's awkward corners are pinned by.
    const REQUIRED_CASES: &[&str] = &[
        "subshell-cwd",
        "failed-cd",
        "pushd-without-popd",
        "subshell-relative-path",
        "and-chain-after-failure",
        "popd-empty-stack",
        "read-via-cat-and-tool",
        "write-via-redirect-and-tool",
        "mv-source-and-destination",
        "pwd-reports-unknown-cwd",
    ];

    #[test]
    fn the_corpus_is_populated_and_every_case_is_paired() {
        let cases = corpus_cases();
        let mut failures = Vec::new();
        for case in &cases {
            if !expectation_of(case).is_file() {
                failures.push(format!("{} has no expected.json", case.display()));
            }
        }
        for entry in std::fs::read_dir(corpus_dir()).expect("the corpus is readable") {
            let path = entry.expect("a readable entry").path();
            if path.extension().is_some_and(|ext| ext == "jsonl") {
                continue;
            }
            let name = path
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or_default();
            match name.strip_suffix(".expected.json") {
                Some(stem) if corpus_dir().join(format!("{stem}.jsonl")).is_file() => {}
                Some(_) => failures.push(format!("orphaned expectation {}", path.display())),
                None => failures.push(format!("stray file {}", path.display())),
            }
        }
        for required in REQUIRED_CASES {
            if !corpus_dir().join(format!("{required}.jsonl")).is_file() {
                failures.push(format!("required case `{required}` is missing"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    fn expectation_of(case: &Path) -> PathBuf {
        let stem = case
            .file_stem()
            .and_then(std::ffi::OsStr::to_str)
            .expect("a case has a stem");
        case.with_file_name(format!("{stem}.expected.json"))
    }

    #[test]
    fn every_corpus_case_ends_where_its_expectation_says() {
        let mut failures = Vec::new();
        for case in corpus_cases() {
            let source = std::fs::read_to_string(&case)
                .unwrap_or_else(|err| panic!("{}: {err}", case.display()));
            let parsed = record::parse(&source)
                .unwrap_or_else(|err| panic!("{} is not a record: {err}", case.display()));
            let mut lane = Lane::default();
            for event in &parsed.events {
                lane.observe(event);
            }
            let mut rendered = String::new();
            json::render(&lane.value(), &mut rendered);
            let actual: serde_json::Value =
                serde_json::from_str(&rendered).expect("the lane renders JSON");
            let expectation = expectation_of(&case);
            let expected: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(&expectation)
                    .unwrap_or_else(|err| panic!("{}: {err}", expectation.display())),
            )
            .unwrap_or_else(|err| panic!("{}: {err}", expectation.display()));
            if actual != expected {
                failures.push(format!(
                    "{}: the mechanical lane ended at {actual}, its expectation says {expected}",
                    case.display()
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "{} corpus failure(s):\n  {}",
            failures.len(),
            failures.join("\n  ")
        );
    }

    #[test]
    fn the_lanes_value_is_the_three_outcomes_the_corpus_pins() {
        let lane = lane_after(&[shell_said("t1", "cd nowhere; cat a.txt", 0, NO_SUCH)]);
        let Value::Object(members) = lane.value() else {
            panic!("an object")
        };
        let keys: Vec<&String> = members.keys().collect();
        assert_eq!(keys, vec!["cwd", "failures", "files"]);
        let mut rendered = String::new();
        json::render(&lane.value(), &mut rendered);
        assert!(rendered.contains(r#""kind":"path""#), "{rendered}");
        assert!(rendered.contains(r#""resolved":true"#), "{rendered}");
        assert!(rendered.contains(r#""report""#), "{rendered}");
    }

    #[test]
    fn a_new_entry_id_is_well_formed_for_every_fact_shape() {
        let lane = lane_after(&[shell("t1", "cd diet; cat Cargo.toml; rm old.txt")]);
        let patches = lane.patches(1);
        assert!(patches.len() >= 4, "{patches:?}");
        for patch in patches {
            let Patch::Add { id, .. } = patch else {
                panic!("an Add")
            };
            assert_eq!(EntryId::new(id.as_str()), Ok(id.clone()));
        }
    }

    // --- what a review proved the gate could not see -------------------------
    //
    // Everything below closes a mutation that survived the whole gate. Each
    // was demonstrated first: the mutation applied, `./verify.sh` exit 0, the
    // test written, the test red under the mutation and green without it.

    /// The content of the one entry an id ends with, for a lane's turn.
    fn entry_saying(lane: &Lane, ends_with: &str) -> String {
        let patches = lane.patches(1);
        let found: Vec<&Patch> = patches
            .iter()
            .filter(|patch| match patch {
                Patch::Add { id, .. } => id.as_str().ends_with(ends_with),
                _ => false,
            })
            .collect();
        assert_eq!(
            found.len(),
            1,
            "{ends_with}: expected one entry, and the lane wrote {:?}",
            patches
                .iter()
                .map(|patch| match patch {
                    Patch::Add { id, content, .. } => format!("{} => {content}", id.as_str()),
                    other => format!("{other:?}"),
                })
                .collect::<Vec<_>>()
        );
        match found[0] {
            Patch::Add { content, .. } => content.clone(),
            other => panic!("{other:?}"),
        }
    }

    // The lane's product is the text of the entries it writes, and nothing
    // read one. Three separate mutations to the three content builders --
    // dropping the derived directory, dropping the path and the verb,
    // dropping the exit code -- each survived the full gate.
    #[test]
    fn an_entry_says_the_fact_it_was_derived_for() {
        let lane = lane_after(&[shell("t1", "cd diet && cat Cargo.toml")]);
        assert_eq!(
            entry_saying(&lane, "/cwd"),
            "the working directory is /work/diet after tool call t1"
        );
        assert_eq!(
            entry_saying(&lane, "/ran"),
            "tool call t1 ran `cd diet && cat Cargo.toml`, exit 0"
        );
        let read = lane
            .patches(1)
            .into_iter()
            .filter_map(|patch| match patch {
                Patch::Add { id, content, .. } if id.as_str().contains("/read") => Some(content),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(read, vec!["tool call t1 read /work/diet/Cargo.toml"]);

        // The two shapes that refuse to name a path still say which they
        // are. Losing the directory takes two calls: the entry is written
        // when the directory changes, and a call that establishes `/work`
        // and then loses it has changed nothing by the time it ends.
        let unknown = lane_after(&[shell("t1", "pwd"), shell("t2", "cd $WHEREVER")]);
        assert_eq!(
            entry_saying(&unknown, "t2/cwd"),
            "the working directory is no longer known after tool call t2"
        );
        let home = lane_after(&[shell("t1", "cd")]);
        assert!(
            entry_saying(&home, "/cwd")
                .starts_with("the working directory is the home directory after tool call t1;"),
            "{}",
            entry_saying(&home, "/cwd")
        );
    }

    // `last_edited` and `last_command` are the published contract the router
    // renders into an ask. Returning the FIRST write and the FIRST command
    // survived the gate, and an ask filled from it states a file the model
    // edited turns ago as if it were the current one.
    #[test]
    fn the_facts_are_the_most_recent_ones_and_not_the_first() {
        let lane = lane_after(&[
            shell("t1", "echo a > first.txt"),
            shell("t2", "echo b > second.txt"),
        ]);
        let facts = lane.facts();
        assert_eq!(
            facts.last_edited.as_deref(),
            Some("/work/second.txt"),
            "the lane reported an earlier write as the latest"
        );
        assert_eq!(
            facts.last_command.as_deref(),
            Some("echo b > second.txt"),
            "the lane reported an earlier command as the latest"
        );
    }

    // The lint's fourth acceptance row. Every ASKED fixture also ends in
    // `?`, so three of the four openers were decided by the `?` branch and
    // never reached -- and a question without a `?` could be written into a
    // template.
    #[test]
    fn every_question_opener_catches_a_question_that_has_no_mark() {
        // Written out rather than iterated from the table: a test that
        // loops over the very list it is guarding passes just as happily
        // when the list has been emptied to one entry, which is the shape
        // of vacuous pass this suite exists to refuse. These four are the
        // rule as the brief states it.
        for opener in ["what ", "which ", "where ", "how many "] {
            assert!(
                QUESTION_OPENERS.contains(&opener),
                "{opener:?} is an interrogative opener and the table has lost it"
            );
            let line = format!("{opener}the working directory was when the tests ran");
            assert!(
                !line.ends_with('?'),
                "{line:?} ends in a mark, so it does not exercise the opener"
            );
            assert!(
                asks_for_a_mechanical_fact(&line).is_some(),
                "{line:?} is a question about a mechanical fact and the lint let it pass"
            );
        }
    }

    // Bare `pushd` swaps the top of the stack with the working directory,
    // and on an empty stack it fails without moving anywhere. Both branches
    // were unexercised: one could discard a stack level, the other could
    // throw the working directory away on a failure it had already recorded.
    #[test]
    fn a_bare_pushd_exchanges_and_on_an_empty_stack_keeps_where_it_is() {
        let swapped = lane_after(&[shell("t1", "pushd diet; pushd")]);
        assert_eq!(swapped.cwd(), &at("/work"), "the exchange did not happen");
        assert_eq!(
            swapped.stack(),
            &[at("/work/diet")],
            "a stack level was discarded rather than exchanged"
        );

        let empty = lane_after(&[shell("t1", "pushd")]);
        assert_eq!(
            empty.cwd(),
            &at("/work"),
            "a failed pushd threw the working directory away"
        );
        assert_eq!(empty.failures().len(), 1, "the failure was not recorded");
    }

    // The documented tie-break: the last bare absolute line of the output is
    // the pwd report. Every fixture printed one line, so taking the first
    // survived -- and the first is wrong for exactly the `cd x && pwd` shape
    // that prints something absolute before the report.
    #[test]
    fn the_last_reported_path_is_the_one_that_counts() {
        let lane = lane_after(&[shell_said("t1", "cd a && pwd", 0, "/work/a\n/work/b\n")]);
        assert_eq!(
            lane.cwd(),
            &at("/work/b"),
            "an earlier line of the output was read as the pwd report"
        );
    }

    // The `cwd` argument is a fact only when it is absolute. A relative one
    // read as the working directory makes every relative path in the session
    // resolve against a root that is not one, and marks them resolved.
    #[test]
    fn a_relative_cwd_argument_is_not_a_working_directory() {
        let lane = lane_after(&[call(
            "bash",
            "t1",
            1,
            &[("cwd", "diet"), ("command", "cat a.txt")],
            Some(0),
            None,
        )]);
        assert_eq!(
            lane.cwd(),
            &Cwd::Unknown,
            "a relative cwd argument was stated as the working directory"
        );
        assert_eq!(lane.facts().cwd, None);
        assert_eq!(
            touched(&lane, "a.txt"),
            Some((TouchKind::Read, false)),
            "a path was resolved against a root that is not one"
        );
    }

    // One shell report is one failure. Without the consumption bookkeeping a
    // single `No such file or directory` charges every `cd` on the line.
    #[test]
    fn one_report_fails_one_builtin() {
        let lane = lane_after(&[shell_said(
            "t1",
            "cd nowhere; cd nowhere",
            0,
            "bash: cd: nowhere: No such file or directory\n",
        )]);
        assert_eq!(
            lane.failures().len(),
            1,
            "one report was charged to more than one cd"
        );
        assert_eq!(
            lane.cwd(),
            &at("/work/nowhere"),
            "the second cd was failed by the first cd's report"
        );
    }

    // A leading `..` is not popped: a path that escapes its base is kept as
    // written rather than flattened into the directory it climbed out of.
    #[test]
    fn a_path_that_climbs_out_of_its_base_is_not_flattened_into_it() {
        let lane = lane_after(&[call(
            "bash",
            "t1",
            1,
            &[("command", "cat ../../secret.txt")],
            Some(0),
            None,
        )]);
        assert_eq!(
            touched(&lane, "../../secret.txt"),
            Some((TouchKind::Read, false)),
            "a path two levels above an unknown cwd was recorded as one inside it"
        );
    }

    // The one that was a bug rather than a gap. `Word.text` is the text
    // after unquoting, so a single-quoted word looks exactly like a
    // substitution to a scanner that does not ask the grammar whether the
    // shell would expand it. It would not: the file was never opened.
    #[test]
    fn a_quoted_substitution_is_three_characters_and_not_a_command() {
        let quoted = lane_after(&[shell("t1", "echo '$(cat notes.txt)'")]);
        assert_eq!(
            touched(&quoted, "/work/notes.txt"),
            None,
            "a single-quoted substitution was descended into: {:?}",
            quoted.files()
        );
        // The same text where the shell really would expand it.
        let expanded = lane_after(&[shell("t1", "echo \"$(cat notes.txt)\"")]);
        assert_eq!(
            touched(&expanded, "/work/notes.txt"),
            Some((TouchKind::Read, true)),
            "a substitution the shell would run was not read: {:?}",
            expanded.files()
        );
    }
}
