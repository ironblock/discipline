//! The `shell` format, v0.
//!
//! The grammar at `diet/formats/shell/grammar.pest` is normative. This module
//! implements it and is the **one authorized implementation**: the mechanical
//! lane and the router both read a tool call's command line through here,
//! and neither carries a regex of its own for `cd` or for the command's first
//! word.
//!
//! What the two lanes need decided the shape of the result. The working
//! directory was once an interview question, and the model got it wrong in
//! exactly the way one would expect -- it was reconstructing state it had no
//! reason to track. Deriving it instead is exact only if the command line is
//! read as the shell reads it: `cd a; (cd b; ls); pwd` ends in `a`, because
//! the subshell ran against its own copy of the state. So a [`Subshell`] is a
//! distinct command, not a word that happens to start with a bracket, and a
//! [`Group`] is distinct from it. A word carries whether it is [`Word::literal`]
//! -- `cd $DIR` names a directory the reader cannot know, and saying so is the
//! honest reading -- and it carries the extent of every `$( … )` inside it in
//! [`Word::substitutions`], so no lane has to find one by scanning text the
//! quoting has already been taken out of. A redirection says which descriptor
//! it moved and to what, so `2>&1` is not mistaken for a file called `1`.
//!
//! The router's question -- which command did the model actually want? -- is
//! answered here too, by [`List::producer`], and for the same reason: the
//! answer is neither the first word of the line nor the last simple command
//! written (`cargo test | tail -15` is a test run), and a router that derives
//! it separately is the second reader this module exists to prevent.
//!
//! Control flow is not in v0; its keywords are words. See the grammar's
//! header for what that costs and what is refused outright.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use pest::Parser as _;
use pest::iterators::Pair;
use pest_derive::Parser;

use crate::formats::record::json::Value;

#[derive(Parser)]
#[grammar = "../formats/shell/grammar.pest"]
struct ShellParser;

/// A command line: AND-OR lists in sequence.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct List {
    /// Each AND-OR list, in the order written.
    pub items: Vec<AndOr>,
}

/// Pipelines joined by `&&` and `||`, evaluated left to right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndOr {
    /// The pipelines, each with the operator that joined it to the one before.
    pub chain: Vec<Link>,
    /// Whether `&` followed the whole chain.
    pub background: bool,
}

/// One pipeline in an AND-OR chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// The operator before this pipeline. `None` for the first of a chain.
    pub join: Option<Join>,
    /// The commands, in pipe order.
    pub pipeline: Vec<Command>,
}

/// How a pipeline is joined to the one before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Join {
    /// `&&`: runs only if the one before succeeded.
    And,
    /// `||`: runs only if the one before failed.
    Or,
}

impl Join {
    /// The spelling the value space uses.
    ///
    /// There was a `Join::ALL` here, described as the thing that kept a
    /// projection from forgetting a join. It did not: this match is
    /// exhaustive, so the compiler already refuses a `Join` variant nothing
    /// spells, and nothing anywhere read the table. `RedirOp::ALL` earns its
    /// keep because it is the parse table -- read in both directions by
    /// `tag` and `from_tag` -- and this one was a copy of that shape without
    /// that reason.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::And => "and",
            Self::Or => "or",
        }
    }
}

/// One command in a pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Words, assignments, redirections.
    Simple(Simple),
    /// `( list )`: runs against a copy of the shell's state.
    Subshell(List),
    /// `{ list; }`: runs against the shell's own state.
    Group(List),
}

/// A simple command.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Simple {
    /// `NAME=value` prefixes, in order.
    pub assignments: Vec<Assignment>,
    /// The command word and its operands, in order.
    pub words: Vec<Word>,
    /// Redirections, in order.
    pub redirections: Vec<Redirection>,
}

/// A `NAME=value` prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    /// The variable.
    pub name: String,
    /// Its value. `None` for `NAME=` with nothing after it.
    pub value: Option<Word>,
}

/// One word, with its quoting resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    /// The word as the shell would pass it on, with quotes removed and
    /// escapes resolved -- except that expansions are left as written.
    pub text: String,
    /// Whether `text` is what the shell would actually use. False when the
    /// word contains an expansion, a glob, or a leading tilde: the shell would
    /// replace those with something this reader cannot know, and a lane that
    /// treated the text as a path would be guessing.
    pub literal: bool,
    /// The commands the shell runs before this word becomes a value: the text
    /// between `$(` and its matching `)`, for each `$( … )` the grammar found.
    ///
    /// Carried on the word because quoting is a property of a PART and
    /// `literal` is a property of the whole: `*.rs'$(cat notes.txt)'` is not
    /// literal, because of the glob, and its single-quoted part is still three
    /// characters and not a command. A reader that scanned `text` for `$(`
    /// would run the second one and record a file the shell never opened, and
    /// a reader that counted brackets by hand would stop at the `)` inside
    /// `$(grep ')' f)`. The grammar already decides both, so it is the only
    /// thing that decides them.
    ///
    /// Only `$( … )`. A backtick substitution is an expansion the grammar
    /// reads and this field does not carry, so a lane sees nothing inside one
    /// rather than half of it, and `${X:-$(cat f)}` runs its substitution only
    /// when `X` is unset, which is a condition no reader here can evaluate.
    pub substitutions: Vec<String>,
}

/// A redirection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirection {
    /// The descriptor written before the operator, if any. Its default is
    /// the operator's business (`>` is 1, `<` is 0) and is not filled in here.
    pub fd: Option<u32>,
    /// The operator.
    pub op: RedirOp,
    /// Where it goes.
    pub target: Target,
}

/// A redirection operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RedirOp {
    /// `>`
    Write,
    /// `>>`
    Append,
    /// `>|`
    Clobber,
    /// `&>`
    WriteBoth,
    /// `&>>`
    AppendBoth,
    /// `<`
    Read,
    /// `<<<`
    HereString,
    /// `>&`
    DupOut,
    /// `<&`
    DupIn,
    /// `<<`
    Heredoc,
    /// `<<-`
    HeredocStrip,
}

