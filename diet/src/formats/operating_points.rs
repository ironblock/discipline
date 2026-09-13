//! operating-points — the per-model operating points, as a format.
//!
//! `diet/formats/operating-points/grammar.pest` says what a document is and
//! why. What this module adds is the part a PEG is not for, and the part a
//! table cannot carry.
//!
//! **The projection is an array.** The file's matching rule is "the first
//! table whose `match` is a substring of the served id, in FILE ORDER", and
//! it says why: `qwen3` is a substring of `qwen3.6`, so it is written after
//! it. A projection into an object would sort, and sorting is the one
//! rearrangement that inverts this rule — the regimen reader accepts this
//! document today and hands back `gemma, qwen3, qwen3_5, qwen3_6` for a file
//! written `qwen3_6, qwen3_5, qwen3, gemma`. Match `qwen3.6` against that and
//! the `qwen3` entry wins with `thinking_kwarg = false`, where the entry that
//! should have won says `true` and carries the receipt saying the
//! `/no_think` soft switch is DEAD on this model and the kwarg is the only
//! working control.
//!
//! The text itself is not read here. It lives once, behind
//! [`crate::dogma::operating_points`], which checks its digest before handing
//! a byte of it out; the fixture in this format's corpus is a copy for the
//! conformance harness, and the test that holds the two together runs on the
//! dogma's side of that accessor rather than opening the file again.

use pest::Parser;
use pest::iterators::Pair;
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "../formats/regimen/grammar.pest"]
#[grammar = "../formats/operating_points/grammar.pest"]
#[grammar = "../formats/number.pest"]
struct PointsParser;

/// An operation that may run with thinking suppressed.
///
/// Closed, and closed in the grammar as well as here: everything not listed
/// keeps thinking, so a spelling nobody measured must not be able to widen
/// the list. Suppressing thinking on an operation no run measured is the
/// failure this vocabulary exists to make impossible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NoThinkOp {
    /// Pulling stated facts out of material that is in front of the model.
    Extraction,
    /// Ruling on something, where the ruling is the product.
    Judgment,
    /// Reviewing working notes against what has happened since.
    Audit,
}

impl NoThinkOp {
    /// Every operation, in declaration order.
    pub const ALL: &'static [Self] = &[Self::Extraction, Self::Judgment, Self::Audit];

    /// The spelling the file uses.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Extraction => "extraction",
            Self::Judgment => "judgment",
            Self::Audit => "audit",
        }
    }

    /// The operation a spelling names.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|op| op.tag() == tag)
    }
}

/// One model's operating point, in the position the file wrote it.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// The table's name.
    pub key: String,
    /// The substring matched against the served model id.
    pub matches: String,
    /// What may run with thinking suppressed.
    pub nothink_ops: Vec<NoThinkOp>,
    /// Whether the thinking control is a kwarg on this family.
    pub thinking_kwarg: Option<bool>,
    /// Whether the reasoning block must be preserved in the prefix.
    pub preserve_thinking: Option<bool>,
    /// Whether this entry is consulted only after every other `match` fails.
    ///
    /// Absent in the document means `false`: an ordinary entry, matched in
    /// file order with the rest. See [`serves`] for what the flag buys.
    pub fallback: bool,
    /// Sampler settings, as the serving stack names them.
    pub sampler: Vec<(String, crate::formats::record::json::Value)>,
    /// Gate settings.
    pub gate: Vec<(String, crate::formats::record::json::Value)>,
    /// What was measured, in prose, under names the measurement chose.
    pub receipts: Vec<(String, crate::formats::record::json::Value)>,
}

