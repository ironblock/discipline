//! `diet::client` -- the inference client, and the receipts it keeps.
//!
//! Every fork, seam and canonical turn is a request to an inference server.
//! The transport is where the regime trio is actually asserted, and prior
//! work's record of it is a catalogue of provenance failures: a 4xx
//! strip-and-retry whose log could not say which request produced the answer;
//! timeouts indistinguishable from empty answers; a server's concurrency
//! setting silently changing what every throughput number meant; sampler
//! settings the client *thought* it sent and the server ignored.
//!
//! Five things follow, and each of them is a type rather than a convention:
//!
//! * The **request shape is data** ([`shape::RequestShape`]), built once and
//!   carried into the record beside the answer it produced.
//! * **Sampler echo** ([`echo`]): every reply is compared against the pins the
//!   request carried, and a contradiction is a `regime.mismatch` that refuses
//!   to bank.
//! * **Retry lineage** ([`Attempt`]): a retry is a new request naming its
//!   predecessor, its reason is typed, and the answer names the attempt that
//!   produced it.
//! * **Timeouts and caps are typed outcomes** ([`Outcome`]), never empty
//!   strings.
//! * **Cache telemetry** ([`Cache`]) per request, because reused-token counts
//!   are the direct measurement of prefix stability and the seam controller's
//!   claims are checked against them.
//!
//! What this module does NOT do is choose or configure a server. It records
//! what it is given: the concurrency is declared ([`shape::Concurrency`]),
//! and where a server reports itself is declared ([`shape::Dialect`]).
//!
//! # What has been seen red
//!
//! A gate that has never been seen red is not a gate, and the twelve claims
//! below were each proved by breaking the implementation and watching one
//! named test fail. They are recorded here rather than as seeded cases in the
//! gate because `verify.sh` and `tools/gate/faults.toml` are not this seat's
//! files; that is disclosed on the pull request, and this table is what a
//! reader can re-run in the meantime. Each is a plausible way to be wrong --
//! not a deleted line.
//!
//! | break this | and this fails |
//! | --- | --- |
//! | `Echo::verdict` never returns `Mismatched` | `a_server_that_reports_a_temperature_it_was_not_sent_is_a_mismatch_that_refuses_to_bank` |
//! | a pin nobody reported counts as agreement | `a_pin_the_server_never_mentions_is_unverified_rather_than_agreed` |
//! | a retry stops naming its predecessor | `a_stripped_field_is_retried_and_the_answer_names_the_request_that_produced_it` |
//! | the stripped pin is not recorded | `a_stripped_field_is_retried_and_the_answer_names_the_request_that_produced_it` |
//! | a timeout becomes `Answered` with `""` | `a_stall_past_the_deadline_is_a_typed_timeout_and_never_an_empty_answer` |
//! | the retry bound is not applied | `retries_are_bounded_by_the_limit_the_request_declares` |
//! | a pinned decimal is rendered through an `f64` | `wire::…::a_pinned_decimal_is_sent_as_the_digits_that_were_pinned` |
//! | cache telemetry is found by scanning rather than by declaration | `cache_telemetry_is_read_from_the_path_the_dialect_declares_and_nowhere_else` |
//! | a timed-out request projects a response row | `a_stall_past_the_deadline_is_a_typed_timeout_and_never_an_empty_answer` |
//! | `Content-Length` is ignored and the body read to close | `a_content_length_reply_is_answered_without_waiting_for_the_connection_to_close` |
//! | a chunked body is not decoded | `a_chunked_reply_is_decoded_rather_than_handed_to_the_reader_with_its_framing` |
//! | `header` gives up on the status line | the three framing tests above |
//! | a chunk size from the wire is added to (`ffffffffffffffff` overflows) | `transport::…::a_chunked_body_is_whole_only_when_its_terminator_has_arrived` |
//! | a truncated reply is clamped to what arrived | `a_reply_that_announces_more_than_it_sends_is_truncated_not_a_shorter_answer` |
//! | the whole-call budget stops binding an attempt | `the_whole_call_budget_binds_even_where_each_attempt_is_inside_its_own` |
//! | the 4xx search reads the whole reply, not the complaint | `a_quoted_request_naming_exactly_one_pin_still_strips_nothing` |
//! | a complaint naming two pins strips the first | `a_complaint_naming_two_pinned_settings_strips_neither` |
//! | a pin this request never sent may be stripped | `a_complaint_naming_a_setting_this_request_did_not_pin_strips_nothing` |
//! | the refusals come out in another order | `two_refusals_are_listed_in_one_order` |
//! | `max_tokens` stops counting as a cap | `max_tokens_is_a_cap_as_much_as_length_is` |
//! | the reply cap stops being applied | `a_reply_past_the_declared_cap_is_refused_rather_than_read_forever` |
//! | an unspellable token count saturates instead of refusing | `a_token_count_no_record_can_spell_makes_the_reply_unreadable` |
//! | the issued-versus-arrived gap stops being named | `a_request_that_never_reached_a_server_is_still_a_request_the_client_issued` |
//!
//! **Three mutations were tried and are NOT in the table, because the suite
//! stayed green under each.** An injection that does not change the tree
//! proves nothing, so in each case the guard went and its rule moved to
//! something that can fire: a `skip(1)` over the status line in [`transport`]
//! (deleted); a sort over `bank`'s refusals, which nothing could produce out
//! of order (replaced by walking the vocabulary, so the order is the loop's);
//! and reading the server's complaint rather than its whole reply, whose real
//! case turned out to be narrower than the test written for it (the narrower
//! case is now the test).

pub mod echo;
pub mod journal;
pub mod shape;
pub mod stub;
pub mod transport;
pub mod wire;

use std::time::{Duration, Instant};

use crate::formats::record::Count;

use echo::{Echo, Verdict};
use journal::{Entry, Journal};
use shape::{RequestShape, SamplerSetting, Serving};
use transport::{Transport, TransportFailure};
use wire::WireError;

