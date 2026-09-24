//! Reading a regimen into the regime a record declares.
//!
//! Lifted out of `bin/drive.rs` when a second program needed it. The regimen
//! format and the record's are different formats with different readers, and
//! this is the crossing between them -- so it is one function, in one place,
//! rather than a copy per binary. A second reader of the same rule is the
//! defect class this repository keeps finding in itself.

use crate::formats::record::json::Value;
use crate::formats::record::{CacheTtl, Count, Engine, Reasoning, Regime, Substrate, Weights};
use crate::formats::regimen::{self, Regimen};

/// The keys this program reads for the regime facts a regimen v1 has no place
/// for, named after the record's own field paths.
///
/// `substrate_model` and `substrate_quantization` used to be here and are not
/// any more. Item 3 made a substrate's identity TYPED -- a sha256 of the
/// weights, or a named hosted model with a version -- and a model's prose name
/// beside a quantization label is neither. Reading them and calling the result
/// an identity is the one thing the ruling excludes, so this program stopped
/// reading them rather than keep two keys whose only use was to be misread.
pub const SUBSTRATE_KEYS: &[&str] = &["substrate_reasoning", "substrate_hardware"];

/// The key a regimen declares a provider path's cache lifetime under.
///
/// OPTIONAL, unlike [`SUBSTRATE_KEYS`], and the difference is the point.
/// Those four are facts the record REQUIRES, so a regimen missing one is
/// refused rather than defaulted. A cache lifetime is not required: a
/// substrate nobody has documented a lifetime for has none to declare, and
/// inventing thirty minutes for it would make every miss under it read as a
/// provider expiry -- which is the estimate #79 replaces, restated as a
/// default. Undeclared reaches the census as `ttl_undeclared`, naming the
/// substrate.
///
/// Two spellings, and no others: a whole number of SECONDS, or the word
/// `"none"` for a path that caches nothing. The word is the regimen's own
/// spelling for a declared absence, matching `budget_tokens = "none"` one
/// table over.
pub const CACHE_TTL_KEY: &str = "substrate_cache_ttl";

/// The regime `regimen` declares, or the list of what it is missing.
///
/// # Errors
///
/// Returns the complaint as a sentence when the regimen omits a key the
/// regime requires, when a value is the wrong shape, or when an endpoint was
/// given -- regimen v1 cannot say which weights sit behind one, and this
/// crossing will not invent an identity it was not told.
pub fn regime_of(regimen: &Regimen, endpoint_given: bool) -> Result<Regime, String> {
    // WHAT ACTUALLY SERVED decides this, not what the regimen says served.
    // A regimen naming a substrate is a declaration; a run against the canned
    // server is a fact, and the identity written into the record has to be the
    // second. This is the same argument as `substrate` coming from the caller
    // in `client::journal::project`, one level up.
    if endpoint_given {
        return Err(
            "an endpoint was given, and regimen v1 cannot say WHICH weights are behind it. \
             Item 3 made a substrate's identity typed -- `weights` is a sha256 of the weights \
             or a hosted `provider`/`model_id`/`version_or_date_observed` -- and this program \
             will not invent either from a prose model name. The registry that resolves a \
             substrate id to those facts lands with the equipment registry; until it does, \
             `diet-drive` runs against its own canned server, whose identity it can compute"
                .to_owned(),
        );
    }

    let mut missing = Vec::new();
    let mut text = |key: &'static str| match regimen.get(key) {
        Some(regimen::Value::String(value)) if !value.is_empty() => value.clone(),
        _ => {
            missing.push(key);
            String::new()
        }
    };

    let arm = text("arm");
    let id = text("substrate");
    let reasoning_written = text(SUBSTRATE_KEYS[0]);
    let hardware_fingerprint = text(SUBSTRATE_KEYS[1]);

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

    // ONE substrate, because this program serves one. A regime may now carry
    // several -- a drive whose interview fork runs on a small local model while
    // the main lane runs a large one is the arrangement this repository exists
    // to measure -- and `diet-drive` makes every call to the same server, so a
    // second entry here would be a substrate nothing was put to.
    Ok(Regime {
        arm,
        substrates: vec![Substrate {
            id,
            // The canned server IS this program, and what it plays is the acts.
            // Naming the same artifact twice is the truth about it; giving the
            // engine a version of its own would be inventing a second fact to
            // fill a second field.
            engine: Engine {
                name: "diet-drive canned".to_owned(),
                version_or_digest: crate::drive::canned::acts_digest(),
            },
            // Computed, not declared, and its OWN kind rather than a digest
            // wearing `Digest`'s name. A canned server serves no weights, and
            // saying so with the variant is what lets gate 1 compare a replay
            // exactly instead of re-firing it within a band meant for sampling
            // and hardware that cannot move here. Ruled 2026-09-11.
            weights: Weights::Canned {
                acts_sha256: crate::drive::canned::acts_digest(),
            },
            hardware_fingerprint,
            sampler_card: sampler,
            reasoning,
            // READ BY THE REGIMEN FORMAT'S OWN READER, not by a second
            // reading of the same table here. `regimen::parse` already
            // refused a half-written pair before this function was called,
            // so the only outcomes left are a whole declaration and none at
            // all -- and `expect` would be a panic on a document this
            // crossing was handed, so the refusal is relayed.
            reasoning_control: regimen::reasoning_of(regimen)
                .map_err(|why| format!("its `[reasoning]` table: {why}"))?,
            // NOT DECLARABLE HERE, and that is a fact about this program
            // rather than a gap in the schema. A chat template's digest
            // comes out of a weights file's header; the canned server has no
            // weights file and renders no template, so there is nothing for
            // this substrate to disclose and inventing a digest-shaped
            // string would be the sentinel class #68 caught in
            // `hardware_fingerprint`. A regimen key for it belongs with the
            // equipment registry, beside the weights it would describe.
            chat_template_sha256: None,
            cache_ttl: cache_ttl_of(regimen)?,
        }],
        dogma_version,
    })
}

