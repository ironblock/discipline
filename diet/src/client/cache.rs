//! The three-register cache-miss census: expected, mutation, unexplained.
//!
//! #30 gave the client cache telemetry per request: how many prompt tokens
//! the server read, and how many it reused. That counts misses. It does not
//! say WHY one happened, and the study this replaces could only estimate the
//! split -- "84% provider eviction", arrived at by residual, which is another
//! way of saying nobody measured it.
//!
//! Two facts make the measurement possible, and this module is where they
//! meet. A head is a byte string the harness controls, so
//! [`crate::client::head::Head`] hashes it and the record carries the
//! fingerprint per request: a miss whose head moved is a **mutation**, on
//! evidence. Cache lifetime is a property of the provider path, declared per
//! substrate as [`crate::formats::record::CacheTtl`]: a miss after a gap
//! longer than the lifetime is **expected**, on a constant somebody wrote
//! down. Everything else is **unexplained** -- a provider restart, a
//! granularity this client cannot see, a bug -- and it is never folded into
//! `expected`, because the fold is exactly what turns a measurement back into
//! an estimate.
//!
//! # Row 3 is proved at the unit, and only at the unit
//!
//! #79's third acceptance row -- a six-minute gap under `cache_ttl = 5m`
//! classified `expected`, a one-minute gap with the same miss classified
//! `unexplained` -- is `a_gap_past_the_declared_lifetime_is_expected_and_a_gap_inside_it_is_not`,
//! beside the boundary case, the precedence case, the undeclared case, the
//! uncached case and the unmeasured case. **It is not driven end to end**,
//! and this is said plainly here rather than left to be inferred from which
//! tests exist: the reference drive runs against a canned loopback stub that
//! plays no provider's cache policy, so there is no expiry for a gap to cross
//! and no six minutes to wait for. The drive's own output demonstrates the
//! other half -- `"ttl_undeclared":["canned"]`, an undeclared lifetime named
//! rather than defaulted -- which is the honest end-to-end claim available
//! against a substrate nobody has documented a lifetime for. Rows 1 and 2
//! carry the same disclosure in `crate::client::head`.
//!
//! # What this census is beside, and not in
//!
//! The record does not carry it. `expected` needs an INTER-CALL GAP, and
//! record v0 carries no clock; inventing a timestamp field here would be
//! #82's territory decided by whoever happened to need it first. The half
//! that is not a clock IS in the record -- `request.head_sha256` and the
//! `prefix.changed` rows -- so `mutation` is re-derivable from the file
//! alone by anyone who doubts this module. The drive measures the gap with
//! an [`std::time::Instant`] taken immediately before each call, so the
//! record it writes stays byte-identical run to run.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use crate::formats::record::{CacheTtl, Count, Regime};

/// What one call saw, as far as the cache is concerned.
///
/// Assembled by the caller that made the call: the drive knows the lane, the
/// substrate and the gap, and the client's [`super::Cache`] carries what the
/// server said. Kept as a value rather than computed inside the client
/// because the gap is the CALLER's measurement -- the client sees one call at
/// a time and has nothing to measure a gap against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    /// The lane the call was made on.
    pub lane: String,
    /// The declared substrate it was put to, by id. What the declared
    /// lifetime is looked up under.
    pub substrate: String,
    /// Whether the head this call sent differs from the head the previous
    /// call on the same lane sent.
    pub head_changed: bool,
    /// How long since the previous call on the same lane. `None` on a lane's
    /// first call, where there is no gap and so nothing a lifetime could be
    /// compared against.
    pub since_previous: Option<Duration>,
    /// How many prompt tokens the server read.
    pub prompt_tokens: Option<Count>,
    /// How many of them it reused.
    pub cached_tokens: Option<Count>,
    /// Whether the dialect declared anywhere to read the reused count from.
    ///
    /// Without it, "the server reported no reuse" and "nobody asked it to"
    /// are the same `None`, and they are not the same fact: the first is a
    /// miss and the second is the absence of a measurement. The same
    /// distinction [`super::wire::Reply::echo_site_declared`] keeps one layer
    /// down.
    pub measured: bool,
}

impl Observation {
    /// Whether this call missed the cache.
    ///
    /// A miss is the server reading prompt tokens and reusing NONE of them.
    /// Where the dialect declares no path, or the server reported nothing,
    /// this is not a miss and not a hit: it is unmeasured, and counting it
    /// either way would be this module inventing a measurement.
    #[must_use]
    pub fn missed(&self) -> Option<bool> {
        if !self.measured {
            return None;
        }
        let (Some(prompt), Some(cached)) = (self.prompt_tokens, self.cached_tokens) else {
            return None;
        };
        if prompt.get() == 0 {
            return None;
        }
        Some(cached.get() == 0)
    }
}