/// Why a document is not a set of operating points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The grammar refused it.
    Syntax(String),
    /// A `[x.sampler]`, `[x.gate]` or `[x.receipts]` under a different `[y]`.
    ///
    /// A PEG will not carry a name across a header to check it against a
    /// later one, so this is checked here — the same place, and for the same
    /// reason, that the record format checks "summary is the last row".
    SubTableBelongsElsewhere {
        /// The model entry the sub-table sits in.
        entry: String,
        /// The name its header actually carried.
        written: String,
        /// Which sub-table it is.
        which: &'static str,
    },
    /// Two entries with the same table name.
    DuplicateKey(String),
    /// Two entries matching the same substring: which one wins would depend
    /// on the order, and an operating point decided by position alone is a
    /// measurement nobody can look up.
    ///
    /// SAME-EXCEPT-CASE COUNTS, because [`serves`] folds. Found by a fresh
    /// instance reviewing the fold that made it true: the fold taught the
    /// MATCHER that two spellings are one match and left the DETECTOR
    /// comparing raw bytes, so `Shared` and `shared` were two entries the
    /// document was allowed to declare and the second could never be reached
    /// for any input. The pair travels together or the invariant is a
    /// sentence rather than a rule.
    ///
    /// BOTH SPELLINGS ARE CARRIED, and that is not cosmetic: the corpus
    /// refuses a second fixture that produces an existing fixture's message,
    /// so a case-only duplicate reported with the byte-identical case's
    /// wording pins nothing that one did not — and worse, sends a reader to
    /// search the file for a literal that appears there only once.
    DuplicateMatch {
        /// The earlier entry's spelling: the one that wins.
        first: String,
        /// The later entry's spelling: the one nothing can reach. Equal to
        /// `first` when the two are byte-identical.
        again: String,
    },
    /// An unmarked entry whose `match` is a proper substring of another's, so
    /// which of the two serves depends on where the lines happen to sit.
    ///
    /// `qwen3` is a substring of `qwen3.6`: every id the narrower entry is
    /// about, the wider one also matches. Under pass one of [`serves`] the
    /// winner is whichever was written first, which makes an operating point
    /// -- a transcribed measurement -- decided by line order. The dogma got
    /// away with it by writing the wider entry last and explaining the
    /// ordering in a comment, and a comment is not a rule: a consumer reading
    /// the tables into an unordered map silently gets the other answer.
    ///
    /// `fallback = true` on the WIDER entry is the fix, and this refusal is
    /// what makes it obligatory rather than available. Marked, the wider entry
    /// is skipped in pass one and reached only when nothing else matched, so
    /// where it sits stops mattering and the comment stops being load-bearing.
    ///
    /// FOLDED, because [`serves`] folds and [`Error::DuplicateMatch`] folds.
    /// A detector that compared raw bytes here would miss `Qwen3` shadowing
    /// `qwen3.6` -- the same one-step-sideways miss the fold already cost
    /// this file once.
    ShadowedByUnmarked {
        /// The wider entry: the one that needs the mark.
        wider: String,
        /// A narrower entry it shadows. There may be more than one; this is
        /// the first in file order, because one example is what a reader
        /// needs to see the problem.
        narrower: String,
    },
    /// An entry with no `match`, or no `nothink_ops`.
    MissingKey {
        /// The entry.
        entry: String,
        /// The key it did not carry.
        key: &'static str,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Syntax(text) => write!(f, "{text}"),
            Self::SubTableBelongsElsewhere {
                entry,
                written,
                which,
            } => write!(
                f,
                "`[{written}.{which}]` sits inside `[{entry}]`; a sub-table belongs to \
                 the entry above it, and one that names another entry is a setting \
                 filed under a model that never had it"
            ),
            Self::DuplicateKey(key) => {
                write!(
                    f,
                    "`[{key}]` is declared twice; which one serves is not a \
                          question a reader should have to answer"
                )
            }
            Self::DuplicateMatch { first, again } if first == again => write!(
                f,
                "two entries match `{first}`; the rule is first match in file order, so \
                 the second is unreachable and the reader that finds it has read a \
                 different file"
            ),
            Self::DuplicateMatch { first, again } => write!(
                f,
                "`{first}` and `{again}` differ only in case, and matching folds case, \
                 so they are one match written twice; the rule is first match in file \
                 order, so `{again}` is unreachable for every input"
            ),
            Self::ShadowedByUnmarked { wider, narrower } => write!(
                f,
                "`{wider}` is a substring of `{narrower}`, so every id `{narrower}` is \
                 about `{wider}` matches too, and which one serves depends on where \
                 the lines sit; mark the wider one `fallback = true` so it is tried \
                 last and the order stops deciding"
            ),
            Self::MissingKey { entry, key } => {
                write!(f, "`[{entry}]` carries no `{key}`")
            }
        }
    }
}

/// The entry that serves a model id, or `None` if the document has none.
///
/// **This function is the matching rule.** It lived in a test until now, as
/// its only implementation — which meant the rule the whole format exists to
/// state had no caller outside the assertion that checked it, and any
/// consumer wanting an operating point had to re-derive it. One reader.
///
/// Two passes, and the second is the whole point of [`Entry::fallback`]:
///
/// 1. The first UNMARKED entry, in file order, whose `match` is a substring
///    of the served id. This is the rule the file's own header states.
/// 2. Failing that, the first entry MARKED `fallback = true`, in file order,
///    whose `match` is a substring of the served id.
///
/// A document with no fallback behaves exactly as it did before this existed:
/// pass two finds nothing to consider, and pass one is the old rule verbatim.
/// A document that marks its shadowing entry gets something the old rule
/// could not give it — the answer stops depending on where the entry sits.
/// `qwen3` is a substring of `qwen3.6`, so today it must be written below the
/// entry it would otherwise steal; marked, it may be written anywhere.
///
/// CASE IS FOLDED, both sides, to ASCII lowercase before the comparison —
/// ruled 2026-09-12, and it is a BUG FIX rather than a spelling change.
///
/// The comparison used to be `served.contains(entry.matches)` byte for byte,
/// and `Qwen3.6-27B` — written 316 times in
/// `diet/capture/sense/register/mined.provenance.jsonl`, the mined corpus
/// this serves, and more than any other model id — served NOTHING, while its
/// lowercase twin served `qwen3_6`. (Counted in the record, not in the tree:
/// a tree-wide grep sweeps in this comment and the test below, which is how
/// a first count of it came out at 317.)
///
/// No `nothink_ops`, no `thinking_kwarg`, no sampler, for the family the file
/// is mostly about. Model identifiers are ASCII and case-preserving but not
/// case-MEANINGFUL, so that was a defect and not a policy.
///
/// **This is not serving-invariant, and must never be recorded as though it
/// were.** Three of the nineteen harvested names go from *nothing served* to
/// *an entry served*; the invariance the respell rests on is between the two
/// FILES under one rule, not between this rule and the one before it.
/// [`tests::the_fold_is_a_bug_fix_and_not_an_invariance`] pins the difference
/// so it cannot be quietly relabelled.
///
/// ASCII only, deliberately: `to_ascii_lowercase` and not `to_lowercase`.
/// Full Unicode folding would make the rule depend on locale-adjacent
/// behaviour (the Turkish dotless i, the Kelvin sign) for identifiers that
/// are ASCII by construction, which is a wider promise than the ruling made.
#[must_use]
pub fn serves<'a>(entries: &'a [Entry], served: &str) -> Option<&'a Entry> {
    let served = served.to_ascii_lowercase();
    let hit = |entry: &&Entry, fallback: bool| {
        entry.fallback == fallback && served.contains(&entry.matches.to_ascii_lowercase())
    };
    entries
        .iter()
        .find(|entry| hit(entry, false))
        .or_else(|| entries.iter().find(|entry| hit(entry, true)))
}

