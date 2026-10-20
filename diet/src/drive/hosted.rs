//! A hosted API's session (#555): what its requests carry beyond the shape,
//! read from the regime and the substrate's registry entry, and checked
//! against what the entry declares.
//!
//! **One way of thinking per session, the forks' included.** The entry's
//! `thinking` names the model's convention -- `adaptive`, an effort; or
//! `budget`, a token count -- and the regimen's `[reasoning]` table sets it.
//! A regimen with reasoning off asks for no thinking at all.
//!
//! **The betas sent are the betas declared.** A beta header changes how the
//! API behaves, so the entry declares the ones it is sent (`engine_betas`),
//! and serve refuses to start when what it would send differs.

use crate::client::anthropic::{Options, Thinking, Ttl, Ttls};
use crate::client::shape::RequestShape;
use crate::client::stream::{Bearer, HttpStream};
use crate::drive::registry::Identity;
use crate::formats::record::{Budget, Reasoning, Substrate};

/// The options `substrate`'s requests carry under its entry `identity`.
///
/// # Errors
///
/// Reasoning on with an entry that names no `thinking` convention; an
/// adaptive model asked with no effort; a budget model asked with no
/// token budget.
pub fn options(substrate: &Substrate, identity: &Identity, ttl: Ttls) -> Result<Options, String> {
    let on = matches!(substrate.reasoning, Reasoning::On | Reasoning::Suppressed);
    if !on {
        return Ok(Options {
            ttl,
            ..Options::default()
        });
    }
    let id = &substrate.id;
    let control = substrate.reasoning_control.as_ref();
    let thinking = match identity.thinking.as_deref() {
        Some("adaptive") => Thinking::Adaptive {
            effort: Some(
                control
                    .map(|control| control.effort.clone())
                    .ok_or_else(|| {
                        format!(
                            "`{id}` thinks adaptively, at an effort, and the regimen's \
                             `[reasoning]` names none"
                        )
                    })?,
            ),
        },
        Some("budget") => match control.map(|control| control.budget_tokens) {
            Some(Budget::Tokens(tokens)) => Thinking::Budget {
                tokens: tokens.get(),
            },
            _ => {
                return Err(format!(
                    "`{id}` thinks to a token budget, and the regimen's `[reasoning]` sets no \
                     `budget_tokens`"
                ));
            }
        },
        _ => {
            return Err(format!(
                "reasoning is on, and `{id}`'s entry declares no `thinking` convention \
                 (\"adaptive\" or \"budget\")"
            ));
        }
    };
    Ok(Options { thinking, ttl })
}

/// The regimen key for the cache-lifetime lever (#556).
pub const CACHE_TTL: &str = "cache_ttl";

/// The regimen keys a hosted session's requests are limited by (#555), in
/// the order a refusal names them missing: the cap, the retries after a
/// first attempt, and the longest `retry-after` waited, in seconds.
pub const REQUEST_LIMITS: [&str; 3] = [
    "max_api_requests",
    "api_retries",
    "api_retry_after_max_seconds",
];

/// What a hosted session's regimen limits its requests to (#555). Every one
/// is declared: none has a default, since each spends money.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestLimits {
    /// `max_api_requests`: every request the session sends, every lane's
    /// and every retry, counted against it; at least 1.
    pub max_api_requests: u64,
    /// `api_retries`: the attempts after a request's first.
    pub api_retries: u64,
    /// `api_retry_after_max_seconds`: the longest `retry-after` waited.
    pub api_retry_after_max_seconds: u64,
}