/// Declare a closed vocabulary: the enum, its `ALL`, and its tag.
///
/// The same shape the record's own vocabularies use, and for the same reason:
/// a variant that is not in `ALL` is invisible to every check that iterates
/// the list, and a variant with no tag has no spelling in a record. Generated
/// together so neither can exist without the other.
///
/// Deliberately a second copy rather than a shared macro. Promoting the
/// record's copy to a crate-wide one is a change to `diet/formats/`, which
/// this seat does not own; the duplication is disclosed rather than smuggled
/// in as a drive-by edit.
macro_rules! vocabulary {
    (
        $(#[$enum_doc:meta])*
        $name:ident { $( $(#[$doc:meta])* $variant:ident => $tag:literal ),+ $(,)? }
    ) => {
        $(#[$enum_doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name { $( $(#[$doc])* $variant ),+ }

        impl $name {
            /// Every variant, generated beside the enum so the two cannot
            /// drift apart.
            pub const ALL: &'static [Self] = &[ $(Self::$variant),+ ];

            /// The name this variant is written under.
            #[must_use]
            pub fn tag(self) -> &'static str {
                match self { $(Self::$variant => $tag),+ }
            }
        }
    };
}

pub(crate) use vocabulary;

/// The finish reasons that mean the answer was cut off rather than finished.
///
/// Written out because they are the servers' words, not this program's. A
/// generation that hits its cap is a typed outcome and is never graded as
/// wrong or malformed; a reason NOT in this list is carried as the server
/// spelled it and decided by nobody.
const CAPPED_FINISH_REASONS: &[&str] = &["length", "max_tokens"];

vocabulary! {
    /// Why an attempt exists after the first one.
    RetryReason {
        /// The server rejected a field, the client removed it, and sent again.
        FieldStripped => "4xx-strip",
        /// The previous attempt ran out of time.
        Timeout => "timeout",
        /// The previous attempt never got an answer at the transport layer.
        Connection => "connection",
    }
}

vocabulary! {
    /// Why a result may not be banked.
    ///
    /// A refusal is about provenance, never about quality. Nothing here says
    /// an answer was bad; each one says the record cannot support a claim
    /// made from it.
    Refusal {
        /// The server contradicted a pin.
        RegimeMismatch => "regime.mismatch",
        /// A pin could not be checked, in either direction.
        RegimeUnverified => "regime.unverified",
        /// A retry removed a sampler pin, so the answer was produced under a
        /// regime that is not the one the regimen declares.
        RegimeStripped => "regime.stripped",
        /// Nothing was pinned, so there is no regime to reproduce.
        RegimeUnpinned => "regime.unpinned",
        /// There is no answer: the call timed out.
        Timeout => "timeout",
        /// There is no answer: the server refused.
        ServerRefused => "server.refused",
        /// There is no answer: the transport failed.
        TransportFailed => "transport.failed",
        /// There is no answer: the reply could not be read.
        Unreadable => "reply.unreadable",
    }
}

/// Identifiers for the requests of one call, derived rather than drawn.
///
/// A random id makes two replays of one drive differ in bytes that mean
/// nothing, and a record whose bytes move is a record whose diff cannot be
/// read. The stem comes from the caller -- the lane and turn -- and the
/// counter makes each attempt of a call distinct.
#[derive(Debug, Clone)]
pub struct IdSource {
    stem: String,
    next: u32,
}

impl IdSource {
    /// Identifiers under `stem`.
    #[must_use]
    pub fn new(stem: impl Into<String>) -> Self {
        Self {
            stem: stem.into(),
            next: 0,
        }
    }

    /// The next identifier.
    pub fn take(&mut self) -> String {
        self.next += 1;
        format!("{}/{}", self.stem, self.next)
    }
}

/// One request as it was actually sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    /// This attempt's identifier, unique within the record.
    pub id: String,
    /// The attempt this one retries. `None` on the first.
    pub retry_of: Option<String>,
    /// Why this attempt exists. `None` on the first.
    pub because: Option<RetryReason>,
    /// What this attempt sent, which is not necessarily what the caller
    /// asked for: a strip-and-retry sends less.
    pub sent: RequestShape,
    /// Every sampler pin removed before this attempt was sent.
    pub stripped: Vec<SamplerSetting>,
}

/// What a server reported about its prompt cache.
///
/// A reused-token count is the direct measurement of prefix stability. It is
/// carried with the path it was read from, because two servers put different
/// numbers under similar names and a count whose provenance is a guess is a
/// measurement of the wrong thing that reads like a measurement of the right
/// one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cache {
    /// The dialect that said where to look.
    pub dialect: String,
    /// How many prompt tokens the server read.
    pub prompt_tokens: Option<Count>,
    /// How many of them it reused.
    pub cached_tokens: Option<Count>,
    /// The path the reused count was read from, when the dialect declares
    /// one. `None` means nobody asked, which is different from asking and
    /// being told nothing.
    pub cached_path: Option<String>,
}

/// An answer, and everything that says what produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// The identifier of the attempt that produced this answer. Required:
    /// the log that could not say which of two requests an answer came from
    /// is the failure this field exists for.
    pub produced_by: String,
    /// What came back. May be empty -- an answer of nothing at all is a fact
    /// about a drive, and a client that could not spell it would be a client
    /// that hid the collapse it exists to catch.
    pub text: String,
    /// How many tokens the server says came back, when it says.
    pub output_tokens: Option<Count>,
    /// Why the server stopped, as the server spells it.
    pub finish_reason: Option<String>,
    /// The pins, against the server's report of them.
    pub echo: Echo,
    /// What the server said about its cache.
    pub cache: Cache,
}

/// What a call came to.
///
/// Every variant is a fact. None of them is an empty string standing in for
/// one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The server answered and said it had finished.
    Answered(Box<Answer>),
    /// The server answered and said it had run out of room. A capped
    /// generation is never graded as wrong or malformed; it is this.
    Capped(Box<Answer>),
    /// The deadline arrived first.
    Timeout {
        /// The attempt that ran out of time.
        by: String,
        /// How long THIS ATTEMPT had run when it did -- not the call. Three
        /// 200 ms attempts under one call report about 200 ms each and not
        /// 600 ms; the call's own elapsed time is the caller's to keep, and a
        /// field that reported it from inside one attempt would be reporting
        /// a number this layer does not measure.
        after: Duration,
    },
    /// The server answered with an error status and this client did not know
    /// how to retry it.
    Refused {
        /// The attempt that was refused.
        by: String,
        /// The status it was refused with.
        status: u16,
        /// What the server said.
        body: String,
    },
    /// No reply arrived.
    Failed {
        /// The attempt that failed.
        by: String,
        /// How.
        failure: TransportFailure,
    },
    /// A reply arrived and could not be read.
    Unreadable {
        /// The attempt whose reply could not be read.
        by: String,
        /// Why.
        why: WireError,
    },
}

impl Outcome {
    /// The attempt this outcome is about.
    #[must_use]
    pub fn by(&self) -> &str {
        match self {
            Self::Answered(answer) | Self::Capped(answer) => &answer.produced_by,
            Self::Timeout { by, .. }
            | Self::Refused { by, .. }
            | Self::Failed { by, .. }
            | Self::Unreadable { by, .. } => by,
        }
    }

    /// The answer, if there is one.
    #[must_use]
    pub fn answer(&self) -> Option<&Answer> {
        match self {
            Self::Answered(answer) | Self::Capped(answer) => Some(answer),
            Self::Timeout { .. }
            | Self::Refused { .. }
            | Self::Failed { .. }
            | Self::Unreadable { .. } => None,
        }
    }
}

/// One call: every attempt it took, what it came to, and the journal.
#[derive(Debug, Clone)]
pub struct Call {
    /// Which lane made it.
    pub lane: String,
    /// Every attempt, in order. The first has no predecessor; each later one
    /// names the one before it.
    pub attempts: Vec<Attempt>,
    /// What it came to.
    pub outcome: Outcome,
    /// The typed event stream this call produced.
    pub journal: Journal,
}

/// Whether a result may be banked, and if not, why not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bank {
    /// The provenance holds.
    Bankable,
    /// It does not, for these reasons, in the order they are listed by
    /// [`Refusal::ALL`].
    Refused(Vec<Refusal>),
}

impl Call {
    /// Whether a graded run may bank this result.
    ///
    /// A capped answer is bankable. Truncation is a typed outcome and the
    /// census counts it; refusing it here would be grading it as wrong, which
    /// is the thing the outcome type exists to stop.
    ///
    /// The refusals come out in [`Refusal::ALL`]'s order BY CONSTRUCTION --
    /// this walks the vocabulary and asks each one -- rather than by sorting
    /// a list built in whatever order the checks happened to run. The sort
    /// was here first, and mutating it away changed no answer, because
    /// nothing could produce two refusals out of order to begin with. A guard
    /// that cannot fire is not a guard; the order is now a property of the
    /// loop.
    #[must_use]
    pub fn bank(&self) -> Bank {
        let refusals: Vec<Refusal> = Refusal::ALL
            .iter()
            .copied()
            .filter(|refusal| self.refuses(*refusal))
            .collect();
        if refusals.is_empty() {
            Bank::Bankable
        } else {
            Bank::Refused(refusals)
        }
    }

    /// Whether this call carries `refusal`.
    ///
    /// An exhaustive match, so a refusal added to the vocabulary fails to
    /// compile here until somebody says when it fires.
    fn refuses(&self, refusal: Refusal) -> bool {
        let verdict = self.outcome.answer().map(|answer| answer.echo.verdict());
        match refusal {
            Refusal::RegimeMismatch => verdict == Some(Verdict::Mismatched),
            Refusal::RegimeUnverified => verdict == Some(Verdict::Unverified),
            Refusal::RegimeUnpinned => verdict == Some(Verdict::NothingPinned),
            Refusal::RegimeStripped => {
                // Only where there is a result to bank: a call that timed out
                // having stripped a pin is refused for the timeout, and a
                // second reason for an answer that does not exist would be an
                // accounting of nothing.
                self.outcome.answer().is_some()
                    && self
                        .attempts
                        .last()
                        .is_some_and(|attempt| !attempt.stripped.is_empty())
            }
            Refusal::Timeout => matches!(self.outcome, Outcome::Timeout { .. }),
            Refusal::ServerRefused => matches!(self.outcome, Outcome::Refused { .. }),
            Refusal::TransportFailed => matches!(self.outcome, Outcome::Failed { .. }),
            Refusal::Unreadable => matches!(self.outcome, Outcome::Unreadable { .. }),
        }
    }
}

/// The client.
///
/// Holds the transport, and the two things nobody may assume: how the server
/// was configured to serve, and where it reports itself.
#[derive(Debug, Clone)]
pub struct Client<T: Transport> {
    transport: T,
    serving: Serving,
}

impl<T: Transport> Client<T> {
    /// A client sending through `transport` to a server serving as `serving`
    /// says.
    ///
    /// Both are required and neither has a default. A default concurrency
    /// would turn "nobody declared it" into "one", which are different facts
    /// about every throughput number taken under them.
    pub fn new(transport: T, serving: Serving) -> Self {
        Self { transport, serving }
    }

