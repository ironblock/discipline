//! Sampler echo: what the client sent, against what the server says it used.
//!
//! In prior work an experiment's arms were pinned in every report and
//! confirmed in none -- the instrument was, on inspection, fabricated for 94
//! of 94 replays. Nothing about the pins was wrong on purpose; nobody had
//! ever asked the server what it did with them.
//!
//! So the client asks, every time, and the answer is one of four things per
//! setting. The two that are not "agreed" and not "disagreed" matter as much
//! as the ones that are: a server that reports nothing and a server that
//! reports something unreadable are both **unverified**, and calling either
//! of them a mismatch would manufacture a finding out of a server's
//! formatting -- while calling either of them agreement is the fabrication
//! this module exists to make impossible.

use std::collections::BTreeMap;

use super::shape::{SamplerCard, SamplerSetting};
use super::wire::{self, Reported};

/// What became of one pinned setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verified {
    /// The server reported the number that was pinned.
    Agreed {
        /// Which setting.
        setting: SamplerSetting,
        /// The value both sides mean, in one spelling.
        value: String,
    },
    /// The server reported a different number. This is the finding.
    Disagreed {
        /// Which setting.
        setting: SamplerSetting,
        /// What the client pinned.
        sent: String,
        /// What the server said it used.
        reported: String,
    },
    /// The server said nothing about this setting -- either because the
    /// dialect declares no echo site, or because the site was there and this
    /// key was not.
    Unreported {
        /// Which setting.
        setting: SamplerSetting,
        /// Whether anywhere to look was declared at all.
        site_declared: bool,
    },
    /// The server reported something this client does not compare: an
    /// exponent, a string that is not a number, an object, a `null`.
    Unreadable {
        /// Which setting.
        setting: SamplerSetting,
        /// What it said, as it said it.
        reported: String,
    },
}

impl Verified {
    /// Which setting this is about.
    #[must_use]
    pub fn setting(&self) -> SamplerSetting {
        match self {
            Self::Agreed { setting, .. }
            | Self::Disagreed { setting, .. }
            | Self::Unreported { setting, .. }
            | Self::Unreadable { setting, .. } => *setting,
        }
    }
}

/// One request's pins, against one reply's report of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Echo {
    /// The dialect that said where to look, by name, so a later reader can
    /// tell "the server reported nothing" from "we read the wrong place".
    pub dialect: String,
    /// What was pinned.
    pub sent: SamplerCard,
    /// What the server reported, by the names it used.
    pub reported: BTreeMap<String, Reported>,
    /// Whether the dialect declares an echo site.
    pub site_declared: bool,
    /// Whether that site was present in the reply.
    pub site_present: bool,
}

/// What an echo says about a whole request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Every pinned setting was reported, and agreed.
    Confirmed,
    /// At least one setting disagreed.
    Mismatched,
    /// Nothing disagreed, and at least one setting could not be checked.
    Unverified,
    /// The card pins nothing, so there was nothing to check. Its own verdict
    /// rather than a flavour of confirmed: a check of nothing is not a pass,
    /// and a regime nobody pinned is a regime nobody can reproduce.
    NothingPinned,
}

impl Echo {
    /// Every pinned setting, judged, in the settings' fixed order.
    #[must_use]
    pub fn settings(&self) -> Vec<Verified> {
        self.sent
            .iter()
            .map(|(setting, pin)| self.judge(setting, &pin.to_string()))
            .collect()
    }

    fn judge(&self, setting: SamplerSetting, sent: &str) -> Verified {
        let Some(reported) = self.reported.get(setting.tag()) else {
            return Verified::Unreported {
                setting,
                site_declared: self.site_declared,
            };
        };

        let raw = reported.to_string();
        let (Some(sent_normal), Some(reported_normal)) =
            (wire::normalize(sent), wire::normalize(&raw))
        else {
            return Verified::Unreadable {
                setting,
                reported: raw,
            };
        };

        if sent_normal == reported_normal {
            Verified::Agreed {
                setting,
                value: sent_normal,
            }
        } else {
            Verified::Disagreed {
                setting,
                sent: sent_normal,
                reported: reported_normal,
            }
        }
    }

    /// Every setting the server contradicted.
    #[must_use]
    pub fn mismatches(&self) -> Vec<Verified> {
        self.settings()
            .into_iter()
            .filter(|verified| matches!(verified, Verified::Disagreed { .. }))
            .collect()
    }

    /// Every setting that could not be checked either way.
    #[must_use]
    pub fn unverified(&self) -> Vec<Verified> {
        self.settings()
            .into_iter()
            .filter(|verified| {
                matches!(
                    verified,
                    Verified::Unreported { .. } | Verified::Unreadable { .. }
                )
            })
            .collect()
    }

    /// What this echo says about the request as a whole.
    ///
    /// A disagreement outranks an unverifiable setting: if the server
    /// contradicted one pin, what it failed to say about another is not the
    /// headline.
    #[must_use]
    pub fn verdict(&self) -> Verdict {
        if self.sent.is_empty() {
            return Verdict::NothingPinned;
        }
        if !self.mismatches().is_empty() {
            return Verdict::Mismatched;
        }
        if self.unverified().is_empty() {
            Verdict::Confirmed
        } else {
            Verdict::Unverified
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::super::shape::{Pin, SamplerCard, SamplerSetting};
    use super::super::wire::Reported;
    use super::{Echo, Verdict, Verified};

    fn echo(pins: SamplerCard, reported: &[(&str, Reported)]) -> Echo {
        Echo {
            dialect: "test".to_owned(),
            sent: pins,
            reported: reported
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect::<BTreeMap<_, _>>(),
            site_declared: true,
            site_present: true,
        }
    }

    fn pinned(setting: SamplerSetting, text: &str) -> SamplerCard {
        SamplerCard::empty()
            .with_decimal(setting, text)
            .expect("a decimal the record can read")
    }

    #[test]
    fn a_different_spelling_of_the_same_number_is_agreement() {
        let echo = echo(
            pinned(SamplerSetting::Temperature, "0.60"),
            &[("temperature", Reported::Number("0.6".to_owned()))],
        );
        assert_eq!(echo.verdict(), Verdict::Confirmed);
        assert_eq!(
            echo.settings(),
            vec![Verified::Agreed {
                setting: SamplerSetting::Temperature,
                value: "0.6".to_owned(),
            }]
        );
    }

    #[test]
    fn a_contradiction_outranks_a_pin_nobody_reported() {
        let card =
            pinned(SamplerSetting::Temperature, "0.6").with(SamplerSetting::TopK, Pin::Integer(40));
        let echo = echo(card, &[("temperature", Reported::Number("0.9".to_owned()))]);

        assert_eq!(
            echo.verdict(),
            Verdict::Mismatched,
            "what the server failed to say about top_k is not the headline"
        );
        assert_eq!(echo.mismatches().len(), 1);
        assert_eq!(
            echo.unverified(),
            vec![Verified::Unreported {
                setting: SamplerSetting::TopK,
                site_declared: true,
            }],
            "and the unreported pin is still on the record"
        );
    }

    #[test]
    fn an_empty_card_is_not_vacuously_confirmed() {
        assert_eq!(
            echo(SamplerCard::empty(), &[]).verdict(),
            Verdict::NothingPinned
        );
    }

    #[test]
    fn a_setting_reported_as_something_unreadable_is_not_a_contradiction() {
        let echo = echo(
            pinned(SamplerSetting::TopP, "0.95"),
            &[("top_p", Reported::Other("null"))],
        );
        assert_eq!(echo.verdict(), Verdict::Unverified);
        assert!(echo.mismatches().is_empty());
    }
}
