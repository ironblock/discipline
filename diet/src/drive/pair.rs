//! A paired run (#912): one scenario under two regimens that differ in one
//! lever, `N` repeats each, interleaved ABBA ABBA so drift on the box falls
//! on both arms. Its repeats are `scenario --repeat`'s (#910): each a fresh
//! session in its own `repeat-NN/`, a void one re-run at its slot, the stop
//! file read before each, one line per repeat.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::drive::scenario::{Repeats, RunOptions, Runs, Slot, script_digest, slot};
use crate::formats::log::Run;
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
/// with those that move no lever named apart; under a declared contrast,
/// the differences it left out and the members that name none.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NoPair {
    /// The levers that differ, by name: (a's state, b's state).
    pub levers: BTreeMap<String, (String, String)>,
    /// The keys that differ.
    pub keys: Vec<String>,
    /// Of those, the keys whose change moves no lever.
    pub naming_no_lever: Vec<String>,
    /// The differences a declared contrast does not name.
    pub missing: Vec<String>,
    /// The members of a declared contrast that name no difference.
    pub extra: Vec<String>,
}

impl NoPair {
    /// The refusal, in words.
    #[must_use]
    pub fn why(&self) -> String {
        let listed = |items: &[String]| {
            if items.is_empty() {
                "none".to_owned()
            } else {
                items.join(", ")
            }
        };
        let levers: Vec<_> = self.levers.keys().cloned().collect();
        let mut why = if self.missing.is_empty() && self.extra.is_empty() {
            format!(
                "a pair differs in exactly one lever, or in the contrast it declares; these \
                 regimens differ in {} ({}), by the keys {}",
                levers.len(),
                listed(&levers),
                listed(&self.keys),
            )
        } else {
            format!(
                "the declared contrast is not what these regimens differ in: it leaves out {} \
                 and names {} that differs in nothing; they differ in the levers {}, by the keys {}",
                listed(&self.missing),
                listed(&self.extra),
                listed(&levers),
                listed(&self.keys),
            )
        };
        if !self.naming_no_lever.is_empty() {
            why.push_str("; ");
            why.push_str(&self.naming_no_lever.join(", "));
            why.push_str(" moves no lever, and counts as a difference");
        }
        why
    }
}

/// The one key a pair may differ in freely: the regimen's `arm`, a label
/// no request carries (`a_regimens_arm_reaches_no_request`).
pub const LABEL: &str = "arm";

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

/// What two regimens differ in: the levers, the keys, the keys that move
/// no lever, and the levers each differing key moves when tried alone.
struct Differences {
    levers: BTreeMap<String, (String, String)>,
    keys: Vec<String>,
    naming_no_lever: Vec<String>,
    moved_by: BTreeMap<String, BTreeSet<String>>,
}

impl Differences {
    fn of(a: &Regimen, b: &Regimen) -> Self {
        let (under_a, under_b) = (levers_of(a), levers_of(b));
        let changed = |left: &BTreeMap<String, String>, right: &BTreeMap<String, String>| {
            left.keys()
                .chain(right.keys())
                .filter(|name| left.get(*name) != right.get(*name))
                .cloned()
                .collect::<BTreeSet<String>>()
        };
        let levers = changed(&under_a, &under_b)
            .into_iter()
            .map(|name| {
                let state = |levers: &BTreeMap<String, String>| {
                    levers.get(&name).cloned().unwrap_or_default()
                };
                let states = (state(&under_a), state(&under_b));
                (name, states)
            })
            .collect();
        let raw = |regimen: &Regimen| -> BTreeMap<String, Value> {
            regimen
                .iter()
                .map(|(key, value)| (key.to_owned(), value.clone()))
                .collect()
        };
        let (raw_a, raw_b) = (raw(a), raw(b));
        let mut found = Self {
            levers,
            keys: Vec::new(),
            naming_no_lever: Vec::new(),
            moved_by: BTreeMap::new(),
        };
        let top: BTreeSet<&String> = raw_a.keys().chain(raw_b.keys()).collect();
        for key in top.into_iter().filter(|key| key.as_str() != LABEL) {
            let mut under = Vec::new();
            differing(key, raw_a.get(key), raw_b.get(key), &mut under);
            if under.is_empty() {
                continue;
            }
            // `b` with this key alone as `a` binds it: a key that moves no
            // lever leaves `b`'s levers as they were.
            let moved = b
                .with(key, raw_a.get(key).cloned())
                .map(|tried| changed(&levers_of(&tried), &under_b))
                .unwrap_or_default();
            if moved.is_empty() {
                found.naming_no_lever.extend(under.iter().cloned());
            }
            for dotted in &under {
                found.moved_by.insert(dotted.clone(), moved.clone());
            }
            found.keys.extend(under);
        }
        found
    }

