//! The request shape, as data.
//!
//! Every fork, seam and canonical turn is a request to an inference server,
//! and in prior work the shape of that request was assembled at each call
//! site. The consequences were paid one at a time: a sampler setting the
//! client believed it had sent, a field stripped by a retry that nobody
//! recorded, a concurrency setting that silently changed the meaning of every
//! throughput number in a report.
//!
//! So the shape is one value. It is built once, it is compared against what
//! the server says it used, and it is carried into the record beside the
//! answer it produced.
//!
//! Nothing here guesses what server is on the other end. Where the server
//! reports its own settings, and where it reports cache reuse, are DECLARED
//! per substrate ([`Dialect`]) rather than sniffed -- a client that guesses a
//! field name and finds nothing has no way to tell "the server ignored me"
//! from "I looked in the wrong place", and those are the two answers the whole
//! module exists to keep apart.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use crate::formats::record::json::Decimal;

use super::vocabulary;

vocabulary! {
    /// Who a message is from.
    Role {
        /// The regimen's standing instruction.
        System => "system",
        /// The turn, the interview ask, the tool result rendered back.
        User => "user",
        /// What the substrate said last time.
        Assistant => "assistant",
    }
}

/// One message of a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Who it is from.
    pub role: Role,
    /// What it says.
    pub content: String,
}

impl Message {
    /// A message from `role` saying `content`.
    #[must_use]
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }
}

vocabulary! {
    /// A sampler setting this client can pin and check.
    ///
    /// Closed on purpose. A setting outside this list is a setting the echo
    /// cannot verify, and a pin nobody verifies is the instrument that was
    /// fabricated for 94 of 94 replays in prior work: the arm's settings were
    /// asserted in every report and confirmed in none.
    SamplerSetting {
        /// Softmax temperature.
        Temperature => "temperature",
        /// Nucleus mass.
        TopP => "top_p",
        /// Top-k truncation.
        TopK => "top_k",
        /// Minimum relative probability.
        MinP => "min_p",
        /// Repetition penalty.
        RepeatPenalty => "repeat_penalty",
        /// The seed, which is what makes a replay a replay.
        Seed => "seed",
    }
}

/// A pinned value for a sampler setting.
///
/// A decimal keeps the digits that were written. `0.6` sent as an `f64` and
/// echoed back through another `f64` compares equal often enough to look
/// fine and unequal often enough to be a mystery; the regime's identity is
/// the text, so the text is what travels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pin {
    /// An exact decimal, as written.
    Decimal(Decimal),
    /// A whole number.
    Integer(i64),
}

impl fmt::Display for Pin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decimal(value) => f.write_str(value.as_str()),
            Self::Integer(value) => write!(f, "{value}"),
        }
    }
}

/// The sampler settings a request pins.
///
/// Empty is a legal card and means "whatever the server defaults to" -- which
/// is a regime nobody can reproduce, so it is a thing you have to write down
/// deliberately rather than a thing you get by forgetting.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SamplerCard {
    pinned: BTreeMap<SamplerSetting, Pin>,
}

impl SamplerCard {
    /// A card pinning nothing.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// This card with `setting` pinned to `pin`.
    #[must_use]
    pub fn with(mut self, setting: SamplerSetting, pin: Pin) -> Self {
        self.pinned.insert(setting, pin);
        self
    }

    /// This card with `setting` pinned to the decimal `text` spells.
    ///
    /// Returns `None` for a spelling the record's value space cannot read
    /// back, which is the only spelling check that matters: a number that
    /// cannot be written into a record cannot be part of a regime.
    #[must_use]
    pub fn with_decimal(self, setting: SamplerSetting, text: &str) -> Option<Self> {
        Some(self.with(setting, Pin::Decimal(Decimal::new(text)?)))
    }

    /// What `setting` is pinned to, if it is pinned.
    #[must_use]
    pub fn get(&self, setting: SamplerSetting) -> Option<&Pin> {
        self.pinned.get(&setting)
    }

    /// Every pinned setting, in a fixed order.
    pub fn iter(&self) -> impl Iterator<Item = (SamplerSetting, &Pin)> {
        self.pinned.iter().map(|(setting, pin)| (*setting, pin))
    }