impl RedirOp {
    /// Every operator, with its spelling. One table, read in both directions.
    pub const ALL: &'static [(Self, &'static str)] = &[
        (Self::Write, ">"),
        (Self::Append, ">>"),
        (Self::Clobber, ">|"),
        (Self::WriteBoth, "&>"),
        (Self::AppendBoth, "&>>"),
        (Self::Read, "<"),
        (Self::HereString, "<<<"),
        (Self::DupOut, ">&"),
        (Self::DupIn, "<&"),
        (Self::Heredoc, "<<"),
        (Self::HeredocStrip, "<<-"),
    ];

    /// The operator as written.
    #[must_use]
    pub fn tag(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(op, _)| *op == self)
            .map_or("", |(_, tag)| tag)
    }

    /// Whether this operator writes to a file the target names.
    #[must_use]
    pub fn writes_file(self) -> bool {
        matches!(
            self,
            Self::Write | Self::Append | Self::Clobber | Self::WriteBoth | Self::AppendBoth
        )
    }

    /// Whether this operator reads a file the target names.
    #[must_use]
    pub fn reads_file(self) -> bool {
        matches!(self, Self::Read)
    }

    fn from_tag(written: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|(_, tag)| *tag == written)
            .map(|(op, _)| *op)
    }
}

/// What a redirection points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A file, named by a word.
    File(Word),
    /// Another descriptor, or `-` to close.
    Descriptor(String),
    /// A heredoc's delimiter and body.
    Heredoc {
        /// The delimiter, without its quotes.
        delimiter: String,
        /// Every line between the operator's line and the terminator.
        body: String,
    },
}

impl List {
    /// Every simple command, in the order written, descending into subshells
    /// and groups. For a reader that wants the words and does not need the
    /// scoping -- the router, asking what the model wanted from the call.
    pub fn simple_commands(&self) -> impl Iterator<Item = &Simple> {
        let mut found = Vec::new();
        collect_simple(self, &mut found);
        found.into_iter()
    }

    /// Whether this list has no commands at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The command whose output the line produces: the FIRST command of the
    /// last pipeline of the last item, descending into a compound producer.
    ///
    /// This is the rule a router classifies by, and it is not the last simple
    /// command written. `cargo test | tail -15` is a test run and not a
    /// `tail`; `cat notes.md | head -20` is a document read and not a `head`.
    /// It lives here rather than in the router because a second reader of a
    /// shell line is what this format exists to prevent, and because the
    /// answer is a property of the line.
    #[must_use]
    pub fn producer(&self) -> Option<&Simple> {
        let tail = self.items.last()?.chain.last()?;
        match tail.pipeline.first()? {
            Command::Simple(simple) => Some(simple),
            Command::Subshell(inner) | Command::Group(inner) => inner.producer(),
        }
    }
}

fn collect_simple<'a>(list: &'a List, into: &mut Vec<&'a Simple>) {
    for and_or in &list.items {
        for link in &and_or.chain {
            for command in &link.pipeline {
                match command {
                    Command::Simple(simple) => into.push(simple),
                    Command::Subshell(inner) | Command::Group(inner) => collect_simple(inner, into),
                }
            }
        }
    }
}

impl Simple {
    /// The command word, if there is one. A command of assignments alone has
    /// none.
    #[must_use]
    pub fn command(&self) -> Option<&Word> {
        self.words.first()
    }

    /// The operands: every word after the command word.
    #[must_use]
    pub fn operands(&self) -> &[Word] {
        self.words.get(1..).unwrap_or(&[])
    }
}

/// Why a text is not a shell command line this format accepts.
#[derive(Debug)]
pub enum ParseError {
    /// The text does not match the grammar.
    Syntax(Box<pest::error::Error<Rule>>),
    /// The grammar matched, and what it matched is a construction v0 reads
    /// but cannot represent. Refused rather than approximated: `|&` after a
    /// subshell pipes the subshell's stderr, and a reading that dropped that
    /// would be a reading that lied about which output went where.
    Unsupported(&'static str),
    /// The grammar matched but produced a shape this module does not expect,
    /// which means the two have drifted apart.
    Shape(&'static str),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(err) => write!(f, "not a shell command line: {err}"),
            Self::Unsupported(what) => write!(f, "outside the shell subset v0 reads: {what}"),
            Self::Shape(what) => write!(f, "grammar and parser disagree: {what}"),
        }
    }
}

impl Error for ParseError {}

/// Parse a command line.
///
/// # Errors
///
/// Returns [`ParseError::Syntax`] when the text is outside the grammar's
/// subset, and [`ParseError::Shape`] if the grammar and this module disagree.
pub fn parse(input: &str) -> Result<List, ParseError> {
    let mut document = ShellParser::parse(Rule::document, input)
        .map_err(|err| ParseError::Syntax(Box::new(err)))?;
    let document = document.next().ok_or(ParseError::Shape("no document"))?;
    match document
        .into_inner()
        .find(|pair| pair.as_rule() == Rule::list)
    {
        Some(pair) => list(&pair),
        None => Ok(List::default()),
    }
}

fn shape_of(rule: Rule) -> ParseError {
    // Diagnostic only, and never matched on: the value is the name of the
    // rule the grammar produced where this module expected another.
    let _ = rule;
    ParseError::Shape("an unexpected rule inside a known one")
}

fn list(pair: &Pair<'_, Rule>) -> Result<List, ParseError> {
    let mut items: Vec<AndOr> = Vec::new();
    for inner in pair.clone().into_inner() {
        match inner.as_rule() {
            Rule::and_or => items.push(and_or(&inner)?),
            Rule::sep => {
                // `&` applies to the chain just before it. A separator is
                // only reached after an and_or, so the last item exists.
                let backgrounded = inner
                    .into_inner()
                    .any(|part| part.as_rule() == Rule::background);
                if backgrounded {
                    items
                        .last_mut()
                        .ok_or(ParseError::Shape("a separator before any command"))?
                        .background = true;
                }
            }
            other => return Err(shape_of(other)),
        }
    }
    Ok(List { items })
}