/// Which register a miss belongs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Register {
    /// The declared lifetime had run out. A constant somebody copied from a
    /// provider's documentation, applied to a gap this run measured.
    Expected,
    /// The head moved. Two digests this run took, compared.
    Mutation,
    /// Neither. A provider restart, a granularity this client cannot see, a
    /// bug. **Never folded into `Expected`:** the fold is what turned the
    /// measurement into an estimate in the work this replaces.
    Unexplained,
}

/// Which register `observation` belongs in, given what its substrate
/// declared.
///
/// **MEASURED BEATS PREDICTED, and the order is the point.** A head digest is
/// something this run observed; a TTL is a constant copied out of a
/// provider's documentation, which may be stale, rounded, or about a
/// different tier. Where both would fire, the evidence wins -- and a run
/// where every miss is `expected` because the gaps happened to be long is a
/// run that has learned nothing about its own prefix.
#[must_use]
pub fn register(observation: &Observation, declared: Option<CacheTtl>) -> Register {
    if observation.head_changed {
        return Register::Mutation;
    }
    match (declared, observation.since_previous) {
        // A path that caches nothing misses at any gap, including none.
        (Some(CacheTtl::Uncached), _) => Register::Expected,
        // PAST the lifetime, not at it: a gap exactly equal to the declared
        // lifetime is a gap the lifetime covers, and a boundary read the
        // other way would attribute a real miss to an expiry that had not
        // happened.
        (Some(CacheTtl::Seconds(ttl)), Some(gap)) if gap.as_secs() > ttl.get() => {
            Register::Expected
        }
        _ => Register::Unexplained,
    }
}

/// What a run's calls came to, as far as the cache is concerned.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Census {
    /// Calls that reused something.
    pub hits: u64,
    /// Misses after a declared lifetime ran out.
    pub expected: u64,
    /// Misses whose head had moved.
    pub mutation: u64,
    /// Misses neither of the above explains.
    pub unexplained: u64,
    /// Calls where nobody could tell: the dialect declared no path, or the
    /// server reported nothing, or the prompt was empty.
    pub unmeasured: u64,
    /// The substrates a call was put to whose cache lifetime nobody
    /// declared, in id order.
    ///
    /// **Named rather than defaulted, and named whether or not it ever
    /// missed.** A substrate with no declared lifetime cannot produce an
    /// `expected` miss: any miss against it lands in `unexplained`, which is
    /// the honest register and a useless one unless a reader can see why.
    /// This list is why -- it names the substrate on the strength of the
    /// declaration alone, not on whether a miss actually happened to land in
    /// `unexplained` this run.
    pub ttl_undeclared: Vec<String>,
}

impl Census {
    /// The census of `observations`, with lifetimes read from `regime`.
    ///
    /// The lifetimes come out of the regime the RECORD declares, rather than
    /// from a table a caller passes in, so that this census and a later
    /// reader of the same file apply one set of constants. A substrate an
    /// observation names and the regime does not declare has no lifetime to
    /// read and is counted under `ttl_undeclared` like any other -- the
    /// record refuses such a row elsewhere, and this function is not a second
    /// place to make that ruling.
    #[must_use]
    pub fn of(regime: &Regime, observations: &[Observation]) -> Self {
        let declared: BTreeMap<&str, Option<CacheTtl>> = regime
            .substrates
            .iter()
            .map(|substrate| (substrate.id.as_str(), substrate.cache_ttl))
            .collect();
        let mut census = Self::default();
        let mut undeclared: BTreeSet<String> = BTreeSet::new();
        for observation in observations {
            let ttl = declared
                .get(observation.substrate.as_str())
                .copied()
                .flatten();
            if ttl.is_none() {
                undeclared.insert(observation.substrate.clone());
            }
            match observation.missed() {
                None => census.unmeasured += 1,
                Some(false) => census.hits += 1,
                Some(true) => match register(observation, ttl) {
                    Register::Expected => census.expected += 1,
                    Register::Mutation => census.mutation += 1,
                    Register::Unexplained => census.unexplained += 1,
                },
            }
        }
        census.ttl_undeclared = undeclared.into_iter().collect();
        census
    }

    /// Every miss, in whichever register it landed.
    #[must_use]
    pub fn misses(&self) -> u64 {
        self.expected + self.mutation + self.unexplained
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Census, Observation, Register, register};
    use crate::formats::record::{
        CacheTtl, Count, Engine, Reasoning, Regime, Substrate, Weights, json::Value,
    };

    fn count(value: u64) -> Count {
        Count::new(value).expect("a count")
    }