    /// How many settings are pinned.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pinned.len()
    }

    /// Whether nothing is pinned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pinned.is_empty()
    }

    /// This card without `setting`.
    ///
    /// What a 4xx strip-and-retry does. It is spelled as a method that
    /// returns a new card rather than a mutation, because the card that was
    /// sent first still happened and the record names both.
    #[must_use]
    pub fn without(&self, setting: SamplerSetting) -> Self {
        let mut pinned = self.pinned.clone();
        pinned.remove(&setting);
        Self { pinned }
    }
}

/// What a request is allowed to cost.
///
/// A cap is not a failure mode: a generation that hits `max_output_tokens` is
/// a [`super::Outcome::Capped`], never a wrong answer or a malformed one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// How long ONE attempt may take.
    ///
    /// Separate from [`Limits::call`] because a single deadline for the whole
    /// call makes `timeout` an unretryable reason by construction: the only
    /// moment an attempt could time out would be the moment the call's budget
    /// ran out, so the retry could never be made and the reason could never
    /// fire. A test written against that shape goes green while proving
    /// nothing, which is how this was found.
    pub attempt: Duration,
    /// How long the whole call may take, retries included. An attempt is
    /// given whichever of the two runs out first.
    pub call: Duration,
    /// How many tokens the answer may run to.
    pub max_output_tokens: u32,
    /// How many retries may be attempted. Zero means the first attempt is the
    /// only attempt.
    pub retries: u8,
}

/// How the server was configured to serve.
///
/// Declared, never assumed. A throughput figure whose record cannot say
/// whether the server was serving one stream or four is a number without its
/// question, and in prior work a concurrency setting changed under a series
/// of measurements that were then compared as if they were comparable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Serving {
    /// How many streams the server was configured for.
    pub concurrency: Concurrency,
    /// The dialect the server speaks, which is what says where to read its
    /// own report of itself.
    pub dialect: Dialect,
}

/// How many streams the server serves at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Concurrency {
    /// The operator declared it.
    Declared(u32),
    /// Nobody declared it. A legal state, and one a reader has to be able to
    /// see: "undeclared" and "one" are different facts, and a default of one
    /// would turn the first into the second.
    Undeclared,
}

/// Where a server reports what it actually did.
///
/// A path, not a guess. Each field is a dotted path into the response object;
/// an empty path means the top level. `Dialect` is data because the servers
/// this client is pointed at (the local llama.cpp family, vLLM, `SGLang`, the
/// Apple-side servers) put these in different places, and a client that
/// pattern-matched on a server banner would be asserting a mapping nobody
/// checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialect {
    /// A name for this dialect, carried into the journal so a later reader
    /// knows which mapping was in force.
    pub name: String,
    /// Where the server reports the sampler settings it used. `None` means
    /// this dialect does not report them at all -- which is a fact about the
    /// server, and makes every pin under it [`super::Verified::Unreported`].
    pub sampler_echo: Option<String>,
    /// Where the server reports how many prompt tokens it read.
    pub prompt_tokens: Option<String>,
    /// Where the server reports how many of them it reused from its cache.
    pub cached_tokens: Option<String>,
    /// Where the server reports why it stopped.
    pub finish_reason: Option<String>,
}

impl Dialect {
    /// The OpenAI-compatible surface as vLLM and `SGLang` serve it.
    ///
    /// **Unverified against a live server.** These paths are read from the
    /// published shape of the surface, not measured; the data seat owns
    /// confirming them against the servers it runs. Until it does, a drive
    /// under this dialect that reports `Unreported` for every setting is
    /// telling you about this constant as much as about the server, and the
    /// journal names the dialect so the two can be told apart.
    #[must_use]
    pub fn openai_compatible() -> Self {
        Self {
            name: "openai-compatible".to_owned(),
            sampler_echo: None,
            prompt_tokens: Some("usage.prompt_tokens".to_owned()),
            cached_tokens: Some("usage.prompt_tokens_details.cached_tokens".to_owned()),
            finish_reason: Some("choices.0.finish_reason".to_owned()),
        }
    }