fn and_or(pair: &Pair<'_, Rule>) -> Result<AndOr, ParseError> {
    let mut chain = Vec::new();
    let mut pending: Option<Join> = None;
    for inner in pair.clone().into_inner() {
        match inner.as_rule() {
            Rule::pipeline => {
                chain.push(Link {
                    join: pending.take(),
                    pipeline: pipeline(&inner)?,
                });
            }
            Rule::and_or_op => {
                let op = inner
                    .into_inner()
                    .next()
                    .ok_or(ParseError::Shape("an operator with no spelling"))?;
                pending = Some(match op.as_rule() {
                    Rule::and => Join::And,
                    Rule::or => Join::Or,
                    other => return Err(shape_of(other)),
                });
            }
            other => return Err(shape_of(other)),
        }
    }
    Ok(AndOr {
        chain,
        background: false,
    })
}

fn pipeline(pair: &Pair<'_, Rule>) -> Result<Vec<Command>, ParseError> {
    let mut commands = Vec::new();
    for inner in pair.clone().into_inner() {
        match inner.as_rule() {
            Rule::command => commands.push(command(&inner)?),
            // `|&` is the shell's abbreviation for `2>&1 |`, and it is read
            // as exactly that: the command before it gains the duplication.
            // Not a flag on the pipe, because a flag would be a second way
            // to say what a redirection already says, and the lane that
            // asks "where did stderr go" would have to check both.
            Rule::pipe if inner.as_str().len() == 2 => match commands.last_mut() {
                Some(Command::Simple(simple)) => simple.redirections.push(Redirection {
                    fd: Some(2),
                    op: RedirOp::DupOut,
                    target: Target::Descriptor("1".to_owned()),
                }),
                Some(Command::Subshell(_) | Command::Group(_)) => {
                    return Err(ParseError::Unsupported("`|&` after a subshell or group"));
                }
                None => return Err(ParseError::Shape("a pipe before any command")),
            },
            Rule::pipe => {}
            other => return Err(shape_of(other)),
        }
    }
    Ok(commands)
}

fn command(pair: &Pair<'_, Rule>) -> Result<Command, ParseError> {
    let inner = pair
        .clone()
        .into_inner()
        .next()
        .ok_or(ParseError::Shape("a command with nothing in it"))?;
    match inner.as_rule() {
        Rule::simple => simple(&inner).map(Command::Simple),
        Rule::subshell => nested_list(&inner).map(Command::Subshell),
        Rule::group => nested_list(&inner).map(Command::Group),
        other => Err(shape_of(other)),
    }
}

fn nested_list(pair: &Pair<'_, Rule>) -> Result<List, ParseError> {
    let inner = pair
        .clone()
        .into_inner()
        .find(|part| part.as_rule() == Rule::list)
        .ok_or(ParseError::Shape("a bracket with no list inside"))?;
    list(&inner)
}

fn simple(pair: &Pair<'_, Rule>) -> Result<Simple, ParseError> {
    let mut built = Simple::default();
    for inner in pair.clone().into_inner() {
        match inner.as_rule() {
            Rule::assignment => built.assignments.push(assignment(&inner)?),
            Rule::element => {
                let part = inner
                    .into_inner()
                    .next()
                    .ok_or(ParseError::Shape("an element with nothing in it"))?;
                match part.as_rule() {
                    Rule::word => built.words.push(word(&part)?),
                    Rule::redirection => built.redirections.push(redirection(&part)?),
                    other => return Err(shape_of(other)),
                }
            }
            other => return Err(shape_of(other)),
        }
    }
    Ok(built)
}

fn assignment(pair: &Pair<'_, Rule>) -> Result<Assignment, ParseError> {
    let mut name = None;
    let mut value = None;
    for inner in pair.clone().into_inner() {
        match inner.as_rule() {
            Rule::name => name = Some(inner.as_str().to_owned()),
            Rule::word => value = Some(word(&inner)?),
            other => return Err(shape_of(other)),
        }
    }
    Ok(Assignment {
        name: name.ok_or(ParseError::Shape("an assignment with no name"))?,
        value,
    })
}

fn redirection(pair: &Pair<'_, Rule>) -> Result<Redirection, ParseError> {
    let inner = pair
        .clone()
        .into_inner()
        .next()
        .ok_or(ParseError::Shape("a redirection with nothing in it"))?;
    let mut fd = None;
    let mut op = None;
    let mut target = None;
    let mut delimiter = None;
    let mut body = None;
    for part in inner.clone().into_inner() {
        match part.as_rule() {
            Rule::fd => {
                fd = Some(
                    part.as_str()
                        .parse::<u32>()
                        .map_err(|_| ParseError::Shape("a descriptor number that does not fit"))?,
                );
            }
            Rule::file_op | Rule::dup_op | Rule::heredoc_op | Rule::strip_op => {
                op = RedirOp::from_tag(part.as_str());
            }
            Rule::word => target = Some(Target::File(word(&part)?)),
            Rule::dup_target => target = Some(Target::Descriptor(part.as_str().to_owned())),
            Rule::delim_text => delimiter = Some(part.as_str().to_owned()),
            Rule::heredoc_body => body = Some(part.as_str().to_owned()),
            Rule::strip_body => body = Some(strip_leading_tabs(part.as_str())),
            Rule::heredoc_end | Rule::strip_end => {}
            other => return Err(shape_of(other)),
        }
    }
    if matches!(inner.as_rule(), Rule::heredoc_plain | Rule::heredoc_strip) {
        target = Some(Target::Heredoc {
            delimiter: delimiter.ok_or(ParseError::Shape("a heredoc with no delimiter"))?,
            body: body.unwrap_or_default(),
        });
    }
    Ok(Redirection {
        fd,
        op: op.ok_or(ParseError::Shape("a redirection with no operator"))?,
        target: target.ok_or(ParseError::Shape("a redirection with no target"))?,
    })
}

