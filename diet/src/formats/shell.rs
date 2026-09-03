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
//! honest reading -- and a redirection says which descriptor it moved and to
//! what, so `2>&1` is not mistaken for a file called `1`.
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
    /// Every join, so a projection cannot forget one.
    pub const ALL: &'static [Self] = &[Self::And, Self::Or];

    /// The spelling the value space uses.
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
            Rule::file_op | Rule::dup_op | Rule::heredoc_op => {
                op = RedirOp::from_tag(part.as_str());
            }
            Rule::word => target = Some(Target::File(word(&part)?)),
            Rule::dup_target => target = Some(Target::Descriptor(part.as_str().to_owned())),
            Rule::delim_text => delimiter = Some(part.as_str().to_owned()),
            Rule::heredoc_body => body = Some(part.as_str().to_owned()),
            Rule::heredoc_end => {}
            other => return Err(shape_of(other)),
        }
    }
    if inner.as_rule() == Rule::heredoc {
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

/// A glob character: the shell would replace the word with whatever matches.
fn is_glob(c: char) -> bool {
    matches!(c, '*' | '?' | '[')
}

/// A brace expansion: `{a,b}` becomes two words and `{1..3}` three, and
/// the reader cannot know which without doing the expansion. A brace pair
/// with neither a comma nor a range inside is left alone, as the shell
/// leaves it.
fn has_brace_expansion(run: &str) -> bool {
    let Some(open) = run.find('{') else {
        return false;
    };
    let Some(close) = run[open..].find('}') else {
        return false;
    };
    let inside = &run[open + 1..open + close];
    inside.contains(',') || inside.contains("..")
}

fn word(pair: &Pair<'_, Rule>) -> Result<Word, ParseError> {
    let mut text = String::new();
    let mut literal = true;
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
                        Rule::expansion => literal &= push_expansion(&mut text, &piece)?,
                        Rule::double_run | Rule::double_backslash => text.push_str(piece.as_str()),
                        other => return Err(shape_of(other)),
                    }
                }
            }
            Rule::expansion => literal &= push_expansion(&mut text, &part)?,
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
    Ok(Word { text, literal })
}

/// The character after a backslash, or nothing for a joined line.
fn push_escaped(text: &mut String, escaped: &str) {
    if let Some(rest) = escaped.strip_prefix('\\')
        && rest != "\n"
    {
        text.push_str(rest);
    }
}

/// An expansion, kept as written. Returns whether the word is still literal:
/// a lone `$` is the character itself, and everything else is something the
/// shell would replace.
fn push_expansion(text: &mut String, pair: &Pair<'_, Rule>) -> Result<bool, ParseError> {
    let inner = pair
        .clone()
        .into_inner()
        .next()
        .ok_or(ParseError::Shape("an expansion with no inner rule"))?;
    text.push_str(pair.as_str());
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
    use super::{Command, Join, List, RedirOp, Target, parse};

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

    #[test]
    fn a_background_ampersand_marks_the_chain_before_it() {
        let parsed = parse("sleep 1 & echo done").expect("a list");
        assert!(parsed.items[0].background);
        assert!(!parsed.items[1].background);
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

    #[test]
    fn every_operator_reads_back_from_its_own_spelling() {
        for (op, tag) in RedirOp::ALL {
            assert_eq!(RedirOp::from_tag(tag), Some(*op), "{tag}");
            assert_eq!(op.tag(), *tag);
        }
    }
}