/// The request limits `regimen` declares (#555).
///
/// # Errors
///
/// Every key missing, named together; a value that is not a whole number,
/// or a `max_api_requests` of 0, which would send nothing.
pub fn request_limits(regimen: &crate::formats::regimen::Regimen) -> Result<RequestLimits, String> {
    let missing: Vec<&str> = REQUEST_LIMITS
        .into_iter()
        .filter(|key| regimen.get(key).is_none())
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "a hosted API's regimen declares its request limits, and this one declares no {}: \
             each spends money, so none has a default",
            missing
                .iter()
                .map(|key| format!("`{key}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let count = |key: &str, least: u64| match regimen.get(key) {
        Some(crate::formats::regimen::Value::Integer(n)) => u64::try_from(*n)
            .ok()
            .filter(|n| *n >= least)
            .ok_or_else(|| format!("`{key}` is {n}, not a whole number of at least {least}")),
        _ => Err(format!("`{key}` is a whole number of at least {least}")),
    };
    Ok(RequestLimits {
        max_api_requests: count(REQUEST_LIMITS[0], 1)?,
        api_retries: count(REQUEST_LIMITS[1], 0)?,
        api_retry_after_max_seconds: count(REQUEST_LIMITS[2], 0)?,
    })
}

/// The lifetimes `regimen`'s `cache_ttl` asks of each breakpoint (#556):
/// `5m` (the default, all three harnesses'), `1h`, or `per-breakpoint` --
/// Qwen Code's shape, the system and tools for an hour and the final user
/// message's tail for five minutes, longer before shorter in the cache's
/// tools, system, messages order.
///
/// # Errors
///
/// Any other value: a lifetime declared and not understood is never sent
/// as another.
pub fn cache_ttl(regimen: &crate::formats::regimen::Regimen) -> Result<Ttls, String> {
    match regimen.get(CACHE_TTL) {
        None => Ok(Ttls::default()),
        Some(crate::formats::regimen::Value::String(word)) => match word.as_str() {
            "5m" => Ok(Ttls::all(Ttl::FiveMinutes)),
            "1h" => Ok(Ttls::all(Ttl::Hour)),
            "per-breakpoint" => Ok(Ttls {
                tools: Ttl::Hour,
                system: Ttl::Hour,
                tail: Ttl::FiveMinutes,
            }),
            other => Err(format!(
                "`{CACHE_TTL} = \"{other}\"` is not \"5m\", \"1h\" or \"per-breakpoint\""
            )),
        },
        Some(_) => Err(format!(
            "`{CACHE_TTL}` is a word: \"5m\", \"1h\" or \"per-breakpoint\""
        )),
    }
}

/// The variable a hosted API's key is read from when no `--key-file` is
/// given: the one Pi, `OpenCode` 2 and Qwen Code read.
pub const ANTHROPIC_API_KEY: &str = "ANTHROPIC_API_KEY";

/// Where a hosted API's key comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    /// `--key-file`, already on the transport.
    Given,
    /// [`ANTHROPIC_API_KEY`]: its value if set, and whether the regimen
    /// passes the variable on to the model's commands.
    Environment(Option<String>, bool),
}

/// `transport` speaking a hosted API: the key, and the Messages wire under
/// the options `substrate` and its entry `identity` set. Checked before
/// anything binds: the betas it would send are the ones the entry
/// declares, and `shape` is one the API can carry.
///
/// # Errors
///
/// No key; a key from the environment the regimen passes on to the
/// model's commands, which would be theirs to read; a thinking convention
/// the regime cannot meet; undeclared betas; a shape the API cannot carry;
/// what the entry declares its API refuses ([`refused_at_start`]). With the
/// transport, the declarations the entry leaves out, which nothing checked.
pub fn transport(
    transport: HttpStream,
    (identity, substrate): (&Identity, &Substrate),
    (key, ttl): (Key, Ttls),
    shape: &RequestShape,
) -> Result<(HttpStream, Vec<&'static str>), String> {
    let id = &substrate.id;
    model_named(identity, id, shape)?;
    let transport = match key {
        Key::Given => transport,
        Key::Environment(_, true) => {
            return Err(format!(
                "the regimen passes {ANTHROPIC_API_KEY} on to the model's commands: a key read \
                 from it would be theirs to read; give it with --key-file instead"
            ));
        }
        Key::Environment(None, false) => {
            return Err(format!(
                "`{id}` is a hosted API, and its key is in neither --key-file nor \
                 {ANTHROPIC_API_KEY}"
            ));
        }
        Key::Environment(Some(key), false) => transport.with_bearer(
            Bearer::new(&key).ok_or_else(|| format!("{ANTHROPIC_API_KEY} holds no usable key"))?,
        ),
    };
    let options = options(substrate, identity, ttl)?;
    betas_agree(id, identity, &options)?;
    crate::client::anthropic::body(shape, &options)
        .map_err(|why| format!("`{id}`'s requests: {why}"))?;
    let unchecked = refused_at_start(id, identity, shape, &options)?;
    Ok((transport.with_anthropic(options), unchecked))
}

