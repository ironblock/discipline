//! What `serve` confirms of a server at start (#509): the maintainer's
//! four, in his words -- "Up, reachable", "Having the right model loaded",
//! "Warmed up", "Running the settings ... we require (if other than
//! default)" -- and nothing else.
//!
//! The registry declares the served configuration as `served_<field>` keys,
//! by the engine's own name for each field. Per field (the duty-of-care
//! rule): the engine reports it and agrees, and it is *corroborated*; the
//! engine is silent, and the declaration stands, *declared*; the engine
//! reports it and disagrees, and `serve` refuses, naming the field, the
//! declared value and the reported one.
//!
//! What each engine reports, read from its source at the commit #509 pins:
//!
//! * **llama.cpp `448147d`**, `GET /props` (`tools/server/server-context.cpp`,
//!   `get_res_props`): `model_alias` (the first `--alias`, else the model's
//!   name), `default_generation_settings.n_ctx` (per slot), `total_slots`,
//!   `modalities.{vision,audio}`, and `chat_template`, compared by digest
//!   against the entry's `chat_template_sha256`.
//! * **`TabbyAPI` `be74bf0`**, `GET /v1/model` (`endpoints/core/router.py:128`;
//!   `endpoints/core/types/model.py:11-54`): `id` (the model directory's
//!   name) and, under `parameters`, `max_seq_len`, `cache_size`,
//!   `cache_mode`, `max_batch_size`, `chunk_size` and `use_vision`. Never
//!   `rope_scale` or `rope_alpha`, which always read their defaults, nor
//!   `draft`, which `model_info()` never fills.
//!
//! **Speculative decoding** is reported by neither listing. Both engines put
//! `draft_n` in a response's `timings` when a draft produced tokens, and
//! leave it out otherwise (llama.cpp `server-common.cpp`
//! `server_slot_stats::to_json`; `TabbyAPI` `endpoints/OAI/utils/common_.py:76-81`).
//! So a declared `served_draft` is corroborated by the probe request's
//! timings, and only draft tokens are a report: a probe the draft produced
//! nothing for is silence, and the declaration stands.
//!
//! **Warmed:** unless the entry declares `served_warmup = "true"` -- the
//! engine warms itself, which neither engine reports -- `serve` sends one
//! tiny request before it announces itself; a declared draft is probed the
//! same way. **The sampler** is pinned on every request (#489), so it is
//! confirmed by construction and is not a field here.
//!
//! **The server's kind** is explicit: `local`, a server we run, is the one
//! built. An `api` -- a server we neither control nor see into -- is to
//! come; under it every field stands declared and nothing is probed.

use std::time::Instant;

use crate::client::shape::{Message, RequestShape, Role};
use crate::client::stream::{Cancel, Ended, Streaming, Timings};
use crate::client::transport::{HttpReply, TransportFailure};
use crate::formats::log::{FieldProvenance, ServedField};

use super::registry::Identity;

/// The engine a substrate's server runs, as far as what it reports goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// llama.cpp's server, and anything answering `/props` in its shape.
    LlamaCpp,
    /// `TabbyAPI`.
    TabbyApi,
}

impl Engine {
    /// The engine the registry's entry names, by its `dialect` (#496).
    #[must_use]
    pub fn of(identity: &Identity) -> Self {
        match identity.dialect.as_deref() {
            Some("tabbyapi") => Self::TabbyApi,
            _ => Self::LlamaCpp,
        }
    }

    /// Where the engine reports its served configuration.
    #[must_use]
    pub fn report_path(self) -> &'static str {
        match self {
            Self::LlamaCpp => "/props",
            Self::TabbyApi => "/v1/model",
        }
    }
}

/// What kind of server answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerKind {
    /// One we run and can reason about.
    Local,
    /// One we neither control nor see into: every field declared, nothing
    /// probed. Not built beyond that.
    Api,
}

impl ServerKind {
    /// The entry's `server_kind`: `local` unless it says otherwise.
    ///
    /// # Errors
    ///
    /// A value other than `local` or `api`.
    pub fn of(id: &str, identity: &Identity) -> Result<Self, String> {
        match identity.server_kind.as_deref() {
            None | Some("local") => Ok(Self::Local),
            Some("api") => Ok(Self::Api),
            Some(other) => Err(format!(
                "the registry's `server_kind` for `{id}` is \"{other}\"; it takes \"local\" or \
                 \"api\": the entry is malformed"
            )),
        }
    }
}

/// The declared key under which the engine warms itself.
const WARMUP: &str = "warmup";

/// The declared key for a draft model.
const DRAFT: &str = "draft";

