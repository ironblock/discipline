//! Reading a regimen into the regime a record declares.
//!
//! Lifted out of `bin/drive.rs` when a second program needed it. The regimen
//! format and the record's are different formats with different readers, and
//! this is the crossing between them -- so it is one function, in one place,
//! rather than a copy per binary. A second reader of the same rule is the
//! defect class this repository keeps finding in itself.

use std::collections::BTreeMap;

use crate::client::shape::{Pin, SamplerCard, SamplerSetting};
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

/// The regimen key for where the interview fork runs (#570).
pub const EXTRACTION_SEAT: &str = "extraction_seat";

/// Where the interview fork runs: `warm`, on the trunk's own server, the
/// default and the only arrangement before #570; or `offboard:<registry id>`,
/// on a second server the registry resolves, never the executor's.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Seat {
    /// The trunk's server, its model, its cache.
    #[default]
    Warm,
    /// The substrate the registry resolves for this id.
    Offboard(String),
}

impl Seat {
    /// The seat `regimen` declares, read leniently: `offboard:<id>` with an
    /// id, and anything else, or nothing, `warm`.
    #[must_use]
    pub fn of(regimen: &Regimen) -> Self {
        match regimen.get(EXTRACTION_SEAT) {
            Some(regimen::Value::String(seat)) => match seat.strip_prefix("offboard:") {
                Some(id) if !id.is_empty() => Self::Offboard(id.to_owned()),
                _ => Self::Warm,
            },
            _ => Self::Warm,
        }
    }

    /// The start row's word for the seat `regimen` declares: `warm-model`
    /// (the lever's word for it before #570) or `offboard:<id>`, and for a
    /// value read as warm that is not `warm`, the value it fell back from
    /// (`warm-model:unknown:cold`).
    #[must_use]
    pub fn lever(regimen: &Regimen) -> String {
        match (Self::of(regimen), regimen.get(EXTRACTION_SEAT)) {
            (Self::Offboard(id), _) => format!("offboard:{id}"),
            (Self::Warm, None) => "warm-model".to_owned(),
            (Self::Warm, Some(regimen::Value::String(seat))) if seat == "warm" => {
                "warm-model".to_owned()
            }
            (Self::Warm, Some(regimen::Value::String(seat))) => {
                format!("warm-model:unknown:{seat}")
            }
            (Self::Warm, Some(_)) => "warm-model:unknown".to_owned(),
        }
    }
}

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
                name: crate::drive::canned::SERVES.to_owned(),
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

/// The regime `regimen` declares, its substrate resolved from the registry
/// in `registry` by the regimen's `substrate` id (#157 Q2): what a session
/// against a live endpoint declares, where [`regime_of`] would refuse one.
///
/// The regimen still says what it is the authority on -- the arm, the dogma,
/// the sampler, the reasoning state. The registry says what the substrate IS:
/// its engine, its weights and its equipment's fingerprint. The regimen's
/// own `substrate_hardware` must agree with the registry's, so the two
/// declarations of one machine cannot disagree in a record.
///
/// # Errors
///
/// Whatever [`regime_of`] refuses for the regimen itself; a substrate the
/// registry does not register, or cannot give the record's identity for
/// ([`crate::drive::registry::identity`]); and a hardware disagreement.
pub fn regime_registered(regimen: &Regimen, registry: &str) -> Result<Regime, String> {
    let mut regime = regime_of(regimen, false)?;
    for substrate in &mut regime.substrates {
        let identity = crate::drive::registry::identity(registry, &substrate.id)?;
        if substrate.hardware_fingerprint != identity.hardware_fingerprint {
            return Err(format!(
                "`substrate_hardware = \"{}\"` disagrees with the registry's \
                 `hardware_fingerprint` for `{}`'s equipment, \"{}\"",
                substrate.hardware_fingerprint, substrate.id, identity.hardware_fingerprint
            ));
        }
        substrate.engine = identity.engine;
        substrate.weights = identity.weights;
        substrate.chat_template_sha256 = identity.chat_template_sha256;
    }
    // An offboard seat is a second substrate (#570): the registry says what
    // it is, and the fork's requests -- a clone of the trunk's, its sampler
    // and reasoning state with it -- say how it was asked.
    if let Seat::Offboard(id) = Seat::of(regimen) {
        let trunk = regime.substrates[0].clone();
        if id == trunk.id {
            return Err(format!(
                "`extraction_seat = \"offboard:{id}\"` names the trunk's own substrate: \
                 a seat on the executor's server is `warm`"
            ));
        }
        let identity = crate::drive::registry::identity(registry, &id)
            .map_err(|why| format!("`extraction_seat`: {why}"))?;
        regime.substrates.push(Substrate {
            id,
            engine: identity.engine,
            weights: identity.weights,
            hardware_fingerprint: identity.hardware_fingerprint,
            chat_template_sha256: identity.chat_template_sha256,
            ..trunk
        });
    }
    Ok(regime)
}

/// The `chat_template_kwargs` a session sends on every request (R1): the
/// reasoning state the regime requests, in the template's own words.
/// `enable_thinking` is `false` under `off` and `true` under `on` or
/// `suppressed` (both requested it); `reasoning_effort` is `[reasoning]`'s
/// level, as written, since the levels are the template's (the 3.8's knows
/// `low`, `medium` and `xhigh`, and `high` is a 400). Nothing is sent for an
/// `undeclared` state.
///
/// A token budget no chat template here has a variable for is returned
/// beside them as unsent: best effort in the duty-of-care sense, recorded on
/// `session.start` and announced, never refused for being missing.
///
/// # Errors
///
/// A level with thinking `off`: the declaration contradicts itself, and that
/// is the operator's to fix.
pub fn template_kwargs(
    substrate: &Substrate,
) -> Result<(BTreeMap<String, Value>, Option<u64>), String> {
    template_kwargs_declared(substrate, None).map(|wire| (wire.kwargs, wire.unsent_budget))
}

