//! Write-time dedupe of similar notes: a new entry whose text is close enough
//! to a live entry of its own kind is merged into that entry, as an identical
//! one already is, instead of added beside it.
//!
//! **Why.** A T1 run's study found two thirds of its gotcha notes duplicated an
//! earlier one, almost all of them restatements: each trouble fork re-records
//! gotchas already known. Exact-text dedupe never catches a restatement.
//!
//! **What "close enough" is.** TF-IDF cosine, as the study scored it: tokens
//! of the lowercased text (a word character and then word characters, `.`,
//! `-` or `/`, ending at a word boundary; or a lone word character), sublinear
//! term frequency, smoothed IDF, each vector L2-normalised. The kind's tag is
//! not part of the text scored.
//!
//! **Online, not as the study fitted it.** The study fitted IDF on every note
//! of the finished session, then scored; at write time only the notes live so
//! far exist, so IDF here is fitted on the kind's live entries and the new
//! one, afresh at each write. The study's numbers -- precision 0.947 and
//! recall 0.667 at cosine 0.40 -- are for the offline form and are not
//! claimed for this one.
//!
//! **Evidence covers gotchas only.** The lever applies to every kind, each
//! only against its own; the study measured gotcha notes and nothing else.
//!
//! **Which entries have a kind.** An entry whose content opens with a kind's
//! canonical tag and `: `, as the lines route writes every note. An entry
//! without one -- the record tools' route writes the field apart from the
//! content -- is deduped by exact text only.
//!
//! **No harness convention.** Pi, `OpenCode` 2 and Qwen Code keep no notes to
//! dedupe, so the lever is this program's own, off by default.

use std::collections::BTreeMap;

use crate::formats::interview::FieldKind;
use crate::formats::regimen::{self, Regimen};

/// The regimen key that turns the lever on: `true` or `"on"`.
pub const NOTE_DEDUPE: &str = "note_dedupe";
/// The regimen key for the cosine a merge needs: a number above 0, at most 1.
pub const NOTE_DEDUPE_THRESHOLD: &str = "note_dedupe_threshold";
/// The threshold when the regimen names none: the study's.
pub const THRESHOLD_DEFAULT: &str = "0.40";

/// The lever as a regimen declares it: off, or on at a threshold.
///
/// # Errors
///
/// `note_dedupe` that is neither a boolean nor `on` or `off`; a threshold that
/// is not a number above 0 and at most 1; or a threshold with the lever off,
/// which would say a merge happens that never does.
pub fn declared(regimen: &Regimen) -> Result<Option<String>, String> {
    let on = match regimen.get(NOTE_DEDUPE) {
        None => false,
        Some(regimen::Value::Boolean(on)) => *on,
        Some(regimen::Value::String(word)) if word == "on" || word == "off" => word == "on",
        Some(_) => return Err(format!("`{NOTE_DEDUPE}` is `true`, `false`, `on` or `off`")),
    };
    let threshold = match regimen.get(NOTE_DEDUPE_THRESHOLD) {
        None => None,
        Some(regimen::Value::Float(decimal)) => Some(decimal.as_str().to_owned()),
        Some(regimen::Value::Integer(n)) => Some(n.to_string()),
        Some(_) => Some(String::new()),
    };
    if let Some(threshold) = &threshold
        && !threshold
            .parse::<f64>()
            .is_ok_and(|value| value > 0.0 && value <= 1.0)
    {
        return Err(format!(
            "`{NOTE_DEDUPE_THRESHOLD}` is a number above 0 and at most 1"
        ));
    }
    if !on {
        return match threshold {
            Some(_) => Err(format!(
                "`{NOTE_DEDUPE_THRESHOLD}` is set but `{NOTE_DEDUPE}` is off: no merge would \
                 happen at it"
            )),
            None => Ok(None),
        };
    }
    Ok(Some(
        threshold.unwrap_or_else(|| THRESHOLD_DEFAULT.to_owned()),
    ))
}

/// The lever's word for the session's start: `off`, or `tfidf:` and the
/// threshold, with `:default` when the regimen names none.
#[must_use]
pub fn lever(regimen: &Regimen) -> String {
    match declared(regimen) {
        Ok(None) => "off".to_owned(),
        Ok(Some(threshold)) if regimen.get(NOTE_DEDUPE_THRESHOLD).is_none() => {
            format!("tfidf:{threshold}:default")
        }
        Ok(Some(threshold)) => format!("tfidf:{threshold}"),
        // Refused at start on its own account.
        Err(_) => crate::drive::regimen::UNDECLARED.to_owned(),
    }
}