/// The engine's report on `field`, as text, when it makes one.
fn reported(engine: Engine, field: &str, report: &serde_json::Value) -> Option<String> {
    let at = match (engine, field) {
        (Engine::LlamaCpp, "model") => report.get("model_alias"),
        (Engine::LlamaCpp, "n_ctx") => report
            .get("default_generation_settings")
            .and_then(|settings| settings.get("n_ctx")),
        (Engine::LlamaCpp, "total_slots") => report.get("total_slots"),
        (Engine::LlamaCpp, "vision" | "audio") => report
            .get("modalities")
            .and_then(|modalities| modalities.get(field)),
        (Engine::TabbyApi, "model") => report.get("id"),
        (
            Engine::TabbyApi,
            "max_seq_len" | "cache_size" | "cache_mode" | "max_batch_size" | "chunk_size"
            | "use_vision",
        ) => report
            .get("parameters")
            .and_then(|parameters| parameters.get(field)),
        _ => None,
    }?;
    match at {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

/// One field, compared: corroborated, declared, or the refusal.
fn compared(
    id: &str,
    key: String,
    declared: &str,
    reported: Option<String>,
) -> Result<ServedField, String> {
    match reported {
        None => Ok(ServedField {
            field: key,
            value: declared.to_owned(),
            provenance: FieldProvenance::Declared,
            reported: None,
        }),
        Some(reported) if reported == declared => Ok(ServedField {
            field: key,
            value: declared.to_owned(),
            provenance: FieldProvenance::Corroborated,
            reported: Some(reported),
        }),
        Some(reported) => Err(format!(
            "the server contradicts `{id}`'s `{key}`: the registry declares \"{declared}\", the \
             server reports \"{reported}\""
        )),
    }
}

/// Each declared field compared against `report` -- the engine's report on
/// itself, or `None` when it made none -- in the registry's key order; the
/// chat template's digest among them where the engine reports the template.
///
/// # Errors
///
/// The first contradiction, naming the field and both values.
pub fn corroborated(
    id: &str,
    identity: &Identity,
    report: Option<&serde_json::Value>,
) -> Result<Vec<ServedField>, String> {
    let engine = Engine::of(identity);
    let mut fields = Vec::new();
    for (field, declared) in &identity.served {
        if field == WARMUP || field == DRAFT {
            continue;
        }
        let said = report.and_then(|report| reported(engine, field, report));
        fields.push(compared(id, format!("served_{field}"), declared, said)?);
    }
    if let Some(declared) = &identity.chat_template_sha256 {
        let said = report
            .filter(|_| engine == Engine::LlamaCpp)
            .and_then(|report| report.get("chat_template"))
            .and_then(serde_json::Value::as_str)
            .map(|template| crate::digest::sha256_hex(template.as_bytes()));
        fields.push(compared(
            id,
            "chat_template_sha256".to_owned(),
            declared,
            said,
        )?);
    }
    Ok(fields)
}

/// Whether the entry declares that the engine warms itself.
#[must_use]
pub fn warms_itself(identity: &Identity) -> bool {
    identity.served.get(WARMUP).map(String::as_str) == Some("true")
}

/// The declared draft, against the probe request's `timings`. Only draft
/// tokens are a report: `draft_n` above zero says a draft ran. A probe with
/// none -- no timings, or a short answer the draft produced nothing for --
/// is silence, and the declaration stands.
///
/// # Errors
///
/// A contradiction: draft tokens reported where the entry declares no draft.
pub fn draft_corroborated(
    id: &str,
    identity: &Identity,
    timings: Option<&Timings>,
) -> Result<Option<ServedField>, String> {
    let Some(declared) = identity.served.get(DRAFT) else {
        return Ok(None);
    };
    let drafted = timings.filter(|timings| timings.draft_n.is_some_and(|drafted| drafted > 0));
    let field = compared(
        id,
        format!("served_{DRAFT}"),
        declared,
        drafted.map(|_| "true".to_owned()),
    )?;
    Ok(Some(ServedField {
        reported: field.reported.as_ref().and(drafted).map(|timings| {
            format!(
                "draft_n {}, draft_n_accepted {}",
                timings.draft_n.unwrap_or(0),
                timings.draft_n_accepted.unwrap_or(0)
            )
        }),
        ..field
    }))
}

/// The probe: one tiny request on the session's own shape -- its head, its
/// sampler -- that warms the server and, where a draft is declared, shows
/// whether one ran.
#[must_use]
pub fn probe_shape(shape: &RequestShape) -> RequestShape {
    let mut probe = shape.clone();
    probe.tools.clear();
    probe.messages.push(Message::new(
        Role::User,
        "Count from one to twenty, in words, one per line.",
    ));
    probe.limits.max_output_tokens = 64;
    probe
}

/// Send the probe through `transport` and return its timings.
///
/// # Errors
///
/// The server did not answer, or refused the request.
pub fn probe(transport: &impl Streaming, shape: &RequestShape) -> Result<Option<Timings>, String> {
    let probe = probe_shape(shape);
    let deadline = Instant::now() + probe.limits.call;
    match transport.stream(&probe, deadline, &Cancel::new(), &mut |_| {}) {
        Ok(Ended::Finished { timings, .. }) => Ok(timings),
        Ok(Ended::Rejected { status, body, .. }) => Err(format!(
            "the warmup request was refused, {status}: {}",
            body.chars().take(200).collect::<String>()
        )),
        Ok(Ended::Cancelled) => Err("the warmup request was cancelled".to_owned()),
        Err(why) => Err(format!("the warmup request had no answer: {why}")),
    }
}

/// The engine's report on itself, parsed, from the reply to its
/// [`Engine::report_path`]: `None` for a reply that is not `200` and JSON,
/// which reports nothing.
///
/// # Errors
///
/// No reply at all: the server is not reachable.
pub fn report_of(
    path: &str,
    reply: Result<HttpReply, TransportFailure>,
) -> Result<Option<serde_json::Value>, String> {
    let reply = reply
        .map_err(|why| format!("the server is not reachable: `GET {path}` had no reply: {why}"))?;
    Ok((reply.status == 200)
        .then(|| serde_json::from_str(&reply.body).ok())
        .flatten())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drive::registry;

    /// A registry holding one substrate `s`, with `extra` lines in its table.
    fn identity(extra: &str) -> Identity {
        let document = format!(
            "[equipment.e]\nhardware_fingerprint = \"{}\"\n\
             [substrate.s]\nequipment = \"e\"\nengine_name = \"n\"\n\
             engine_identity = \"i\"\nweights_main = \"{}\"\n{extra}",
            "a".repeat(64),
            "b".repeat(64)
        );
        registry::identity(&document, "s").expect("registered")
    }

    fn props(template: &str) -> serde_json::Value {
        serde_json::json!({
            "default_generation_settings": {"params": {}, "n_ctx": 32_768},
            "total_slots": 4,
            "model_alias": "qwen38-27b",
            "model_path": "/models/qwen38-27b-iq3s.gguf",
            "modalities": {"vision": true, "video": false, "audio": false},
            "chat_template": template,
            "build_info": "b1-448147d",
        })
    }

    fn provenance(fields: &[ServedField]) -> Vec<(&str, FieldProvenance)> {
        fields
            .iter()
            .map(|field| (field.field.as_str(), field.provenance))
            .collect()
    }

    /// llama.cpp's `/props`, at `448147d`: each declared field it reports is
    /// corroborated, the chat template by digest, and a field it does not
    /// report (the KV cache type) stands declared.
    #[test]
    fn llama_cpp_props_corroborate_the_model_context_slots_modalities_and_template() {
        let template = "{{ messages }}";
        let declared = identity(&format!(
            "served_model = \"qwen38-27b\"\nserved_n_ctx = \"32768\"\nserved_total_slots = \"4\"\n\
             served_vision = \"true\"\nserved_audio = \"false\"\nserved_cache_type_k = \"q8_0\"\n\
             chat_template_sha256 = \"{}\"\n",
            crate::digest::sha256_hex(template.as_bytes())
        ));
        let fields = corroborated("s", &declared, Some(&props(template))).expect("agrees");
        assert_eq!(
            provenance(&fields),
            [
                ("served_audio", FieldProvenance::Corroborated),
                ("served_cache_type_k", FieldProvenance::Declared),
                ("served_model", FieldProvenance::Corroborated),
                ("served_n_ctx", FieldProvenance::Corroborated),
                ("served_total_slots", FieldProvenance::Corroborated),
                ("served_vision", FieldProvenance::Corroborated),
                ("chat_template_sha256", FieldProvenance::Corroborated),
            ]
        );
    }

    /// Each field the engine reports is compared: a contradiction on any of
    /// them refuses, naming the field and both values.
    #[test]
    fn a_llama_cpp_contradiction_on_any_reported_field_refuses_naming_it() {
        for (line, field, reported) in [
            ("served_model = \"another\"", "served_model", "qwen38-27b"),
            ("served_n_ctx = \"65536\"", "served_n_ctx", "32768"),
            ("served_total_slots = \"1\"", "served_total_slots", "4"),
            ("served_vision = \"false\"", "served_vision", "true"),
            (
                "chat_template_sha256 = \"0000000000000000000000000000000000000000000000000000000000000000\"",
                "chat_template_sha256",
                "",
            ),
        ] {
            let refused = corroborated("s", &identity(&format!("{line}\n")), Some(&props("t")))
                .expect_err(line);
            assert!(
                refused.contains(&format!("`{field}`")) && refused.contains(reported),
                "{refused}"
            );
        }
    }

    /// `TabbyAPI`'s `/v1/model` never corroborates `rope_scale` or
    /// `rope_alpha`, which always read their defaults; nor does it report a
    /// llama.cpp field.
    #[test]
    fn tabbyapi_never_corroborates_rope_and_reports_only_its_own_fields() {
        let declared = identity(
            "dialect = \"tabbyapi\"\nserved_rope_scale = \"2.0\"\nserved_n_ctx = \"1\"\n\
             served_max_seq_len = \"4096\"\n",
        );
        let card = serde_json::json!({
            "id": "m",
            "parameters": {"max_seq_len": 4096, "rope_scale": 1.0, "rope_alpha": 1.0},
        });
        let fields = corroborated("s", &declared, Some(&card)).expect("no contradiction");
        assert_eq!(
            provenance(&fields),
            [
                ("served_max_seq_len", FieldProvenance::Corroborated),
                ("served_n_ctx", FieldProvenance::Declared),
                ("served_rope_scale", FieldProvenance::Declared),
            ]
        );
    }

    /// An engine that reported nothing leaves every field declared.
    #[test]
    fn a_silent_engine_leaves_every_field_declared() {
        let declared = identity("served_model = \"m\"\nserved_n_ctx = \"8\"\n");
        let fields = corroborated("s", &declared, None).expect("declared");
        assert!(
            fields
                .iter()
                .all(|field| field.provenance == FieldProvenance::Declared
                    && field.reported.is_none()),
            "{fields:?}"
        );
    }

    /// The draft against the probe's timings: `draft_n` corroborates a
    /// declared draft; timings without it, or none, are silence and leave it
    /// declared; a draft declared absent but reported running contradicts.
    #[test]
    fn the_probes_timings_corroborate_or_contradict_a_declared_draft() {
        let ran = Timings {
            draft_n: Some(72),
            draft_n_accepted: Some(44),
            ..Timings::default()
        };
        let none = Timings::default();
        let drafted = identity("served_draft = \"true\"\n");
        let field = draft_corroborated("s", &drafted, Some(&ran))
            .expect("agrees")
            .expect("a draft is declared");
        assert_eq!(field.provenance, FieldProvenance::Corroborated);
        assert_eq!(
            field.reported.as_deref(),
            Some("draft_n 72, draft_n_accepted 44")
        );
        assert_eq!(
            draft_corroborated("s", &drafted, Some(&none))
                .expect("no draft tokens are silence")
                .map(|field| field.provenance),
            Some(FieldProvenance::Declared)
        );
        assert_eq!(
            draft_corroborated("s", &drafted, None)
                .expect("silent")
                .map(|field| field.provenance),
            Some(FieldProvenance::Declared)
        );
        let undrafted = identity("served_draft = \"false\"\n");
        assert!(draft_corroborated("s", &undrafted, Some(&ran)).is_err());
        assert_eq!(draft_corroborated("s", &identity(""), Some(&ran)), Ok(None));
    }

    /// The server's kind is explicit: `local` by default, `api` declared, and
    /// anything else a malformed entry.
    #[test]
    fn the_server_kind_is_local_unless_declared_and_never_a_stray_word() {
        assert_eq!(ServerKind::of("s", &identity("")), Ok(ServerKind::Local));
        assert_eq!(
            ServerKind::of("s", &identity("server_kind = \"api\"\n")),
            Ok(ServerKind::Api)
        );
        assert!(ServerKind::of("s", &identity("server_kind = \"cloud\"\n")).is_err());
        assert!(warms_itself(&identity("served_warmup = \"true\"\n")));
        assert!(!warms_itself(&identity("")));
    }

    /// The probe is the session's own shape, its tools cleared, one short
    /// ask added, and a small cap.
    #[test]
    fn the_probe_is_the_sessions_shape_with_one_short_ask() {
        let shape = crate::drive::session::tests::template();
        let probe = probe_shape(&shape);
        assert_eq!(probe.messages.len(), shape.messages.len() + 1);
        assert_eq!(probe.messages[..shape.messages.len()], shape.messages[..]);
        assert_eq!(probe.sampler, shape.sampler);
        assert_eq!(probe.limits.max_output_tokens, 64);
        assert!(probe.tools.is_empty());
    }
}