    fn miss(gap: Option<Duration>, head_changed: bool) -> Observation {
        Observation {
            lane: "main".to_owned(),
            substrate: "served".to_owned(),
            head_changed,
            since_previous: gap,
            prompt_tokens: Some(count(1200)),
            cached_tokens: Some(count(0)),
            measured: true,
        }
    }

    fn regime(ttl: Option<CacheTtl>) -> Regime {
        Regime {
            arm: "a".to_owned(),
            substrates: vec![Substrate {
                id: "served".to_owned(),
                engine: Engine {
                    name: "a-runtime".to_owned(),
                    version_or_digest: "1.0".to_owned(),
                },
                weights: Weights::Digest("a".repeat(64)),
                hardware_fingerprint: "b".repeat(64),
                sampler_card: std::collections::BTreeMap::from([(
                    "seed".to_owned(),
                    Value::Integer(7),
                )]),
                reasoning: Reasoning::Off,
                reasoning_control: None,
                chat_template_sha256: None,
                cache_ttl: ttl,
            }],
            dogma_version: 0,
        }
    }

    /// The issue's third acceptance row, and its boundary.
    ///
    /// Six minutes under a five-minute lifetime is an expiry; one minute
    /// under the same lifetime is not, and the difference is the whole
    /// register. The boundary case is here too, because `>` versus `>=` is
    /// the one line of this module a reviewer cannot check by reading.
    #[test]
    fn a_gap_past_the_declared_lifetime_is_expected_and_a_gap_inside_it_is_not() {
        let five_minutes = Some(CacheTtl::Seconds(count(300)));
        assert_eq!(
            register(&miss(Some(Duration::from_secs(360)), false), five_minutes),
            Register::Expected
        );
        assert_eq!(
            register(&miss(Some(Duration::from_secs(60)), false), five_minutes),
            Register::Unexplained,
            "a miss inside the declared lifetime is not an expiry, and folding it \
             into one is what turns this census back into an estimate"
        );
        assert_eq!(
            register(&miss(Some(Duration::from_secs(300)), false), five_minutes),
            Register::Unexplained,
            "a gap exactly equal to the lifetime is a gap the lifetime covers"
        );
    }

    /// Measured beats predicted: a head that moved is a mutation even where
    /// the gap would also have explained the miss.
    #[test]
    fn a_head_that_moved_outranks_a_lifetime_that_ran_out() {
        assert_eq!(
            register(
                &miss(Some(Duration::from_secs(3600)), true),
                Some(CacheTtl::Seconds(count(300))),
            ),
            Register::Mutation,
            "the digests are something this run observed; the lifetime is a \
             constant copied from documentation"
        );
    }

    /// An undeclared lifetime never reads as a provider expiry, and the
    /// census names the substrate rather than leaving the reader to wonder.
    #[test]
    fn an_undeclared_lifetime_is_unexplained_and_the_substrate_is_named() {
        let census = Census::of(
            &regime(None),
            &[miss(Some(Duration::from_secs(3600)), false)],
        );
        assert_eq!(census.unexplained, 1);
        assert_eq!(census.expected, 0);
        assert_eq!(census.ttl_undeclared, vec!["served".to_owned()]);
    }

    /// A path that caches nothing misses at any gap, and says so.
    #[test]
    fn a_path_declared_uncached_misses_expectedly_at_any_gap() {
        let census = Census::of(
            &regime(Some(CacheTtl::Uncached)),
            &[miss(None, false), miss(Some(Duration::from_secs(1)), false)],
        );
        assert_eq!(census.expected, 2);
        assert!(census.ttl_undeclared.is_empty());
    }

    /// The fourth register: a call nobody could measure is not a miss.
    #[test]
    fn a_call_nobody_measured_is_neither_a_hit_nor_a_miss() {
        let undeclared_path = Observation {
            measured: false,
            ..miss(None, false)
        };
        let silent_server = Observation {
            cached_tokens: None,
            ..miss(None, false)
        };
        let empty_prompt = Observation {
            prompt_tokens: Some(count(0)),
            ..miss(None, false)
        };
        let census = Census::of(
            &regime(Some(CacheTtl::Seconds(count(300)))),
            &[undeclared_path, silent_server, empty_prompt],
        );
        assert_eq!(census.unmeasured, 3);
        assert_eq!(census.misses(), 0);
        assert_eq!(census.hits, 0);
    }

    /// And a call that reused something is a hit, whatever else was true of
    /// it.
    #[test]
    fn a_call_that_reused_tokens_is_a_hit() {
        let hit = Observation {
            cached_tokens: Some(count(1100)),
            head_changed: true,
            ..miss(None, false)
        };
        let census = Census::of(&regime(Some(CacheTtl::Uncached)), &[hit]);
        assert_eq!(census.hits, 1);
        assert_eq!(census.misses(), 0);
    }
}