/// A `<<-` body, with the leading tabs the shell strips taken off each line.
///
/// The grammar accepts the tabs on the terminator; this takes them off the
/// body, because the two halves of `<<-` are one operator and a reader that
/// got the terminator right and the body wrong would be recording a body that
/// never reached the command.
fn strip_leading_tabs(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    for line in body.split_inclusive('\n') {
        out.push_str(line.trim_start_matches('\t'));
    }
    out
}

/// A glob character: the shell would replace the word with whatever matches.
fn is_glob(c: char) -> bool {
    matches!(c, '*' | '?' | '[')
}

/// A brace expansion: `{a,b}` becomes two words and `{1..3}` three, and
/// the reader cannot know which without doing the expansion. A brace pair
/// with neither a comma nor a range inside is left alone, as the shell
/// leaves it.
///
/// EVERY pair is inspected, not the first. This looked at the first pair
/// once, so `x{y}z{a,b}` reported literal on the strength of `y` while the
/// shell expanded it to two words -- the exact overclaim this module's own
/// seeded fault is about, shipped inside the check that fault protects.
fn has_brace_expansion(run: &str) -> bool {
    let bytes = run.as_bytes();
    let mut at = 0;
    while let Some(open) = run[at..].find('{') {
        let open = at + open;
        let Some(close) = run[open..].find('}') else {
            return false;
        };
        let close = open + close;
        let inside = &run[open + 1..close];
        if inside.contains(',') || inside.contains("..") {
            return true;
        }
        // Past this pair's closing brace, so a later pair is still seen.
        at = close + 1;
        if at >= bytes.len() {
            break;
        }
    }
    false
}

fn word(pair: &Pair<'_, Rule>) -> Result<Word, ParseError> {
    let mut text = String::new();
    let mut literal = true;
    let mut substitutions = Vec::new();
    let mut first = true;
    for part in pair.clone().into_inner() {
        match part.as_rule() {
            Rule::single_quoted => {
                let inner = part
                    .into_inner()
                    .next()
                    .ok_or(ParseError::Shape("single quotes with no inner rule"))?;
                text.push_str(inner.as_str());
            }
            Rule::double_quoted => {
                for piece in part.into_inner() {
                    match piece.as_rule() {
                        Rule::double_escape => push_escaped(&mut text, piece.as_str()),
                        Rule::expansion => {
                            literal &= push_expansion(&mut text, &piece, &mut substitutions)?;
                        }
                        Rule::double_run | Rule::double_backslash => text.push_str(piece.as_str()),
                        other => return Err(shape_of(other)),
                    }
                }
            }
            Rule::expansion => literal &= push_expansion(&mut text, &part, &mut substitutions)?,
            Rule::escaped => push_escaped(&mut text, part.as_str()),
            Rule::bare => {
                let run = part.as_str();
                if run.contains(is_glob)
                    || has_brace_expansion(run)
                    || (first && run.starts_with('~'))
                {
                    literal = false;
                }
                text.push_str(run);
            }
            other => return Err(shape_of(other)),
        }
        first = false;
    }
    Ok(Word {
        text,
        literal,
        substitutions,
    })
}

/// What a command substitution opens and closes with.
const SUBSTITUTION_OPEN: &str = "$(";
const SUBSTITUTION_CLOSE: &str = ")";

/// The character after a backslash, or nothing for a joined line.
///
/// A line ending is CRLF or LF, matching `nl`: a backslash before a CRLF was
/// once an escaped carriage return, so a continued line written on Windows put
/// a `\r` in the middle of a word and ended the command on the newline.
fn push_escaped(text: &mut String, escaped: &str) {
    if let Some(rest) = escaped.strip_prefix('\\')
        && rest != "\n"
        && rest != "\r\n"
    {
        text.push_str(rest);
    }
}

/// An expansion, kept as written. Returns whether the word is still literal:
/// a lone `$` is the character itself, and everything else is something the
/// shell would replace.
///
/// A `$( … )` also leaves its inner text behind, because the grammar is where
/// its extent is decided: `paren_inner` already knows that a `)` inside quotes
/// closes nothing, and every reader that counted brackets for itself got that
/// wrong.
fn push_expansion(
    text: &mut String,
    pair: &Pair<'_, Rule>,
    substitutions: &mut Vec<String>,
) -> Result<bool, ParseError> {
    let inner = pair
        .clone()
        .into_inner()
        .next()
        .ok_or(ParseError::Shape("an expansion with no inner rule"))?;
    text.push_str(pair.as_str());
    if inner.as_rule() == Rule::dollar_paren {
        let whole = inner.as_str();
        let opened = whole
            .strip_prefix(SUBSTITUTION_OPEN)
            .and_then(|rest| rest.strip_suffix(SUBSTITUTION_CLOSE))
            .ok_or(ParseError::Shape("a substitution without its brackets"))?;
        substitutions.push(opened.to_owned());
    }
    Ok(inner.as_rule() == Rule::dollar_alone)
}

// ---------------------------------------------------------------------------
// projection
// ---------------------------------------------------------------------------

/// This command line, as the record's value space.
///
/// # Errors
///
/// Returns the reason the text is not a shell command line.
pub fn project(source: &str) -> Result<Value, String> {
    parse(source)
        .map(|parsed| Value::Object(BTreeMap::from([("list".to_owned(), list_value(&parsed))])))
        .map_err(|err| err.to_string())
}

fn list_value(list: &List) -> Value {
    Value::Array(
        list.items
            .iter()
            .map(|and_or| {
                Value::Object(BTreeMap::from([
                    ("background".to_owned(), Value::Boolean(and_or.background)),
                    (
                        "chain".to_owned(),
                        Value::Array(and_or.chain.iter().map(link_value).collect()),
                    ),
                ]))
            })
            .collect(),
    )
}

fn link_value(link: &Link) -> Value {
    let mut members = BTreeMap::from([(
        "pipeline".to_owned(),
        Value::Array(link.pipeline.iter().map(command_value).collect()),
    )]);
    if let Some(join) = link.join {
        members.insert("join".to_owned(), Value::String(join.tag().to_owned()));
    }
    Value::Object(members)
}

