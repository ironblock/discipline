//! A paired run (#912): one scenario under two regimens that differ in one
//! lever, `N` repeats each, interleaved ABBA ABBA so drift on the box falls
//! on both arms. Its repeats are `scenario --repeat`'s (#910): each a fresh
//! session in its own `repeat-NN/`, a void one re-run at its slot, the stop
//! file read before each, one line per repeat.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::drive::scenario::{Repeats, RunOptions, Slot, script_digest, slot};
use crate::formats::log::{Run, RunArm};
use crate::formats::regimen::{self, Regimen, Value};

/// The one lever two regimens differ in, and its state under each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lever {
    /// Its name, as `session.start`'s `levers` names it.
    pub name: String,
    /// Its state under `--a`.
    pub a: String,
    /// Its state under `--b`.
    pub b: String,
}

/// Why two regimens are no pair: every lever they differ in, with its
/// state under each, and every key they differ in, dotted into its table,
/// with those that move no lever named apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoPair {
    /// The levers that differ, by name: (a's state, b's state).
    pub levers: BTreeMap<String, (String, String)>,
    /// The keys that differ.
    pub keys: Vec<String>,
    /// Of those, the keys whose change moves no lever.
    pub naming_no_lever: Vec<String>,
}

impl NoPair {
    /// The refusal, in words.
    #[must_use]
    pub fn why(&self) -> String {
        let levers: Vec<_> = self.levers.keys().cloned().collect();
        let mut why = format!(
            "a pair differs in exactly one lever; these regimens differ in {} ({}), by the keys {}",
            levers.len(),
            if levers.is_empty() {
                "none".to_owned()
            } else {
                levers.join(", ")
            },
            if self.keys.is_empty() {
                "none".to_owned()
            } else {
                self.keys.join(", ")
            },
        );
        if !self.naming_no_lever.is_empty() {
            why.push_str("; ");
            why.push_str(&self.naming_no_lever.join(", "));
            why.push_str(" moves no lever, and counts as a difference");
        }
        why
    }
}

/// Each lever's state a session under `regimen` starts at, as serve names
/// it on `session.start`.
fn levers_of(regimen: &Regimen) -> BTreeMap<String, String> {
    let substrate = match regimen.get("substrate") {
        Some(Value::String(id)) => id.clone(),
        _ => String::new(),
    };
    crate::drive::regimen::start_levers(
        regimen,
        crate::drive::regimen::output_cap(None, Some(regimen)),
        &substrate,
    )
}

/// The keys `a` and `b` bind differently under `prefix`, dotted into their
/// tables.
fn differing(prefix: &str, a: Option<&Value>, b: Option<&Value>, into: &mut Vec<String>) {
    match (a, b) {
        (Some(Value::Table(a)), Some(Value::Table(b))) => {
            let keys: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
            for key in keys {
                differing(&format!("{prefix}.{key}"), a.get(key), b.get(key), into);
            }
        }
        (a, b) if a != b => into.push(prefix.to_owned()),
        _ => {}
    }
}

/// The one lever `a` and `b` differ in (#912), as `session.start` names
/// the levers. A key whose change moves no lever counts as a difference:
/// it is tried alone, `b` with only that key as `a` binds it.
///
/// # Errors
///
/// [`NoPair`] when they differ in no lever, in more than one, or in a key
/// that moves none.
pub fn one_lever(a: &Regimen, b: &Regimen) -> Result<Lever, NoPair> {
    let (under_a, under_b) = (levers_of(a), levers_of(b));
    let levers: BTreeMap<String, (String, String)> = under_a
        .keys()
        .chain(under_b.keys())
        .filter(|name| under_a.get(*name) != under_b.get(*name))
        .map(|name| {
            let state =
                |levers: &BTreeMap<String, String>| levers.get(name).cloned().unwrap_or_default();
            (name.clone(), (state(&under_a), state(&under_b)))
        })
        .collect();
    let raw = |regimen: &Regimen| -> BTreeMap<String, Value> {
        regimen
            .iter()
            .map(|(key, value)| (key.to_owned(), value.clone()))
            .collect()
    };
    let (raw_a, raw_b) = (raw(a), raw(b));
    let mut keys = Vec::new();
    let mut naming_no_lever = Vec::new();
    let top: BTreeSet<&String> = raw_a.keys().chain(raw_b.keys()).collect();
    for key in top {
        let mut under = Vec::new();
        differing(key, raw_a.get(key), raw_b.get(key), &mut under);
        if under.is_empty() {
            continue;
        }
        // `b` with this key alone as `a` binds it: a key that moves no
        // lever leaves `b`'s levers as they were.
        let moves = b
            .with(key, raw_a.get(key).cloned())
            .is_ok_and(|tried| levers_of(&tried) != under_b);
        if !moves {
            naming_no_lever.extend(under.iter().cloned());
        }
        keys.extend(under);
    }
    if levers.len() == 1
        && naming_no_lever.is_empty()
        && let Some((name, (a, b))) = levers.first_key_value()
    {
        return Ok(Lever {
            name: name.clone(),
            a: a.clone(),
            b: b.clone(),
        });
    }
    Err(NoPair {
        levers,
        keys,
        naming_no_lever,
    })
}