/// Whether `shape` is sent as the model `identity`'s entry names (#555).
///
/// # Errors
///
/// Another model, naming both; an entry that names none.
pub fn model_named(identity: &Identity, id: &str, shape: &RequestShape) -> Result<(), String> {
    match identity.model_id.as_deref() {
        Some(named) if named == shape.model => Ok(()),
        Some(named) => Err(format!(
            "`{id}` declares `weights_model_id = \"{named}\"`, and this session's requests \
             would be sent as \"{}\" (--model): the provider bills the model a request names, \
             and the record names the entry's, so the two are one",
            shape.model
        )),
        None => Err(format!(
            "`{id}` is a hosted API and names no `weights_model_id`: a paid rung names the \
             model it bills, and its requests are sent as that model"
        )),
    }
}

/// What the entry declares its API would refuse with a 400 (#555), checked
/// against what this session would send, before anything is sent: its
/// `max_tokens`, its effort, its sampler. Only the entry's declarations are
/// read; what it leaves out is unchecked, and returned, so the session's
/// start can say so.
///
/// # Errors
///
/// Every declared refusal the session would meet, each naming the
/// declaration and what would be sent.
pub fn refused_at_start(
    id: &str,
    identity: &Identity,
    shape: &RequestShape,
    options: &Options,
) -> Result<Vec<&'static str>, String> {
    let declared = &identity.api;
    let mut refused = Vec::new();
    let asked = u64::from(shape.limits.max_output_tokens);
    if let Some(cap) = declared.max_output_tokens.filter(|cap| asked > *cap) {
        refused.push(format!(
            "it declares `max_output_tokens = {cap}`, and this session asks for {asked}"
        ));
    }
    if let (
        Some(levels),
        Thinking::Adaptive {
            effort: Some(effort),
        },
    ) = (&declared.efforts, &options.thinking)
        && !levels.contains(effort)
    {
        refused.push(format!(
            "it declares `efforts = [{}]`, and this session asks for \"{effort}\"",
            levels
                .iter()
                .map(|level| format!("\"{level}\""))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let mut sent = Vec::new();
    for (setting, pin) in shape.sampler.iter() {
        let name = setting.tag();
        let digits = match pin {
            crate::client::shape::Pin::Decimal(value) => value.as_str().to_owned(),
            crate::client::shape::Pin::Integer(value) => value.to_string(),
        };
        sent.push(name);
        match declared.sampler.get(name).map(String::as_str) {
            None => {}
            Some("none") => refused.push(format!(
                "it declares `api_{name} = \"none\"`, and this session sends {name} = {digits}"
            )),
            Some(accepted) if same_number(accepted, &digits) => {}
            Some(accepted) => refused.push(format!(
                "it declares `api_{name} = \"{accepted}\"`, and this session sends \
                 {name} = {digits}"
            )),
        }
    }
    if declared.temperature_with_top_p_refused == Some(true)
        && sent.contains(&"temperature")
        && sent.contains(&"top_p")
    {
        refused.push(
            "it declares `api_temperature_with_top_p = \"refused\"`, and this session sends both"
                .to_owned(),
        );
    }
    if refused.is_empty() {
        return Ok(declared.unchecked());
    }
    Err(format!(
        "`{id}`'s API would refuse this session's requests with a 400, as its entry declares \
         from the provider's docs: {}",
        refused.join("; ")
    ))
}

/// Whether two decimals written as text are one number: `1`, `1.0` and
/// `1.00` are; trailing zeros after the point say nothing.
fn same_number(a: &str, b: &str) -> bool {
    fn plain(digits: &str) -> &str {
        if digits.contains('.') {
            digits.trim_end_matches('0').trim_end_matches('.')
        } else {
            digits
        }
    }
    plain(a) == plain(b)
}

/// Whether the betas `options` would send are the ones the entry declares.
///
/// # Errors
///
/// A beta sent and not declared, or declared and not sent, named.
pub fn betas_agree(id: &str, identity: &Identity, options: &Options) -> Result<(), String> {
    let sent: Vec<&str> = options.beta().into_iter().collect();
    let declared: Vec<&str> = identity.engine_betas.iter().map(String::as_str).collect();
    if sent == declared {
        return Ok(());
    }
    Err(format!(
        "`{id}` declares the beta headers [{}] and this session would send [{}]: a beta \
         changes the API's behaviour, so the entry says which it is sent",
        declared.join(", "),
        sent.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::anthropic::INTERLEAVED_THINKING;
    use crate::client::shape::{Pin, SamplerSetting};

    fn entry(thinking: &str, betas: &str) -> Identity {
        let registry = format!(
            "[equipment.api]\nhardware_fingerprint = \"{}\"\n\
             [substrate.h]\nequipment = \"api\"\nengine_name = \"anthropic-messages\"\n\
             engine_identity = \"2023-06-01\"\nserver_kind = \"api\"\n\
             weights_kind = \"hosted\"\nweights_provider = \"p\"\nweights_model_id = \"a-hosted-model\"\n\
             weights_version_or_date_observed = \"d\"\n{thinking}{betas}",
            "a".repeat(64)
        );
        crate::drive::registry::identity(&registry, "h").expect("an entry")
    }

    fn substrate(reasoning: &str) -> Substrate {
        let text = format!(
            "arm = \"a\"\ndogma_version = 0\nsubstrate = \"h\"\n\
             substrate_reasoning = \"{}\"\nsubstrate_hardware = \"{}\"\n{reasoning}\n\
             [sampler]\ntemperature = 1\n",
            if reasoning.is_empty() { "off" } else { "on" },
            "a".repeat(64)
        );
        let parsed = crate::formats::regimen::parse(&text).expect("a regimen");
        crate::drive::regimen::regime_of(&parsed, false)
            .expect("a regime")
            .substrates
            .remove(0)
    }

    #[test]
    fn thinking_follows_the_entrys_convention_and_the_regimens_reasoning() {
        let adaptive = entry("thinking = \"adaptive\"\n", "");
        assert_eq!(
            options(&substrate(""), &adaptive, Ttls::default()),
            Ok(Options::default())
        );
        assert_eq!(
            options(
                &substrate("[reasoning]\neffort = \"medium\"\nbudget_tokens = \"none\"\n"),
                &adaptive,
                Ttls::default()
            ),
            Ok(Options {
                thinking: Thinking::Adaptive {
                    effort: Some("medium".to_owned())
                },
                ttl: Ttls::default(),
            })
        );
        let budget = entry("thinking = \"budget\"\n", "");
        assert_eq!(
            options(
                &substrate("[reasoning]\neffort = \"high\"\nbudget_tokens = 16384\n"),
                &budget,
                Ttls::default()
            ),
            Ok(Options {
                thinking: Thinking::Budget { tokens: 16_384 },
                ttl: Ttls::default(),
            })
        );
        assert!(
            options(
                &substrate("[reasoning]\neffort = \"high\"\nbudget_tokens = \"none\"\n"),
                &budget,
                Ttls::default()
            )
            .is_err(),
            "a budget model with no budget"
        );
        assert!(
            options(
                &substrate("[reasoning]\neffort = \"high\"\nbudget_tokens = \"none\"\n"),
                &entry("", ""),
                Ttls::default()
            )
            .is_err(),
            "no convention declared"
        );
    }

    fn shape() -> RequestShape {
        RequestShape {
            model: "a-hosted-model".to_owned(),
            messages: vec![crate::client::shape::Message::new(
                crate::client::shape::Role::User,
                "hi",
            )],
            sampler: crate::client::shape::SamplerCard::empty(),
            limits: crate::client::shape::Limits {
                attempt: std::time::Duration::from_secs(5),
                call: std::time::Duration::from_secs(5),
                max_output_tokens: 64,
                retries: 0,
                context_window: None,
                idle: None,
            },
            grammar: None,
            template_kwargs: std::collections::BTreeMap::new(),
            tools: Vec::new(),
        }
    }

    /// An entry declaring what its API refuses, as Haiku 5.5's and Sonnet
    /// 5.5's do: `sampler` its `api_<setting>` lines.
    fn declaring(sampler: &str) -> Identity {
        entry(
            &format!(
                "thinking = \"adaptive\"\nserving_context = 1000000\n\
                 max_output_tokens = 128000\nefforts = [\"low\", \"medium\", \"high\"]\n\
                 {sampler}"
            ),
            "",
        )
    }

    fn pinned(pins: &[(SamplerSetting, Pin)]) -> RequestShape {
        let mut shape = shape();
        for (setting, pin) in pins {
            shape.sampler = shape.sampler.with(*setting, pin.clone());
        }
        shape
    }

    fn decimal(digits: &str) -> Pin {
        Pin::Decimal(crate::formats::record::json::Decimal::new(digits).expect("digits"))
    }

    fn at(effort: &str) -> Options {
        Options {
            thinking: Thinking::Adaptive {
                effort: Some(effort.to_owned()),
            },
            ..Options::default()
        }
    }

    const HAIKU: &str = "api_temperature = \"1\"\napi_top_p = \"0.99\"\napi_top_k = \"none\"\n\
                         api_temperature_with_top_p = \"refused\"\n";
    const SONNET: &str = "api_temperature = \"none\"\napi_top_p = \"none\"\napi_top_k = \"none\"\n";

    /// #555: what a rung declares its API would 400 on is refused at start,
    /// each refusal naming the declaration and what would be sent; what it
    /// accepts passes; a value is compared as a number, so `1.0` is `1`.
    #[test]
    fn what_a_rung_declares_its_api_refuses_is_refused_at_start() {
        let haiku = declaring(HAIKU);
        let mut big = shape();
        big.limits.max_output_tokens = 200_000;
        let refused = refused_at_start("h", &haiku, &big, &at("low")).expect_err("over the cap");
        assert!(
            refused.contains("max_output_tokens = 128000") && refused.contains("200000"),
            "{refused}"
        );
        let refused =
            refused_at_start("h", &haiku, &shape(), &at("max")).expect_err("an effort undeclared");
        assert!(
            refused.contains("efforts") && refused.contains("\"max\""),
            "{refused}"
        );
        for (pins, names) in [
            (
                vec![(SamplerSetting::Temperature, decimal("0.6"))],
                "api_temperature = \"1\"",
            ),
            (
                vec![(SamplerSetting::TopP, Pin::Integer(1))],
                "api_top_p = \"0.99\"",
            ),
            (
                vec![(SamplerSetting::TopK, Pin::Integer(20))],
                "api_top_k = \"none\"",
            ),
            (
                vec![
                    (SamplerSetting::Temperature, Pin::Integer(1)),
                    (SamplerSetting::TopP, decimal("0.99")),
                ],
                "api_temperature_with_top_p",
            ),
        ] {
            let refused =
                refused_at_start("h", &haiku, &pinned(&pins), &at("low")).expect_err(names);
            assert!(refused.contains(names), "{names}: {refused}");
        }
        // Every refusal at once, not the first alone.
        let mut both = pinned(&[(SamplerSetting::TopK, Pin::Integer(20))]);
        both.limits.max_output_tokens = 200_000;
        let refused = refused_at_start("h", &haiku, &both, &at("low")).expect_err("two");
        assert!(
            refused.contains("api_top_k") && refused.contains("max_output_tokens"),
            "{refused}"
        );
        for pins in [
            vec![(SamplerSetting::Temperature, decimal("1.0"))],
            vec![(SamplerSetting::TopP, decimal("0.990"))],
            Vec::new(),
        ] {
            assert_eq!(
                refused_at_start("h", &haiku, &pinned(&pins), &at("medium")),
                Ok(Vec::new()),
                "{pins:?}"
            );
        }
        let sonnet = declaring(SONNET);
        for setting in [
            SamplerSetting::Temperature,
            SamplerSetting::TopP,
            SamplerSetting::TopK,
        ] {
            let refused = refused_at_start(
                "h",
                &sonnet,
                &pinned(&[(setting, Pin::Integer(1))]),
                &at("low"),
            )
            .expect_err("none passes");
            assert!(
                refused.contains(&format!("api_{} = \"none\"", setting.tag())),
                "{refused}"
            );
        }
        // Sonnet declares nothing of temperature beside top_p: unchecked.
        assert_eq!(
            refused_at_start("h", &sonnet, &shape(), &at("high")),
            Ok(vec!["api_temperature_with_top_p"])
        );
        // Thinking off sends no effort, so none is checked.
        assert_eq!(
            refused_at_start("h", &haiku, &shape(), &Options::default()),
            Ok(Vec::new())
        );
    }

    /// #555: an `api` entry's requests go as the model it names: another
    /// `--model` is refused, naming both, and an entry naming none is
    /// refused rather than passed -- a paid rung names what it bills.
    #[test]
    fn a_hosted_session_sends_the_model_its_entry_names_or_nothing() {
        let identity = entry("thinking = \"adaptive\"\n", "");
        let mut asked = shape();
        assert_eq!(model_named(&identity, "h", &asked), Ok(()));
        asked.model = "claude-opus-5-5".to_owned();
        let refused = model_named(&identity, "h", &asked).expect_err("another model");
        assert!(
            refused.contains("claude-opus-5-5")
                && refused.contains("`weights_model_id = \"a-hosted-model\"`"),
            "{refused}"
        );
        let canned = crate::drive::registry::identity(
            &format!(
                "[equipment.api]\nhardware_fingerprint = \"{}\"\n\
                 [substrate.c]\nequipment = \"api\"\nengine_name = \"e\"\n\
                 engine_identity = \"2023-06-01\"\nserver_kind = \"api\"\n\
                 weights_kind = \"canned\"\nweights_acts_sha256 = \"{}\"\n",
                "a".repeat(64),
                "b".repeat(64)
            ),
            "c",
        )
        .expect("a canned api entry");
        let refused = model_named(&canned, "c", &asked).expect_err("no model named");
        assert!(refused.contains("names no `weights_model_id`"), "{refused}");
    }

    /// #555: an entry that declares nothing of what its API refuses is
    /// checked for nothing, and says which keys it left out -- every one.
    #[test]
    fn an_entry_declaring_nothing_is_unchecked_and_says_so() {
        let silent = entry("thinking = \"adaptive\"\n", "");
        let mut big = pinned(&[(SamplerSetting::TopK, Pin::Integer(20))]);
        big.limits.max_output_tokens = 900_000;
        assert_eq!(
            refused_at_start("h", &silent, &big, &at("max")),
            Ok(crate::drive::registry::API_DECLARABLE.to_vec())
        );
    }

    /// #555: the key comes from the file, else the harnesses' variable --
    /// never one the regimen hands the model's commands -- and the request
    /// that goes out is the Messages API's, its key as `x-api-key` and its
    /// thinking the entry's convention at the regimen's effort.
    #[test]
    fn a_hosted_transport_takes_its_key_and_speaks_the_messages_api() {
        use crate::client::anthropic::tests::{answering, streamed};
        use crate::client::stream::{Cancel, Streaming as _};
        use crate::client::stub::Stub;
        use crate::client::transport::Endpoint;

        let identity = entry(
            "thinking = \"adaptive\"\n",
            &format!("engine_betas = \"{INTERLEAVED_THINKING}\"\n"),
        );
        let on = substrate("[reasoning]\neffort = \"low\"\nbudget_tokens = \"none\"\n");
        let plain =
            || HttpStream::new(Endpoint::parse("http://127.0.0.1:9/v1").expect("an endpoint"));
        for (key, refused) in [
            (Key::Environment(None, false), "neither --key-file nor"),
            (
                Key::Environment(Some("sk-x".to_owned()), true),
                "passes ANTHROPIC_API_KEY on to the model's commands",
            ),
        ] {
            let why = transport(plain(), (&identity, &on), (key, Ttls::default()), &shape())
                .expect_err("refused");
            assert!(why.contains(refused), "{why}");
        }

        let stub = Stub::serving(vec![streamed(&answering("hello", (5, 0, 0)))]).expect("loopback");
        let hosted = transport(
            HttpStream::new(Endpoint::parse(&stub.url()).expect("an endpoint")),
            (&identity, &on),
            (
                Key::Environment(Some("sk-from-env".to_owned()), false),
                Ttls::default(),
            ),
            &shape(),
        )
        .expect("a hosted transport")
        .0;
        let mut text = String::new();
        hosted
            .stream(
                &shape(),
                std::time::Instant::now() + std::time::Duration::from_secs(10),
                &Cancel::new(),
                &mut |piece| {
                    if let crate::client::stream::Piece::Text(piece) = piece {
                        text.push_str(piece);
                    }
                },
            )
            .expect("an answer");
        assert_eq!(text, "hello");
        let head = stub.heads()[0].to_ascii_lowercase();
        assert!(head.contains("x-api-key: sk-from-env\r\n"), "{head}");
        let body: serde_json::Value =
            serde_json::from_str(&stub.received()[0]).expect("a JSON body");
        assert_eq!(
            (&body["thinking"]["type"], &body["output_config"]["effort"]),
            (&serde_json::json!("adaptive"), &serde_json::json!("low"))
        );
    }

    /// #556: the lever reads `5m`, `1h` and `per-breakpoint` (Qwen Code's
    /// shape: system and tools for an hour, the tail for five minutes);
    /// absent is five minutes, and any other word is refused.
    #[test]
    fn the_cache_ttl_lever_reads_its_three_words() {
        let read =
            |text: &str| cache_ttl(&crate::formats::regimen::parse(text).expect("a regimen"));
        assert_eq!(read("arm = \"a\"\n"), Ok(Ttls::default()));
        assert_eq!(
            read("cache_ttl = \"5m\"\n"),
            Ok(Ttls::all(Ttl::FiveMinutes))
        );
        assert_eq!(read("cache_ttl = \"1h\"\n"), Ok(Ttls::all(Ttl::Hour)));
        assert_eq!(
            read("cache_ttl = \"per-breakpoint\"\n"),
            Ok(Ttls {
                tools: Ttl::Hour,
                system: Ttl::Hour,
                tail: Ttl::FiveMinutes
            })
        );
        assert!(read("cache_ttl = \"10m\"\n").is_err());
    }

    #[test]
    fn the_betas_sent_are_the_betas_declared() {
        let thinking = Options {
            thinking: Thinking::Adaptive { effort: None },
            ttl: Ttls::default(),
        };
        let declaring = entry("", &format!("engine_betas = \"{INTERLEAVED_THINKING}\"\n"));
        assert_eq!(betas_agree("h", &declaring, &thinking), Ok(()));
        assert!(betas_agree("h", &declaring, &Options::default()).is_err());
        let silent = entry("", "");
        assert_eq!(betas_agree("h", &silent, &Options::default()), Ok(()));
        let refused = betas_agree("h", &silent, &thinking).expect_err("undeclared");
        assert!(refused.contains(INTERLEAVED_THINKING), "{refused}");
    }
}