/// Read a document, in the order it was written.
///
/// # Errors
///
/// [`Error`] when the grammar refuses the text, or when a structure rule the
/// grammar cannot state is broken.
///
/// # Panics
///
/// Never in practice: `points_file` is the rule that was matched, so the
/// parse holds exactly one pair of it. A panic here would mean pest returned
/// success with nothing in it.
pub fn parse(source: &str) -> Result<Vec<Entry>, Error> {
    let file = PointsParser::parse(Rule::points_file, source)
        .map_err(|err| Error::Syntax(err.to_string()))?
        .next()
        .expect("points_file matched, so there is one");
    let mut entries: Vec<Entry> = Vec::new();
    for model in file.into_inner() {
        if model.as_rule() != Rule::model {
            continue; // EOI
        }
        entries.push(entry(model)?);
    }
    let mut keys = std::collections::BTreeSet::new();
    // A map rather than a set, because the message must name the spelling
    // that WON as well as the one that lost -- see `Error::DuplicateMatch`.
    let mut matches = std::collections::BTreeMap::new();
    for found in &entries {
        if !keys.insert(found.key.clone()) {
            return Err(Error::DuplicateKey(found.key.clone()));
        }
        // Folded, because `serves` folds. Compared on the folded spelling and
        // REPORTED in the ones the file was written in, so the message names
        // bytes a reader can search for.
        if let Some(first) =
            matches.insert(found.matches.to_ascii_lowercase(), found.matches.clone())
        {
            return Err(Error::DuplicateMatch {
                first,
                again: found.matches.clone(),
            });
        }
    }

    // AND NOW SHADOWING, which duplication is only the equal case of. Two
    // entries whose matches are one a proper substring of the other are not
    // duplicates and are not independent: every id the narrower is about, the
    // wider matches too. `serves` gives it to whichever was written first,
    // which is an operating point decided by line order.
    //
    // Refused unless the WIDER one is marked `fallback = true`, which takes it
    // out of pass one entirely. Enabled 2026-09-13 with the dogma's respell,
    // and the two had to land together: the dogma's own `qwen3` shadowed
    // `qwen3.6` and `qwen3.5` unmarked, so this refusal would have refused the
    // dogma. That is not a coincidence to note and move past -- it is the
    // measurement. The rule was already being broken by the one file it exists
    // to protect, and the comment explaining the ordering was what stood in
    // for it.
    //
    // Folded on both sides, because `serves` folds. Quadratic, over a document
    // whose entries number four.
    for wider in &entries {
        if wider.fallback {
            continue;
        }
        let wide = wider.matches.to_ascii_lowercase();
        let shadowed = entries.iter().find(|narrower| {
            let narrow = narrower.matches.to_ascii_lowercase();
            narrow.len() > wide.len() && narrow.contains(&wide)
        });
        if let Some(narrower) = shadowed {
            return Err(Error::ShadowedByUnmarked {
                wider: wider.matches.clone(),
                narrower: narrower.matches.clone(),
            });
        }
    }
    Ok(entries)
}

fn entry(model: Pair<'_, Rule>) -> Result<Entry, Error> {
    let mut key = String::new();
    let mut matches: Option<String> = None;
    let mut nothink: Option<Vec<NoThinkOp>> = None;
    let mut thinking_kwarg = None;
    let mut preserve_thinking = None;
    let mut fallback = false;
    let mut sampler = Vec::new();
    let mut gate = Vec::new();
    let mut receipts = Vec::new();

    for part in model.into_inner() {
        match part.as_rule() {
            Rule::model_header => key = named(&part),
            Rule::match_pair => matches = Some(unquote(&text_of(&part, Rule::string))),
            Rule::nothink_pair => {
                nothink = Some(
                    part.into_inner()
                        .filter(|inner| inner.as_rule() == Rule::nothink_array)
                        .flat_map(pest::iterators::Pair::into_inner)
                        .filter(|op| op.as_rule() == Rule::nothink_op)
                        .map(|op| {
                            NoThinkOp::from_tag(&unquote(op.as_str()))
                                .expect("the grammar admits only the closed vocabulary")
                        })
                        .collect(),
                );
            }
            Rule::thinking_kwarg_pair => thinking_kwarg = Some(flag(&part)),
            Rule::preserve_thinking_pair => preserve_thinking = Some(flag(&part)),
            Rule::fallback_pair => fallback = flag(&part),
            Rule::sampler_table => sampler = sub_table(&part, &key, "sampler")?,
            Rule::gate_table => gate = sub_table(&part, &key, "gate")?,
            Rule::receipts_table => receipts = sub_table(&part, &key, "receipts")?,
            _ => {}
        }
    }
    Ok(Entry {
        matches: matches.ok_or(Error::MissingKey {
            entry: key.clone(),
            key: "match",
        })?,
        nothink_ops: nothink.ok_or(Error::MissingKey {
            entry: key.clone(),
            key: "nothink_ops",
        })?,
        key,
        thinking_kwarg,
        preserve_thinking,
        fallback,
        sampler,
        gate,
        receipts,
    })
}