    /// Make one call, retrying within `shape`'s limits.
    ///
    /// Returns a [`Call`] in every case. There is no error path: a failure is
    /// an [`Outcome`], because a call that failed is something that happened
    /// and the record has to be able to say so.
    pub fn call(&self, shape: &RequestShape, lane: &str, ids: &mut IdSource) -> Call {
        let call_deadline = Instant::now() + shape.limits.call;
        let mut journal = Journal::new();
        journal.push(Entry::Serving {
            concurrency: self.serving.concurrency,
            dialect: self.serving.dialect.name.clone(),
            endpoint: self.transport.describes(),
        });

        let mut sending = shape.clone();
        let mut stripped: Vec<SamplerSetting> = Vec::new();
        let mut attempts: Vec<Attempt> = Vec::new();
        let mut retry_of: Option<String> = None;
        let mut because: Option<RetryReason> = None;

        loop {
            let id = ids.take();
            attempts.push(Attempt {
                id: id.clone(),
                retry_of: retry_of.clone(),
                because,
                sent: sending.clone(),
                stripped: stripped.clone(),
            });
            journal.push(Entry::Issued {
                id: id.clone(),
                lane: lane.to_owned(),
                retry_of: retry_of.clone(),
                because,
                sampler: sending.sampler.clone(),
            });

            let more_attempts_allowed = attempts.len() <= usize::from(sending.limits.retries);
            let attempt_deadline = (Instant::now() + sending.limits.attempt).min(call_deadline);
            let step = self.attempt(&sending, &id, attempt_deadline, &mut journal);

            match step {
                Step::Done(outcome) => {
                    return Call {
                        lane: lane.to_owned(),
                        attempts,
                        outcome,
                        journal,
                    };
                }
                Step::Retryable {
                    reason,
                    strip,
                    give_up,
                } => {
                    // A retry with no budget left is not a request. Recording
                    // an attempt that never reached a socket would put a
                    // `request` row in the record for bytes nobody sent.
                    if !more_attempts_allowed || Instant::now() >= call_deadline {
                        return Call {
                            lane: lane.to_owned(),
                            attempts,
                            outcome: give_up,
                            journal,
                        };
                    }
                    if let Some(setting) = strip {
                        sending.sampler = sending.sampler.without(setting);
                        stripped.push(setting);
                        journal.push(Entry::Stripped {
                            id: id.clone(),
                            setting,
                        });
                    }
                    journal.push(Entry::Retried {
                        after: id.clone(),
                        because: reason,
                    });
                    retry_of = Some(id);
                    because = Some(reason);
                }
            }
        }
    }

    /// One attempt: send, read, and decide whether there is anything to do
    /// again.
    fn attempt(
        &self,
        sending: &RequestShape,
        id: &str,
        deadline: Instant,
        journal: &mut Journal,
    ) -> Step {
        let reply = match self.transport.send(&wire::body(sending), deadline) {
            Ok(reply) => reply,
            Err(TransportFailure::Timeout { after }) => {
                journal.push(Entry::TimedOut {
                    id: id.to_owned(),
                    after,
                });
                return Step::Retryable {
                    reason: RetryReason::Timeout,
                    strip: None,
                    give_up: Outcome::Timeout {
                        by: id.to_owned(),
                        after,
                    },
                };
            }
            Err(failure) => {
                journal.push(Entry::Failed {
                    id: id.to_owned(),
                    failure: failure.clone(),
                });
                return Step::Retryable {
                    reason: RetryReason::Connection,
                    strip: None,
                    give_up: Outcome::Failed {
                        by: id.to_owned(),
                        failure,
                    },
                };
            }
        };

        if reply.status >= 400 {
            journal.push(Entry::Refused {
                id: id.to_owned(),
                status: reply.status,
                body: reply.body.clone(),
            });
            let strip = if reply.status < 500 {
                names_a_pin(&reply.body, &sending.sampler)
            } else {
                None
            };
            let give_up = Outcome::Refused {
                by: id.to_owned(),
                status: reply.status,
                body: reply.body.clone(),
            };
            return match strip {
                // A 4xx this client cannot act on is refused now rather than
                // retried: sending the same bytes again would be a retry whose
                // reason is "hope", and the bound would hide it as a loop.
                None => Step::Done(give_up),
                Some(setting) => Step::Retryable {
                    reason: RetryReason::FieldStripped,
                    strip: Some(setting),
                    give_up,
                },
            };
        }

        Step::Done(self.judge(sending, id, &reply.body, journal))
    }

    /// Turn a `2xx` body into an outcome.
    fn judge(
        &self,
        sending: &RequestShape,
        id: &str,
        body: &str,
        journal: &mut Journal,
    ) -> Outcome {
        let unreadable = |why: WireError, journal: &mut Journal| {
            journal.push(Entry::Unreadable {
                id: id.to_owned(),
                why: why.clone(),
            });
            Outcome::Unreadable {
                by: id.to_owned(),
                why,
            }
        };

        let parsed = match wire::read(&self.serving.dialect, body) {
            Ok(parsed) => parsed,
            Err(why) => return unreadable(why, journal),
        };
        let Some(text) = parsed.text else {
            return unreadable(WireError::NoAnswer, journal);
        };

        let counted = |value: Option<u64>| value.map(Count::new).transpose();
        let (output_tokens, prompt_tokens, cached_tokens) = match (
            counted(parsed.output_tokens),
            counted(parsed.prompt_tokens),
            counted(parsed.cached_tokens),
        ) {
            (Ok(output), Ok(prompt), Ok(cached)) => (output, prompt, cached),
            (Err(too_large), ..) | (_, Err(too_large), _) | (.., Err(too_large)) => {
                return unreadable(WireError::CountTooLarge(too_large.0), journal);
            }
        };

        let cache = Cache {
            dialect: self.serving.dialect.name.clone(),
            prompt_tokens,
            cached_tokens,
            cached_path: self.serving.dialect.cached_tokens.clone(),
        };
        journal.push(Entry::Cache {
            id: id.to_owned(),
            cache: cache.clone(),
        });

        let echo = Echo {
            dialect: self.serving.dialect.name.clone(),
            sent: sending.sampler.clone(),
            reported: parsed.sampler_echo,
            site_declared: parsed.echo_site_declared,
            site_present: parsed.echo_site_present,
        };
        match echo.verdict() {
            Verdict::Mismatched => journal.push(Entry::Mismatch {
                id: id.to_owned(),
                settings: echo.mismatches(),
            }),
            Verdict::Unverified | Verdict::NothingPinned => journal.push(Entry::Unverified {
                id: id.to_owned(),
                settings: echo.unverified(),
            }),
            Verdict::Confirmed => {}
        }

        let capped = parsed
            .finish_reason
            .as_deref()
            .is_some_and(|reason| CAPPED_FINISH_REASONS.contains(&reason));

        let answer = Box::new(Answer {
            produced_by: id.to_owned(),
            text,
            output_tokens,
            finish_reason: parsed.finish_reason,
            echo,
            cache,
        });
        journal.push(Entry::Received {
            to_request: id.to_owned(),
            output_tokens,
            capped,
        });

        if capped {
            journal.push(Entry::Capped {
                id: id.to_owned(),
                at: sending.limits.max_output_tokens,
            });
            Outcome::Capped(answer)
        } else {
            Outcome::Answered(answer)
        }
    }
}

/// What one attempt leaves the loop to do.
enum Step {
    /// Nothing more to try.
    Done(Outcome),
    /// Something to try again, and what to return if the budget is spent.
    Retryable {
        reason: RetryReason,
        strip: Option<SamplerSetting>,
        give_up: Outcome,
    },
}

/// The pinned setting a 4xx body names, if it names exactly one this request
/// sent.
///
/// Bounded three times, and the third was bought by review. To the closed
/// sampler vocabulary; to what this request actually carries; and to the
/// server's COMPLAINT rather than its whole reply.
///
/// The third bound is not fussiness. OpenAI-compatible stacks routinely quote
/// the offending request back inside the error object, so a body objecting to
/// `max_tokens` carries `"temperature":0.6` a few bytes later -- and a search
/// over the whole body found `temperature` first every time, because it is
/// first in the vocabulary. The client would then strip a pin nobody
/// complained about and retry under a regime the regimen does not declare.
///
/// And where the complaint names two pinned settings, this strips NEITHER. A
/// server that mentions two is a server this client cannot read; guessing
/// which one it meant is the same coincidence one bound further out.
fn names_a_pin(body: &str, card: &shape::SamplerCard) -> Option<SamplerSetting> {
    let lowered = complaint(body).to_ascii_lowercase();
    let mut named = SamplerSetting::ALL
        .iter()
        .copied()
        .filter(|setting| card.get(*setting).is_some() && lowered.contains(setting.tag()));
    let first = named.next()?;
    named.next().is_none().then_some(first)
}

