//! The phase graph, and transitions as proposals.
//!
//! Who decides when the work moves from planning to implementing is the
//! metacognition problem this architecture refuses to hand to the model. The
//! answer is structural: the regimen declares the shape of the work, the model
//! may PROPOSE a move, and the graph decides.
//!
//! A proposal the graph refuses is still recorded. It is a fact about the
//! drive -- the model thought the work had moved on -- and a controller that
//! dropped it would be hiding the one signal that says the graph and the work
//! disagree.

use super::policy::PolicyError;
use crate::client::vocabulary;

use std::collections::{BTreeMap, BTreeSet};

vocabulary! {
    /// Why a proposed transition was refused.
    Refusal {
        /// The regimen declares no phases, so there is nothing to move
        /// between. Its own refusal rather than "no edge": a session with no
        /// phase graph is the scale-adaptive case working as declared, not a
        /// graph that happens to be missing an edge.
        NoGraph => "no_graph",
        /// The name is not one of the declared phases.
        NotAPhase => "not_a_phase",
        /// Both names are phases and no edge joins them.
        NoEdge => "no_edge",
        /// The proposal names the phase the session is already in.
        AlreadyThere => "already_there",
    }
}

/// What the graph says about a proposal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// The move is allowed. A seam fires.
    Ratified,
    /// The move is not allowed. The proposal is recorded and nothing else
    /// happens.
    Refused(Refusal),
}

/// The phases a session may be in, and which moves between them are allowed.
///
/// Empty is legal and is the default. A regimen that declares no phases
/// declares a session with one implicit phase and no transitions -- which is
/// what a one-line change should get, because forcing a three-phase ceremony
/// onto it is the known rigidity cost of phase-structured workflows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PhaseGraph {
    order: Vec<String>,
    edges: BTreeMap<String, BTreeSet<String>>,
}

impl PhaseGraph {
    /// A graph with no phases.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// A graph over `phases`, with no edges yet.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::DuplicatePhase`] if a name appears twice. Two
    /// phases with one name is a graph where an edge's target is ambiguous,
    /// and the ambiguity would be resolved silently by whichever lookup ran.
    pub fn of(phases: &[String]) -> Result<Self, PolicyError> {
        let mut seen = BTreeSet::new();
        for phase in phases {
            if !seen.insert(phase.clone()) {
                return Err(PolicyError::DuplicatePhase(phase.clone()));
            }
        }
        Ok(Self {
            order: phases.to_vec(),
            edges: BTreeMap::new(),
        })
    }

    /// Allow a move from `from` to `to`.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::UnknownPhase`] if either name is not a declared
    /// phase. An edge to a phase that does not exist is a transition nothing
    /// can ever take, and it would sit in the regimen looking like a rule.
    pub fn allow(&mut self, from: &str, to: &str) -> Result<(), PolicyError> {
        for name in [from, to] {
            if !self.order.iter().any(|phase| phase == name) {
                return Err(PolicyError::UnknownPhase {
                    named: name.to_owned(),
                    under: from.to_owned(),
                });
            }
        }
        self.edges
            .entry(from.to_owned())
            .or_default()
            .insert(to.to_owned());
        Ok(())
    }

    /// Whether this graph declares any phases.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// The phase a session under this graph starts in.
    #[must_use]
    pub fn first(&self) -> Option<&str> {
        self.order.first().map(String::as_str)
    }

    /// Every phase, in the order the regimen declared them.
    #[must_use]
    pub fn phases(&self) -> &[String] {
        &self.order
    }

    /// What this graph says about moving from `from` to `to`.
    #[must_use]
    pub fn decide(&self, from: Option<&str>, to: &str) -> Decision {
        if self.order.is_empty() {
            return Decision::Refused(Refusal::NoGraph);
        }
        if !self.order.iter().any(|phase| phase == to) {
            return Decision::Refused(Refusal::NotAPhase);
        }
        if from == Some(to) {
            return Decision::Refused(Refusal::AlreadyThere);
        }
        let Some(from) = from else {
            // Nothing is in no phase under a graph that declares one: the
            // controller starts in `first()`. Reaching here means a caller
            // built a controller by hand with a phase of `None` and a
            // non-empty graph, which is a state the constructor does not
            // produce.
            return Decision::Refused(Refusal::NoEdge);
        };
        match self.edges.get(from) {
            Some(targets) if targets.contains(to) => Decision::Ratified,
            _ => Decision::Refused(Refusal::NoEdge),
        }
    }
}

/// A transition the model asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    /// The turn it was made in.
    pub at_turn: u32,
    /// The phase the session was in.
    pub from: Option<String>,
    /// The phase the model asked for, as it asked -- kept even when it is not
    /// a phase at all, because "the model proposed a phase that does not
    /// exist" is a different fact from "the model proposed a move the graph
    /// forbids", and the name is the evidence for which.
    pub to: String,
    /// What the graph said.
    pub decision: Decision,
}