    fn refused(self) -> NoPair {
        NoPair {
            levers: self.levers,
            keys: self.keys,
            naming_no_lever: self.naming_no_lever,
            ..NoPair::default()
        }
    }
}

/// The one lever `a` and `b` differ in (#912), as `session.start` names
/// the levers. A key whose change moves no lever counts as a difference:
/// it is tried alone, `b` with only that key as `a` binds it. Only
/// [`LABEL`] may differ besides.
///
/// # Errors
///
/// [`NoPair`] when they differ in no lever, in more than one, or in a key
/// that moves none.
pub fn one_lever(a: &Regimen, b: &Regimen) -> Result<Lever, NoPair> {
    let found = Differences::of(a, b);
    if found.levers.len() == 1
        && found.naming_no_lever.is_empty()
        && let Some((name, (a, b))) = found.levers.first_key_value()
    {
        return Ok(Lever {
            name: name.clone(),
            a: a.clone(),
            b: b.clone(),
        });
    }
    Err(found.refused())
}

/// The levers `a` and `b` differ in when they are `declared` to differ in
/// exactly those (#912, for #932's bundle): every difference named, and
/// every member naming one. A lever is named by its own name or by a key
/// that moves it; a key that moves no lever, by its dotted name. Only
/// [`LABEL`] may differ besides.
///
/// # Errors
///
/// [`NoPair`] naming what the declaration leaves out and what it names
/// that differs in nothing.
pub fn contrast_of(
    a: &Regimen,
    b: &Regimen,
    declared: &[String],
) -> Result<BTreeMap<String, (String, String)>, NoPair> {
    let found = Differences::of(a, b);
    let names = |member: &String, lever: &String| {
        member == lever
            || found
                .moved_by
                .get(member)
                .is_some_and(|moved| moved.contains(lever))
    };
    let mut missing: Vec<String> = found
        .levers
        .keys()
        .filter(|lever| !declared.iter().any(|member| names(member, lever)))
        .cloned()
        .collect();
    missing.extend(
        found
            .naming_no_lever
            .iter()
            .filter(|key| !declared.contains(key))
            .cloned(),
    );
    let extra: Vec<String> = declared
        .iter()
        .filter(|member| {
            !found.levers.keys().any(|lever| names(member, lever))
                && !found.naming_no_lever.contains(member)
        })
        .cloned()
        .collect();
    if missing.is_empty() && extra.is_empty() && !found.levers.is_empty() {
        return Ok(found.levers);
    }
    Err(NoPair {
        missing,
        extra,
        ..found.refused()
    })
}

/// A batch's digest (#912, #932): the sha256 of every arm's regimen
/// sha256, in arm-name order, as lowercase hex with nothing between. A
/// pair's arms are `a` and `b`, so it is `a`'s then `b`'s.
#[must_use]
pub fn batch_digest(regimen_sha256s: &[&str]) -> String {
    crate::digest::sha256_hex(regimen_sha256s.concat().as_bytes())
}