fn command_value(command: &Command) -> Value {
    let (key, value) = match command {
        Command::Simple(simple) => ("simple", simple_value(simple)),
        Command::Subshell(list) => ("subshell", list_value(list)),
        Command::Group(list) => ("group", list_value(list)),
    };
    Value::Object(BTreeMap::from([(key.to_owned(), value)]))
}

fn simple_value(simple: &Simple) -> Value {
    Value::Object(BTreeMap::from([
        (
            "assignments".to_owned(),
            Value::Array(
                simple
                    .assignments
                    .iter()
                    .map(|assignment| {
                        let mut members = BTreeMap::from([(
                            "name".to_owned(),
                            Value::String(assignment.name.clone()),
                        )]);
                        if let Some(value) = &assignment.value {
                            members.insert("value".to_owned(), word_value(value));
                        }
                        Value::Object(members)
                    })
                    .collect(),
            ),
        ),
        (
            "words".to_owned(),
            Value::Array(simple.words.iter().map(word_value).collect()),
        ),
        (
            "redirections".to_owned(),
            Value::Array(simple.redirections.iter().map(redirection_value).collect()),
        ),
    ]))
}

fn word_value(word: &Word) -> Value {
    Value::Object(BTreeMap::from([
        ("text".to_owned(), Value::String(word.text.clone())),
        ("literal".to_owned(), Value::Boolean(word.literal)),
    ]))
}

fn redirection_value(redirection: &Redirection) -> Value {
    let mut members = BTreeMap::from([(
        "op".to_owned(),
        Value::String(redirection.op.tag().to_owned()),
    )]);
    if let Some(fd) = redirection.fd {
        members.insert("fd".to_owned(), Value::Integer(i64::from(fd)));
    }
    let (key, value) = match &redirection.target {
        Target::File(word) => ("file", word_value(word)),
        Target::Descriptor(descriptor) => ("descriptor", Value::String(descriptor.clone())),
        Target::Heredoc { delimiter, body } => (
            "heredoc",
            Value::Object(BTreeMap::from([
                ("delimiter".to_owned(), Value::String(delimiter.clone())),
                ("body".to_owned(), Value::String(body.clone())),
            ])),
        ),
    };
    members.insert(key.to_owned(), value);
    Value::Object(members)
}

#[cfg(test)]
mod tests {
    use super::{Command, Join, List, RedirOp, Simple, Target, parse};

    fn words(list: &List) -> Vec<Vec<&str>> {
        list.simple_commands()
            .map(|simple| simple.words.iter().map(|word| word.text.as_str()).collect())
            .collect()
    }

    // The case the grammar exists for.
    #[test]
    fn a_subshell_is_a_command_of_its_own_and_not_a_word() {
        let parsed = parse("cd a; (cd b; ls); pwd").expect("a list");
        assert_eq!(parsed.items.len(), 3);
        assert!(
            matches!(parsed.items[1].chain[0].pipeline[0], Command::Subshell(_)),
            "the bracketed list was not read as a subshell"
        );
        assert_eq!(
            words(&parsed),
            vec![vec!["cd", "a"], vec!["cd", "b"], vec!["ls"], vec!["pwd"]]
        );
    }