/// `content`'s kind and the text after its tag, when it opens with a kind's
/// canonical tag and `: `.
#[must_use]
pub fn kind_of(content: &str) -> Option<(&'static str, &str)> {
    let (tag, rest) = content.split_once(": ")?;
    FieldKind::ALL
        .iter()
        .map(|kind| kind.canonical_tag())
        .find(|canonical| *canonical == tag)
        .map(|canonical| (canonical, rest))
}

/// Whether `c` is a word character: alphanumeric or `_`.
fn word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `text`'s tokens, lowercased: at each word boundary that opens a word
/// character, the longest run of word characters, `.`, `-` and `/` that is at
/// least two long and ends at a word boundary; failing that, the lone word
/// character.
#[must_use]
pub fn tokens(text: &str) -> Vec<String> {
    let letters: Vec<char> = text.to_lowercase().chars().collect();
    let is_word = |at: usize| letters.get(at).copied().is_some_and(word);
    // A boundary between `at - 1` and `at`.
    let boundary = |at: usize| (at > 0 && is_word(at - 1)) != is_word(at);
    let mut out = Vec::new();
    let mut at = 0;
    while at < letters.len() {
        if !(is_word(at) && boundary(at)) {
            at += 1;
            continue;
        }
        let mut run = at + 1;
        while run < letters.len() && (word(letters[run]) || matches!(letters[run], '.' | '-' | '/'))
        {
            run += 1;
        }
        let end = (at + 2..=run)
            .rev()
            .find(|end| boundary(*end))
            .unwrap_or(at + 1);
        out.push(letters[at..end].iter().collect());
        at = end;
    }
    out
}

/// Each document's TF-IDF vector over `documents`: sublinear term frequency,
/// smoothed IDF, L2-normalised.
fn vectors(documents: &[Vec<String>]) -> Vec<BTreeMap<&str, f64>> {
    let mut frequency: BTreeMap<&str, f64> = BTreeMap::new();
    for document in documents {
        let mut seen: Vec<&str> = document.iter().map(String::as_str).collect();
        seen.sort_unstable();
        seen.dedup();
        for term in seen {
            *frequency.entry(term).or_default() += 1.0;
        }
    }
    let n = f64::from(u32::try_from(documents.len()).unwrap_or(u32::MAX));
    documents
        .iter()
        .map(|document| {
            let mut counts: BTreeMap<&str, f64> = BTreeMap::new();
            for term in document {
                *counts.entry(term.as_str()).or_default() += 1.0;
            }
            let mut vector: BTreeMap<&str, f64> = counts
                .into_iter()
                .map(|(term, count)| {
                    let idf = ((1.0 + n) / (1.0 + frequency[term])).ln() + 1.0;
                    (term, (1.0 + count.ln()) * idf)
                })
                .collect();
            let norm = vector.values().map(|v| v * v).sum::<f64>().sqrt();
            if norm > 0.0 {
                for value in vector.values_mut() {
                    *value /= norm;
                }
            }
            vector
        })
        .collect()
}

/// The cosine of every pair of `texts`, IDF fitted on `texts` alone.
#[must_use]
pub fn cosines(texts: &[&str]) -> Vec<Vec<f64>> {
    let documents: Vec<Vec<String>> = texts.iter().map(|text| tokens(text)).collect();
    let vectors = vectors(&documents);
    vectors
        .iter()
        .map(|a| {
            vectors
                .iter()
                .map(|b| {
                    a.iter()
                        // An empty sum is -0.0; plus 0.0, it is 0.
                        .filter_map(|(term, x)| b.get(term).map(|y| x * y))
                        .sum::<f64>()
                        + 0.0
                })
                .collect()
        })
        .collect()
}

/// Every entry of `held` with its cosine to `text`, IDF fitted on `held` and
/// `text`, nearest first; equals in `held`'s order.
#[must_use]
pub fn ranked<'a, T>(held: &'a [(T, &str)], text: &str) -> Vec<(&'a T, f64)> {
    let texts: Vec<&str> = held
        .iter()
        .map(|(_, held)| *held)
        .chain(std::iter::once(text))
        .collect();
    let Some(row) = cosines(&texts).pop() else {
        return Vec::new();
    };
    let mut ranked: Vec<(&T, f64)> = held.iter().map(|(id, _)| id).zip(row).collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    ranked
}