/// What a session sends its chat template, and what it records beside it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemplateWire {
    /// The `chat_template_kwargs` every request carries.
    pub kwargs: BTreeMap<String, Value>,
    /// A token budget declared and not sent.
    pub unsent_budget: Option<u64>,
    /// With thinking on and no level sent, the level the template renders
    /// by default, as the registry declares it.
    pub reasoning_effort_default: Option<String>,
}

/// [`template_kwargs`], with what the substrate's registry entry declares
/// its model's convention needs (`identity`): `preserve_thinking`, sent on
/// every request when declared, and the template's default effort, named
/// when thinking is on and no level is sent.
///
/// # Errors
///
/// As [`template_kwargs`].
pub fn template_kwargs_declared(
    substrate: &Substrate,
    identity: Option<&crate::drive::registry::Identity>,
) -> Result<TemplateWire, String> {
    let (mut kwargs, unsent_budget) = reasoning_kwargs(substrate)?;
    if let Some(preserve) = identity.and_then(|identity| identity.template_preserve_thinking) {
        kwargs.insert("preserve_thinking".to_owned(), Value::Boolean(preserve));
    }
    let thinking_on = kwargs.get("enable_thinking") == Some(&Value::Boolean(true));
    let reasoning_effort_default = identity
        .and_then(|identity| identity.template_default_effort.clone())
        .filter(|_| thinking_on && !kwargs.contains_key("reasoning_effort"));
    Ok(TemplateWire {
        kwargs,
        unsent_budget,
        reasoning_effort_default,
    })
}

/// The regime's own reasoning state, as [`template_kwargs`] describes it.
fn reasoning_kwargs(
    substrate: &Substrate,
) -> Result<(BTreeMap<String, Value>, Option<u64>), String> {
    let mut kwargs = BTreeMap::new();
    let thinking = match substrate.reasoning {
        Reasoning::Off => Some(false),
        Reasoning::On | Reasoning::Suppressed => Some(true),
        Reasoning::Undeclared => None,
    };
    if let Some(thinking) = thinking {
        kwargs.insert("enable_thinking".to_owned(), Value::Boolean(thinking));
    }
    if let Some(control) = &substrate.reasoning_control {
        if thinking == Some(false) {
            return Err(format!(
                "`[reasoning]` names the level \"{}\" and `substrate_reasoning` is \"off\": a \
                 level instructs thinking that was not requested",
                control.effort
            ));
        }
        kwargs.insert(
            "reasoning_effort".to_owned(),
            Value::String(control.effort.clone()),
        );
    }
    let unsent =
        substrate
            .reasoning_control
            .as_ref()
            .and_then(|control| match control.budget_tokens {
                crate::formats::record::Budget::Tokens(cap) => Some(cap.get()),
                crate::formats::record::Budget::Uncapped => None,
            });
    Ok((kwargs, unsent))
}

