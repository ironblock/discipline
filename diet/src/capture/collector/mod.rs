//! The collector: supersession detection.
//!
//! The working object accumulates entries, and some of them stop being true:
//! a gotcha gets resolved, an open question gets answered, a plan gets
//! replaced. Without a mechanism to notice, the object silently accumulates
//! stale high-confidence facts, and a later turn acts on one -- the
//! silent-loss class in reverse. In the research phase this was the single
//! most load-bearing unbuilt piece: everything else in the capture
//! architecture is only safe once supersession is detected.
//!
//! The design is a tiered nominator with a constrained verdict, and it is
//! precision-first on purpose. A false nomination costs a confirm slot; a
//! missed supersession costs correctness, but the archive retains everything,
//! so misses are recoverable and over-firing is the enemy. Imperative
//! nominations in the injection experiments caused capitulation on false
//! positives; nomination here is advisory, and a precision gate is declared
//! before any policy is tuned.
//!
//! * [`literal`] -- tier 0: an entry's anchors recurring in new text.
//!
//! Small models classify; they never trigger or generate. Nothing in this
//! module writes an entry: a nomination goes to a fork whose answer is a
//! [`crate::formats::verdict`], and the reconciler applies a link.

pub mod literal;