/// The `key` a header carried.
fn named(pair: &Pair<'_, Rule>) -> String {
    pair.clone()
        .into_inner()
        .find(|inner| inner.as_rule() == Rule::key)
        .map(|inner| inner.as_str().to_owned())
        .unwrap_or_default()
}

fn text_of(pair: &Pair<'_, Rule>, rule: Rule) -> String {
    pair.clone()
        .into_inner()
        .find(|inner| inner.as_rule() == rule)
        .map(|inner| inner.as_str().to_owned())
        .unwrap_or_default()
}

fn unquote(text: &str) -> String {
    text.trim_matches('"').to_owned()
}

/// The boolean a flag pair carried.
///
/// Read off the RULE the grammar matched, not off the text: `thinking_kwarg =
/// false  # true on the newer family` is a false whose line says "true", and
/// a reader that searched the text would have said the opposite of the
/// document.
fn flag(pair: &Pair<'_, Rule>) -> bool {
    pair.clone()
        .into_inner()
        .any(|inner| inner.as_rule() == Rule::boolean_true)
}

/// A sampler, gate or receipts table's pairs, once its header is confirmed to
/// name the entry it sits in.
fn sub_table(
    pair: &Pair<'_, Rule>,
    entry: &str,
    which: &'static str,
) -> Result<Vec<(String, crate::formats::record::json::Value)>, Error> {
    use crate::formats::record::json::Value as Json;
    let written = named(pair);
    if written != entry {
        return Err(Error::SubTableBelongsElsewhere {
            entry: entry.to_owned(),
            written,
            which,
        });
    }
    let mut out = Vec::new();
    for inner in pair.clone().into_inner() {
        if inner.as_rule() != Rule::plain_pair {
            continue;
        }
        let name = text_of(&inner, Rule::key);
        // The value is whatever the REGIMEN reader says it is, built by handing
        // that reader the one binding rather than by decoding the text here.
        // Two grammars are concatenated into this parser and each `derive`
        // mints its own `Rule`, so a pair from here cannot be handed to
        // regimen's value builder directly -- and writing a second value
        // decoder is how the two would come to disagree about what `0.30` is.
        // regimen's own float rule already does this to the record's decimal
        // constructor, for the same reason and with the same words.
        let binding = format!("{name} = {}", value_text(&inner));
        let projected = crate::formats::regimen::project(&binding)
            .map_err(|err| Error::Syntax(format!("`{name}` under `[{entry}.{which}]`: {err}")))?;
        let Json::Object(mut members) = projected else {
            return Err(Error::Syntax(format!(
                "`{name}` under `[{entry}.{which}]` did not project as a binding"
            )));
        };
        let value = members.remove(&name).ok_or_else(|| {
            Error::Syntax(format!(
                "`{name}` under `[{entry}.{which}]` projected under another name"
            ))
        })?;
        out.push((name, value));
    }
    Ok(out)
}

/// A pair's value, as written: everything after the first `=`, with any
/// trailing comment and whitespace taken off.
fn value_text(pair: &Pair<'_, Rule>) -> String {
    for inner in pair.clone().into_inner() {
        if matches!(
            inner.as_rule(),
            Rule::string | Rule::boolean_true | Rule::boolean_false | Rule::float | Rule::integer
        ) {
            return inner.as_str().to_owned();
        }
    }
    String::new()
}

/// Project a document into the record's value space, in file order.
///
/// # Errors
///
/// The reason, as text, when [`parse`] refuses.
pub fn project(source: &str) -> Result<crate::formats::record::json::Value, String> {
    use crate::formats::record::json::Value as Json;
    let entries = parse(source).map_err(|err| err.to_string())?;
    let mut out = Vec::new();
    for found in entries {
        let mut members = std::collections::BTreeMap::new();
        members.insert("key".to_owned(), Json::String(found.key));
        members.insert("match".to_owned(), Json::String(found.matches));
        members.insert(
            "nothink_ops".to_owned(),
            Json::Array(
                found
                    .nothink_ops
                    .iter()
                    .map(|op| Json::String(op.tag().to_owned()))
                    .collect(),
            ),
        );
        // The schema knows these two are booleans, so they are not tagged the
        // way an open table's values are: a tag exists to say what the
        // document said where the reader could not otherwise tell.
        if let Some(flag) = found.thinking_kwarg {
            members.insert("thinking_kwarg".to_owned(), Json::Boolean(flag));
        }
        if let Some(flag) = found.preserve_thinking {
            members.insert("preserve_thinking".to_owned(), Json::Boolean(flag));
        }
        // Emitted only when true, because absent and false are the same
        // thing here -- an ordinary entry -- and a projection that spelled
        // out the default would change every existing expected value to say
        // nothing new.
        if found.fallback {
            members.insert("fallback".to_owned(), Json::Boolean(true));
        }
        for (name, pairs) in [
            ("sampler", found.sampler),
            ("gate", found.gate),
            ("receipts", found.receipts),
        ] {
            if name == "receipts" && pairs.is_empty() {
                continue;
            }
            let mut table = std::collections::BTreeMap::new();
            for (field, value) in pairs {
                table.insert(field, value);
            }
            members.insert(name.to_owned(), Json::Object(table));
        }
        out.push(Json::Object(members));
    }
    Ok(Json::Array(out))
}