/// The pins a request sends for `card`, a regime's `sampler_card`: one per
/// setting, each the very value the record holds (#486).
///
/// DERIVED FROM THE RECORD'S CARD, never read from the regimen a second
/// time, so the sampler a record claims and the sampler the wire carries are
/// one value and cannot disagree. A decimal crosses as the [`Decimal`] the
/// regimen's reader kept -- the digits as written, `0.6` and never an `f64`'s
/// `0.59999` -- and a whole number as itself.
///
/// [`Decimal`]: crate::formats::record::json::Decimal
///
/// # Errors
///
/// The complaint, naming the key, when a key is not a setting the client can
/// pin ([`SamplerSetting`]'s closed vocabulary, which it lists) or its value
/// is not a number. A setting the record claims and the wire cannot carry is
/// the disagreement this function exists to rule out, so it is refused rather
/// than dropped.
pub fn sampler_pins(card: &BTreeMap<String, Value>) -> Result<SamplerCard, String> {
    let accepted = || {
        SamplerSetting::ALL
            .iter()
            .map(|setting| setting.tag())
            .collect::<Vec<_>>()
            .join(", ")
    };
    card.iter()
        .try_fold(SamplerCard::empty(), |pins, (key, value)| {
            let Some(setting) = SamplerSetting::ALL
                .iter()
                .copied()
                .find(|setting| setting.tag() == key)
            else {
                return Err(format!(
                    "`[sampler]` names `{key}`, which is not a setting the request can pin and \
                 the echo can check; the record would claim a sampler the wire never \
                 carried. The settings it can pin: {}",
                    accepted()
                ));
            };
            match value {
                Value::Decimal(decimal) => Ok(pins.with(setting, Pin::Decimal(decimal.clone()))),
                Value::Integer(whole) => Ok(pins.with(setting, Pin::Integer(*whole))),
                _ => Err(format!(
                    "`[sampler] {key}` is not a number, and a pin is one: the wire would \
                 carry something other than what the record claims"
                )),
            }
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

/// The regimen's key for the output cap (#569): the `max_tokens` every
/// request carries, at the top level beside `tool_surface`.
pub const MAX_OUTPUT_TOKENS: &str = "max_output_tokens";

/// The output cap when neither `--max-output-tokens` nor the regimen names
/// one (#569), by the harness vote: Pi sends a local model's `maxTokens`,
/// 16,384 unless its definition says otherwise
/// (`pi:packages/coding-agent/src/core/provider-composer.ts:243`), and
/// `OpenCode` 2 sends none on the `OpenAI` chat protocol
/// (`oc:packages/llm/src/protocols/openai-chat.ts:361`, `generation.maxTokens`,
/// which its session runner never sets). They differ, so Qwen Code decides: a
/// `qwen3.x` model's catalogued 64K output limit, clipped to its 64,000
/// ceiling (`qc:packages/core/src/core/tokenLimits.ts:24,34,335,453-459`).
pub const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 64_000;

/// A positive integer at `key`, leniently: absent, or anything else, is
/// `None`, as an unset setting is.
fn positive(regimen: &Regimen, key: &str) -> Option<u32> {
    match regimen.get(key) {
        Some(regimen::Value::Integer(n)) if *n > 0 => Some(u32::try_from(*n).unwrap_or(u32::MAX)),
        _ => None,
    }
}

/// The output cap a session runs at, and where it came from: `flag`
/// (`--max-output-tokens`) beats `regimen` ([`MAX_OUTPUT_TOKENS`]), which
/// beats `default` ([`DEFAULT_MAX_OUTPUT_TOKENS`]). A regimen value that is
/// not a positive integer is read as unset.
#[must_use]
pub fn output_cap(flag: Option<u32>, regimen: Option<&Regimen>) -> (u32, &'static str) {
    if let Some(given) = flag {
        return (given, "flag");
    }
    regimen
        .and_then(|regimen| positive(regimen, MAX_OUTPUT_TOKENS))
        .map_or((DEFAULT_MAX_OUTPUT_TOKENS, "default"), |cap| {
            (cap, "regimen")
        })
}

/// The step limit at the regimen's top level (#569), leniently: a positive
/// integer, or `None`. `[limits] max_steps` is still read beneath it.
#[must_use]
pub fn top_level_max_steps(regimen: &Regimen) -> Option<u32> {
    positive(regimen, crate::drive::tool_loop::MAX_STEPS)
}

/// The word a lever is written with when nothing says its state: the lever
/// exists in docs/program.md §2, and this run cannot describe where it sat.
pub const UNDECLARED: &str = "undeclared";
/// The phase graph a served session moves on (#563), as the start row names
/// it beside `seam_trigger`: `none`, or its phases and allowed moves; a graph
/// serve would refuse is undeclared.
fn phase_graph_lever(regimen: &Regimen) -> String {
    match crate::seam::policy::phase_graph(regimen) {
        Ok(graph) if graph.is_empty() => "none".to_owned(),
        Ok(graph) => format!(
            "phases:{};transitions:{}",
            graph.phases().join(","),
            graph
                .transitions()
                .iter()
                .map(|(from, to)| format!("{from}>{to}"))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Err(_) => UNDECLARED.to_owned(),
    }
}

/// The state of each lever (docs/program.md §2) a session `serve` runs
/// under `regimen`, with `output_cap` its output cap and where that came
/// from ([`output_cap`]), sits at, by
/// lever, in that table's words: best effort. A lever this build cannot
/// describe is [`UNDECLARED`], never an error, and nothing reads a missing
/// one as a fault (the maintainer, 2026-10-09: "best effort in the 'duty of
/// care' sense"). A value a reader would refuse is undeclared here too: serve
/// refuses it at start, so no recorded session carries one.
///
/// Some states are the build's, not the regimen's -- the operator may always
/// declare a seam, a fork sees the whole warm trunk and asks at its tail, a
/// failed turn's commands stay on the trunk -- and are written as the build
/// has them.
#[must_use]
pub fn serve_levers(regimen: &Regimen, output_cap: (u32, &str)) -> BTreeMap<String, String> {
    use crate::seam::policy::SEAM_TAIL_TOKENS;
    let word = |key: &str| match regimen.get(key) {
        Some(regimen::Value::String(word)) => word.clone(),
        Some(regimen::Value::Integer(n)) => n.to_string(),
        _ => UNDECLARED.to_owned(),
    };
    let undeclared = || UNDECLARED.to_owned();
    let warrant = match crate::drive::session::interview_warrant(regimen) {
        Ok(rules) if rules.is_empty() => "none".to_owned(),
        Ok(rules) => format!(
            "one-per-gap-gated:{}",
            rules
                .iter()
                .map(|rule| rule.tag())
                .collect::<Vec<_>>()
                .join("+")
        ),
        Err(_) => undeclared(),
    };
    let depth = match regimen.get(SEAM_TAIL_TOKENS) {
        None | Some(regimen::Value::Integer(0)) => "total".to_owned(),
        Some(regimen::Value::Integer(n)) => format!("tail:{n}"),
        Some(_) => undeclared(),
    };
    let triggers = seam_triggers(regimen);
    let [disposition, approval, surface, limits] = command_levers(regimen, output_cap);
    let reasoning = reasoning_lever(regimen);
    let delivery = crate::drive::session::fork_delivery(regimen)
        .map_or_else(|_| undeclared(), |delivery| delivery.tag().to_owned());
    // #560: whether a call written as text is recovered. `off` by default,
    // as Pi and `OpenCode` 2 have it; a value serve refuses is undeclared.
    let text_fallback = match regimen.get(crate::drive::tool_loop::TOOL_CALL_TEXT_FALLBACK) {
        None => "off".to_owned(),
        Some(regimen::Value::String(state)) if state == "off" || state == "on" => state.clone(),
        Some(_) => undeclared(),
    };
    let pruning = crate::drive::prune::lever(regimen);
    BTreeMap::from([
        ("compaction_depth".to_owned(), depth),
        ("seam_trigger".to_owned(), triggers),
        (
            "seam_audit".to_owned(),
            seam_work_lever(regimen, crate::seam::policy::SEAM_AUDIT),
        ),
        (
            "seam_warm".to_owned(),
            seam_work_lever(regimen, crate::seam::policy::SEAM_WARM),
        ),
        ("phase_graph".to_owned(), phase_graph_lever(regimen)),
        (
            "archive_recall".to_owned(),
            crate::drive::archive::Recall::lever(regimen),
        ),
        ("fork_warrant".to_owned(), warrant.clone()),
        ("fork_delivery".to_owned(), delivery),
        ("tool_output_disposition".to_owned(), disposition),
        (
            "isolation".to_owned(),
            word(crate::isolation::policy::ISOLATION),
        ),
        ("approval".to_owned(), approval),
        ("reasoning_state".to_owned(), reasoning),
        ("cache_lifetime".to_owned(), word(CACHE_TTL_KEY)),
        ("substrate_rung".to_owned(), word("substrate")),
        ("tool_surface".to_owned(), surface),
        ("background_commands".to_owned(), background_lever(regimen)),
        ("bash_timeout".to_owned(), timeout_lever(regimen)),
        ("tool_call_text_fallback".to_owned(), text_fallback),
        (
            "instruction_files".to_owned(),
            instruction_files_lever(regimen),
        ),
        // The operator opens and closes a tangent where there is working
        // memory to scope (#22); an agent nominating is a later ask set.
        ("tangent_closure".to_owned(), tangent_closure_lever(regimen)),
        ("self_capture".to_owned(), self_capture_lever(regimen)),
        (
            "capture_modality".to_owned(),
            capture_modality_lever(regimen),
        ),
        (
            "interview_routing_and_cadence".to_owned(),
            cadence_lever(regimen),
        ),
        ("render_budget".to_owned(), render_budget_lever(regimen)),
        (
            "fork_memory_share".to_owned(),
            fork_memory_share_lever(regimen),
        ),
        ("fork_input_view".to_owned(), fork_input_view_lever(regimen)),
        ("fork_asks".to_owned(), fork_asks_lever(regimen)),
        ("fork_delivery_site".to_owned(), "tail".to_owned()),
        ("interview_role".to_owned(), interview_role_lever(regimen)),
        ("step_and_output_limits".to_owned(), limits),
        ("extraction_seat".to_owned(), Seat::lever(regimen)),
        ("failed_turns_on_the_trunk".to_owned(), "kept".to_owned()),
        ("subagent".to_owned(), "harness".to_owned()),
        ("model_pruning".to_owned(), pruning),
    ])
}

/// The fork input view lever's word (#567): the regimen's `fork_view`, and
/// undeclared, the seat's default -- the whole trunk warm, the last turn
/// offboard, which the word says was defaulted (`last_turn:defaulted`, #570).
fn fork_input_view_lever(regimen: &Regimen) -> String {
    let declared = regimen.get(crate::drive::session::FORK_VIEW).is_some();
    match Seat::of(regimen) {
        Seat::Offboard(_) if !declared => format!(
            "{}:defaulted",
            crate::drive::session::ForkView::Last(1).word()
        ),
        _ => crate::drive::session::fork_view(regimen).word(),
    }
}

/// The render budget lever's word (#565): `none`, or `<tokens>:<tier|elide>`.
fn render_budget_lever(regimen: &Regimen) -> String {
    crate::seam::policy::render_budget(regimen).map_or_else(
        || "none".to_owned(),
        |budget| {
            let over = match budget.over {
                crate::seam::render::OverBudget::Tier => "tier",
                crate::seam::render::OverBudget::Elide => "elide",
            };
            format!("{}:{over}", budget.tokens)
        },
    )
}

/// The interview routing and cadence lever (#564): the cadence's word, and
/// the read fork's output threshold when one is declared.
fn cadence_lever(regimen: &Regimen) -> String {
    let cadence = crate::drive::session::interview_cadence(regimen).word();
    let threshold = crate::drive::session::interview_threshold_bytes(regimen)
        .map(|bytes| format!(":threshold:{bytes}-bytes"))
        .unwrap_or_default();
    // #611: forks the turn's self-capture already recorded, skipped.
    let skip = if crate::drive::session::interview_skip_self_recorded(regimen) {
        ":skip-self-recorded"
    } else {
        ""
    };
    format!("{cadence}{threshold}{skip}")
}

/// The fork ask set lever's word (#595): the set's name and digest.
fn fork_asks_lever(regimen: &Regimen) -> String {
    let set = crate::drive::session::fork_asks(regimen);
    format!("{}:{}", set.name, set.digest())
}

/// The tangent closure lever's word (#22): `operator` where there is
/// working memory to scope, else [`UNDECLARED`].
fn tangent_closure_lever(regimen: &Regimen) -> String {
    if crate::drive::session::interview_warrant(regimen).is_ok_and(|rules| !rules.is_empty()) {
        "operator".to_owned()
    } else {
        UNDECLARED.to_owned()
    }
}

/// The bash timeout lever (#613): the default in milliseconds, `off` for
/// none; undeclared where the regimen runs no commands.
fn timeout_lever(regimen: &Regimen) -> String {
    crate::drive::tool_loop::declared(regimen)
        .ok()
        .flatten()
        .map_or_else(
            || UNDECLARED.to_owned(),
            |declared| {
                declared
                    .timeout_ms
                    .map_or_else(|| "off".to_owned(), |ms| format!("{ms}ms"))
            },
        )
}

/// The background commands lever (#614): on unless the regimen turns them
/// off; undeclared where it runs no commands.
fn background_lever(regimen: &Regimen) -> String {
    crate::drive::tool_loop::declared(regimen)
        .ok()
        .flatten()
        .map_or_else(
            || UNDECLARED.to_owned(),
            |declared| if declared.background { "on" } else { "off" }.to_owned(),
        )
}

/// The interview role lever's word (#599): the role the regimen asks in.
fn interview_role_lever(regimen: &Regimen) -> String {
    crate::drive::session::interview_role(regimen)
        .tag()
        .to_owned()
}

/// The self-capture lever's word (#609): `off`, or `on:every:<n>` with the
/// reminder's cadence of silent turns.
fn self_capture_lever(regimen: &Regimen) -> String {
    crate::drive::session::self_capture(regimen).map_or_else(
        || "off".to_owned(),
        |cadence| format!("on:every:{}", cadence.interval()),
    )
}

/// The instruction files lever's word: `on` or `off`.
fn instruction_files_lever(regimen: &Regimen) -> String {
    if crate::drive::instructions::enabled(regimen) {
        "on"
    } else {
        "off"
    }
    .to_owned()
}

/// The seam triggers a regimen leaves armed, in [`serve_levers`]' words:
/// the operator's always, then the cadence and budget it declares, and the
/// automatic seam unless it turns it off (#617).
fn seam_triggers(regimen: &Regimen) -> String {
    use crate::seam::policy::{SEAM_AT_WORKING_SET_BYTES, SEAM_EVERY_TURNS};
    let mut triggers = vec!["operator-declared"];
    if regimen.get(SEAM_EVERY_TURNS).is_some() {
        triggers.push("cadence");
    }
    if [SEAM_AT_WORKING_SET_BYTES, "seam_at_context_fraction"]
        .iter()
        .any(|key| regimen.get(key).is_some())
    {
        triggers.push("budget");
    }
    // The automatic seam (#617): on unless the regimen turns it off; it
    // fires only where serve knows the window and keeps working memory.
    if crate::seam::policy::Served::from_regimen(regimen, None)
        .map_or(true, |served| !served.window_off)
    {
        triggers.push("window");
    }
    triggers.join("+")
}

/// The reasoning state lever's word: the substrate's reasoning state, with
/// the effort its `reasoning` table names when it names one.
fn reasoning_lever(regimen: &Regimen) -> String {
    match (regimen.get("substrate_reasoning"), regimen.get("reasoning")) {
        (Some(regimen::Value::String(state)), Some(regimen::Value::Table(table))) => {
            match table.get("effort") {
                Some(regimen::Value::String(effort)) => format!("{state}:effort:{effort}"),
                _ => state.clone(),
            }
        }
        (Some(regimen::Value::String(state)), _) => state.clone(),
        _ => UNDECLARED.to_owned(),
    }
}

/// A seam's audit or pre-warm lever's word (#504): `on` or `off`.
fn seam_work_lever(regimen: &Regimen, key: &str) -> String {
    let on = match regimen.get(key) {
        Some(regimen::Value::Boolean(on)) => *on,
        Some(regimen::Value::String(word)) => word == "on",
        _ => false,
    };
    if on { "on" } else { "off" }.to_owned()
}

/// The fork memory share lever's word (#406): the tail a fork's call is
/// clamped from, `tail:output-cap` or the regimen's `tail:<tokens>`; a fork
/// that would not fit its window is refused unsent either way.
fn fork_memory_share_lever(regimen: &Regimen) -> String {
    crate::drive::session::fork_tail(regimen).map_or_else(
        || "tail:output-cap".to_owned(),
        |tokens| format!("tail:{tokens}"),
    )
}

/// The capture modality lever's word (#610): how a fork answers, `fields`
/// or `tools`; [`UNDECLARED`] for a word serve refuses at start.
fn capture_modality_lever(regimen: &Regimen) -> String {
    crate::drive::session::capture_modality(regimen).map_or_else(
        |_| UNDECLARED.to_owned(),
        |modality| modality.word().to_owned(),
    )
}

/// The levers a regimen's commands set -- tool-output disposition, approval,
/// tool surface, step and output limits -- in [`serve_levers`]' words; the
/// first three [`UNDECLARED`] when it runs none.
fn command_levers(regimen: &Regimen, (cap, cap_from): (u32, &str)) -> [String; 4] {
    use crate::drive::output::OutputCap;
    use crate::drive::tool_loop::{self, ToolSurface};
    let commands = tool_loop::declared(regimen).ok().flatten();
    let undeclared = || UNDECLARED.to_owned();
    // The cap on arrival (#554), then what a seam carries of the outputs it
    // compacts away (#553).
    let seam = crate::seam::policy::seam_tool_outputs(regimen).tag();
    let disposition = match commands.as_ref().map(|declared| declared.output_cap) {
        Some(OutputCap::Keep) => format!("keep+seam:{seam}"),
        Some(OutputCap::Capped {
            max_lines,
            max_bytes,
        }) => format!("cap-on-arrival:{max_lines}-lines:{max_bytes}-bytes+seam:{seam}"),
        None => undeclared(),
    };
    let approval = match &commands {
        Some(declared) if declared.approvals_off => "none".to_owned(),
        Some(declared) if declared.allowed_commands.is_empty() => "denylist-prompt".to_owned(),
        Some(_) => "denylist-prompt-preseeded".to_owned(),
        None => undeclared(),
    };
    let surface = match commands.as_ref().map(|declared| declared.surface) {
        Some(ToolSurface::Bash) => "bash".to_owned(),
        Some(ToolSurface::Standard) => "standard".to_owned(),
        None => undeclared(),
    };
    // Each limit, then where it came from: a step limit has no flag, so it
    // is the regimen's or the default's (none).
    let steps = commands
        .as_ref()
        .and_then(|declared| declared.max_steps)
        .map_or_else(
            || "none:default".to_owned(),
            |steps| format!("{steps}:regimen"),
        );
    let limits = format!("max_steps:{steps}:output_cap:{cap}:{cap_from}");
    [disposition, approval, surface, limits]
}

#[cfg(test)]
mod tests {
    use super::{UNDECLARED, regime_of, serve_levers};

    #[test]
    fn the_output_cap_is_the_flag_else_the_regimens_else_the_default_and_steps_read_top_level_first()
     {
        use super::{DEFAULT_MAX_OUTPUT_TOKENS, output_cap};
        let read = |text: &str| regimen::parse(text).expect("a regimen");
        let keyed = read("max_output_tokens = 32768\n");
        assert_eq!(output_cap(Some(512), Some(&keyed)), (512, "flag"));
        assert_eq!(output_cap(None, Some(&keyed)), (32_768, "regimen"));
        assert_eq!(
            output_cap(None, None),
            (DEFAULT_MAX_OUTPUT_TOKENS, "default")
        );
        // Leniently: a value that is not a positive integer is unset.
        for unset in ["max_output_tokens = 0\n", "max_output_tokens = \"lots\"\n"] {
            assert_eq!(
                output_cap(None, Some(&read(unset))),
                (DEFAULT_MAX_OUTPUT_TOKENS, "default")
            );
        }
        let both = read("approval = \"none\"\nmax_steps = 12\n[limits]\nmax_steps = 40\n");
        let declared = crate::drive::tool_loop::declared(&both)
            .expect("read")
            .expect("runs commands");
        assert_eq!(declared.max_steps, Some(12), "the top-level key first");
        let levers = serve_levers(&both, (32_768, "regimen"));
        assert_eq!(
            levers.get("step_and_output_limits").map(String::as_str),
            Some("max_steps:12:regimen:output_cap:32768:regimen")
        );
        let under = read("approval = \"none\"\n[limits]\nmax_steps = 40\n");
        let declared = crate::drive::tool_loop::declared(&under)
            .expect("read")
            .expect("runs commands");
        assert_eq!(declared.max_steps, Some(40), "`[limits]` beneath it");
    }

    /// The interview routing and cadence lever names the cadence (#564)
    /// and, when declared, the read fork's threshold: no longer the warrant.
    #[test]
    fn the_interview_lever_names_the_cadence_and_its_threshold() {
        let lever = |text: &str| {
            serve_levers(&regimen::parse(text).expect("a regimen"), (8192, "default"))
                .get("interview_routing_and_cadence")
                .cloned()
                .unwrap_or_default()
        };
        assert_eq!(lever("interview_warrant = [\"scoping\"]\n"), "gap");
        assert_eq!(
            lever("interview_cadence = \"per_class\"\ninterview_threshold_bytes = 3000\n"),
            "per_class:threshold:3000-bytes"
        );
        // #611, off unless declared.
        assert_eq!(
            lever("interview_skip_self_recorded = true\n"),
            "gap:skip-self-recorded"
        );
        assert_eq!(lever("interview_skip_self_recorded = \"maybe\"\n"), "gap");
    }

    /// The tool-output disposition names the cap on arrival and, beside
    /// it, what a seam carries of the outputs it compacts away (#553):
    /// `evict` unless the regimen declares a state, an unknown word unset.
    #[test]
    fn the_disposition_names_the_seams_tool_output_state_beside_the_cap() {
        let disposition = |extra: &str| {
            let regimen =
                regimen::parse(&format!("approval = \"none\"\n{extra}")).expect("a regimen");
            serve_levers(&regimen, (32_768, "regimen"))
                .get("tool_output_disposition")
                .cloned()
                .unwrap_or_default()
        };
        assert_eq!(
            disposition(""),
            "cap-on-arrival:2000-lines:51200-bytes+seam:evict"
        );
        assert_eq!(
            disposition("seam_tool_outputs = \"reference\"\n"),
            "cap-on-arrival:2000-lines:51200-bytes+seam:reference"
        );
        assert_eq!(
            disposition("seam_tool_outputs = \"most of them\"\n"),
            "cap-on-arrival:2000-lines:51200-bytes+seam:evict",
            "read leniently: an unknown word is unset"
        );
        assert_eq!(
            disposition("seam_tool_outputs = \"keep\"\n[tool_output]\ncap = false\n"),
            "keep+seam:keep"
        );
    }

    /// Background commands (#614) are on unless the regimen turns them
    /// off, and undeclared where it runs no commands.
    #[test]
    fn the_background_commands_lever_is_on_unless_turned_off() {
        let lever = |text: &str| {
            serve_levers(&regimen::parse(text).expect("a regimen"), (8192, "default"))
                .get("background_commands")
                .cloned()
                .unwrap_or_default()
        };
        assert_eq!(lever("approval = \"none\"\n"), "on");
        assert_eq!(
            lever("approval = \"none\"\nbackground_commands = false\n"),
            "off"
        );
        assert_eq!(
            lever("approval = \"none\"\nbackground_commands = \"off\"\n"),
            "off"
        );
        assert_eq!(lever(""), UNDECLARED);
    }

    /// The default timeout (#613): 120000 ms unless the regimen says, 0
    /// for none, and undeclared where it runs no commands.
    #[test]
    fn the_bash_timeout_lever_names_the_default_or_off() {
        let lever = |text: &str| {
            serve_levers(&regimen::parse(text).expect("a regimen"), (8192, "default"))
                .get("bash_timeout")
                .cloned()
                .unwrap_or_default()
        };
        assert_eq!(lever("approval = \"none\"\n"), "120000ms");
        assert_eq!(
            lever("approval = \"none\"\nbash_timeout_ms = 30000\n"),
            "30000ms"
        );
        assert_eq!(lever("approval = \"none\"\nbash_timeout_ms = 0\n"), "off");
        assert_eq!(lever(""), UNDECLARED);
    }

    #[test]
    fn t1s_draft_regimens_put_every_lever_at_a_word_and_undescribed_ones_at_undeclared() {
        let draft = |name: &str| {
            let path = format!("{}/../drafts/{name}", env!("CARGO_MANIFEST_DIR"));
            let text = std::fs::read_to_string(&path).expect("the draft");
            serve_levers(&regimen::parse(&text).expect("a regimen"), (32_768, "flag"))
        };
        let floor = draft("t1-session-one.regimen.toml");
        let at = |levers: &std::collections::BTreeMap<String, String>, lever: &str| {
            levers.get(lever).cloned().unwrap_or_default()
        };
        assert_eq!(at(&floor, "fork_warrant"), "one-per-gap-gated:scoping");
        assert_eq!(at(&floor, "isolation"), "sandbox");
        assert_eq!(at(&floor, "approval"), "denylist-prompt-preseeded");
        assert_eq!(
            at(&floor, "substrate_rung"),
            "accel24-beellama-qwen27b-q4kxl"
        );
        assert_eq!(at(&floor, "tool_surface"), "bash");
        assert_eq!(at(&floor, "tangent_closure"), "operator");
        assert_eq!(at(&floor, "capture_modality"), "fields");
        let line = draft("t1-session-one-qwen38.regimen.toml");
        assert_eq!(at(&line, "approval"), "none");
        assert_eq!(at(&line, "reasoning_state"), "on:effort:xhigh");
        assert_eq!(
            at(&line, "step_and_output_limits"),
            "max_steps:none:default:output_cap:32768:flag"
        );
        // A regimen that says nothing still gets a table, never an error.
        let empty = serve_levers(
            &regimen::parse("").expect("an empty regimen"),
            (8192, "default"),
        );
        assert_eq!(at(&empty, "fork_warrant"), "none");
        assert_eq!(at(&empty, "isolation"), UNDECLARED);
        assert_eq!(at(&empty, "compaction_depth"), "total");
        assert_eq!(at(&empty, "render_budget"), "none");
        let budgeted = serve_levers(
            &regimen::parse("render_budget_tokens = 2000\nrender_over_budget = \"elide\"\n")
                .expect("a regimen"),
            (8192, "default"),
        );
        assert_eq!(at(&budgeted, "render_budget"), "2000:elide");
        let paced = serve_levers(
            &regimen::parse("seam_every_turns = 3\nseam_tail_tokens = 8000\n").expect("a regimen"),
            (8192, "default"),
        );
        assert_eq!(
            at(&paced, "seam_trigger"),
            "operator-declared+cadence+window"
        );
        assert_eq!(at(&paced, "compaction_depth"), "tail:8000");
    }
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

    /// A regimen naming substrate `id`, its hardware `hardware`.
    fn naming(id: &str, hardware: &str) -> crate::formats::regimen::Regimen {
        let text = format!(
            "arm = \"a\"\ndogma_version = 0\nsubstrate = \"{id}\"\n\
             substrate_reasoning = \"off\"\nsubstrate_hardware = \"{hardware}\"\n\
             [sampler]\nseed = 7\n"
        );
        regimen::parse(&text).expect("the document is a regimen")
    }

    #[test]
    fn a_registered_substrate_crosses_with_the_registrys_identity() {
        use crate::drive::registry::{REGISTRY, identity};
        use crate::formats::record::Weights;
        let id = "accel24-beellama-qwen27b-q4kxl";
        let registered = identity(REGISTRY, id).expect("registered");
        let regime =
            super::regime_registered(&naming(id, &registered.hardware_fingerprint), REGISTRY)
                .expect("a registered substrate, its hardware agreeing");
        let substrate = &regime.substrates[0];
        assert_eq!(substrate.engine, registered.engine);
        // The floor's whole set: its main file, its draft and its projector.
        assert!(
            matches!(&substrate.weights, Weights::Set(set) if set.main.len() == 1 && set.draft.is_some() && set.projector.is_some())
        );
        assert_eq!(substrate.weights, registered.weights);

        // The dev loop's own regimen resolves to the canned instance it plays.
        let dev_loop = regimen::parse(crate::drive::canned::DEV_LOOP).expect("a regimen");
        let regime = super::regime_registered(&dev_loop, REGISTRY).expect("canned-cache-n");
        assert_eq!(
            regime.substrates[0].weights,
            Weights::Canned {
                acts_sha256: crate::drive::canned::acts_digest()
            }
        );
    }

    /// #570: the seat reads leniently, and an offboard one is a second
    /// substrate the registry says what it is, asked as the trunk is.
    #[test]
    fn an_offboard_seat_is_a_second_substrate_the_registry_resolves() {
        use crate::drive::registry::{REGISTRY, identity};
        let read = |line: &str| {
            let text = format!("{line}\narm = \"a\"\n");
            let parsed = regimen::parse(&text).expect("a regimen");
            (super::Seat::of(&parsed), super::Seat::lever(&parsed))
        };
        assert_eq!(read(""), (super::Seat::Warm, "warm-model".to_owned()));
        assert_eq!(
            read("extraction_seat = \"warm\""),
            (super::Seat::Warm, "warm-model".to_owned())
        );
        assert_eq!(
            read("extraction_seat = \"offboard:cpu\""),
            (
                super::Seat::Offboard("cpu".to_owned()),
                "offboard:cpu".to_owned()
            )
        );
        for unread in ["offboard:", "cold"] {
            assert_eq!(
                read(&format!("extraction_seat = \"{unread}\"")),
                (super::Seat::Warm, format!("warm-model:unknown:{unread}"))
            );
        }

        let dev_loop = crate::drive::canned::DEV_LOOP;
        let seated = |seat: &str| {
            let text = format!("extraction_seat = \"offboard:{seat}\"\n{dev_loop}");
            super::regime_registered(&regimen::parse(&text).expect("a regimen"), REGISTRY)
        };
        let regime = seated("canned-replay").expect("a registered seat");
        let [trunk, seat] = regime.substrates.as_slice() else {
            panic!("two substrates: {:#?}", regime.substrates);
        };
        let registered = identity(REGISTRY, "canned-replay").expect("registered");
        assert_eq!(seat.id, "canned-replay");
        assert_eq!(seat.engine, registered.engine);
        assert_eq!(seat.weights, registered.weights);
        assert_eq!(seat.hardware_fingerprint, registered.hardware_fingerprint);
        // Asked as the trunk is: a fork's request is a clone of the trunk's.
        assert_eq!(seat.sampler_card, trunk.sampler_card);
        assert_eq!(seat.reasoning, trunk.reasoning);
        assert_ne!(seat.weights, trunk.weights);

        let own = seated(&trunk.id).expect_err("the trunk's own substrate");
        assert!(own.contains("is `warm`"), "{own}");
        let unknown = seated("no-such-seat").expect_err("unregistered");
        assert!(unknown.starts_with("`extraction_seat`: "), "{unknown}");
    }

    #[test]
    fn a_registered_chat_template_crosses_into_the_regime() {
        use crate::drive::registry::{REGISTRY, identity};
        let id = "accel24-llamacpp-qwen38-27b-iq3s";
        let registered = identity(REGISTRY, id).expect("registered");
        let regime =
            super::regime_registered(&naming(id, &registered.hardware_fingerprint), REGISTRY)
                .expect("registered");
        assert!(registered.chat_template_sha256.is_some());
        assert_eq!(
            regime.substrates[0].chat_template_sha256,
            registered.chat_template_sha256
        );
    }

    #[test]
    fn a_regimen_whose_hardware_disagrees_with_the_registry_is_refused() {
        use crate::drive::registry::REGISTRY;
        let refused = super::regime_registered(
            &naming("accel24-beellama-qwen27b-q4kxl", &"b".repeat(64)),
            REGISTRY,
        )
        .expect_err("two declarations of one machine disagree");
        assert!(refused.contains("disagrees with the registry"), "{refused}");
    }

    #[test]
    fn a_regimen_naming_an_unregistered_substrate_is_refused() {
        use crate::drive::registry::REGISTRY;
        let refused =
            super::regime_registered(&naming("nowhere-at-all", &"a".repeat(64)), REGISTRY)
                .expect_err("unregistered");
        assert!(
            refused.contains("`nowhere-at-all` is not a substrate"),
            "{refused}"
        );
    }

    /// R1: the regime's reasoning state as the template variables every
    /// request sends.
    #[test]
    fn the_reasoning_state_is_sent_in_the_templates_words() {
        use crate::formats::record::json::Value;
        use std::collections::BTreeMap;
        let kwargs = |reasoning: &str, table: &str| {
            let text = format!(
                "arm = \"a\"\ndogma_version = 0\nsubstrate = \"canned\"\n\
                 substrate_reasoning = \"{reasoning}\"\nsubstrate_hardware = \"{}\"\n\
                 {table}[sampler]\nseed = 7\n",
                "a".repeat(64)
            );
            let regime =
                regime_of(&regimen::parse(&text).expect("a regimen"), false).expect("a regime");
            super::template_kwargs(&regime.substrates[0])
        };
        assert_eq!(
            kwargs("off", ""),
            Ok((
                BTreeMap::from([("enable_thinking".to_owned(), Value::Boolean(false))]),
                None
            ))
        );
        let level = "[reasoning]\neffort = \"medium\"\nbudget_tokens = \"none\"\n";
        let medium = BTreeMap::from([
            ("enable_thinking".to_owned(), Value::Boolean(true)),
            (
                "reasoning_effort".to_owned(),
                Value::String("medium".to_owned()),
            ),
        ]);
        assert_eq!(kwargs("on", level), Ok((medium.clone(), None)));
        let refused = kwargs("off", level).expect_err("a level with thinking off");
        assert!(refused.contains("\"medium\""), "{refused}");
        // A budget no template carries is recorded as unsent, never refused.
        let capped = "[reasoning]\neffort = \"medium\"\nbudget_tokens = 512\n";
        assert_eq!(kwargs("on", capped), Ok((medium, Some(512))));
    }

    /// The substrate's model convention, declared in the registry, rides
    /// with the regime's reasoning state: `preserve_thinking` on every
    /// request, and the template's default level named when thinking is on
    /// and none is sent.
    #[test]
    fn the_registrys_declared_kwargs_ride_with_the_reasoning_state() {
        use crate::formats::record::json::Value;
        use std::collections::BTreeMap;
        let floor = crate::drive::registry::identity(
            crate::drive::registry::REGISTRY,
            "accel24-tabbyapi-exl3-qwen38-27b-3p00",
        )
        .expect("registered");
        let wire = |reasoning: &str, table: &str| {
            let text = format!(
                "arm = \"a\"\ndogma_version = 0\nsubstrate = \"canned\"\n\
                 substrate_reasoning = \"{reasoning}\"\nsubstrate_hardware = \"{}\"\n\
                 {table}[sampler]\nseed = 7\n",
                "a".repeat(64)
            );
            let regime =
                regime_of(&regimen::parse(&text).expect("a regimen"), false).expect("a regime");
            super::template_kwargs_declared(&regime.substrates[0], Some(&floor)).expect("sent")
        };
        let on = wire("on", "");
        assert_eq!(
            on.kwargs,
            BTreeMap::from([
                ("enable_thinking".to_owned(), Value::Boolean(true)),
                ("preserve_thinking".to_owned(), Value::Boolean(true)),
            ])
        );
        assert_eq!(on.reasoning_effort_default.as_deref(), Some("xhigh"));
        let medium = wire(
            "on",
            "[reasoning]\neffort = \"medium\"\nbudget_tokens = \"none\"\n",
        );
        assert_eq!(medium.reasoning_effort_default, None);
        assert_eq!(
            medium.kwargs.get("reasoning_effort"),
            Some(&Value::String("medium".to_owned()))
        );
        let off = wire("off", "");
        assert_eq!(off.reasoning_effort_default, None);
        assert_eq!(
            off.kwargs.get("preserve_thinking"),
            Some(&Value::Boolean(true))
        );
    }

    /// #486: the pins are the record's card, setting for setting, and a
    /// decimal goes onto the wire as the digits the regimen wrote.
    #[test]
    fn the_wires_pins_are_the_records_sampler_card_in_the_digits_written() {
        use crate::client::shape::{Pin, SamplerSetting};
        let text = format!(
            "arm = \"a\"\ndogma_version = 0\nsubstrate = \"canned\"\n\
             substrate_reasoning = \"off\"\nsubstrate_hardware = \"{}\"\n\
             [sampler]\ntemperature = 0.6\ntop_p = 1.0\nmin_p = 0.0\ntop_k = 20\nseed = 7\n",
            "a".repeat(64)
        );
        let regime =
            regime_of(&regimen::parse(&text).expect("a regimen"), false).expect("a regime");
        let card = &regime.substrates[0].sampler_card;
        let pins = super::sampler_pins(card).expect("every key is pinnable");
        assert_eq!(pins.len(), card.len());
        let spelled: Vec<(&str, String)> = pins
            .iter()
            .map(|(setting, pin)| (setting.tag(), pin.to_string()))
            .collect();
        assert_eq!(
            spelled,
            [
                ("temperature", "0.6".to_owned()),
                ("top_p", "1.0".to_owned()),
                ("top_k", "20".to_owned()),
                ("min_p", "0.0".to_owned()),
                ("seed", "7".to_owned()),
            ]
        );
        assert_eq!(pins.get(SamplerSetting::Seed), Some(&Pin::Integer(7)));
    }

    #[test]
    fn a_sampler_key_the_wire_cannot_pin_is_refused_by_name() {
        use crate::formats::record::json::Value;
        let card = std::collections::BTreeMap::from([
            ("temperature".to_owned(), Value::Integer(1)),
            ("max_tokens".to_owned(), Value::Integer(4096)),
        ]);
        let refused = super::sampler_pins(&card).expect_err("not a sampler setting");
        assert!(refused.contains("`max_tokens`"), "{refused}");
        assert!(
            refused.contains("temperature, top_p, top_k, min_p, repeat_penalty, seed"),
            "{refused}"
        );
        let card = std::collections::BTreeMap::from([("seed".to_owned(), Value::Boolean(true))]);
        let refused = super::sampler_pins(&card).expect_err("not a number");
        assert!(
            refused.contains("`[sampler] seed` is not a number"),
            "{refused}"
        );
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