/// A pair's digest: [`batch_digest`] of `a`'s and `b`'s.
#[must_use]
pub fn pair_digest(a_sha256: &str, b_sha256: &str) -> String {
    batch_digest(&[a_sha256, b_sha256])
}

/// The rows of a Williams design for `k` arms (#932), each a block's order
/// of the arms by index. Over a whole cycle of rows, every arm sits in
/// every position equally often and follows every other arm once, so a
/// linear drift and a first-order carryover from the run before (the
/// server's cache) balance across arms. `k` rows when `k` is even, and
/// `2k` when it is odd, each row then mirrored. For two arms the rows are
/// AB and BA: ABBA ABBA.
#[must_use]
pub fn williams(k: usize) -> Vec<Vec<usize>> {
    let first: Vec<usize> = (0..k)
        .map(|j| match j {
            0 => 0,
            j if j % 2 == 1 => j.div_ceil(2),
            j => k - j / 2,
        })
        .collect();
    let mut rows: Vec<Vec<usize>> = (0..k)
        .map(|shift| first.iter().map(|arm| (arm + shift) % k).collect())
        .collect();
    if k % 2 == 1 {
        let mirrored: Vec<Vec<usize>> = rows
            .iter()
            .map(|row| row.iter().rev().copied().collect())
            .collect();
        rows.extend(mirrored);
    }
    rows
}

/// One arm of a batch (#932): its name, its regimen, and the contrast it
/// declares against the baseline, when it declares one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arm {
    /// Its name: a directory its runs are kept in.
    pub name: String,
    /// Its regimen.
    pub regimen: std::path::PathBuf,
    /// The levers and keys it differs from the baseline in, when declared;
    /// else exactly one.
    pub contrast: Option<Vec<String>>,
}