#[cfg(test)]
mod tests {
    use super::{Entry, Error, NoThinkOp, parse, project, serves};

    const FIXTURE: &str = include_str!("../../formats/operating_points/fixtures/valid/dogma.toml");

    /// The corpus fixture is the dogma's own file, byte for byte.
    ///
    /// Held against the text the ACCESSOR hands back, never against the file:
    /// the dogma's text is embedded once and given out only after its digest
    /// is checked, and a test that opened the file again would be the second
    /// reader that arrangement exists to prevent. So the copy in this format's
    /// corpus cannot go stale without this failing, and the check costs
    /// nothing but the pin it already runs behind.
    #[test]
    fn the_corpus_fixture_is_the_dogmas_own_file() {
        let pinned = crate::dogma::operating_points().expect("the operating points are pinned");
        assert_eq!(
            FIXTURE, pinned,
            "the fixture in this format's corpus has drifted from the dogma's file"
        );
    }

    /// The property the format exists for.
    ///
    /// The regimen reader accepts this same document and hands back
    /// `gemma, qwen3, qwen3_5, qwen3_6`, because its projection is a table and
    /// a table sorts. Matching `qwen3.6` against THAT order picks the `qwen3`
    /// entry, whose `thinking_kwarg` is false, where the entry that should win
    /// says true -- and that model's receipt says the soft switch is dead on
    /// it, so the kwarg is the only control there is. Order is not cosmetic
    /// here; it is which control gets used.
    #[test]
    fn the_entries_come_back_in_the_order_the_file_wrote_them() {
        let entries = parse(FIXTURE).expect("the dogma's operating points parse");
        let order: Vec<&str> = entries.iter().map(|entry| entry.key.as_str()).collect();
        assert_eq!(
            order,
            vec!["qwen3_6", "qwen3_5", "qwen3", "gemma"],
            "the projection lost the file's order, which is the matching rule: \
             `qwen3` is a substring of `qwen3.6` and is written after it for \
             that reason, so any order but the file's hands `qwen3.6` to the \
             wrong entry"
        );

        let sorted = {
            let mut copy = order.clone();
            copy.sort_unstable();
            copy
        };
        assert_ne!(
            order, sorted,
            "this file no longer distinguishes file order from sorted order, so it \
             has stopped being able to demonstrate the thing it is here to demonstrate"
        );
    }

    /// First match in file order, on the id the receipt was written about.
    ///
    /// Through [`serves`], which is now the only implementation of the rule.
    /// It used to be spelled out here, inline, which made this test both the
    /// assertion and the thing asserted — a consumer wanting an operating
    /// point had to write the rule a second time from the file's comment.
    #[test]
    fn the_first_match_in_file_order_is_the_one_that_serves() {
        let entries = parse(FIXTURE).expect("the dogma's operating points parse");
        let chosen = serves(&entries, "qwen3.6").expect("something matches");
        assert_eq!(chosen.key, "qwen3_6");
        assert_eq!(
            chosen.thinking_kwarg,
            Some(true),
            "the kwarg is the only working control on this family"
        );
    }

    /// A document that marks nothing, for the assertions that need one.
    ///
    /// Since the respell the dogma carries a mark, so `FIXTURE` can no longer
    /// stand for the unmarked case. This is the smallest valid document there
    /// is: one entry, so nothing can shadow anything.
    const UNMARKED: &str =
        include_str!("../../formats/operating_points/fixtures/valid/one-entry.toml");

    /// The dogma as it was written before the respell: `qwen3` unmarked, and
    /// shadowing the two entries above it. Refused by [`parse`] since
    /// 2026-09-13, which is the only thing it is here to show.
    const SHADOWING: &str =
        include_str!("../../formats/operating_points/fixtures/invalid/shadow-unmarked.toml");

