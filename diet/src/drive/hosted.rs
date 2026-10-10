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
/// the regime cannot meet; undeclared betas; a shape the API cannot carry.
pub fn transport(
    transport: HttpStream,
    (identity, substrate): (&Identity, &Substrate),
    (key, ttl): (Key, Ttls),
    shape: &RequestShape,
) -> Result<HttpStream, String> {
    let id = &substrate.id;
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
    Ok(transport.with_anthropic(options))
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

    fn entry(thinking: &str, betas: &str) -> Identity {
        let registry = format!(
            "[equipment.api]\nhardware_fingerprint = \"{}\"\n\
             [substrate.h]\nequipment = \"api\"\nengine_name = \"anthropic-messages\"\n\
             engine_identity = \"2023-06-01\"\nserver_kind = \"api\"\n\
             weights_kind = \"hosted\"\nweights_provider = \"p\"\nweights_model_id = \"m\"\n\
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
            },
            grammar: None,
            template_kwargs: std::collections::BTreeMap::new(),
            tools: Vec::new(),
        }
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
        .expect("a hosted transport");
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