/// The regimen's word for a path that caches nothing.
///
/// The same word `budget_tokens = "none"` uses one table over, because it is
/// the same kind of statement: a declared absence, told apart from nobody
/// having said anything by the key being there at all.
const NO_CACHE: &str = "none";

/// The cache lifetime `regimen` declares, if it declares one.
///
/// # Errors
///
/// Returns the complaint when the key is bound to something that is neither a
/// count of seconds nor the word `"none"`. A key bound to nonsense is a
/// different finding from a key nobody wrote, and defaulting the first to the
/// second would make a typo read as a deliberate silence.
fn cache_ttl_of(regimen: &Regimen) -> Result<Option<CacheTtl>, String> {
    let complaint = |found: &str| {
        format!(
            "`{CACHE_TTL_KEY} = {found}` is neither a whole number of seconds nor \
             `\"none\"`. A cache lifetime is a property of the PROVIDER PATH, and this \
             program will not guess one: leave the key out and the census names the \
             substrate as undeclared, which is true, rather than reading its misses as \
             a provider expiry"
        )
    };
    match regimen.get(CACHE_TTL_KEY) {
        None => Ok(None),
        // Through `Count`, like every other number crossing into the record:
        // a lifetime no record can spell is not a lifetime this regimen may
        // declare, and a negative one is not a lifetime at all.
        Some(regimen::Value::Integer(seconds)) => {
            match u64::try_from(*seconds)
                .ok()
                .and_then(|n| Count::new(n).ok())
            {
                Some(count) => Ok(Some(CacheTtl::Seconds(count))),
                None => Err(complaint(&seconds.to_string())),
            }
        }
        Some(regimen::Value::String(word)) if word == NO_CACHE => Ok(Some(CacheTtl::Uncached)),
        Some(other) => Err(complaint(&format!("{other:?}"))),
    }
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

#[cfg(test)]
mod tests {
    use super::regime_of;
    use crate::formats::record::{Budget, Count, ReasoningControl};
    use crate::formats::regimen;

    /// The smallest regimen this crossing accepts, plus whatever `extra` the
    /// test is about.
    fn crossing(extra: &str) -> Result<crate::formats::record::Regime, String> {
        let text = format!(
            "arm = \"a\"\ndogma_version = 0\nsubstrate = \"canned\"\n\
             substrate_reasoning = \"off\"\n\
             substrate_hardware = \"{}\"\n{extra}\n[sampler]\nseed = 7\n",
            "a".repeat(64)
        );
        let parsed = regimen::parse(&text).expect("the document is a regimen");
        regime_of(&parsed, false)
    }

    /// #94 design point 1: the reasoning control crosses from the regimen
    /// into the regime, as the pair the regimen declared.
    ///
    /// Through the FORMAT's reader, which is why the half-written pair never
    /// reaches here: `regimen::parse` refused it first. A crossing that read
    /// the table itself would be a second reader of the same rule, which is
    /// the defect class this file's own header is about.
    #[test]
    fn a_drives_regime_carries_the_reasoning_control_the_regimen_declared() {
        let declared = crossing("[reasoning]\neffort = \"high\"\nbudget_tokens = 4096\n")
            .expect("a whole pair is a regime");
        assert_eq!(
            declared.substrates[0].reasoning_control,
            Some(ReasoningControl {
                effort: "high".to_owned(),
                budget_tokens: Budget::Tokens(Count::new(4096).expect("4096 is a count")),
            })
        );

        // And a regimen that declares none crosses as none, rather than as a
        // level this program picked. Every committed regimen is one of these.
        let silent = crossing("").expect("a regimen without one is still a regime");
        assert_eq!(silent.substrates[0].reasoning_control, None);

        // The chat template is not declarable here, and the substrate says
        // so rather than carrying a digest-shaped string nothing computed.
        assert_eq!(silent.substrates[0].chat_template_sha256, None);
    }
}