    #[test]
    fn and_or_chains_keep_their_operators_and_their_order() {
        let parsed = parse("cd diet && cargo test || echo failed").expect("a list");
        let chain = &parsed.items[0].chain;
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[0].join, None);
        assert_eq!(chain[1].join, Some(Join::And));
        assert_eq!(chain[2].join, Some(Join::Or));
    }

    /// `&` marks the chain BEFORE it, and `;` marks nothing. The pair is
    /// what makes the test able to fail: checking only the `&` line left a
    /// reader that set `background` from "there was a separator at all"
    /// passing, and the two lines differ by one character.
    #[test]
    fn a_background_ampersand_marks_the_chain_before_it_and_a_semicolon_does_not() {
        let parsed = parse("sleep 1 & echo done").expect("a list");
        assert_eq!(parsed.items.len(), 2, "`&` did not separate the two lists");
        assert!(parsed.items[0].background, "`sleep 1` was not backgrounded");
        assert!(!parsed.items[1].background, "`echo done` was backgrounded");

        let sequenced = parse("sleep 1 ; echo done").expect("a list");
        assert_eq!(sequenced.items.len(), 2);
        assert!(
            sequenced.items.iter().all(|item| !item.background),
            "`;` backgrounded something"
        );

        // A trailing `&` has nothing after it and still marks what it follows.
        let trailing = parse("cargo test &").expect("a list");
        assert_eq!(trailing.items.len(), 1);
        assert!(trailing.items[0].background);
    }

    /// `<<-` is the operator that strips leading tabs, from the body and from
    /// its own terminator. Both halves, because v0 shipped the spelling with
    /// neither: the operator was recorded, the body kept its tabs, and the
    /// tab-indented terminator every real `<<-` has was refused.
    #[test]
    fn a_stripping_heredoc_strips_its_tabs_and_a_plain_one_keeps_them() {
        let parsed = parse("cat <<-END\n\tindented\n\tEND\n").expect("a list");
        let simple = parsed.simple_commands().next().expect("one command");
        assert_eq!(simple.redirections[0].op, RedirOp::HeredocStrip);
        assert!(
            matches!(
                &simple.redirections[0].target,
                Target::Heredoc { body, .. } if body == "indented\n"
            ),
            "`<<-` did not strip the tabs the shell strips"
        );

        let plain = parse("cat <<END\n\tindented\nEND\n").expect("a list");
        let simple = plain.simple_commands().next().expect("one command");
        assert_eq!(simple.redirections[0].op, RedirOp::Heredoc);
        assert!(
            matches!(
                &simple.redirections[0].target,
                Target::Heredoc { body, .. } if body == "\tindented\n"
            ),
            "`<<` stripped a tab it must keep"
        );

        // Only `<<-` may have its terminator indented; `<<` reads the
        // indented line as body and then never finds its delimiter.
        assert!(parse("cat <<END\n\tbody\n\tEND\n").is_err());
    }

    /// A carriage return belongs to the line ending and nowhere else. It was
    /// horizontal space once, which split `echo a\rb` into three words where
    /// the shell gives two -- a wrong reading of the one input class (a file
    /// written on Windows) the allowance existed to serve.
    #[test]
    fn a_carriage_return_ends_a_line_and_is_otherwise_an_ordinary_character() {
        let crlf = parse("ls\r\npwd\r\n").expect("a list");
        assert_eq!(words(&crlf), vec![vec!["ls"], vec!["pwd"]]);

        let inside = parse("echo a\rb\n").expect("a list");
        assert_eq!(words(&inside), vec![vec!["echo", "a\rb"]]);

        // A backslash before a CRLF joins the lines, as it does before a LF.
        let continued = parse("cargo test \\\r\n  --workspace\r\n").expect("a list");
        assert_eq!(
            words(&continued),
            vec![vec!["cargo", "test", "--workspace"]]
        );
    }

    /// A tilde expands only at the start of a word, so only there does it
    /// cost the word its literalness.
    #[test]
    fn a_tilde_is_an_expansion_only_where_the_shell_expands_it() {
        let parsed = parse(r#"cd ~/work && ls ~ a~b "x"~"#).expect("a list");
        let literal: Vec<(&str, bool)> = parsed
            .simple_commands()
            .flat_map(|simple| simple.words.iter())
            .map(|word| (word.text.as_str(), word.literal))
            .collect();
        assert_eq!(
            literal,
            vec![
                ("cd", true),
                ("~/work", false),
                ("ls", true),
                ("~", false),
                ("a~b", true),
                ("x~", true),
            ]
        );
    }

    /// The three glob characters, each of which costs a word its
    /// literalness. `[` had no fixture and no test: it could be dropped from
    /// `is_glob` and every gate stayed green.
    #[test]
    fn each_glob_character_makes_its_word_unliteral() {
        let parsed = parse("rm -f *.tmp build/?.o log[0-9].txt plain.txt").expect("a list");
        let simple = parsed.simple_commands().next().expect("one command");
        let literal: Vec<(&str, bool)> = simple
            .words
            .iter()
            .map(|word| (word.text.as_str(), word.literal))
            .collect();
        assert_eq!(
            literal,
            vec![
                ("rm", true),
                ("-f", true),
                ("*.tmp", false),
                ("build/?.o", false),
                ("log[0-9].txt", false),
                ("plain.txt", true),
            ]
        );
    }

    #[test]
    fn quoting_is_resolved_and_expansions_make_a_word_unliteral() {
        let parsed = parse(r#"cd 'a b' "$HOME/x" c\ d $DIR "lit\"eral" ~/e f*"#).expect("a list");
        let simple = parsed.simple_commands().next().expect("one command");
        let texts: Vec<(&str, bool)> = simple
            .words
            .iter()
            .map(|word| (word.text.as_str(), word.literal))
            .collect();
        assert_eq!(
            texts,
            vec![
                ("cd", true),
                ("a b", true),
                ("$HOME/x", false),
                ("c d", true),
                ("$DIR", false),
                ("lit\"eral", true),
                ("~/e", false),
                ("f*", false),
            ]
        );
    }

    // A word's substitutions come from the grammar's own `dollar_paren`
    // extents, and not from a scan of the text it produced. `literal` is a
    // property of the whole word and quoting is a property of a part, so a
    // scan cannot tell `'$(cat f)'` -- three characters -- from the command
    // beside it; and a scan that counted brackets stops at the `)` inside
    // `$(grep ')' f)`.
    #[test]
    fn a_words_substitutions_are_the_ones_the_grammar_found() {
        let cases: &[(&str, Vec<Vec<&str>>)] = &[
            ("echo $(cat a.txt)", vec![vec![], vec!["cat a.txt"]]),
            ("echo \"$(cat a.txt)\"", vec![vec![], vec!["cat a.txt"]]),
            ("echo '$(cat a.txt)'", vec![vec![], vec![]]),
            ("echo *.rs'$(cat a.txt)'", vec![vec![], vec![]]),
            (
                "echo \"$(grep ')' a.txt)\"",
                vec![vec![], vec!["grep ')' a.txt"]],
            ),
            (
                "echo $(cat a.txt)$(cat b.txt)",
                vec![vec![], vec!["cat a.txt", "cat b.txt"]],
            ),
            (
                "echo $(echo $(cat a.txt) done)",
                vec![vec![], vec!["echo $(cat a.txt) done"]],
            ),
            ("echo ${X:-$(cat a.txt)}", vec![vec![], vec![]]),
            ("echo `cat a.txt`", vec![vec![], vec![]]),
        ];
        for (line, expected) in cases {
            let parsed = parse(line).unwrap_or_else(|err| panic!("{line}: {err}"));
            let simple = parsed.simple_commands().next().expect("one command");
            let found: Vec<Vec<&str>> = simple
                .words
                .iter()
                .map(|word| word.substitutions.iter().map(String::as_str).collect())
                .collect();
            assert_eq!(&found, expected, "{line}");
        }
        let parsed = parse("V=$(pwd) cat < $(echo f.txt)").expect("a list");
        let simple = parsed.simple_commands().next().expect("one command");
        let value = simple.assignments[0].value.as_ref().expect("a value");
        assert_eq!(value.substitutions, vec!["pwd".to_owned()]);
        let Target::File(target) = &simple.redirections[0].target else {
            panic!("a file target")
        };
        assert_eq!(target.substitutions, vec!["echo f.txt".to_owned()]);
    }

    #[test]
    fn a_stderr_pipe_is_the_duplication_it_abbreviates() {
        let parsed = parse("cargo build |& tail -3").expect("a list");
        let build = parsed.simple_commands().next().expect("the first command");
        assert_eq!(
            build.redirections.len(),
            1,
            "`|&` did not become the duplication it abbreviates"
        );
        assert_eq!(build.redirections[0].fd, Some(2));
        assert_eq!(build.redirections[0].op, RedirOp::DupOut);
        assert!(matches!(
            parse("(cargo build) |& tail -3"),
            Err(super::ParseError::Unsupported(_))
        ));
    }

    #[test]
    fn a_brace_expansion_is_not_the_word_it_looks_like() {
        let parsed = parse("echo {a,b} {1..3} {x} ${VAR}").expect("a list");
        let simple = parsed.simple_commands().next().expect("one command");
        let literal: Vec<bool> = simple.words.iter().map(|word| word.literal).collect();
        assert_eq!(
            literal,
            vec![true, false, false, true, false],
            "a word the shell would expand was reported literal"
        );
    }

    #[test]
    fn a_descriptor_duplication_is_not_a_file_called_one() {
        let parsed = parse("cargo test 2>&1 >out.txt").expect("a list");
        let simple = parsed.simple_commands().next().expect("one command");
        assert_eq!(simple.redirections.len(), 2);
        assert_eq!(simple.redirections[0].fd, Some(2));
        assert_eq!(simple.redirections[0].op, RedirOp::DupOut);
        assert!(matches!(
            &simple.redirections[0].target,
            Target::Descriptor(d) if d == "1"
        ));
        assert!(matches!(
            &simple.redirections[1].target,
            Target::File(word) if word.text == "out.txt"
        ));
        assert!(simple.redirections[1].op.writes_file());
    }

    #[test]
    fn a_heredoc_body_runs_to_its_own_delimiter() {
        let parsed = parse("python3 - <<'EOF'\nprint(1)\nEOF\nls\n").expect("a list");
        assert_eq!(parsed.items.len(), 2);
        let simple = parsed.simple_commands().next().expect("one command");
        assert!(matches!(
            &simple.redirections[0].target,
            Target::Heredoc { delimiter, body } if delimiter == "EOF" && body == "print(1)\n"
        ));
    }

    #[test]
    fn an_empty_document_is_an_empty_list() {
        assert!(parse("").expect("empty").is_empty());
        assert!(parse("  \n# only a comment\n").expect("comment").is_empty());
    }

    #[test]
    fn what_is_refused_is_refused() {
        for text in [
            "; ls",
            "ls &&",
            "echo 'unterminated",
            "(cd a; ls",
            "echo >",
            "()",
            "cat <<EOF | sort\nx\nEOF\n",
            "ls | | wc",
        ] {
            assert!(parse(text).is_err(), "{text:?} must be refused");
        }
    }

    /// Every operator, written out HERE rather than read from the table
    /// under test. The version of this that looped over `RedirOp::ALL` to
    /// check `RedirOp::ALL` asserted nothing a mutation could break: empty
    /// the table and the loop passes on zero rounds, reorder it and it
    /// passes, drop a row and it passes. The eleven spellings are the claim,
    /// so the eleven spellings are in the test.
    #[test]
    fn every_operator_is_named_here_and_the_table_says_the_same() {
        let named = [
            (RedirOp::Write, ">"),
            (RedirOp::Append, ">>"),
            (RedirOp::Clobber, ">|"),
            (RedirOp::WriteBoth, "&>"),
            (RedirOp::AppendBoth, "&>>"),
            (RedirOp::Read, "<"),
            (RedirOp::HereString, "<<<"),
            (RedirOp::DupOut, ">&"),
            (RedirOp::DupIn, "<&"),
            (RedirOp::Heredoc, "<<"),
            (RedirOp::HeredocStrip, "<<-"),
        ];
        assert_eq!(
            RedirOp::ALL.len(),
            named.len(),
            "the table gained or lost an operator and this test was not told"
        );
        for (op, tag) in named {
            assert_eq!(op.tag(), tag, "{tag} is not what the table spells it");
            assert_eq!(RedirOp::from_tag(tag), Some(op), "{tag} does not read back");
        }
    }

    /// Which operators name a file on disk, as three named groups rather
    /// than as the two predicates restated. The lengths add up to the whole
    /// table, so an operator added tomorrow cannot arrive unclassified.
    #[test]
    fn naming_a_file_is_a_partition_of_the_operators() {
        let writes = [
            RedirOp::Write,
            RedirOp::Append,
            RedirOp::Clobber,
            RedirOp::WriteBoth,
            RedirOp::AppendBoth,
        ];
        let reads = [RedirOp::Read];
        // A duplication names a descriptor, a here-document carries its own
        // body, and a here-string's target is the text itself: none of them
        // is a path the mechanical lane may record as touched.
        let neither = [
            RedirOp::HereString,
            RedirOp::DupOut,
            RedirOp::DupIn,
            RedirOp::Heredoc,
            RedirOp::HeredocStrip,
        ];
        assert_eq!(
            writes.len() + reads.len() + neither.len(),
            RedirOp::ALL.len(),
            "an operator is in the table and in none of these three groups"
        );
        for op in writes {
            assert!(op.writes_file(), "{} must write a file", op.tag());
            assert!(!op.reads_file(), "{} must not read a file", op.tag());
        }
        for op in reads {
            assert!(op.reads_file(), "{} must read a file", op.tag());
            assert!(!op.writes_file(), "{} must not write a file", op.tag());
        }
        for op in neither {
            assert!(!op.writes_file(), "{} names no file to write", op.tag());
            assert!(!op.reads_file(), "{} names no file to read", op.tag());
        }
    }

    /// The two accessors the mechanical lane reads a command through. Both
    /// were unpinned: emptying either left the whole gate green.
    #[test]
    fn the_command_word_is_the_first_word_and_the_operands_are_the_rest() {
        let parsed = parse("CARGO_TARGET_DIR=/work/target cargo test --workspace -- capture")
            .expect("a list");
        let simple = parsed.simple_commands().next().expect("one command");
        assert_eq!(
            simple.command().map(|word| word.text.as_str()),
            Some("cargo"),
            "an assignment prefix is not the command word"
        );
        assert_eq!(
            operands_of(simple),
            vec!["test", "--workspace", "--", "capture"],
            "the command word is not one of its own operands"
        );

        // A command of assignments alone runs nothing, so it has no command
        // word and no operands -- not a first word that happens to be blank.
        let only = parse("FOO=bar").expect("a list");
        let simple = only.simple_commands().next().expect("one command");
        assert_eq!(simple.command(), None);
        assert!(simple.operands().is_empty());

        // One word is a command and no operands, which is the case an
        // off-by-one in either accessor gets wrong.
        let bare = parse("pwd").expect("a list");
        let simple = bare.simple_commands().next().expect("one command");
        assert_eq!(simple.command().map(|word| word.text.as_str()), Some("pwd"));
        assert!(simple.operands().is_empty());
    }

    /// The rule the router classifies by, in the cases where the answer is
    /// not the first word and not the last command written.
    #[test]
    fn the_producer_is_the_head_of_the_last_pipeline_and_not_its_tail() {
        for (line, want) in [
            ("cargo test | tail -15", "cargo"),
            ("cat notes.md | head -20", "cat"),
            ("cd diet && cargo test", "cargo"),
            ("cargo build |& tail -3", "cargo"),
            ("ls; (cd a && git status --short)", "git"),
            ("ls; { cd a; pwd; }", "pwd"),
            ("pwd", "pwd"),
        ] {
            let parsed = parse(line).expect("a list");
            assert_eq!(
                parsed
                    .producer()
                    .and_then(|simple| simple.command())
                    .map(|word| word.text.as_str()),
                Some(want),
                "{line:?} produces its output with {want}"
            );
        }
        assert!(
            parse("").expect("empty").producer().is_none(),
            "a document with no command produces nothing"
        );
    }

    /// No refusal in the invalid corpus is a [`ParseError::Shape`].
    ///
    /// The three refusals are not equals. `Syntax` is the grammar's, and is
    /// the ordinary one. `Unsupported` is v0 declining to read a document the
    /// grammar accepts -- `|&` after a subshell, which has nowhere on a
    /// compound command to record the duplication -- and is a decision the
    /// grammar's header states, so the fixtures that carry it are named here.
    /// `Shape` is neither: it means the text parsed and the tree was not the
    /// shape this module expected, which is drift between the normative file
    /// and its implementation and never a fact about the input.
    ///
    /// The conformance harness cannot tell the three apart -- it asks only
    /// that an invalid fixture be rejected -- so a widened grammar keeps the
    /// corpus green while the reason files describe rules nothing reaches.
    /// Widen `subshell` to `"(" ~ gap* ~ list? ~ gap* ~ close_paren` and `()`
    /// is still refused, by `a bracket with no list inside` from here, rather
    /// than by the rule `empty-subshell.reason` names. This test is what
    /// notices.
    #[test]
    fn no_refusal_in_the_invalid_corpus_is_the_parsers_own() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("formats/shell/fixtures/invalid");
        let v0_declines = ["pipe-both-after-subshell.sh"];
        let mut checked = 0;
        let mut seen_declined = 0;
        for entry in std::fs::read_dir(&dir).expect("the invalid fixture directory") {
            let path = entry.expect("a readable entry").path();
            if path.extension().is_none_or(|ext| ext != "sh") {
                continue;
            }
            let name = path
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .expect("a named fixture")
                .to_owned();
            let bytes = std::fs::read(&path).expect("a readable fixture");
            // A fixture that is not text never reaches the grammar: its
            // refusal is the decode, which the CLI and the harness do.
            let Ok(text) = std::str::from_utf8(&bytes) else {
                continue;
            };
            checked += 1;
            let declines = v0_declines.contains(&name.as_str());
            match parse(text) {
                Err(super::ParseError::Syntax(_)) => assert!(
                    !declines,
                    "{name}: named as declined by v0, but the grammar refuses it"
                ),
                Err(super::ParseError::Unsupported(reason)) => {
                    assert!(
                        declines,
                        "{name}: refused as unsupported ({reason}) and not named as declined"
                    );
                    seen_declined += 1;
                }
                Err(super::ParseError::Shape(reason)) => panic!(
                    "{name}: refused by the parser ({reason}) and not by the grammar; \
                     the grammar accepted a document its reason file says it refuses"
                ),
                Ok(_) => panic!("{name}: accepted"),
            }
        }
        assert_eq!(
            seen_declined,
            v0_declines.len(),
            "a fixture named as declined by v0 is no longer refused that way"
        );
        assert!(
            checked >= 15,
            "only {checked} invalid fixtures were text; the corpus is not being read"
        );
    }

    /// `}` closes a group where a command starts and is an ordinary word
    /// everywhere else, which is the mirror of `{`. The two were asymmetric:
    /// `echo { a` parsed and `echo }` did not, although the shell reads both
    /// as three words and two.
    #[test]
    fn a_closing_brace_closes_a_group_and_is_otherwise_a_word() {
        assert_eq!(
            words(&parse("echo }").expect("a list")),
            vec![vec!["echo", "}"]]
        );
        assert_eq!(
            words(&parse("echo { a").expect("a list")),
            vec![vec!["echo", "{", "a"]]
        );

        let group = parse("{ cd a; ls; }").expect("a list");
        assert!(matches!(
            group.items[0].chain[0].pipeline[0],
            Command::Group(_)
        ));
        assert_eq!(words(&group), vec![vec!["cd", "a"], vec!["ls"]]);

        // Still refused where it would have to be a command: an unterminated
        // group must not become a command called `}`.
        assert!(parse("{ ls").is_err());
        assert!(parse("ls; }").is_err());
    }

    /// A backslash before a newline INSIDE a word joins the two halves into
    /// one word. `line-continuation.sh` only covers the continuation between
    /// words, which `hs` handles; this is the `escaped` path, and dropping
    /// its guard left `echo foo\<newline>bar` as a word with a newline in it.
    #[test]
    fn a_continuation_inside_a_word_joins_it() {
        let parsed = parse("echo foo\\\nbar baz").expect("a list");
        assert_eq!(words(&parsed), vec![vec!["echo", "foobar", "baz"]]);
    }

    fn operands_of(simple: &Simple) -> Vec<&str> {
        simple
            .operands()
            .iter()
            .map(|word| word.text.as_str())
            .collect()
    }
}