/// What the server said was wrong.
///
/// `error.message` where the body is a JSON object carrying one -- which is
/// the shape every server on this surface uses -- and the whole body
/// otherwise, because a server that answers in prose is still answering.
fn complaint(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .as_ref()
        .and_then(|root| root.get("error"))
        .and_then(|error| match error {
            serde_json::Value::String(text) => Some(text.clone()),
            _ => error
                .get("message")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
        })
        .unwrap_or_else(|| body.to_owned())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    /// The substrate these calls were put to, as the projection's caller
    /// states it.
    ///
    /// A journal does not carry one: the schema refuses a `request` row with
    /// no substrate reference, and refuses it even when a run declares exactly
    /// one, so that the reference cannot be inferred from a count. In a test
    /// that means saying it out loud, which is the same thing `diet-drive`
    /// does one level up.
    const SUBSTRATE: &str = "a-substrate";

    use super::echo::{Verdict, Verified};
    use super::journal::{Entry, EntryKind};
    use super::shape::{
        Concurrency, Dialect, Limits, Message, Pin, RequestShape, Role, SamplerCard,
        SamplerSetting, Serving,
    };
    use super::stub::{Act, Stub};
    use super::transport::{Endpoint, Http};
    use super::{Bank, Client, IdSource, Outcome, Refusal, RetryReason, journal, transport, wire};
    use crate::formats::record::{Count, Event};

    /// The pins every test starts from, unless it is about a different card.
    fn card() -> SamplerCard {
        SamplerCard::empty()
            .with_decimal(SamplerSetting::Temperature, "0.6")
            .expect("0.6 is a decimal")
            .with_decimal(SamplerSetting::TopP, "0.95")
            .expect("0.95 is a decimal")
            .with(SamplerSetting::TopK, Pin::Integer(40))
    }

    fn shaped(sampler: SamplerCard, attempt_ms: u64, call_ms: u64, retries: u8) -> RequestShape {
        RequestShape {
            model: "a-model".to_owned(),
            messages: vec![
                Message::new(Role::System, "the regimen"),
                Message::new(Role::User, "the turn"),
            ],
            sampler,
            limits: Limits {
                attempt: Duration::from_millis(attempt_ms),
                call: Duration::from_millis(call_ms),
                max_output_tokens: 128,
                retries,
            },
            grammar: None,
        }
    }

    /// A reply in the shape a server sends, with an echo site this test writes.
    ///
    /// The echo is a string the CALLER supplies rather than something built
    /// from a `SamplerCard`, so that a test about a server contradicting the
    /// client cannot be satisfied by the client agreeing with itself.
    fn answered(text: &str, finish: &str, echo: &str) -> String {
        format!(
            "{{\"choices\":[{{\"message\":{{\"role\":\"assistant\",\"content\":\"{text}\"}},\
             \"finish_reason\":\"{finish}\"}}],\"usage\":{{\"prompt_tokens\":600,\
             \"completion_tokens\":7}},\"generation_settings\":{{{echo}}},\
             \"timings\":{{\"prompt_n_cached\":512}}}}"
        )
    }

    /// Every pin of [`card`], echoed back as sent.
    const HONEST_ECHO: &str = "\"temperature\":0.6,\"top_p\":0.95,\"top_k\":40";

    fn drive(
        acts: Vec<Act>,
        shape: &RequestShape,
        dialect: Dialect,
        concurrency: Concurrency,
    ) -> (super::Call, Vec<String>) {
        let stub = Stub::serving(acts).expect("loopback binds");
        let endpoint = Endpoint::parse(&stub.url()).expect("the stub's URL is an endpoint");
        let client = Client::new(
            Http::new(endpoint),
            Serving {
                concurrency,
                dialect,
            },
        );
        let mut ids = IdSource::new("turn-1/main");
        let call = client.call(shape, "main", &mut ids);
        (call, stub.received())
    }

    /// The common case, so every other test's setup can be trusted.
    fn honest(acts: Vec<Act>, shape: &RequestShape) -> (super::Call, Vec<String>) {
        drive(acts, shape, Dialect::llama_cpp(), Concurrency::Declared(2))
    }

    // -----------------------------------------------------------------------
    // #30's acceptance rows
    // -----------------------------------------------------------------------

    /// Row two: a stub server that ignores `temperature` and reports its own.
    #[test]
    fn a_server_that_reports_a_temperature_it_was_not_sent_is_a_mismatch_that_refuses_to_bank() {
        let (call, asked) = honest(
            vec![Act::Answer(answered(
                "an answer",
                "stop",
                "\"temperature\":0.9,\"top_p\":0.95,\"top_k\":40",
            ))],
            &shaped(card(), 5_000, 20_000, 0),
        );

        assert_eq!(asked.len(), 1, "one call, one request");
        assert!(
            asked[0].contains("\"temperature\":0.6"),
            "the client sent 0.6; the server reporting 0.9 is the server's doing: {}",
            asked[0]
        );

        let answer = call.outcome.answer().expect("the server answered");
        assert_eq!(answer.echo.verdict(), Verdict::Mismatched);

        let mismatches = answer.echo.mismatches();
        assert_eq!(
            mismatches,
            vec![Verified::Disagreed {
                setting: SamplerSetting::Temperature,
                sent: "0.6".to_owned(),
                reported: "0.9".to_owned(),
            }],
            "exactly the contradicted setting, and not the two the server echoed faithfully"
        );

        let recorded = call.journal.of(EntryKind::Mismatch);
        assert_eq!(recorded.len(), 1, "the mismatch is recorded, once");
        assert_eq!(
            EntryKind::Mismatch.tag(),
            "regime.mismatch",
            "the event the issue names, spelled the way it names it"
        );

        assert_eq!(
            call.bank(),
            Bank::Refused(vec![Refusal::RegimeMismatch]),
            "a graded run does not bank a result whose regime the server contradicted"
        );
        assert_eq!(Refusal::RegimeMismatch.tag(), "regime.mismatch");
    }

    /// Row three: a stub that 4xx-rejects a field once, then accepts.
    #[test]
    fn a_stripped_field_is_retried_and_the_answer_names_the_request_that_produced_it() {
        let (call, asked) = honest(
            vec![
                Act::Status(
                    400,
                    "{\"error\":{\"message\":\"unknown field: top_k\"}}".to_owned(),
                ),
                Act::Answer(answered(
                    "an answer",
                    "stop",
                    "\"temperature\":0.6,\"top_p\":0.95",
                )),
            ],
            &shaped(card(), 5_000, 20_000, 1),
        );

        assert_eq!(call.attempts.len(), 2, "one rejection, one retry");
        let first = &call.attempts[0];
        let second = &call.attempts[1];
        assert_eq!(first.retry_of, None, "the first attempt retries nothing");
        assert_eq!(
            second.retry_of.as_deref(),
            Some(first.id.as_str()),
            "the retry names its predecessor"
        );
        assert_ne!(first.id, second.id, "two requests, two identifiers");
        assert_eq!(second.because, Some(RetryReason::FieldStripped));
        assert_eq!(RetryReason::FieldStripped.tag(), "4xx-strip");
        assert_eq!(second.stripped, vec![SamplerSetting::TopK]);

        let answer = call.outcome.answer().expect("the second attempt answered");
        assert_eq!(
            answer.produced_by, second.id,
            "the record names the request that produced the answer, which is the SECOND one"
        );
        assert_ne!(
            answer.produced_by, first.id,
            "the answer did not come from the request the server rejected"
        );

        assert_eq!(asked.len(), 2);
        assert!(
            asked[0].contains("\"top_k\":40"),
            "the first send carried it"
        );
        assert!(
            !asked[1].contains("top_k"),
            "the retry sent the field the server rejected: {}",
            asked[1]
        );
        assert!(
            asked[1].contains("\"temperature\":0.6"),
            "and kept the pins nobody objected to"
        );

        assert_eq!(
            call.bank(),
            Bank::Refused(vec![Refusal::RegimeStripped]),
            "the answer was produced under a regime missing a pin the regimen declared"
        );
    }

    /// Row four: a stub that stalls past the timeout.
    #[test]
    fn a_stall_past_the_deadline_is_a_typed_timeout_and_never_an_empty_answer() {
        let (call, _asked) = honest(
            vec![Act::Stall(
                Duration::from_millis(600),
                answered("too late", "stop", HONEST_ECHO),
            )],
            &shaped(card(), 150, 20_000, 0),
        );

        let Outcome::Timeout { by, after } = &call.outcome else {
            panic!("a stall is a timeout, not {:?}", call.outcome);
        };
        assert_eq!(by, &call.attempts[0].id, "the timeout names the attempt");
        assert!(
            *after >= Duration::from_millis(140),
            "the reported elapsed time is measured, not the budget echoed back: {after:?}"
        );
        assert!(
            call.outcome.answer().is_none(),
            "a timeout carries no answer -- not an empty one, none"
        );
        assert!(call.journal.carries(EntryKind::TimedOut));
        assert_eq!(call.bank(), Bank::Refused(vec![Refusal::Timeout]));

        // The half that matters most: nothing downstream can mistake this for
        // a response, because the projection emits none.
        let projected = journal::project(&call.journal, SUBSTRATE);
        let responses = projected
            .events
            .iter()
            .filter(|event| matches!(event, Event::Response { .. }))
            .count();
        assert_eq!(responses, 0, "a timed-out request has no response row");
        assert_eq!(
            projected
                .events
                .iter()
                .filter(|event| matches!(event, Event::Request { .. }))
                .count(),
            1,
            "the request itself did happen and is recorded"
        );
    }

    // -----------------------------------------------------------------------
    // the distinctions the rows are shorthand for
    // -----------------------------------------------------------------------

    /// The defect the whole module exists for, stated as a test: these two
    /// runs differ in exactly one way, and the outcomes must not be the same
    /// value.
    #[test]
    fn an_answer_of_nothing_at_all_is_not_the_same_outcome_as_a_timeout() {
        let (empty, _) = honest(
            vec![Act::Answer(answered("", "stop", HONEST_ECHO))],
            &shaped(card(), 5_000, 20_000, 0),
        );
        let Outcome::Answered(answer) = &empty.outcome else {
            panic!("an empty answer is an answer, not {:?}", empty.outcome);
        };
        assert_eq!(answer.text, "", "the emptiness is preserved, as an answer");
        assert_eq!(empty.bank(), Bank::Bankable, "and it banks");

        let (stalled, _) = honest(
            vec![Act::Stall(
                Duration::from_millis(600),
                answered("", "stop", HONEST_ECHO),
            )],
            &shaped(card(), 150, 20_000, 0),
        );
        assert!(matches!(stalled.outcome, Outcome::Timeout { .. }));
        assert_ne!(
            std::mem::discriminant(&empty.outcome),
            std::mem::discriminant(&stalled.outcome),
            "the census counts these separately or it counts one of them wrong"
        );
    }

    #[test]
    fn a_capped_generation_is_a_typed_outcome_and_is_still_bankable() {
        let (call, _) = honest(
            vec![Act::Answer(answered(
                "as far as it g",
                "length",
                HONEST_ECHO,
            ))],
            &shaped(card(), 5_000, 20_000, 0),
        );

        let Outcome::Capped(answer) = &call.outcome else {
            panic!("`length` is a cap, not {:?}", call.outcome);
        };
        assert_eq!(answer.text, "as far as it g", "the partial answer is kept");
        assert_eq!(answer.finish_reason.as_deref(), Some("length"));
        assert!(call.journal.carries(EntryKind::Capped));
        assert_eq!(
            call.bank(),
            Bank::Bankable,
            "truncation is a typed outcome; refusing it here would be grading it as wrong"
        );
    }

    #[test]
    fn a_pin_the_server_never_mentions_is_unverified_rather_than_agreed() {
        // The OpenAI-compatible dialect declares no echo site at all, so there
        // is nowhere to look. That is the fabricated-instrument case: the pins
        // are asserted in the request and confirmed by nothing.
        let (call, _) = drive(
            vec![Act::Answer(answered("an answer", "stop", HONEST_ECHO))],
            &shaped(card(), 5_000, 20_000, 0),
            Dialect::openai_compatible(),
            Concurrency::Declared(1),
        );

        let answer = call.outcome.answer().expect("the server answered");
        assert_eq!(answer.echo.verdict(), Verdict::Unverified);
        assert!(
            !answer.echo.site_declared,
            "this dialect declares no echo site, and the record says so"
        );
        assert_eq!(
            answer.echo.unverified().len(),
            3,
            "all three pins are unverified, not silently agreed"
        );
        assert!(
            answer.echo.mismatches().is_empty(),
            "and none is a mismatch"
        );
        assert_eq!(call.bank(), Bank::Refused(vec![Refusal::RegimeUnverified]));
    }

    #[test]
    fn an_unreadable_echo_is_not_a_mismatch_and_not_an_agreement() {
        let (call, _) = honest(
            vec![Act::Answer(answered(
                "an answer",
                "stop",
                "\"temperature\":\"warm\",\"top_p\":0.95,\"top_k\":40",
            ))],
            &shaped(card(), 5_000, 20_000, 0),
        );

        let answer = call.outcome.answer().expect("the server answered");
        assert!(
            answer.echo.mismatches().is_empty(),
            "`warm` is not a number this client compares, so it is not a contradiction"
        );
        assert_eq!(
            answer.echo.unverified(),
            vec![Verified::Unreadable {
                setting: SamplerSetting::Temperature,
                reported: "warm".to_owned(),
            }]
        );
        assert_eq!(call.bank(), Bank::Refused(vec![Refusal::RegimeUnverified]));
    }

    #[test]
    fn a_regime_nobody_pinned_is_refused_rather_than_vacuously_confirmed() {
        let (call, _) = honest(
            vec![Act::Answer(answered("an answer", "stop", ""))],
            &shaped(SamplerCard::empty(), 5_000, 20_000, 0),
        );

        let answer = call.outcome.answer().expect("the server answered");
        assert_eq!(answer.echo.verdict(), Verdict::NothingPinned);
        assert_eq!(
            call.bank(),
            Bank::Refused(vec![Refusal::RegimeUnpinned]),
            "a check of nothing is not a pass"
        );
    }

    #[test]
    fn a_4xx_naming_no_pin_this_request_carried_is_refused_rather_than_retried() {
        let (call, asked) = honest(
            vec![
                Act::Status(
                    400,
                    "{\"error\":{\"message\":\"unknown field: presence_penalty\"}}".to_owned(),
                ),
                Act::Answer(answered("never reached", "stop", HONEST_ECHO)),
            ],
            &shaped(card(), 5_000, 20_000, 3),
        );

        assert_eq!(
            asked.len(),
            1,
            "sending the same bytes again would be a retry whose reason is hope"
        );
        assert_eq!(call.attempts.len(), 1);
        let Outcome::Refused { status, .. } = &call.outcome else {
            panic!("expected a refusal, got {:?}", call.outcome);
        };
        assert_eq!(*status, 400);
        assert_eq!(call.bank(), Bank::Refused(vec![Refusal::ServerRefused]));
    }

    #[test]
    fn a_timeout_that_has_a_retry_left_links_the_second_attempt_to_the_first() {
        let (call, _) = honest(
            vec![
                Act::Stall(
                    Duration::from_millis(600),
                    answered("late", "stop", HONEST_ECHO),
                ),
                Act::Stall(
                    Duration::from_millis(600),
                    answered("later", "stop", HONEST_ECHO),
                ),
            ],
            &shaped(card(), 300, 20_000, 1),
        );

        assert_eq!(call.attempts.len(), 2);
        assert_eq!(
            call.attempts[1].retry_of.as_deref(),
            Some(call.attempts[0].id.as_str())
        );
        assert_eq!(call.attempts[1].because, Some(RetryReason::Timeout));
        assert_eq!(RetryReason::Timeout.tag(), "timeout");
        assert!(matches!(call.outcome, Outcome::Timeout { .. }));
    }

    #[test]
    fn retries_are_bounded_by_the_limit_the_request_declares() {
        // The strip path rather than the stall path, because it is the one a
        // server can be made to walk deterministically: three rejections are
        // offered and the bound has to stop at the second request. A
        // stall-based version of this test cannot count requests server-side
        // -- the listener's backlog accepts the retry while the previous act
        // is still sleeping, so the count measures the stub's scheduling and
        // not the client's bound.
        let reject =
            |field: &str| Act::Status(400, format!("{{\"error\":\"unknown field: {field}\"}}"));
        let (call, asked) = honest(
            vec![reject("top_k"), reject("top_p"), reject("temperature")],
            &shaped(card(), 5_000, 20_000, 1),
        );

        assert_eq!(
            call.attempts.len(),
            2,
            "one attempt plus one retry, no more"
        );
        assert_eq!(asked.len(), 2, "and the server saw exactly two requests");
        assert!(
            matches!(call.outcome, Outcome::Refused { .. }),
            "the budget ran out on a refusal, and that is what it says: {:?}",
            call.outcome
        );
        assert_eq!(
            call.attempts[1].stripped,
            vec![SamplerSetting::TopK],
            "one strip happened; the second rejection was never acted on"
        );
    }

    #[test]
    fn a_server_that_hangs_up_is_a_transport_failure_and_not_an_answer() {
        let (call, _) = honest(vec![Act::Hangup], &shaped(card(), 5_000, 20_000, 0));

        let Outcome::Failed { by, failure } = &call.outcome else {
            panic!("a hangup is a failure, not {:?}", call.outcome);
        };
        assert_eq!(by, &call.attempts[0].id);
        assert!(matches!(failure, transport::TransportFailure::Malformed(_)));
        assert!(call.outcome.answer().is_none());
        assert_eq!(call.bank(), Bank::Refused(vec![Refusal::TransportFailed]));
    }

    #[test]
    fn a_200_whose_body_is_not_a_reply_is_unreadable_rather_than_an_empty_answer() {
        let (call, _) = honest(
            vec![Act::Answer("{\"choices\":[]}".to_owned())],
            &shaped(card(), 5_000, 20_000, 0),
        );

        let Outcome::Unreadable { why, .. } = &call.outcome else {
            panic!("expected unreadable, got {:?}", call.outcome);
        };
        assert_eq!(*why, wire::WireError::NoAnswer);
        assert_eq!(call.bank(), Bank::Refused(vec![Refusal::Unreadable]));
    }

    #[test]
    fn a_chunked_reply_is_decoded_rather_than_handed_to_the_reader_with_its_framing() {
        let body = answered("an answer", "stop", HONEST_ECHO);
        let (head, tail) = body.split_at(30);
        let (call, _) = honest(
            vec![Act::Chunked(vec![head.to_owned(), tail.to_owned()])],
            &shaped(card(), 5_000, 20_000, 0),
        );

        let answer = call
            .outcome
            .answer()
            .unwrap_or_else(|| panic!("a chunked reply is a reply, not {:?}", call.outcome));
        assert_eq!(answer.text, "an answer");
        assert_eq!(
            answer.echo.verdict(),
            Verdict::Confirmed,
            "and the whole body was reassembled, not just its first chunk"
        );
    }

    #[test]
    fn a_content_length_reply_is_answered_without_waiting_for_the_connection_to_close() {
        // The server announces its length and then holds the socket open. A
        // reader that only knows how to read to end-of-stream sits here until
        // its deadline and reports a timeout -- against a server that answered
        // immediately.
        let (call, _) = honest(
            vec![Act::AnswerAndHold(
                answered("an answer", "stop", HONEST_ECHO),
                Duration::from_millis(1_200),
            )],
            &shaped(card(), 300, 20_000, 0),
        );

        let answer = call.outcome.answer().unwrap_or_else(|| {
            panic!(
                "the length was announced and the bytes arrived: {:?}",
                call.outcome
            )
        });
        assert_eq!(answer.text, "an answer");
    }

    // -----------------------------------------------------------------------
    // cache telemetry and the declared serving
    // -----------------------------------------------------------------------

    #[test]
    fn cache_telemetry_is_read_from_the_path_the_dialect_declares_and_nowhere_else() {
        let body = answered("an answer", "stop", HONEST_ECHO);

        let (llama, _) = drive(
            vec![Act::Answer(body.clone())],
            &shaped(card(), 5_000, 20_000, 0),
            Dialect::llama_cpp(),
            Concurrency::Declared(2),
        );
        let cache = &llama.outcome.answer().expect("answered").cache;
        assert_eq!(
            cache.cached_tokens,
            Some(Count::new(512).expect("512 fits"))
        );
        assert_eq!(
            cache.prompt_tokens,
            Some(Count::new(600).expect("600 fits"))
        );
        assert_eq!(
            cache.cached_path.as_deref(),
            Some("timings.prompt_n_cached")
        );

        // The same bytes under a dialect that looks somewhere else. If the
        // client sniffed the body rather than reading its declared path, this
        // would find 512 too -- and a reader would have no way to know which
        // server's meaning of "cached" the number carried.
        let (openai, _) = drive(
            vec![Act::Answer(body)],
            &shaped(card(), 5_000, 20_000, 0),
            Dialect::openai_compatible(),
            Concurrency::Declared(2),
        );
        let cache = &openai.outcome.answer().expect("answered").cache;
        assert_eq!(
            cache.cached_tokens, None,
            "read by declaration, not by sniff"
        );
        assert_eq!(
            cache.cached_path.as_deref(),
            Some("usage.prompt_tokens_details.cached_tokens"),
            "and the path it looked at is on the record"
        );
    }

    #[test]
    fn the_serving_declaration_is_recorded_and_undeclared_is_not_one() {
        let (declared, _) = drive(
            vec![Act::Answer(answered("an answer", "stop", HONEST_ECHO))],
            &shaped(card(), 5_000, 20_000, 0),
            Dialect::llama_cpp(),
            Concurrency::Declared(1),
        );
        let (undeclared, _) = drive(
            vec![Act::Answer(answered("an answer", "stop", HONEST_ECHO))],
            &shaped(card(), 5_000, 20_000, 0),
            Dialect::llama_cpp(),
            Concurrency::Undeclared,
        );

        let concurrency_of = |call: &super::Call| {
            let entries = call.journal.of(EntryKind::Serving);
            assert_eq!(entries.len(), 1, "declared once per call");
            match entries[0] {
                Entry::Serving { concurrency, .. } => *concurrency,
                other => panic!("not a serving entry: {other:?}"),
            }
        };

        assert_eq!(concurrency_of(&declared), Concurrency::Declared(1));
        assert_eq!(concurrency_of(&undeclared), Concurrency::Undeclared);
        assert_ne!(
            concurrency_of(&declared),
            concurrency_of(&undeclared),
            "a throughput number taken under each of these answers a different question"
        );
    }

    // -----------------------------------------------------------------------
    // the claims review found nothing would notice the loss of
    // -----------------------------------------------------------------------

    #[test]
    fn a_reply_that_announces_more_than_it_sends_is_truncated_not_a_shorter_answer() {
        // The dangerous shape: the bytes that DID arrive are valid JSON with a
        // shorter answer in them. A reader that clamped to what arrived would
        // return `Answered` with a truncated answer and no marker at all --
        // a failure becoming a WRONG answer rather than an empty one.
        let (call, _) = honest(
            vec![Act::Undercount(
                answered("an answer", "stop", HONEST_ECHO),
                900,
            )],
            &shaped(card(), 5_000, 20_000, 0),
        );

        let Outcome::Failed { failure, .. } = &call.outcome else {
            panic!("a short reply is not an answer: {:?}", call.outcome);
        };
        assert!(
            matches!(failure, transport::TransportFailure::Truncated { .. }),
            "{failure:?}"
        );
        assert!(call.outcome.answer().is_none());
    }

    #[test]
    fn the_whole_call_budget_binds_even_where_each_attempt_is_inside_its_own() {
        // Every other test gives the call twenty seconds and each attempt a
        // fraction of a second, so `Limits::call` never binds and could be
        // deleted with the suite green. Here it is the only thing that stops
        // the drive: the attempt is allowed five seconds, the call four
        // hundred milliseconds, and the server answers after one second.
        let (call, _) = honest(
            vec![
                Act::Stall(
                    Duration::from_millis(1_000),
                    answered("too late", "stop", HONEST_ECHO),
                ),
                Act::Stall(
                    Duration::from_millis(1_000),
                    answered("later still", "stop", HONEST_ECHO),
                ),
            ],
            &shaped(card(), 5_000, 400, 2),
        );

        let Outcome::Timeout { after, .. } = &call.outcome else {
            panic!(
                "the call's budget ran out before the attempt's: {:?}",
                call.outcome
            );
        };
        assert!(
            *after < Duration::from_millis(2_000),
            "the attempt was cut to the call's remaining budget, not its own: {after:?}"
        );
        assert_eq!(
            call.attempts.len(),
            1,
            "two retries were allowed and none had any budget to run in"
        );
    }

    #[test]
    fn a_4xx_that_quotes_the_request_back_strips_nothing() {
        // What these servers actually send. The complaint is about
        // `max_tokens`; the request echoed beside it names every pin. A search
        // over the whole body finds `temperature` first -- it is first in the
        // vocabulary -- and the client would retry under a regime nobody
        // declared.
        let (call, asked) = honest(
            vec![
                Act::Status(
                    400,
                    "{\"error\":{\"message\":\"max_tokens is above the model's limit\",\
                     \"request\":{\"temperature\":0.6,\"top_p\":0.95,\"top_k\":40}}}"
                        .to_owned(),
                ),
                Act::Answer(answered("never reached", "stop", HONEST_ECHO)),
            ],
            &shaped(card(), 5_000, 20_000, 2),
        );

        assert_eq!(
            asked.len(),
            1,
            "nothing was stripped and nothing was retried"
        );
        assert!(matches!(call.outcome, Outcome::Refused { .. }));
        assert!(call.attempts[0].stripped.is_empty());
    }

    #[test]
    fn a_quoted_request_naming_exactly_one_pin_still_strips_nothing() {
        // The case the "exactly one" bound does NOT catch, and the reason the
        // complaint is read rather than the whole body: the server objects to
        // `max_tokens`, quotes the request back, and the request carries a
        // single pin. A whole-body search finds one pinned setting, believes
        // it, and retries under a regime nobody declared.
        let card = SamplerCard::empty()
            .with_decimal(SamplerSetting::Temperature, "0.6")
            .expect("0.6 is a decimal");
        let (call, asked) = honest(
            vec![
                Act::Status(
                    400,
                    "{\"error\":{\"message\":\"max_tokens is above the model's limit\",\
                     \"request\":{\"temperature\":0.6}}}"
                        .to_owned(),
                ),
                Act::Answer(answered("never reached", "stop", "\"temperature\":0.6")),
            ],
            &shaped(card, 5_000, 20_000, 2),
        );

        assert_eq!(
            asked.len(),
            1,
            "the complaint named no pin this request sent"
        );
        assert!(matches!(call.outcome, Outcome::Refused { .. }));
        assert!(call.attempts[0].stripped.is_empty());
    }

    #[test]
    fn a_complaint_naming_two_pinned_settings_strips_neither() {
        let (call, asked) = honest(
            vec![
                Act::Status(
                    400,
                    "{\"error\":{\"message\":\"top_k and top_p may not both be set\"}}".to_owned(),
                ),
                Act::Answer(answered("never reached", "stop", HONEST_ECHO)),
            ],
            &shaped(card(), 5_000, 20_000, 2),
        );

        assert_eq!(asked.len(), 1);
        assert!(
            matches!(call.outcome, Outcome::Refused { .. }),
            "guessing which of two the server meant is the coincidence one bound out"
        );
    }

    #[test]
    fn a_complaint_naming_a_setting_this_request_did_not_pin_strips_nothing() {
        // The second bound, which had no test: the vocabulary knows `min_p`,
        // and this request did not send it. Without the `card.get(..)` half,
        // the client would strip a pin it never had and retry identically
        // forever, up to the bound.
        let card = SamplerCard::empty()
            .with_decimal(SamplerSetting::Temperature, "0.6")
            .expect("0.6 is a decimal");
        let (call, asked) = honest(
            vec![
                Act::Status(
                    400,
                    "{\"error\":{\"message\":\"min_p is not supported\"}}".to_owned(),
                ),
                Act::Answer(answered("never reached", "stop", "\"temperature\":0.6")),
            ],
            &shaped(card, 5_000, 20_000, 2),
        );

        assert_eq!(asked.len(), 1);
        assert!(matches!(call.outcome, Outcome::Refused { .. }));
    }

    #[test]
    fn a_request_that_never_reached_a_server_is_still_a_request_the_client_issued() {
        // A stub with no acts stops at once and drops its listener, so the
        // port is closed by the time the client dials it.
        let stub = Stub::serving(Vec::new()).expect("loopback binds");
        let url = stub.url();
        assert!(stub.received().is_empty());

        let client = Client::new(
            Http::new(Endpoint::parse(&url).expect("an endpoint")),
            Serving {
                concurrency: Concurrency::Declared(1),
                dialect: Dialect::llama_cpp(),
            },
        );
        let mut ids = IdSource::new("turn-1/main");
        let call = client.call(&shaped(card(), 500, 5_000, 0), "main", &mut ids);

        assert_eq!(call.attempts.len(), 1, "the client issued one request");
        assert!(
            matches!(
                call.outcome,
                Outcome::Failed { .. } | Outcome::Timeout { .. }
            ),
            "and it did not arrive: {:?}",
            call.outcome
        );
        assert!(call.journal.carries(EntryKind::Issued));

        // The record gets a `request` row for it, and the record's `request`
        // means "a request went to the substrate" -- which this did not. The
        // projection names that gap rather than papering over it.
        let projected = journal::project(&call.journal, SUBSTRATE);
        assert!(
            projected.unspellable.iter().any(|entry| entry
                .what
                .contains("the difference between a request the client ISSUED")),
            "the gap is named: {:?}",
            projected.unspellable
        );
    }

    #[test]
    fn max_tokens_is_a_cap_as_much_as_length_is() {
        let (call, _) = honest(
            vec![Act::Answer(answered("cut off", "max_tokens", HONEST_ECHO))],
            &shaped(card(), 5_000, 20_000, 0),
        );
        assert!(
            matches!(call.outcome, Outcome::Capped(_)),
            "both spellings are the servers' words, and both are in the list: {:?}",
            call.outcome
        );
    }

    #[test]
    fn two_refusals_are_listed_in_one_order() {
        // A strip AND a contradicted pin, so `bank()` has two things to say
        // and the order it says them in is pinned rather than incidental.
        let (call, _) = honest(
            vec![
                Act::Status(
                    400,
                    "{\"error\":{\"message\":\"unknown field: top_k\"}}".to_owned(),
                ),
                Act::Answer(answered(
                    "an answer",
                    "stop",
                    "\"temperature\":0.9,\"top_p\":0.95",
                )),
            ],
            &shaped(card(), 5_000, 20_000, 1),
        );

        assert_eq!(
            call.bank(),
            Bank::Refused(vec![Refusal::RegimeMismatch, Refusal::RegimeStripped]),
            "declaration order of `Refusal::ALL`, not the order they were noticed in"
        );
    }

    #[test]
    fn a_reply_past_the_declared_cap_is_refused_rather_than_read_forever() {
        let stub = Stub::serving(vec![Act::Answer(answered(
            "an answer",
            "stop",
            HONEST_ECHO,
        ))])
        .expect("loopback binds");
        let client = Client::new(
            Http::with_reply_cap(Endpoint::parse(&stub.url()).expect("an endpoint"), 16),
            Serving {
                concurrency: Concurrency::Declared(1),
                dialect: Dialect::llama_cpp(),
            },
        );
        let mut ids = IdSource::new("turn-1/main");
        let call = client.call(&shaped(card(), 5_000, 20_000, 0), "main", &mut ids);
        let _ = stub.received();

        let Outcome::Failed { failure, .. } = &call.outcome else {
            panic!("past the cap is a failure: {:?}", call.outcome);
        };
        assert!(
            matches!(failure, transport::TransportFailure::Read(_)),
            "{failure:?}"
        );
    }

    #[test]
    fn a_token_count_no_record_can_spell_makes_the_reply_unreadable() {
        let (call, _) = honest(
            vec![Act::Answer(
                "{\"choices\":[{\"message\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}],\
                 \"usage\":{\"completion_tokens\":9223372036854775808}}"
                    .to_owned(),
            )],
            &shaped(card(), 5_000, 20_000, 0),
        );

        let Outcome::Unreadable { why, .. } = &call.outcome else {
            panic!(
                "a count with no spelling is not an answer: {:?}",
                call.outcome
            );
        };
        assert_eq!(
            *why,
            wire::WireError::CountTooLarge(9_223_372_036_854_775_808)
        );
    }

    // -----------------------------------------------------------------------
    // what the record cannot carry
    // -----------------------------------------------------------------------

    /// The disclosure, as an assertion.
    ///
    /// Record v0 can carry a request, a response, and the retry LINK. It
    /// cannot carry anything else this client learns. That list is written out
    /// here so that the day the schema grows a field, this goes red and the
    /// projection has to be brought up to it -- rather than the client
    /// quietly continuing to drop what the record could by then hold.
    #[test]
    fn the_record_projection_names_every_fact_record_v0_cannot_carry() {
        let (call, _) = honest(
            vec![
                Act::Status(400, "{\"error\":\"unknown field: top_k\"}".to_owned()),
                Act::Answer(answered("an answer", "length", "\"temperature\":0.9")),
            ],
            &shaped(card(), 5_000, 20_000, 1),
        );

        let projected = journal::project(&call.journal, SUBSTRATE);
        let kinds: Vec<EntryKind> = projected
            .unspellable
            .iter()
            .map(|entry| entry.kind)
            .collect();

        assert_eq!(
            kinds,
            vec![
                EntryKind::Serving,
                EntryKind::Issued,
                EntryKind::Issued,
                EntryKind::Refused,
                EntryKind::Stripped,
                EntryKind::Issued,
                EntryKind::Cache,
                EntryKind::Mismatch,
                EntryKind::Received,
                EntryKind::Capped,
            ],
            "the schema request, in the order the drive produced it. Three \
             `request.issued` rows because one request loses its sampler card, every \
             request loses the issued-versus-arrived distinction, and the retry also \
             loses its reason -- deduplicated by WHAT is lost, not by which kind lost it"
        );

        // And the part that IS carried, so the projection is not vacuously
        // "everything is lost".
        assert_eq!(
            projected.events.len(),
            3,
            "two requests and one response reach the record"
        );
        let Event::Request { id, retry_of, .. } = &projected.events[1] else {
            panic!("the second event is the retry: {:?}", projected.events[1]);
        };
        assert_eq!(
            retry_of.as_deref(),
            Some(call.attempts[0].id.as_str()),
            "the retry link survives the projection"
        );
        let Event::Response { to_request, .. } = &projected.events[2] else {
            panic!("the third event is the response: {:?}", projected.events[2]);
        };
        assert_eq!(
            to_request, id,
            "and the response names the request that produced it"
        );
    }

    #[test]
    fn every_journal_entry_kind_is_decided_by_the_projection() {
        /// What the projection does with a kind.
        #[derive(Debug, PartialEq, Eq)]
        enum Decision {
            /// It becomes a record event.
            Carried,
            /// The record cannot hold it, and the projection says so.
            Named,
            /// Neither, deliberately: the fact is already carried by another
            /// row's field, so there is nothing to emit and nothing to ask
            /// for.
            Structural,
        }

        let decide = |kind: EntryKind| {
            let mut journal = journal::Journal::new();
            journal.push(sample(kind));
            let projected = journal::project(&journal, SUBSTRATE);
            match (
                projected.events.is_empty(),
                projected.unspellable.is_empty(),
            ) {
                (false, _) => Decision::Carried,
                (true, false) => Decision::Named,
                (true, true) => Decision::Structural,
            }
        };

        // Written out rather than derived. A kind that is neither carried nor
        // named is a fact the client learned and nobody can find; the only one
        // allowed to be structural is the retry, whose link is already the
        // next request's `retry_of`. Add a kind and this fails until somebody
        // decides which column it is in.
        let expected = [
            (EntryKind::Serving, Decision::Named),
            (EntryKind::Issued, Decision::Carried),
            (EntryKind::Received, Decision::Carried),
            (EntryKind::Mismatch, Decision::Named),
            (EntryKind::Unverified, Decision::Named),
            (EntryKind::Stripped, Decision::Named),
            (EntryKind::Retried, Decision::Structural),
            (EntryKind::TimedOut, Decision::Named),
            (EntryKind::Capped, Decision::Named),
            (EntryKind::Refused, Decision::Named),
            (EntryKind::Failed, Decision::Named),
            (EntryKind::Unreadable, Decision::Named),
            (EntryKind::Cache, Decision::Named),
        ];

        assert_eq!(
            expected.len(),
            EntryKind::ALL.len(),
            "a kind was added and not decided here"
        );
        for (kind, decision) in expected {
            assert_eq!(decide(kind), decision, "{}", kind.tag());
        }
    }

    /// One entry of each kind. Written out rather than derived, because a
    /// constructor that walked the enum would agree with whatever the enum
    /// said and prove nothing about the projection.
    fn sample(kind: EntryKind) -> Entry {
        let id = "r/1".to_owned();
        match kind {
            EntryKind::Serving => Entry::Serving {
                concurrency: Concurrency::Undeclared,
                dialect: "d".to_owned(),
                endpoint: "http://127.0.0.1:1/v1".to_owned(),
            },
            EntryKind::Issued => Entry::Issued {
                id,
                lane: "main".to_owned(),
                retry_of: None,
                because: None,
                sampler: card(),
            },
            EntryKind::Received => Entry::Received {
                to_request: id,
                output_tokens: None,
                capped: false,
            },
            EntryKind::Mismatch => Entry::Mismatch {
                id,
                settings: Vec::new(),
            },
            EntryKind::Unverified => Entry::Unverified {
                id,
                settings: Vec::new(),
            },
            EntryKind::Stripped => Entry::Stripped {
                id,
                setting: SamplerSetting::TopK,
            },
            EntryKind::Retried => Entry::Retried {
                after: id,
                because: RetryReason::Timeout,
            },
            EntryKind::TimedOut => Entry::TimedOut {
                id,
                after: Duration::from_secs(1),
            },
            EntryKind::Capped => Entry::Capped { id, at: 128 },
            EntryKind::Refused => Entry::Refused {
                id,
                status: 400,
                body: String::new(),
            },
            EntryKind::Failed => Entry::Failed {
                id,
                failure: transport::TransportFailure::Connect("no".to_owned()),
            },
            EntryKind::Unreadable => Entry::Unreadable {
                id,
                why: wire::WireError::NoAnswer,
            },
            EntryKind::Cache => Entry::Cache {
                id,
                cache: super::Cache {
                    dialect: "d".to_owned(),
                    prompt_tokens: None,
                    cached_tokens: None,
                    cached_path: None,
                },
            },
        }
    }

    /// Every fault in this lane's `gate.toml` still names source that exists.
    ///
    /// The manifest carries each mutation's exact source text so the
    /// orchestrator (#46) can apply it. That only works while the text is
    /// still in the file it names, and nothing else checks: the orchestrator
    /// is not built, and `check-fault-manifest.py` reads the gate's own
    /// manifest rather than a package's. A manifest nobody checks is a
    /// manifest that goes stale, and a stale seeded fault is one that
    /// silently stops testing what it says it tests.
    ///
    /// Scanned rather than parsed as TOML: this crate has no TOML reader for
    /// multi-line strings, and writing one to check a file this repository
    /// generates would be a second reader of a format that already has one.
    /// The scan mirrors the generator's shape exactly, and a scan that finds
    /// no faults fails rather than passing over nothing.
    ///
    /// **Every field an orchestrator acts on, not just the anchor.** A
    /// `catches` naming a test that had been renamed or deleted used to pass:
    /// the fault would be applied, the named test would not run, and the run
    /// would be scored against a catcher that does not exist. `expect_exit`
    /// is checked for the same reason -- it is the number the orchestrator
    /// compares against.
    #[test]
    fn every_seeded_fault_still_names_source_that_is_there() {
        let manifest = include_str!("../../client/gate.toml");
        // The lane's whole source, so a catcher can be looked for wherever
        // its test lives rather than only in this file.
        let lane = concat!(
            include_str!("mod.rs"),
            include_str!("echo.rs"),
            include_str!("journal.rs"),
            include_str!("shape.rs"),
            include_str!("stub.rs"),
            include_str!("transport.rs"),
            include_str!("wire.rs")
        );
        let mut checked = 0;
        for block in manifest.split("\n[[fault]]\n").skip(1) {
            let id = between(block, "id = \"", "\"").expect("a fault has an id");
            let target = between(block, "target = \"", "\"").expect("a fault has a target");
            let anchor =
                between(block, "anchor = '''\n", "'''\nbecomes = ").expect("a fault has an anchor");

            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("the workspace root")
                .join(target);
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|why| panic!("{id}: {} could not be read: {why}", path.display()));
            assert_eq!(
                source.matches(anchor).count(),
                1,
                "{id}: its anchor no longer appears exactly once in {target}. The \
                 manifest is stale: either the mutation has to move with the code, \
                 or the fault it seeds is gone."
            );
            between(block, "expect_exit = ", "\n")
                .expect("a fault declares the exit it expects")
                .parse::<i32>()
                .unwrap_or_else(|why| panic!("{id}: its `expect_exit` is not a number: {why}"));

            let catches = between(block, "catches = [\n", "]").expect("a fault names its catchers");
            let mut named = 0;
            for line in catches.lines() {
                let Some(name) = between(line, "\"", "\"") else {
                    continue;
                };
                let path: Vec<&str> = name.split("::").collect();
                let (Some(leaf), true) = (
                    path.last().copied(),
                    path.len() == 1
                        || (path.len() >= 3
                            && path[0] == "client"
                            && path[path.len() - 2] == "tests"),
                ) else {
                    panic!("{id}: `{name}` is not a test this lane contains");
                };
                assert_eq!(
                    lane.matches(&format!("fn {leaf}(")).count(),
                    1,
                    "{id}: it claims to be caught by `{name}`, and no such test is in \
                     the lane. A fault whose catcher was renamed or deleted is applied, \
                     caught by nothing, and scored against a name."
                );
                named += 1;
            }
            assert!(
                named > 0,
                "{id}: a fault with an empty `catches` is a mutation nothing proves"
            );
            checked += 1;
        }
        let declared: usize = between(manifest, "\nfaults = ", "\n")
            .expect("the package block declares a count")
            .parse()
            .expect("the count is a number");
        assert_eq!(
            checked, declared,
            "the manifest declares its own count and this READS it. Hardcoded, it \
             said 24 beside a doc comment claiming the manifest declared it, and \
             `faults = 9001` passed."
        );
    }

    /// The text between `open` and the next `close` after it.
    fn between<'a>(haystack: &'a str, open: &str, close: &str) -> Option<&'a str> {
        let start = haystack.find(open)? + open.len();
        let end = haystack[start..].find(close)? + start;
        Some(&haystack[start..end])
    }

    #[test]
    fn the_clients_vocabularies_are_the_words_a_reader_will_see() {
        assert_eq!(
            RetryReason::ALL
                .iter()
                .map(|reason| reason.tag())
                .collect::<Vec<_>>(),
            ["4xx-strip", "timeout", "connection"],
            "the three reasons #30 names"
        );
        assert_eq!(
            Refusal::ALL
                .iter()
                .map(|refusal| refusal.tag())
                .collect::<Vec<_>>(),
            [
                "regime.mismatch",
                "regime.unverified",
                "regime.stripped",
                "regime.unpinned",
                "timeout",
                "server.refused",
                "transport.failed",
                "reply.unreadable",
            ]
        );
        assert_eq!(
            EntryKind::ALL
                .iter()
                .map(|kind| kind.tag())
                .collect::<Vec<_>>(),
            [
                "serving.declared",
                "request.issued",
                "response.received",
                "regime.mismatch",
                "regime.unverified",
                "regime.stripped",
                "request.retried",
                "outcome.timeout",
                "outcome.capped",
                "outcome.refused",
                "outcome.failed",
                "outcome.unreadable",
                "cache.observed",
            ]
        );
    }
}