    /// Every token in the tracked tree that any entry's `match` could match.
    ///
    /// NOT a list of model ids: `qwen3_6.sampler` is a TOML header fragment
    /// and `Gemma` is prose. That is deliberate. The domain this invariance
    /// has to hold over is every string the rule could be handed, and the
    /// ones nobody meant as an id are the ones a hand-written list omits.
    ///
    /// Re-derive with, from the repository root:
    ///
    /// ```text
    /// python3 - <<'EOF'
    /// import re, subprocess, pathlib
    /// matches = ["qwen3.6", "qwen3.5", "qwen3", "gemma"]
    /// tok = re.compile(r"[A-Za-z0-9._-]+")
    /// SELF = "diet/src/formats/operating_points.rs"
    /// found = set()
    /// for name in subprocess.run(["git","ls-files","-z"],
    ///                            capture_output=True).stdout.split(b"\0"):
    ///     if not name: continue
    ///     rel = name.decode()
    ///     if rel == SELF: continue
    ///     path = pathlib.Path(rel)
    ///     if not path.is_file(): continue
    ///     for t in tok.findall(path.read_text(encoding="utf-8", errors="replace")):
    ///         if any(m in t.lower() for m in matches): found.add(t)
    /// print(len(found)); [print(t) for t in sorted(found)]
    /// EOF
    /// ```
    ///
    /// Nineteen, and re-derived on 2026-09-12 to check that it still is.
    ///
    /// THIS FILE IS EXCLUDED, and the exclusion is the correction rather than
    /// a convenience. Without it the script yields TWENTY: the extra token is
    /// `qwen3.4`, a probe id in this module's own tests, swept in because it
    /// contains `qwen3`. A harvest that reads the file stating the harvest is
    /// measuring itself, and the number it produces moves whenever a test
    /// below gains a probe — so the claim "nineteen, re-derive with this" was
    /// false as written within a day of being written.
    ///
    /// It is the third time this exact shape has cost something here: the
    /// `Qwen3.6-27B` frequency was first reported as 317 for "the tree" when
    /// the corpus figure is 316, the other two sweeping in the doc comment and
    /// the test making the claim. A count over a tree that contains the count
    /// needs to say what it excludes, every time.
    ///
    /// An earlier count of thirty-six was reported on #65 from a looser
    /// tokenisation that never got written down; the sixteen-that-serve figure
    /// is the same under both, which is what that measurement was actually
    /// about. This definition is the one that is reproducible, so it is the
    /// one pinned.
    const HARVESTED: &[&str] = &[
        "Gemma",
        "Qwen3",
        "Qwen3.6-27B",
        "gemma",
        "gemma.gate",
        "gemma.receipts",
        "gemma.sampler",
        "qwen3",
        "qwen3.5",
        "qwen3.6",
        "qwen3.gate",
        "qwen3.sampler",
        "qwen3_5",
        "qwen3_5.gate",
        "qwen3_5.sampler",
        "qwen3_6",
        "qwen3_6.gate",
        "qwen3_6.receipts",
        "qwen3_6.sampler",
    ];