/// A pair's digest (#912): the sha256 of its two regimens' sha256s, `a`'s
/// then `b`'s, as lowercase hex with nothing between.
#[must_use]
pub fn pair_digest(a_sha256: &str, b_sha256: &str) -> String {
    crate::digest::sha256_hex(format!("{a_sha256}{b_sha256}").as_bytes())
}

/// Which arm the `index`th run of a pair runs, from 0: ABBA ABBA.
#[must_use]
pub fn arm_at(index: u64) -> RunArm {
    match index % 4 {
        0 | 3 => RunArm::A,
        _ => RunArm::B,
    }
}

/// Run `options.file` under `a` and under `b`, `repeats.count` times each
/// (#912), interleaved ABBA ABBA, each arm's repeats in `--out/a/` and
/// `--out/b/`, and each run's `run` naming its arm, the pair, the script's
/// digest and its repeat of its arm. Refused before any run unless the two
/// regimens differ in exactly one lever. `emit` is given one line per run.
/// The batch's line, and whether it ran to its end.
pub fn pair(
    drive: &Path,
    options: &RunOptions,
    (a, b): (&Path, &Path),
    repeats: &Repeats,
    emit: &mut dyn FnMut(&serde_json::Value),
) -> (serde_json::Value, bool) {
    let mut said = serde_json::json!({
        "batch": options.file.display().to_string(),
        "a": a.display().to_string(),
        "b": b.display().to_string(),
        "of": repeats.count,
        "ok": false,
    });
    let read = |path: &Path| -> Result<(Regimen, String), String> {
        let text = std::fs::read_to_string(path)
            .map_err(|why| format!("{} cannot be read: {why}", path.display()))?;
        let regimen = regimen::parse(&text)
            .map_err(|why| format!("{} is not a regimen: {why:?}", path.display()))?;
        Ok((regimen, crate::digest::sha256_hex(text.as_bytes())))
    };
    let read = script_digest(&options.file).and_then(|script| {
        let (a_read, a_sha256) = read(a)?;
        let (b_read, b_sha256) = read(b)?;
        Ok((script, a_read, a_sha256, b_read, b_sha256))
    });
    let (script_sha256, a_read, a_sha256, b_read, b_sha256) = match read {
        Ok(read) => read,
        Err(why) => {
            said["failure"] = serde_json::Value::String(why);
            return (said, false);
        }
    };
    let lever = match one_lever(&a_read, &b_read) {
        Ok(lever) => lever,
        Err(no) => {
            said["refused"] = serde_json::Value::String(no.why());
            said["levers"] = serde_json::json!(
                no.levers
                    .iter()
                    .map(|(name, (a, b))| (name.clone(), serde_json::json!({"a": a, "b": b})))
                    .collect::<BTreeMap<_, _>>()
            );
            said["keys"] = serde_json::json!(no.keys);
            said["keys_naming_no_lever"] = serde_json::json!(no.naming_no_lever);
            return (said, false);
        }
    };
    let digest = pair_digest(&a_sha256, &b_sha256);
    said["script_sha256"] = serde_json::Value::String(script_sha256.clone());
    said["pair"] = serde_json::Value::String(digest.clone());
    said["lever"] = serde_json::json!({"name": lever.name, "a": lever.a, "b": lever.b});
    if let Err(why) = std::fs::create_dir_all(&options.out) {
        said["failure"] =
            serde_json::Value::String(format!("{} cannot be made: {why}", options.out.display()));
        return (said, false);
    }
    let stop_file = repeats
        .stop_file
        .clone()
        .unwrap_or_else(|| options.out.join("STOP"));
    let mut ran: BTreeMap<&str, u64> = BTreeMap::from([("a", 0), ("b", 0)]);
    let mut ok = true;
    for index in 0..repeats.count * 2 {
        let arm = arm_at(index);
        let (regimen, tag) = match arm {
            RunArm::A => (a, "a"),
            RunArm::B => (b, "b"),
        };
        let run = Run {
            script_sha256: script_sha256.clone(),
            repeat: ran[tag] + 1,
            of: repeats.count,
            paired: Some((arm, digest.clone())),
        };
        let arm_options = RunOptions {
            out: options.out.join(tag),
            regimen: Some(regimen.to_path_buf()),
            ..options.clone()
        };
        match slot(
            drive,
            &arm_options,
            &run,
            (&stop_file, repeats.void_cap),
            emit,
        ) {
            Slot::Ran => *ran.entry(tag).or_default() += 1,
            Slot::Stopped => {
                said["stopped_at"] = serde_json::json!({"arm": tag, "repeat": run.repeat});
                said["stopped_by"] = serde_json::Value::String(stop_file.display().to_string());
                break;
            }
            Slot::Voided(voids) => {
                said["void_cap_reached"] =
                    serde_json::json!({"arm": tag, "repeat": run.repeat, "voids": voids});
                ok = false;
                break;
            }
        }
    }
    said["ran"] = serde_json::json!(ran);
    said["ok"] = serde_json::Value::Bool(ok);
    (said, ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regimen(text: &str) -> Regimen {
        regimen::parse(text).expect("a regimen")
    }

    const BASE: &str = "arm = \"pair\"\ndogma_version = 0\nsubstrate = \"canned-scenario\"\n";

    /// #912: two regimens one lever apart are a pair; the lever is named
    /// with its state under each.
    #[test]
    fn regimens_one_lever_apart_are_a_pair() {
        let a = regimen(&format!("{BASE}refutation_interview = \"off\"\n"));
        let b = regimen(&format!("{BASE}refutation_interview = \"on\"\n"));
        let lever = one_lever(&a, &b).expect("one lever");
        assert_eq!(lever.name, "refutation_interview");
        assert_eq!(lever.a, "off");
        assert!(lever.b.starts_with("on:"), "{lever:?}");
    }

    /// #912: two levers apart is refused, naming both levers and both keys;
    /// the same regimen twice is refused, naming none.
    #[test]
    fn regimens_two_levers_apart_or_none_are_refused_naming_them() {
        let a = regimen(BASE);
        let b = regimen(&format!(
            "{BASE}refutation_interview = \"on\"\nnote_dedupe = \"on\"\n"
        ));
        let no = one_lever(&a, &b).expect_err("two levers");
        assert_eq!(
            no.levers.keys().collect::<Vec<_>>(),
            ["note_dedupe", "refutation_interview"]
        );
        assert_eq!(no.keys, ["note_dedupe", "refutation_interview"]);
        assert!(no.naming_no_lever.is_empty(), "{no:?}");
        let why = no.why();
        assert!(
            why.contains("note_dedupe") && why.contains("refutation_interview"),
            "{why}"
        );
        let same = one_lever(&a, &a).expect_err("no lever");
        assert!(same.levers.is_empty() && same.keys.is_empty(), "{same:?}");
    }

    /// #912: a key that moves no lever counts as a difference, beside the
    /// one lever that does, and is named, dotted into its table.
    #[test]
    fn a_key_that_moves_no_lever_counts_as_a_difference() {
        let a = regimen(&format!(
            "{BASE}refutation_interview = \"off\"\n[sampler]\ntemperature = 0.6\n"
        ));
        let b = regimen(&format!(
            "{BASE}refutation_interview = \"on\"\n[sampler]\ntemperature = 0.7\n"
        ));
        let no = one_lever(&a, &b).expect_err("a key naming no lever");
        assert_eq!(
            no.levers.keys().collect::<Vec<_>>(),
            ["refutation_interview"]
        );
        assert_eq!(no.naming_no_lever, ["sampler.temperature"]);
        assert!(no.why().contains("sampler.temperature"), "{}", no.why());
    }

    /// #912: ABBA ABBA.
    #[test]
    fn the_arms_interleave_abba() {
        let order: String = (0..8)
            .map(|index| match arm_at(index) {
                RunArm::A => 'A',
                RunArm::B => 'B',
            })
            .collect();
        assert_eq!(order, "ABBAABBA");
    }
}