/// The entry of `held` nearest `text` at `threshold` or above, and the cosine,
/// IDF fitted on `held` and `text`; the first of equals. `None` when none
/// reaches it.
#[must_use]
pub fn nearest<'a, T>(held: &'a [(T, &str)], text: &str, threshold: f64) -> Option<(&'a T, f64)> {
    if held.is_empty() {
        return None;
    }
    let texts: Vec<&str> = held
        .iter()
        .map(|(_, held)| *held)
        .chain(std::iter::once(text))
        .collect();
    let row = cosines(&texts).pop()?;
    held.iter()
        .zip(row)
        .filter(|(_, cosine)| *cosine >= threshold)
        .fold(
            None,
            |best: Option<(&T, f64)>, ((id, _), cosine)| match best {
                Some((_, top)) if top >= cosine => best,
                _ => Some((id, cosine)),
            },
        )
}

/// `cosine` as the log writes it: four decimals.
#[must_use]
pub fn written(cosine: f64) -> String {
    format!("{cosine:.4}")
}

#[cfg(test)]
mod tests {
    use super::{cosines, declared, kind_of, lever, nearest, tokens};
    use crate::formats::regimen;

    /// The tokens scikit-learn's `TfidfVectorizer` gives with the study's
    /// token pattern, measured on 1.9.1.
    #[test]
    fn the_tokens_are_the_studys() {
        assert_eq!(
            tokens("a.b x. foo./bar_baz -q I é1 ./x v1.2.3- tests/"),
            [
                "a.b",
                "x",
                "foo./bar_baz",
                "q",
                "i",
                "é1",
                "x",
                "v1.2.3",
                "tests"
            ]
        );
    }

    /// The cosines scikit-learn gives these three, to six decimals.
    #[test]
    fn the_cosines_are_the_studys() {
        let cosines = cosines(&[
            "The pnpm lockfile is stale: run `pnpm install` before tests in packages/core.",
            "Stale pnpm lockfile in packages/core -- run pnpm install first.",
            "Port 8080 is taken by another dev server.",
        ]);
        let six = |x: f64| format!("{x:.6}");
        assert_eq!(six(cosines[0][1]), "0.702073");
        assert_eq!(six(cosines[0][2]), "0.071204");
        assert_eq!(six(cosines[1][2]), "0.000000");
        assert_eq!(six(cosines[2][2]), "1.000000");
    }

    #[test]
    fn the_nearest_is_the_highest_at_the_threshold_or_above() {
        let held = [
            ("far", "Port 8080 is taken by another dev server."),
            (
                "near",
                "The pnpm lockfile is stale: run `pnpm install` before tests in packages/core.",
            ),
        ];
        let text = "Stale pnpm lockfile in packages/core -- run pnpm install first.";
        let (id, cosine) = nearest(&held, text, 0.40).expect("one reaches it");
        assert_eq!(*id, "near");
        assert!(cosine >= 0.40);
        assert!(nearest(&held, text, 0.99).is_none());
        assert!(nearest::<&str>(&[], text, 0.0).is_none());
    }

    #[test]
    fn a_kind_is_a_canonical_tag_and_a_colon() {
        assert_eq!(kind_of("gotcha: x"), Some(("gotcha", "x")));
        assert_eq!(kind_of("plan_impact: y"), Some(("plan_impact", "y")));
        assert_eq!(kind_of("Gotcha: x"), None);
        assert_eq!(kind_of("no kind here"), None);
    }

    #[test]
    fn the_lever_is_off_unless_declared_and_refuses_a_bad_threshold() {
        let read = |text: &str| declared(&regimen::parse(text).expect("a regimen"));
        let word = |text: &str| lever(&regimen::parse(text).expect("a regimen"));
        assert_eq!(read(""), Ok(None));
        assert_eq!(word(""), "off");
        assert_eq!(read("note_dedupe = true\n"), Ok(Some("0.40".to_owned())));
        assert_eq!(word("note_dedupe = true\n"), "tfidf:0.40:default");
        assert_eq!(
            read("note_dedupe = \"on\"\nnote_dedupe_threshold = 0.55\n"),
            Ok(Some("0.55".to_owned()))
        );
        assert_eq!(
            word("note_dedupe = \"on\"\nnote_dedupe_threshold = 0.55\n"),
            "tfidf:0.55"
        );
        for bad in [
            "note_dedupe = 3\n",
            "note_dedupe = \"yes\"\n",
            "note_dedupe = true\nnote_dedupe_threshold = 0\n",
            "note_dedupe = true\nnote_dedupe_threshold = 1.5\n",
            "note_dedupe = true\nnote_dedupe_threshold = \"0.4\"\n",
            "note_dedupe_threshold = 0.5\n",
        ] {
            assert!(read(bad).is_err(), "{bad} is refused");
        }
    }
}
