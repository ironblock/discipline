//! Reading a regimen into the regime a record declares.
//!
//! Lifted out of `bin/drive.rs` when a second program needed it. The regimen
//! format and the record's are different formats with different readers, and
//! this is the crossing between them -- so it is one function, in one place,
//! rather than a copy per binary. A second reader of the same rule is the
//! defect class this repository keeps finding in itself.

use crate::formats::record::json::Value;
use crate::formats::record::{Reasoning, Regime, Substrate};
use crate::formats::regimen::{self, Regimen};

/// The regimen keys that describe what served, beyond its name.
///
/// Named here rather than at each use so that a key added to the regime has
/// one place to be added, and the program that reports a regimen's shape and
/// the program that reads it cannot disagree about what the shape is.
pub const SUBSTRATE_KEYS: &[&str] = &[
    "substrate_model",
    "substrate_quantization",
    "substrate_reasoning",
    "substrate_hardware",
];

/// The regime `regimen` declares, or the list of what it is missing.
///
/// # Errors
///
/// A sentence naming every key the regimen does not carry, or the one value
/// it carries that is not a value of its type. Never a default: a regime tag
/// a program invented makes every result under it incomparable with every
/// other, which is what the tags are for.
pub fn regime_of(regimen: &Regimen) -> Result<Regime, String> {
    let mut missing = Vec::new();
    let mut text = |key: &'static str| match regimen.get(key) {
        Some(regimen::Value::String(value)) if !value.is_empty() => value.clone(),
        _ => {
            missing.push(key);
            String::new()
        }
    };

    let arm = text("arm");
    let name = text("substrate");
    let model = text(SUBSTRATE_KEYS[0]);
    let quantization = text(SUBSTRATE_KEYS[1]);
    let reasoning_written = text(SUBSTRATE_KEYS[2]);
    let hardware = text(SUBSTRATE_KEYS[3]);

    let Some(regimen::Value::Integer(dogma_version)) = regimen.get("dogma_version") else {
        missing.push("dogma_version");
        return Err(complaint(&missing));
    };
    if !missing.is_empty() {
        return Err(complaint(&missing));
    }

    let Some(reasoning) = Reasoning::ALL
        .iter()
        .copied()
        .find(|state| state.tag() == reasoning_written)
    else {
        return Err(format!(
            "`substrate_reasoning = \"{reasoning_written}\"` is not one of {}",
            Reasoning::ALL
                .iter()
                .map(|state| state.tag())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    };

    let sampler = match regimen.get("sampler") {
        Some(regimen::Value::Table(table)) if !table.is_empty() => table
            .iter()
            .map(|(key, value)| (key.clone(), sampled(value)))
            .collect(),
        _ => {
            return Err(
                "`[sampler]` is required and must not be empty: the record refuses a blank \
                 `substrate.sampler`, because \"nobody wrote the settings down\" and \"the \
                 settings were these\" are different facts about a run"
                    .to_owned(),
            );
        }
    };

    let Ok(dogma_version) = u32::try_from(*dogma_version) else {
        return Err(format!(
            "`dogma_version = {dogma_version}` is not a version"
        ));
    };

    Ok(Regime {
        arm,
        substrate: Substrate {
            name,
            model,
            quantization,
            sampler,
            reasoning,
            hardware,
        },
        dogma_version,
    })
}

/// What is missing, named, with why it is not defaulted.
fn complaint(missing: &[&str]) -> String {
    format!(
        "the regimen does not carry {}. A regime tag this program invented would make every \
         result under it incomparable with every other, which is what the tags are for",
        missing.join(", ")
    )
}

/// One sampler setting, in the record's value space.
///
/// The regimen's value space and the record's are different formats with
/// different readers; this is the crossing, and it is one function so there
/// is one place a new kind has to be decided.
fn sampled(value: &regimen::Value) -> Value {
    // Every variant, and no fallback. The first version had a `{other:?}`
    // arm and a regimen's `temperature = 0.6` reached the record as the
    // string `Float(Decimal("0.6"))` -- a regime tag that is a Rust debug
    // rendering, which compares equal to nothing and is what the exact
    // decimal exists to prevent. A crossing with a default arm is a crossing
    // that loses whatever nobody thought of.
    match value {
        regimen::Value::String(text) => Value::String(text.clone()),
        regimen::Value::Integer(number) => Value::Integer(*number),
        regimen::Value::Float(decimal) => Value::Decimal(decimal.clone()),
        regimen::Value::Boolean(flag) => Value::Boolean(*flag),
        regimen::Value::Array(items) => Value::Array(items.iter().map(sampled).collect()),
        regimen::Value::Table(table) => Value::Object(
            table
                .iter()
                .map(|(key, value)| (key.clone(), sampled(value)))
                .collect(),
        ),
    }
}