    /// The llama.cpp server's OpenAI-compatible endpoint.
    ///
    /// **Unverified against a live server**, on the same terms as
    /// [`Dialect::openai_compatible`].
    #[must_use]
    pub fn llama_cpp() -> Self {
        Self {
            name: "llama.cpp".to_owned(),
            sampler_echo: Some("generation_settings".to_owned()),
            prompt_tokens: Some("usage.prompt_tokens".to_owned()),
            cached_tokens: Some("timings.prompt_n_cached".to_owned()),
            finish_reason: Some("choices.0.finish_reason".to_owned()),
        }
    }
}

/// One request to an inference server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestShape {
    /// The model, as the serving stack names it.
    pub model: String,
    /// The conversation, in order.
    pub messages: Vec<Message>,
    /// What the sampler is pinned to.
    pub sampler: SamplerCard,
    /// What the request may cost.
    pub limits: Limits,
    /// A grammar the answer must satisfy, when the regimen constrains it.
    pub grammar: Option<String>,
}

impl RequestShape {
    /// The prefix this request would prefill: every message before the last.
    ///
    /// The seam controller's whole claim is that this stays byte-identical
    /// between seams, so the client is where it can be read off a request
    /// rather than reconstructed from one.
    #[must_use]
    pub fn prefix(&self) -> String {
        let mut out = String::new();
        for message in self
            .messages
            .iter()
            .take(self.messages.len().saturating_sub(1))
        {
            out.push_str(message.role.tag());
            out.push('\n');
            out.push_str(&message.content);
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Limits, Message, Pin, RequestShape, Role, SamplerCard, SamplerSetting};

    #[test]
    fn the_shapes_vocabularies_are_the_words_the_wire_carries() {
        assert_eq!(
            Role::ALL.iter().map(|role| role.tag()).collect::<Vec<_>>(),
            ["system", "user", "assistant"]
        );
        assert_eq!(
            SamplerSetting::ALL
                .iter()
                .map(|setting| setting.tag())
                .collect::<Vec<_>>(),
            [
                "temperature",
                "top_p",
                "top_k",
                "min_p",
                "repeat_penalty",
                "seed",
            ]
        );
    }

    #[test]
    fn a_spelling_the_record_cannot_read_back_is_not_a_pin() {
        assert!(
            SamplerCard::empty()
                .with_decimal(SamplerSetting::Temperature, "7e-1")
                .is_none(),
            "an exponent has no spelling in a record, so it cannot be part of a regime"
        );
        assert!(
            SamplerCard::empty()
                .with_decimal(SamplerSetting::Temperature, "01.5")
                .is_none(),
            "one spelling per value"
        );
        assert!(
            SamplerCard::empty()
                .with_decimal(SamplerSetting::Temperature, "0.6")
                .is_some()
        );
    }

    #[test]
    fn stripping_a_pin_leaves_the_card_that_was_already_sent_alone() {
        let sent = SamplerCard::empty()
            .with_decimal(SamplerSetting::Temperature, "0.6")
            .expect("a decimal")
            .with(SamplerSetting::TopK, Pin::Integer(40));
        let retried = sent.without(SamplerSetting::TopK);

        assert_eq!(retried.len(), 1);
        assert!(retried.get(SamplerSetting::TopK).is_none());
        assert_eq!(
            sent.len(),
            2,
            "the request that was already sent still happened"
        );
    }

    #[test]
    fn the_prefix_is_every_message_but_the_last_and_does_not_move_when_the_last_does() {
        let base = RequestShape {
            model: "a-model".to_owned(),
            messages: vec![
                Message::new(Role::System, "the regimen"),
                Message::new(Role::User, "turn one"),
            ],
            sampler: SamplerCard::empty(),
            limits: Limits {
                attempt: Duration::from_secs(1),
                call: Duration::from_secs(1),
                max_output_tokens: 16,
                retries: 0,
            },
            grammar: None,
        };
        let next_turn = RequestShape {
            messages: vec![
                Message::new(Role::System, "the regimen"),
                Message::new(Role::User, "turn two, which is longer"),
            ],
            ..base.clone()
        };

        assert_eq!(
            base.prefix(),
            next_turn.prefix(),
            "the prefill the two share is byte-identical, which is the whole seam claim"
        );
        assert!(base.prefix().contains("the regimen"));
        assert!(
            !base.prefix().contains("turn one"),
            "the turn itself is not part of the prefix it is appended to"
        );
    }
}