    fn key_of<'a>(entries: &'a [Entry], served: &str) -> Option<&'a str> {
        serves(entries, served).map(|entry| entry.key.as_str())
    }

    /// Every harvested name serves the entry it is pinned to here.
    ///
    /// THIS IS THE DRIFT GUARD ON THE DOGMA, and it is what the respell left
    /// behind. Until 2026-09-13 the invariance was carried by holding the
    /// dogma against a candidate file proposing the `fallback` mark, and
    /// showing the two served identically. The candidate has since become the
    /// dogma; a test comparing the dogma to itself proves nothing, and the
    /// retired spelling it was compared against is now REFUSED by `parse`, so
    /// it cannot be the other half of a comparison either. It lives on as
    /// `fixtures/invalid/shadow-unmarked.toml`, which is a better job for it.
    ///
    /// So the reference stopped being a second file and became this table.
    /// That is the stronger shape anyway: two files can drift together and a
    /// comparison between them still passes, where a written-down expectation
    /// can only be changed by somebody editing it and saying why.
    ///
    /// Ruled 2026-09-13, with the respell: *"The invariance test stays as the
    /// drift guard for every future dogma edit."* An edit that changes which
    /// entry any of the nineteen names reaches fails here, and the diff that
    /// fixes this table is the place the change gets read.
    #[test]
    fn every_harvested_name_serves_the_entry_it_is_pinned_to() {
        // Nineteen names, and the entry each one reaches. `None` appears
        // nowhere: a harvested name serving nothing is what the fold fixed.
        const PINNED: &[(&str, &str)] = &[
            ("Gemma", "gemma"),
            ("Qwen3", "qwen3"),
            ("Qwen3.6-27B", "qwen3_6"),
            ("gemma", "gemma"),
            ("gemma.gate", "gemma"),
            ("gemma.receipts", "gemma"),
            ("gemma.sampler", "gemma"),
            ("qwen3", "qwen3"),
            ("qwen3.5", "qwen3_5"),
            ("qwen3.6", "qwen3_6"),
            ("qwen3.gate", "qwen3"),
            ("qwen3.sampler", "qwen3"),
            // UNDERSCORE, so these reach the fallback and not the entry they
            // are named after: `qwen3_5` does not contain `qwen3.5`. They are
            // TOML header fragments rather than model ids, as `HARVESTED`
            // says, and `qwen3` is the honest answer for a token that is not
            // a version at all. Written down because it looks like a bug and
            // is not; a table that quietly said `qwen3_5` here would be a
            // pin nobody could tell from a typo.
            ("qwen3_5", "qwen3"),
            ("qwen3_5.gate", "qwen3"),
            ("qwen3_5.sampler", "qwen3"),
            ("qwen3_6", "qwen3"),
            ("qwen3_6.gate", "qwen3"),
            ("qwen3_6.receipts", "qwen3"),
            ("qwen3_6.sampler", "qwen3"),
        ];

        let dogma = parse(FIXTURE).expect("the dogma's operating points parse");
        let pinned: Vec<&str> = PINNED.iter().map(|(name, _)| *name).collect();
        let harvested: Vec<&str> = HARVESTED.to_vec();
        assert_eq!(
            pinned, harvested,
            "the pinned table and the harvest have come apart; a name in one and \
             not the other is a name whose serving nothing is checking"
        );

        for (name, want) in PINNED {
            assert_eq!(
                key_of(&dogma, name),
                Some(*want),
                "`{name}` serves a different entry than the dogma is pinned to; if \
                 that is the edit you meant, this table is where you say so"
            );
        }
    }

    /// The fold is a BUG FIX, and this is what stops it being filed as a
    /// spelling change.
    ///
    /// Ruled 2026-09-12: *"This is not serving-invariant and must not be
    /// recorded as such."* Every other test in this module asserts that
    /// something did NOT move; this one asserts that three things DID, names
    /// them, and fails if the count drifts in either direction.
    ///
    /// The direction matters as much as the count. Every difference must be
    /// `None` becoming `Some` — a name that served nothing now served by the
    /// entry its lowercase twin already reached. A name that served one entry
    /// and now serves a DIFFERENT one would be a rerouting, which is a change
    /// to what a measurement applies to and is not what was ruled.
    ///
    /// That rerouting arm is NOT reachable with today's data — every `match`
    /// in the dogma is already lowercase, so folding can only add reach, never
    /// move it. It is written for the document that carries a mixed-case
    /// `match` one day, and it is declared here rather than left to look like
    /// something this run exercised.
    #[test]
    fn the_fold_is_a_bug_fix_and_not_an_invariance() {
        let dogma = parse(FIXTURE).expect("the dogma's operating points parse");
        let unfolded = |served: &str| {
            dogma
                .iter()
                .find(|entry| !entry.fallback && served.contains(entry.matches.as_str()))
                .or_else(|| {
                    dogma
                        .iter()
                        .find(|entry| entry.fallback && served.contains(entry.matches.as_str()))
                })
                .map(|entry| entry.key.as_str())
        };

        let mut gained: Vec<(&str, &str)> = Vec::new();
        for name in HARVESTED {
            let (was, now) = (unfolded(name), key_of(&dogma, name));
            if was == now {
                continue;
            }
            assert_eq!(
                was, None,
                "`{name}` served {was:?} before the fold and {now:?} after: that is a \
                 REROUTING, not a name the fold reached, and it changes what a \
                 transcribed measurement applies to"
            );
            gained.push((name, now.expect("a difference from None is a Some")));
        }

        assert_eq!(
            gained,
            vec![
                ("Gemma", "gemma"),
                ("Qwen3", "qwen3"),
                ("Qwen3.6-27B", "qwen3_6"),
            ],
            "the fold reaches exactly three of the nineteen harvested names, and \
             `Qwen3.6-27B` is the one that matters: 316 occurrences in the mined \
             provenance record, the most frequent model id in the corpus, \
             previously served by nothing at all"
        );
    }

    /// The dogma says the same thing whatever order it is written in.
    ///
    /// This is what the respell bought, and it is asserted on the dogma itself
    /// now rather than on a candidate: since 2026-09-13 the marked file IS the
    /// dogma.
    ///
    /// THE CONTROL IS THE SECOND HALF, and without it this test would pass on
    /// any document that never had the dependence. It takes the dogma, strips
    /// the mark, and shows the answers move — `qwen3.6` stops reaching
    /// `qwen3_6` and lands on `qwen3`, whose `thinking_kwarg = false` is the
    /// control that does not work on that family. That is what the file did
    /// until the respell, and it is what a reader has to be able to see to
    /// know the mark is doing something.
    ///
    /// The stripped document is built by unmarking the parsed entries rather
    /// than by parsing an unmarked one, because `parse` now refuses an
    /// unmarked document that shadows. The refusal and this control are the
    /// same fact from two directions.
    #[test]
    fn the_dogma_does_not_depend_on_where_the_lines_sit() {
        let dogma = parse(FIXTURE).expect("the dogma's operating points parse");
        let reversed: Vec<Entry> = dogma.iter().rev().cloned().collect();
        for name in HARVESTED {
            assert_eq!(
                key_of(&dogma, name),
                key_of(&reversed, name),
                "`{name}` serves a different entry when the dogma is reversed, \
                 which is the dependence the fallback mark exists to remove"
            );
        }

        let unmarked: Vec<Entry> = dogma
            .iter()
            .cloned()
            .map(|mut entry| {
                entry.fallback = false;
                entry
            })
            .collect();
        let unmarked_reversed: Vec<Entry> = unmarked.iter().rev().cloned().collect();
        let moved: Vec<&&str> = HARVESTED
            .iter()
            .filter(|name| key_of(&unmarked, name) != key_of(&unmarked_reversed, name))
            .collect();
        assert!(
            !moved.is_empty(),
            "stripping the mark and reversing changed nothing, so this test is \
             passing on a document that never had the ordering dependence and \
             proves nothing about removing it"
        );
        assert_eq!(
            key_of(&unmarked_reversed, "qwen3.6"),
            Some("qwen3"),
            "unmarked and reversed, `qwen3.6` should land on the entry that \
             shadows it — the one whose receipt says the soft switch is dead and \
             the kwarg is the only control there is"
        );
    }

    /// A document with no fallback is served by the rule it always had.
    ///
    /// The second pass must be inert, not merely harmless: if `serves` ever
    /// reordered or re-ranked an unmarked document, every consumer of the
    /// dogma would silently change which measurement it read.
    ///
    /// The subject used to be the dogma, which marked nothing. Since the
    /// respell it marks `qwen3`, so the subject is the dogma with the mark
    /// STRIPPED — the same entries, none of them marked. Stripped rather than
    /// re-parsed from an unmarked file, because `parse` now refuses one that
    /// shadows, and stripping keeps the four real entries rather than shrinking
    /// the test to a document small enough to be legal.
    #[test]
    fn a_document_that_marks_nothing_is_matched_exactly_as_before() {
        let dogma: Vec<Entry> = parse(FIXTURE)
            .expect("the dogma's operating points parse")
            .into_iter()
            .map(|mut entry| {
                entry.fallback = false;
                entry
            })
            .collect();
        assert!(
            dogma.iter().all(|entry| !entry.fallback),
            "the subject of this test is a document that marks nothing, and this one              does"
        );
        for name in HARVESTED {
            // The single-pass rule, spelled out. It FOLDS, because the
            // subject here is whether the second pass is inert -- not whether
            // the fold happens, which is the test below.
            let folded = name.to_ascii_lowercase();
            let one_pass = dogma
                .iter()
                .find(|entry| folded.contains(&entry.matches.to_ascii_lowercase()))
                .map(|entry| entry.key.as_str());
            assert_eq!(key_of(&dogma, name), one_pass, "`{name}`");
        }
    }

    /// A marked entry is not consulted while any unmarked one still can be.
    ///
    /// Built here rather than taken from a fixture, so the assertion does not
    /// depend on the candidate keeping its shape: two entries, the marked one
    /// written FIRST, so file order alone would hand it the id.
    #[test]
    fn a_marked_entry_loses_to_an_unmarked_one_written_below_it() {
        let document = "[wide]\nmatch = \"qwen3\"\nfallback = true\nnothink_ops = []\n                        [wide.sampler]\nt = 1\n[wide.gate]\nv = 1\n                        [narrow]\nmatch = \"qwen3.6\"\nnothink_ops = []\n                        [narrow.sampler]\nt = 1\n[narrow.gate]\nv = 1\n";
        let entries = parse(document).expect("it parses");
        assert_eq!(key_of(&entries, "qwen3.6"), Some("narrow"));
        assert_eq!(
            key_of(&entries, "qwen3.4"),
            Some("wide"),
            "nothing unmarked matches, so the marked entry is what is left"
        );
        assert_eq!(key_of(&entries, "gemma"), None);
    }

    /// The retired spelling is refused, and refused for the shadowing.
    ///
    /// The conformance corpus already holds that
    /// `fixtures/invalid/shadow-unmarked.toml` is rejected, which is the
    /// gate. It does not hold WHICH refusal rejected it, and every one of the
    /// seven other invalid fixtures would satisfy "rejected" just as well. A
    /// document refused for the wrong reason is a fixture that has stopped
    /// pointing at the rule it was written for -- so the variant is named
    /// here, and both spellings with it, because the message sends a reader to
    /// two literals and they have to be the ones in the file.
    #[test]
    fn the_retired_spelling_is_refused_for_shadowing_and_not_something_else() {
        let refused = parse(SHADOWING).expect_err("the retired spelling is refused");
        assert_eq!(
            refused,
            Error::ShadowedByUnmarked {
                wider: "qwen3".to_owned(),
                narrower: "qwen3.6".to_owned(),
            },
            "the retired dogma is refused, but not for the missing mark"
        );
    }

    /// `fallback` is projected only when it is true.
    #[test]
    fn the_projection_carries_a_mark_and_is_silent_without_one() {
        let shown = format!("{:?}", project(FIXTURE).expect("it projects"));
        assert!(shown.contains("fallback"), "{shown}");
        let plain = format!("{:?}", project(UNMARKED).expect("it projects"));
        assert!(
            !plain.contains("fallback"),
            "an unmarked document projected the default, which would rewrite              every expected value on record to say nothing new"
        );
    }

    #[test]
    fn every_operation_round_trips_through_its_spelling() {
        for op in NoThinkOp::ALL {
            assert_eq!(NoThinkOp::from_tag(op.tag()), Some(*op));
        }
        assert_eq!(NoThinkOp::from_tag("guessing"), None);
    }

    #[test]
    fn a_sub_table_under_another_entry_is_refused_by_name() {
        let document =
            "[a]\nmatch = \"a\"\nnothink_ops = []\n[b.sampler]\nt = 1\n[a.gate]\nv = 1\n";
        assert!(matches!(
            parse(document),
            Err(Error::SubTableBelongsElsewhere { .. })
        ));
    }

    #[test]
    fn the_projection_is_an_array_because_an_object_would_sort() {
        let projected = project(FIXTURE).expect("it projects");
        assert!(matches!(
            projected,
            crate::formats::record::json::Value::Array(_)
        ));
    }
}
