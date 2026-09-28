//! What a scripted drive does, as data.
//!
//! A drive is scripted so that the *machinery* can be exercised without a
//! model. #23's whole argument is that iterating on routers, reconcilers and
//! seams does not need a capable substrate -- it needs a protocol-compliant
//! one -- and the floor below that is a substrate whose every answer is
//! written down in advance. What a script cannot do is decide anything: the
//! turns it declares are put to whatever is serving, and what comes back is
//! what comes back.

use crate::formats::record::Regime;

/// A scripted session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Script {
    /// The regime every row of the record is tagged with.
    ///
    /// Carried by the script rather than derived from the server, because a
    /// regime is a declaration about how a session was run and a server's
    /// report of itself is evidence about part of it. The client checks the
    /// second against the first; nothing here invents either.
    pub regime: Regime,
    /// The turns, in order. Their indices are 1, 2, 3 by position: the record
    /// requires it, so the script does not get to disagree.
    pub turns: Vec<Turn>,
}

/// One turn of a scripted session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    /// What the operator says this turn.
    pub ask: String,
    /// The commands the turn runs, in order, under the confinement.
    pub commands: Vec<Command>,
    /// The interview fork this turn opens, if it opens one, and what it asks.
    pub fork: Option<String>,
    /// Whether the operator declares a boundary before this turn settles.
    ///
    /// The operator's own declaration is the highest-precedence seam trigger,
    /// so a script that wants a seam at a KNOWN turn sets this rather than
    /// tuning a cadence until one happens to fall where it wanted. A drive
    /// whose seam lands somewhere the script did not choose is a drive whose
    /// record proves less than it looks like it does.
    pub boundary: bool,
    /// A phase the operator proposes before this turn settles.
    ///
    /// Separate from the boundary because they are separate acts: a phase
    /// proposal that the graph refuses still leaves the boundary declared,
    /// and a boundary is often declared with no phase change at all.
    pub phase: Option<String>,
}

/// A command a turn runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// Which tool, as the record's `tool_call.tool` spells it.
    pub tool: String,
    /// The argv, run under the confinement the regimen declares.
    pub argv: Vec<String>,
}

impl Command {
    /// A command named `tool` running `argv`.
    #[must_use]
    pub fn new(tool: &str, argv: &[&str]) -> Self {
        Self {
            tool: tool.to_owned(),
            argv: argv.iter().map(|part| (*part).to_owned()).collect(),
        }
    }
}