/// A batch's arms, in name order, and its baseline, from an arms manifest
/// (#932): `baseline = "NAME"`, then one `[arm.NAME]` table each, holding
/// `regimen` (relative to the manifest) and, optionally, `contrast`.
///
/// # Errors
///
/// When it does not read, names an arm badly, names a baseline that is no
/// arm, gives the baseline a contrast, holds fewer than two arms, or holds
/// a key it does not take.
pub fn manifest(path: &Path) -> Result<(Vec<Arm>, String, Option<String>), String> {
    let text = std::fs::read_to_string(path)
        .map_err(|why| format!("{} cannot be read: {why}", path.display()))?;
    let read = regimen::parse(&text)
        .map_err(|why| format!("{} is not an arms manifest: {why:?}", path.display()))?;
    let wrong = |why: String| format!("{}: {why}", path.display());
    for (key, _) in read.iter() {
        if key != "baseline" && key != "arm" && key != "purpose" {
            return Err(wrong(format!(
                "an arms manifest takes `baseline` and `[arm.NAME]`, not `{key}`"
            )));
        }
    }
    let Some(Value::String(baseline)) = read.get("baseline") else {
        return Err(wrong("names no `baseline`".to_owned()));
    };
    let Some(Value::Table(tables)) = read.get("arm") else {
        return Err(wrong("declares no `[arm.NAME]`".to_owned()));
    };
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut arms = Vec::new();
    for (name, table) in tables {
        if !crate::formats::log::is_an_arm(name) {
            return Err(wrong(format!(
                "`{name}` is no arm's name: lowercase letters, digits, `-` and `_`"
            )));
        }
        let Value::Table(table) = table else {
            return Err(wrong(format!("`arm.{name}` is not a table")));
        };
        if let Some(key) = table
            .keys()
            .find(|key| *key != "regimen" && *key != "contrast")
        {
            return Err(wrong(format!(
                "`[arm.{name}]` takes `regimen` and `contrast`, not `{key}`"
            )));
        }
        let Some(Value::String(regimen)) = table.get("regimen") else {
            return Err(wrong(format!("`[arm.{name}]` names no `regimen`")));
        };
        let contrast = match table.get("contrast") {
            None => None,
            Some(Value::Array(members)) => Some(
                members
                    .iter()
                    .map(|member| match member {
                        Value::String(member) if !member.is_empty() => Ok(member.clone()),
                        _ => Err(wrong(format!(
                            "`[arm.{name}]`'s `contrast` is a list of names"
                        ))),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            Some(_) => {
                return Err(wrong(format!(
                    "`[arm.{name}]`'s `contrast` is a list of names"
                )));
            }
        };
        if name == baseline && contrast.is_some() {
            return Err(wrong(format!(
                "the baseline `{name}` is what the others contrast with: it declares no contrast"
            )));
        }
        arms.push(Arm {
            name: name.clone(),
            regimen: dir.join(regimen),
            contrast,
        });
    }
    if arms.len() < 2 {
        return Err(wrong(format!(
            "holds {} arm(s): a batch compares two or more",
            arms.len()
        )));
    }
    if !arms.iter().any(|arm| arm.name == *baseline) {
        return Err(wrong(format!(
            "its baseline `{baseline}` is none of its arms"
        )));
    }
    // What the batch is for, when it says (#942): a `smoke` test of the
    // plumbing is not the headline, and its line says so.
    let purpose = match read.get("purpose") {
        None => None,
        Some(Value::String(purpose)) if !purpose.is_empty() => Some(purpose.clone()),
        Some(_) => return Err(wrong("`purpose` is text, such as `smoke`".to_owned())),
    };
    Ok((arms, baseline.clone(), purpose))
}

/// Levers by name, each with its state under `a` and under `b`.
fn states(levers: &BTreeMap<String, (String, String)>) -> serde_json::Value {
    serde_json::json!(
        levers
            .iter()
            .map(|(name, (a, b))| (name.clone(), serde_json::json!({"a": a, "b": b})))
            .collect::<BTreeMap<_, _>>()
    )
}

/// What the batch's line names of the pair once it is checked -- its
/// `lever`, or its declared `contrast` -- or, refused, the refusal's keys.
fn checked(
    a: &Regimen,
    b: &Regimen,
    contrast: Option<&[String]>,
) -> Result<serde_json::Value, serde_json::Value> {
    let checked = match contrast {
        None => one_lever(a, b).map(
            |lever| serde_json::json!({"lever": {"name": lever.name, "a": lever.a, "b": lever.b}}),
        ),
        Some(declared) => contrast_of(a, b, declared).map(|levers| {
            serde_json::json!({"contrast": {"declared": declared, "levers": states(&levers)}})
        }),
    };
    checked.map_err(|no| {
        let mut refused = serde_json::json!({
            "refused": no.why(),
            "levers": states(&no.levers),
            "keys": no.keys,
            "keys_naming_no_lever": no.naming_no_lever,
        });
        if contrast.is_some() {
            refused["missing"] = serde_json::json!(no.missing);
            refused["extra"] = serde_json::json!(no.extra);
        }
        refused
    })
}

/// A regimen read, with its bytes' digest.
fn regimen_read(path: &Path) -> Result<(Regimen, String), String> {
    let text = std::fs::read_to_string(path)
        .map_err(|why| format!("{} cannot be read: {why}", path.display()))?;
    let regimen = regimen::parse(&text)
        .map_err(|why| format!("{} is not a regimen: {why:?}", path.display()))?;
    Ok((regimen, crate::digest::sha256_hex(text.as_bytes())))
}

/// Run `options.file` under each of `arms`' regimens, `repeats.count` times
/// each (#932), in a Williams order ([`williams`]), each arm's repeats in
/// `--out/<arm>/`, each run's `run` naming its arm, the batch's digest
/// ([`batch_digest`]), the script's digest and its repeat of its arm.
/// Refused before any run unless every other arm differs from `baseline` in
/// exactly one lever, or in exactly its declared contrast. `emit` is given
/// one line per run; the batch's line is also kept as `--out/batch.json`.
/// The batch's line, and whether it ran to its end.
#[allow(clippy::too_many_lines)]
pub fn arms(
    drive: &Path,
    options: &RunOptions,
    (arms, baseline, purpose): (&[Arm], &str, Option<&str>),
    repeats: &Repeats,
    emit: &mut dyn FnMut(&serde_json::Value),
) -> (serde_json::Value, bool) {
    let mut said = serde_json::json!({
        "batch": options.file.display().to_string(),
        "baseline": baseline,
        "purpose": purpose,
        "regimens": arms
            .iter()
            .map(|arm| (arm.name.clone(), arm.regimen.display().to_string()))
            .collect::<BTreeMap<_, _>>(),
        "of": repeats.count,
        "ok": false,
    });
    let mut arms: Vec<Arm> = arms.to_vec();
    arms.sort_by(|left, right| left.name.cmp(&right.name));
    let read = script_digest(&options.file).and_then(|script| {
        arms.iter()
            .map(|arm| regimen_read(&arm.regimen))
            .collect::<Result<Vec<_>, _>>()
            .map(|reads| (script, reads))
    });
    let (script_sha256, reads) = match read {
        Ok(read) => read,
        Err(why) => {
            said["failure"] = serde_json::Value::String(why);
            return (said, false);
        }
    };
    let Some(base) = arms.iter().position(|arm| arm.name == baseline) else {
        said["failure"] =
            serde_json::Value::String(format!("the baseline `{baseline}` is none of the arms"));
        return (said, false);
    };
    let mut against = serde_json::Map::new();
    for (at, arm) in arms.iter().enumerate().filter(|(at, _)| *at != base) {
        match checked(&reads[base].0, &reads[at].0, arm.contrast.as_deref()) {
            Ok(named) => {
                against.insert(arm.name.clone(), named);
            }
            Err(refused) => {
                said["arm"] = serde_json::Value::String(arm.name.clone());
                if let serde_json::Value::Object(refused) = refused {
                    for (key, value) in refused {
                        said[key] = value;
                    }
                }
                return (said, false);
            }
        }
    }
    let digest = batch_digest(
        &reads
            .iter()
            .map(|(_, sha)| sha.as_str())
            .collect::<Vec<_>>(),
    );
    said["script_sha256"] = serde_json::Value::String(script_sha256.clone());
    said["arms"] = serde_json::Value::String(digest.clone());
    said["against"] = serde_json::Value::Object(against);
    let rows = williams(arms.len());
    let balanced = repeats.count.is_multiple_of(rows.len() as u64);
    said["order"] = serde_json::json!({
        "design": "williams",
        "rows": rows.len(),
        "blocks": repeats.count,
        "balanced": balanced,
        "is": if balanced {
            "every arm in every position equally often, after every other arm once per cycle".to_owned()
        } else {
            format!(
                "not balanced: {} block(s) of a {}-row cycle; positions and carryover balance only at a multiple of {}",
                repeats.count,
                rows.len(),
                rows.len()
            )
        },
    });
    if let Err(why) = std::fs::create_dir_all(&options.out) {
        said["failure"] =
            serde_json::Value::String(format!("{} cannot be made: {why}", options.out.display()));
        return (said, false);
    }
    let stop_file = repeats
        .stop_file
        .clone()
        .unwrap_or_else(|| options.out.join("STOP"));
    let mut runs = match Runs::create(&options.out) {
        Ok(runs) => runs,
        Err(why) => {
            said["failure"] = serde_json::Value::String(why);
            return (said, false);
        }
    };
    // The sandbox probes the batch's runs passed (#942), each once.
    let mut probes: Vec<serde_json::Value> = Vec::new();
    let mut kept = |line: &serde_json::Value| {
        if let Some(probe) = line.get("sandbox_probe").filter(|probe| !probe.is_null()) {
            let mut probe = probe.clone();
            if let Some(probe) = probe.as_object_mut() {
                probe.remove("reused");
            }
            if !probes.contains(&probe) {
                probes.push(probe);
            }
        }
        runs.keep(line);
        emit(line);
    };
    let mut ran: BTreeMap<String, u64> = arms.iter().map(|arm| (arm.name.clone(), 0)).collect();
    let mut ok = true;
    'blocks: for block in 0..repeats.count {
        let row = &rows[usize::try_from(block).unwrap_or(0) % rows.len()];
        for &at in row {
            let arm = &arms[at];
            let run = Run {
                script_sha256: script_sha256.clone(),
                repeat: ran[&arm.name] + 1,
                of: repeats.count,
                paired: Some((arm.name.clone(), digest.clone())),
            };
            let arm_options = RunOptions {
                out: options.out.join(&arm.name),
                regimen: Some(arm.regimen.clone()),
                ..options.clone()
            };
            match slot(
                drive,
                &arm_options,
                &run,
                (&stop_file, repeats.void_cap),
                &mut kept,
            ) {
                Slot::Ran => *ran.entry(arm.name.clone()).or_default() += 1,
                Slot::Stopped => {
                    said["stopped_at"] = serde_json::json!({"arm": arm.name, "repeat": run.repeat});
                    said["stopped_by"] = serde_json::Value::String(stop_file.display().to_string());
                    break 'blocks;
                }
                Slot::Voided(voids) => {
                    said["void_cap_reached"] =
                        serde_json::json!({"arm": arm.name, "repeat": run.repeat, "voids": voids});
                    ok = false;
                    break 'blocks;
                }
            }
        }
    }
    if !probes.is_empty() {
        said["sandbox_probes"] = serde_json::Value::Array(probes);
    }
    let ok = runs.report(&mut said) && ok;
    said["ran"] = serde_json::json!(ran);
    said["ok"] = serde_json::Value::Bool(ok);
    let kept_as = options.out.join(BATCH);
    if let Err(why) = std::fs::write(&kept_as, format!("{said:#}\n")) {
        said["batch_failed"] =
            serde_json::Value::String(format!("{} cannot be written: {why}", kept_as.display()));
        said["ok"] = serde_json::Value::Bool(false);
        return (said, false);
    }
    (said, ok)
}

/// The file a batch keeps its line in, under its `--out` (#932): what
/// `table` reads the baseline from.
pub const BATCH: &str = "batch.json";

/// Run `options.file` under `a` and under `b`, `repeats.count` times each
/// (#912): [`arms`] with two arms, `a` the baseline, so ABBA ABBA. Refused
/// before any run unless the two regimens differ in exactly one lever, or,
/// with `contrast` declared, in exactly the levers and keys it names. The
/// batch's line names the pair as `pair` and its lever or contrast at the
/// top, as before.
pub fn pair(
    drive: &Path,
    options: &RunOptions,
    (a, b, contrast): (&Path, &Path, Option<&[String]>),
    repeats: &Repeats,
    emit: &mut dyn FnMut(&serde_json::Value),
) -> (serde_json::Value, bool) {
    let two = [
        Arm {
            name: "a".to_owned(),
            regimen: a.to_path_buf(),
            contrast: None,
        },
        Arm {
            name: "b".to_owned(),
            regimen: b.to_path_buf(),
            contrast: contrast.map(<[String]>::to_vec),
        },
    ];
    let (mut said, ok) = arms(drive, options, (&two, "a", None), repeats, emit);
    said["a"] = serde_json::Value::String(a.display().to_string());
    said["b"] = serde_json::Value::String(b.display().to_string());
    if let Some(digest) = said.get("arms").cloned() {
        said["pair"] = digest;
    }
    if let Some(serde_json::Value::Object(named)) = said
        .get("against")
        .and_then(|against| against.get("b"))
        .cloned()
    {
        for (key, value) in named {
            said[key] = value;
        }
    }
    (said, ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A manifest's text, read from its own file under the temp directory.
    fn manifest_of(case: &str, text: &str) -> Result<(Vec<Arm>, String, Option<String>), String> {
        let dir =
            std::env::temp_dir().join(format!("diet-arms-manifest-{case}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the directory");
        let path = dir.join("ARMS.toml");
        std::fs::write(&path, text).expect("the manifest");
        let read = manifest(&path);
        let _ = std::fs::remove_dir_all(&dir);
        read
    }

    /// #932: a manifest's arms come back in name order, each regimen beside
    /// the manifest, each contrast as declared.
    #[test]
    fn a_manifest_reads_its_arms_in_name_order() {
        let (arms, baseline, purpose) = manifest_of(
            "read",
            "baseline = \"native\"\npurpose = \"smoke\"\n[arm.native]\nregimen = \"n.toml\"\n\
             [arm.limits]\nregimen = \"l.toml\"\ncontrast = [\"max_steps\"]\n",
        )
        .expect("it reads");
        assert_eq!(baseline, "native");
        assert_eq!(purpose.as_deref(), Some("smoke"));
        let names: Vec<&str> = arms.iter().map(|arm| arm.name.as_str()).collect();
        assert_eq!(names, ["limits", "native"]);
        assert!(arms[0].regimen.ends_with("l.toml"), "{arms:?}");
        assert_eq!(arms[0].contrast, Some(vec!["max_steps".to_owned()]));
        assert_eq!(arms[1].contrast, None);
    }

    /// #932: what a manifest is refused for, each refusal saying why.
    #[test]
    fn a_manifest_is_refused_naming_what_is_wrong() {
        let two = "[arm.native]\nregimen = \"n.toml\"\n[arm.limits]\nregimen = \"l.toml\"\n";
        for (case, text, says) in [
            ("no-baseline", two.to_owned(), "names no `baseline`"),
            (
                "stranger",
                format!("baseline = \"other\"\n{two}"),
                "is none of its arms",
            ),
            (
                "one-arm",
                "baseline = \"native\"\n[arm.native]\nregimen = \"n.toml\"\n".to_owned(),
                "a batch compares two or more",
            ),
            (
                "bad-name",
                format!("baseline = \"native\"\n{two}[arm.Upper]\nregimen = \"u.toml\"\n"),
                "is no arm's name",
            ),
            (
                "baseline-contrast",
                "baseline = \"native\"\n[arm.native]\nregimen = \"n.toml\"\ncontrast = [\"x\"]\n\
                 [arm.limits]\nregimen = \"l.toml\"\n"
                    .to_owned(),
                "declares no contrast",
            ),
            (
                "unknown-key",
                format!("baseline = \"native\"\nrepeat = 3\n{two}"),
                "not `repeat`",
            ),
            (
                "arm-key",
                "baseline = \"native\"\n[arm.native]\nregimen = \"n.toml\"\nseed = 1\n\
                 [arm.limits]\nregimen = \"l.toml\"\n"
                    .to_owned(),
                "not `seed`",
            ),
            (
                "no-regimen",
                "baseline = \"native\"\n[arm.native]\nregimen = \"n.toml\"\n[arm.limits]\n\
                 contrast = [\"x\"]\n"
                    .to_owned(),
                "names no `regimen`",
            ),
        ] {
            let refused = manifest_of(case, &text).expect_err(case);
            assert!(refused.contains(says), "{case}: {refused}");
        }
    }

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

    /// #912: `arm` is a label (`a_regimens_arm_reaches_no_request`), the
    /// one key a pair may differ in besides its lever.
    #[test]
    fn a_pair_may_differ_in_its_arm_alone_besides() {
        let a = regimen(&format!("{BASE}refutation_interview = \"off\"\n"));
        let b = regimen(
            &format!("{BASE}refutation_interview = \"on\"\n")
                .replace("arm = \"pair\"", "arm = \"other\""),
        );
        let lever = one_lever(&a, &b).expect("one lever, and a label");
        assert_eq!(lever.name, "refutation_interview");
        let no = one_lever(
            &a,
            &a.with("arm", Some(Value::String("x".to_owned())))
                .expect("a regimen"),
        )
        .expect_err("a label alone is no lever");
        assert!(no.keys.is_empty() && no.levers.is_empty(), "{no:?}");
    }

    /// #912: a declared contrast runs when it names exactly what differs:
    /// a lever by its name or by a key that moves it.
    #[test]
    fn a_declared_contrast_naming_exactly_what_differs_is_a_pair() {
        let a = regimen(&format!("{BASE}refutation_interview = \"on\"\n"));
        let b = regimen(&format!(
            "{BASE}meditation_carry = \"native_summary\"\nmax_output_tokens = 512\n"
        ));
        let declared = |names: &[&str]| {
            names
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>()
        };
        let levers = contrast_of(
            &a,
            &b,
            &declared(&[
                "refutation_interview",
                "meditation_carry",
                "max_output_tokens",
            ]),
        )
        .expect("the declared contrast");
        assert_eq!(
            levers.keys().collect::<Vec<_>>(),
            [
                "meditation_carry",
                "refutation_interview",
                "step_and_output_limits"
            ]
        );
        assert!(
            one_lever(&a, &b).is_err(),
            "undeclared, three levers are refused"
        );
    }

    /// #912: a declared contrast that leaves a difference out, or names one
    /// that is not there, is refused, naming it.
    #[test]
    fn a_declared_contrast_with_a_missing_or_extra_member_is_refused_naming_it() {
        let a = regimen(&format!("{BASE}refutation_interview = \"on\"\n"));
        let b = regimen(&format!("{BASE}meditation_carry = \"native_summary\"\n"));
        let missing = contrast_of(&a, &b, &["refutation_interview".to_owned()])
            .expect_err("meditation_carry left out");
        assert_eq!(missing.missing, ["meditation_carry"], "{missing:?}");
        assert!(missing.extra.is_empty(), "{missing:?}");
        assert!(
            missing.why().contains("meditation_carry"),
            "{}",
            missing.why()
        );
        let extra = contrast_of(
            &a,
            &b,
            &[
                "refutation_interview".to_owned(),
                "meditation_carry".to_owned(),
                "note_dedupe".to_owned(),
            ],
        )
        .expect_err("note_dedupe differs in nothing");
        assert_eq!(extra.extra, ["note_dedupe"], "{extra:?}");
        assert!(extra.missing.is_empty(), "{extra:?}");
        assert!(extra.why().contains("note_dedupe"), "{}", extra.why());
    }

    /// #912, #932: two arms' Williams rows are AB and BA, so ABBA ABBA; four
    /// arms' are the standard square, each arm once in each place and once
    /// after each other arm; three arms' are six rows, mirrored.
    #[test]
    fn the_williams_rows_balance_place_and_predecessor() {
        assert_eq!(williams(2), [vec![0, 1], vec![1, 0]]);
        for k in [2, 3, 4, 5, 6] {
            let rows = williams(k);
            assert_eq!(rows.len(), if k % 2 == 0 { k } else { 2 * k }, "k = {k}");
            let cycle = rows.len() / k;
            for place in 0..k {
                for arm in 0..k {
                    let times = rows.iter().filter(|row| row[place] == arm).count();
                    assert_eq!(times, cycle, "k = {k}: arm {arm} in place {place}");
                }
            }
            for before in 0..k {
                for after in (0..k).filter(|after| *after != before) {
                    let times = rows
                        .iter()
                        .filter(|row| row.windows(2).any(|pair| pair == [before, after]))
                        .count();
                    assert_eq!(times, cycle, "k = {k}: {after} after {before}");
                }
            }
        }
    }
}
