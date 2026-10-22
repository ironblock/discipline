//! A scenario (#760): a drive written as steps and expectations, run on the
//! canned substrate or re-fired on a named endpoint.
//!
//! The file is JSON Lines in the record's value space, read with the
//! record's own reader (`diet/scenarios/README.md` is its note). Its
//! first line names the scenario, its two regimens and its head; every other
//! line is one `reply` the canned substrate answers with, one `classify`
//! rule its canned classifier scores by, one `step` the runner drives, or one
//! `expect` the log must meet.

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};

use crate::formats::record::json::{self, Value};

/// A scenario, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scenario {
    /// Its name: what a run of it is labelled with.
    pub name: String,
    /// The regimen it runs under on the canned substrate, relative to the
    /// file.
    pub regimen: String,
    /// The regimen a live re-fire runs under, relative to the file.
    pub live_regimen: Option<String>,
    /// The trunk's head, relative to the file.
    pub head: String,
    /// How a prompt waiting on the operator is answered: a `POST /approve`
    /// scope. Absent, a prompt fails the run.
    pub approvals: Option<String>,
    /// Files the runner copies into the fresh worktree before serve starts:
    /// each path in the worktree, and its source beside the scenario.
    pub seed: BTreeMap<String, String>,
    /// The asks, by their place among the asks, that are fixed (#908): an
    /// operator script lists its first; every later ask was typed in answer
    /// to what the model did, unless a person who knows it was written in
    /// advance adds it. Empty for a scenario written by hand.
    pub fixed_asks: Vec<u64>,
    /// The canned substrate's replies, in the file's order.
    pub replies: Vec<Reply>,
    /// Its canned classifier's rules, in the file's order (#862).
    pub classified: Vec<Classify>,
    /// What the runner drives, in order.
    pub steps: Vec<Step>,
    /// How long a settling step may take, in seconds, by its place in
    /// `steps`; a step absent here takes [`STEP_PATIENCE`].
    pub patience: BTreeMap<usize, u64>,
    /// What the log must show.
    pub expects: Vec<Expect>,
    /// The ports reserved for each run, by name (#942).
    pub ports: Vec<String>,
    /// What each run starts beside serve, outside the sandbox (#942).
    pub services: Vec<crate::drive::services::Service>,
    /// The Unix sockets the model's commands may connect to, each a
    /// service's value, `service.NAME.KEY` (#942): serve's
    /// `--sandbox-unix-socket`.
    pub sandbox_unix_sockets: Vec<(String, String)>,
    /// Variables added to serve's environment alone, each a text filled
    /// from the run's ports and values (#942): what a command sees of them
    /// is still the regimen's `env_passthrough`.
    pub serve_env: BTreeMap<String, String>,
    /// What a sealed session is denied besides the deny list (#948,
    /// `refill_session = "sealed"`): services' values, `service.NAME.KEY`.
    pub seal: Vec<(String, String)>,
    /// Files a sealed session's probe must fail to read (#948): texts filled
    /// from the run's ports and values.
    pub seal_probe: Vec<String>,
}

/// One canned reply: the text a request's last message must hold, or none
/// for a reply taken in order; what it says; and the calls it makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// Text the request's last message holds.
    pub when: Option<String>,
    /// The answer's text.
    pub text: String,
    /// The calls it makes.
    pub calls: Vec<Call>,
    /// How long it holds its answer back, in milliseconds (#782): what
    /// orders a canned trunk's answer after a fork beside it.
    pub delay_ms: Option<u64>,
    /// The prompt its usage reports, in tokens (#798); absent, the canned
    /// server's own count.
    pub prompt_tokens: Option<u64>,
    /// What it thinks before it answers, signed (#555): a hosted substrate's
    /// thinking block, replayed on the next request as the API requires.
    pub thinking: Option<String>,
    /// The prompt tokens its usage reports read from the cache (#555), of
    /// `prompt_tokens`: a hosted substrate's `cache_read_input_tokens`.
    pub cache_read_tokens: Option<u64>,
    /// The prompt tokens its usage reports written to the cache, at the
    /// 5-minute lifetime (#555), of `prompt_tokens`.
    pub cache_write_tokens: Option<u64>,
    /// An error in place of an answer (#555): the status, and the seconds its
    /// `retry-after` header names -- what a hosted API's 429 or 529 says.
    pub status: Option<(u16, Option<u64>)>,
}

impl Reply {
    /// Whether it says what only a hosted substrate's wire carries (#555):
    /// thinking, cache counts, or an error status.
    #[must_use]
    pub fn is_hosted_only(&self) -> bool {
        self.thinking.is_some()
            || self.cache_read_tokens.is_some()
            || self.cache_write_tokens.is_some()
            || self.status.is_some()
    }
}

/// One rule of the canned classifier (#862): a window holding `when` scores
/// a hypothesis holding `hypothesis` at `score`, a decimal from 0 to 1 as
/// written. A pair no rule names scores 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classify {
    /// Text the window holds.
    pub when: String,
    /// Text the hypothesis holds.
    pub hypothesis: String,
    /// The score, as written.
    pub score: String,
}

/// One call a reply makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// Its id.
    pub id: String,
    /// The tool.
    pub name: String,
    /// Its arguments, as JSON text.
    pub arguments: String,
}

/// Which live entry a step acts on, read from `GET /memory`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// The live entry at this place, in id order.
    Index(usize),
    /// The first live entry whose content holds this text.
    Containing(String),
}

/// One thing the runner does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// An ask; `refused` names the refusal it must meet instead.
    Ask {
        /// What is asked.
        text: String,
        /// The operator's scoping mark.
        scoping: bool,
        /// The refusal it must be answered with, when it must be refused.
        refused: Option<String>,
        /// Worktree files uploaded through `POST /files` and attached to
        /// the ask by their digests (#372).
        attach: Vec<String>,
        /// The task it asks (#942): its own `task`, or the `grade` step's
        /// after it.
        task: Option<String>,
    },
    /// The turn just settled, graded by the operator's check, its verdict
    /// sent as the next ask until it is right or its attempts run out
    /// (#942).
    Grade(Box<Grading>),
    /// The operator picks the phase the session starts in, before the first
    /// ask (#788).
    StartPhase {
        /// The phase.
        phase: String,
    },
    /// The operator proposes a phase move for the model to accept or
    /// decline (#788): `meditate`, absent, is serve's default.
    ProposePhase {
        /// The phase proposed.
        to: String,
        /// Whether an accepted move goes through a meditation.
        meditate: Option<bool>,
    },
    /// The operator moves the phase alone (#788).
    MovePhase {
        /// The phase moved to.
        to: String,
        /// Whether the move goes through a meditation.
        meditate: Option<bool>,
    },
    /// The operator opens a tangent (#22).
    OpenTangent {
        /// Its id.
        id: String,
    },
    /// The operator closes the open tangent, ruling `each` -- `keep`,
    /// `drop` or `park` -- on every live entry born under it.
    CloseTangent {
        /// The ruling on each entry.
        each: String,
    },
    /// The operator flags an entry, bound to `bind` when named.
    Flag {
        /// The entry.
        pick: Pick,
        /// The flag's note.
        note: String,
        /// The name it is bound to.
        bind: Option<String>,
    },
    /// The operator edits an entry; the operator's new entry is bound.
    Edit {
        /// The entry.
        pick: Pick,
        /// The new text.
        text: String,
        /// The name the operator's entry is bound to.
        bind: Option<String>,
    },
    /// The operator declares a meditation.
    DeclareMeditation,
    /// The operator refills a held meditation anyway.
    RefillAnyway,
    /// The operator rules on the model's phase proposal (#787, #788): the
    /// proposal's call, and `meditation`, `keep` (read too as `continue`)
    /// or `stay`, posted as written.
    RulePhase {
        /// The `propose_phase_transition` call.
        call: String,
        /// The ruling.
        choice: String,
    },
    /// Wait until the session is idle or a line of this kind is logged.
    Wait {
        /// `idle`, or a log line's kind.
        until: String,
        /// How long to wait, in seconds.
        timeout: Option<u64>,
        /// The fields the line must have, as an expectation's `where`
        /// reads them: a line of the kind that does not match is waited
        /// past. Empty for `idle`, and for the first line of the kind.
        matches: Vec<(String, Match)>,
    },
    /// End the session.
    End,
}

/// A `grade` step (#942).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grading {
    /// The task graded.
    pub task: String,
    /// The check: a command the runner runs in the worktree, printing
    /// `{"right", "expected", "got"}`.
    pub check: String,
    /// How many attempts the task is given, the first the `ask`'s.
    pub attempts: u64,
    /// The ask a wrong attempt with attempts left sends.
    pub wrong: String,
    /// The ask a wrong last attempt sends, when given.
    pub last: Option<String>,
    /// The ask a right attempt sends, when given.
    pub right: Option<String>,
    /// A command run before each attempt's ask, its stdout handed to the
    /// check, when given.
    pub before: Option<String>,
    /// How long the check, or `before`, may run.
    pub timeout: std::time::Duration,
}

/// A grade step's attempts unless it names its own.
pub const GRADE_ATTEMPTS: u64 = 3;

/// How long a check may run unless its step names its own `timeout_s`.
pub const GRADE_TIMEOUT_S: u64 = 30;

impl Scenario {
    /// How long step `at` may take to settle: its `patience`, or
    /// [`STEP_PATIENCE`].
    #[must_use]
    pub fn patience(&self, at: usize) -> std::time::Duration {
        self.patience.get(&at).map_or(STEP_PATIENCE, |seconds| {
            std::time::Duration::from_secs(*seconds)
        })
    }

    /// Every text a run fills from its services (#942): each ask's, and
    /// each grade's feedback.
    fn filled_texts(&mut self) -> Vec<&mut String> {
        let mut texts = Vec::new();
        for step in &mut self.steps {
            match step {
                Step::Ask { text, .. } => texts.push(text),
                Step::Grade(grading) => {
                    texts.push(&mut grading.wrong);
                    texts.extend(grading.last.as_mut());
                    texts.extend(grading.right.as_mut());
                }
                _ => {}
            }
        }
        texts
    }

    /// Whether every placeholder names a declared port or service (#942).
    fn placeholders_are_declared(&mut self) -> Result<(), ScenarioError> {
        let serve_env: Vec<String> = self
            .serve_env
            .values()
            .chain(&self.seal_probe)
            .cloned()
            .collect();
        let (ports, services) = (
            self.ports.clone(),
            self.services
                .iter()
                .map(|service| service.name.clone())
                .collect::<Vec<_>>(),
        );
        let mut texts: Vec<&str> = serve_env.iter().map(String::as_str).collect();
        let filled: Vec<String> = self
            .filled_texts()
            .into_iter()
            .map(|text| text.clone())
            .collect();
        texts.extend(filled.iter().map(String::as_str));
        for text in texts {
            for place in crate::drive::services::placeholders(text) {
                let declared = match place.as_slice() {
                    ["port", key] => ports.iter().any(|port| port == key),
                    ["service", name, _] => services.iter().any(|service| service == name),
                    _ => false,
                };
                if !declared {
                    return Err(ScenarioError::Syntax(format!(
                        "`{{{}}}` names a port or a service the header does not declare",
                        place.join(".")
                    )));
                }
            }
        }
        Ok(())
    }

    /// This scenario with every placeholder filled from `running` (#942).
    ///
    /// # Errors
    ///
    /// When a placeholder names a value no service gave.
    pub fn filled(&self, running: &crate::drive::services::Running) -> Result<Self, String> {
        let mut filled = self.clone();
        for text in filled.filled_texts() {
            *text = running.filled(text)?;
        }
        Ok(filled)
    }
}

impl Step {
    /// Whether the runner waits for the session to settle after this step:
    /// an ask that is to be admitted, and each step that is one command.
    fn settles(&self) -> bool {
        match self {
            Self::Ask { refused, .. } => refused.is_none(),
            Self::Grade(_)
            | Self::DeclareMeditation
            | Self::RefillAnyway
            | Self::RulePhase { .. }
            | Self::StartPhase { .. }
            | Self::ProposePhase { .. }
            | Self::MovePhase { .. } => true,
            Self::OpenTangent { .. }
            | Self::CloseTangent { .. }
            | Self::Flag { .. }
            | Self::Edit { .. }
            | Self::Wait { .. }
            | Self::End => false,
        }
    }

    /// The `POST /commands` body of a step that is one command and waits
    /// for the session to settle after it.
    fn command(&self) -> serde_json::Value {
        let move_to = |kind: &str, to: &str, meditate: Option<bool>| {
            let mut command = serde_json::json!({"kind": kind, "to": to});
            if let Some(meditate) = meditate {
                command["meditate"] = serde_json::Value::Bool(meditate);
            }
            command
        };
        match self {
            Self::DeclareMeditation => serde_json::json!({"kind": "declare-meditation"}),
            Self::RefillAnyway => serde_json::json!({"kind": "refill-anyway"}),
            Self::RulePhase { call, choice } => {
                serde_json::json!({"kind": "ratify-phase", "call": call, "choice": choice})
            }
            Self::StartPhase { phase } => {
                serde_json::json!({"kind": "start-phase", "phase": phase})
            }
            Self::ProposePhase { to, meditate } => move_to("propose-phase", to, *meditate),
            Self::MovePhase { to, meditate } => move_to("move-phase", to, *meditate),
            _ => unreachable!("a step of its own"),
        }
    }
}

/// What one log line must show: its `kind` and fields, at least `at_least`
/// times, after the first line `after` names matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expect {
    /// The line's kind.
    pub kind: String,
    /// Each field and what it must be: `key` equal to the text, `key~`
    /// holding it. A `$name` in the text is the entry bound to it.
    pub fields: Vec<(String, Match)>,
    /// How many lines must match.
    pub at_least: u64,
    /// Its name, for another expectation's `after`.
    pub name: Option<String>,
    /// The expectation whose first match this one's must come after.
    pub after: Option<String>,
}

/// What a field must be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Match {
    /// Equal to this text.
    Equals(String),
    /// Holding this text.
    Contains(String),
}

impl Scenario {
    /// The routes the canned substrate serves (#760): each reply streamed as
    /// llama.cpp streams one, routed by its `when`, else in order.
    #[must_use]
    pub fn routes(&self) -> Vec<crate::client::stub::Route> {
        self.routes_on(false)
    }

    /// [`Self::routes`], each reply streamed as Anthropic's Messages API
    /// streams one when `hosted` (#555) -- its thinking, its cache counts, or
    /// its error status with the `retry-after` it names -- else as llama.cpp
    /// streams one.
    #[must_use]
    pub fn routes_on(&self, hosted: bool) -> Vec<crate::client::stub::Route> {
        use crate::client::stub::{Act, Route};
        use crate::drive::canned::{
            HostedReply, PROMPT_TOKENS, StreamedCall, hosted_error, hosted_events, hosted_streamed,
            streamed_events_reporting, streamed_reporting,
        };
        self.replies
            .iter()
            .map(|reply| {
                let calls: Vec<StreamedCall<'_>> = reply
                    .calls
                    .iter()
                    .map(|call| StreamedCall {
                        id: &call.id,
                        name: &call.name,
                        arguments: &call.arguments,
                    })
                    .collect();
                // Held back: the same events, chunked, the first after the
                // delay (#782).
                let prompt = reply.prompt_tokens.unwrap_or(PROMPT_TOKENS);
                let hosted_reply = HostedReply {
                    thinking: reply.thinking.as_deref(),
                    text: &reply.text,
                    calls: &calls,
                    prompt_tokens: prompt,
                    cache_read: reply.cache_read_tokens.unwrap_or(0),
                    cache_write: reply.cache_write_tokens.unwrap_or(0),
                };
                let act = match (hosted, reply.status, reply.delay_ms) {
                    (true, Some((status, retry_after)), _) => {
                        Act::Raw(hosted_error(status, retry_after))
                    }
                    (true, None, Some(ms)) => Act::Trickle(
                        std::time::Duration::from_millis(ms),
                        vec![hosted_events(&hosted_reply)],
                    ),
                    (true, None, None) => Act::Raw(hosted_streamed(&hosted_reply)),
                    (false, _, Some(ms)) => Act::Trickle(
                        std::time::Duration::from_millis(ms),
                        vec![streamed_events_reporting(&reply.text, &calls, prompt)],
                    ),
                    (false, _, None) => Act::Raw(streamed_reporting(&reply.text, &calls, prompt)),
                };
                match &reply.when {
                    Some(text) => Route::when(text, act),
                    None => Route::ordered(act),
                }
            })
            .collect()
    }

    /// The canned classifier (#862): this scenario's `classify` rules, under
    /// the model, revision and compute units `classifier` declares.
    #[must_use]
    pub fn classifying(
        &self,
        classifier: &crate::drive::warrant_gate::Classifier,
    ) -> crate::client::stub::Classifying {
        crate::client::stub::Classifying {
            model: format!("{}@{}", classifier.model, classifier.revision),
            compute_units: classifier.compute_units.clone(),
            rules: self
                .classified
                .iter()
                .map(|rule| crate::client::stub::ClassifyRule {
                    when: rule.when.clone(),
                    hypothesis: rule.hypothesis.clone(),
                    // Read when the line was: a decimal from 0 to 1.
                    score: rule.score.parse().unwrap_or(0.0),
                })
                .collect(),
        }
    }

    /// The canned substrate's identity (#760): the sha256 of its replies,
    /// each rendered as the record renders a value, one per line, in the
    /// file's order, then its classifier's rules as `{"classify": ...}`
    /// (#862). What `canned-scenario`'s `engine_identity_is` names,
    /// and what a run says it played.
    #[must_use]
    pub fn replies_digest(&self) -> String {
        let text = |s: &str| Value::String(s.to_owned());
        let mut out = String::new();
        for reply in &self.replies {
            let mut members = BTreeMap::from([("text".to_owned(), text(&reply.text))]);
            if let Some(when) = &reply.when {
                members.insert("when".to_owned(), text(when));
            }
            if !reply.calls.is_empty() {
                let calls = reply
                    .calls
                    .iter()
                    .map(|call| {
                        Value::Object(BTreeMap::from([
                            ("id".to_owned(), text(&call.id)),
                            ("name".to_owned(), text(&call.name)),
                            ("arguments".to_owned(), text(&call.arguments)),
                        ]))
                    })
                    .collect();
                members.insert("calls".to_owned(), Value::Array(calls));
            }
            if let Some(ms) = reply.delay_ms {
                members.insert(
                    "delay_ms".to_owned(),
                    Value::Integer(i64::try_from(ms).unwrap_or(i64::MAX)),
                );
            }
            if let Some(tokens) = reply.prompt_tokens {
                members.insert(
                    "prompt_tokens".to_owned(),
                    Value::Integer(i64::try_from(tokens).unwrap_or(i64::MAX)),
                );
            }
            // A hosted reply's own fields (#555), digested only when present:
            // every scenario before them digests as it always has.
            let count = |n: u64| Value::Integer(i64::try_from(n).unwrap_or(i64::MAX));
            if let Some(thinking) = &reply.thinking {
                members.insert("thinking".to_owned(), text(thinking));
            }
            if let Some(tokens) = reply.cache_read_tokens {
                members.insert("cache_read_tokens".to_owned(), count(tokens));
            }
            if let Some(tokens) = reply.cache_write_tokens {
                members.insert("cache_write_tokens".to_owned(), count(tokens));
            }
            if let Some((status, retry_after)) = reply.status {
                members.insert("status".to_owned(), count(u64::from(status)));
                if let Some(seconds) = retry_after {
                    members.insert("retry_after".to_owned(), count(seconds));
                }
            }
            json::render(&Value::Object(members), &mut out);
            out.push('\n');
        }
        // The canned classifier's rules, after the replies: a scenario with
        // none digests as it always has.
        for rule in &self.classified {
            let members = BTreeMap::from([
                ("when".to_owned(), text(&rule.when)),
                ("hypothesis".to_owned(), text(&rule.hypothesis)),
                ("score".to_owned(), text(&rule.score)),
            ]);
            let line = BTreeMap::from([("classify".to_owned(), Value::Object(members))]);
            json::render(&Value::Object(line), &mut out);
            out.push('\n');
        }
        crate::digest::sha256_hex(out.as_bytes())
    }
}

/// The rung a dry run plans for (#555), as its regimen and registry entry
/// declare it.
#[derive(Debug, Clone)]
struct Intended {
    /// Its substrate id.
    rung: String,
    /// The model its requests would be billed as: `weights_model_id`.
    model: Option<String>,
    /// The regimen's request limits: a hosted rung's, none for a local one.
    limits: Option<crate::drive::hosted::RequestLimits>,
    /// What the rehearsal ran on: [`REHEARSED_ON`] for a hosted rung,
    /// [`CANNED_LOCAL`] for a local one (#942).
    rehearsed_on: &'static str,
    /// What the entry declares its API refuses.
    api: crate::drive::registry::ApiDeclared,
}

/// A dry run's copy of its regimen, removed when the run is done, however
/// it ends.
struct Rehearsal(std::path::PathBuf);

impl Drop for Rehearsal {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The canned hosted substrate a dry run rehearses on.
pub const REHEARSED_ON: &str = "canned-scenario-hosted";

/// `regimen`, the one a live run would take, as a dry run rehearses it: a
/// copy in the temporary directory with its `substrate` and
/// `substrate_hardware` the canned hosted substrate's and every other line
/// as written, and what the plan reads of the rung it names.
///
/// # Errors
///
/// A regimen that does not read, names a substrate that is not an `api`
/// entry, or declares no request limits.
fn rehearsing(regimen: &std::path::Path) -> Result<(Rehearsal, Intended), Failure> {
    use crate::drive::registry::{REGISTRY, identity};
    let text = std::fs::read_to_string(regimen)
        .map_err(|why| Failure(format!("{} cannot be read: {why}", regimen.display())))?;
    let read = crate::formats::regimen::parse(&text)
        .map_err(|why| Failure(format!("{}: {why:?}", regimen.display())))?;
    let Some(crate::formats::regimen::Value::String(rung)) = read.get("substrate") else {
        return Err(Failure(format!("{} names no substrate", regimen.display())));
    };
    let entry = identity(REGISTRY, rung).map_err(Failure)?;
    // A hosted rung's rehearsal plans its spend under its limits; a local
    // rung's (#942) counts its requests, on the local canned substrate.
    let (limits, rehearsed_on) = if entry.server_kind.as_deref() == Some("api") {
        let limits = crate::drive::hosted::request_limits(&read)
            .map_err(|why| Failure(format!("{}: {why}", regimen.display())))?;
        (Some(limits), REHEARSED_ON)
    } else {
        (None, CANNED_LOCAL)
    };
    let copy = swapped_onto(&text, rehearsed_on)?;
    Ok((
        copy,
        Intended {
            rung: rung.clone(),
            model: entry.model_id.clone(),
            limits,
            rehearsed_on,
            api: entry.api,
        },
    ))
}

/// The local routed canned substrate: what a rehearsal of a local rung's
/// regimen runs on (#942).
pub const CANNED_LOCAL: &str = "canned-scenario";

/// A copy of the regimen `text` with its substrate alone swapped for the
/// canned substrate `onto`, and that substrate's hardware fingerprint.
fn swapped_onto(text: &str, onto: &str) -> Result<Rehearsal, Failure> {
    use crate::drive::registry::{REGISTRY, identity};
    let canned = identity(REGISTRY, onto).map_err(Failure)?;
    let mut top = true;
    let swapped: String = text
        .lines()
        .map(|line| {
            top &= !line.trim_start().starts_with('[');
            let key = line.split('=').next().map(str::trim);
            match key {
                Some("substrate") if top => format!("substrate = \"{onto}\""),
                Some("substrate_hardware") if top => {
                    format!("substrate_hardware = \"{}\"", canned.hardware_fingerprint)
                }
                _ => line.to_owned(),
            }
        })
        .map(|line| line + "\n")
        .collect();
    let copy = std::env::temp_dir().join(format!(
        "diet-dry-run-{}-{}.toml",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos())
    ));
    std::fs::write(&copy, swapped)
        .map_err(|why| Failure(format!("the rehearsal's regimen cannot be written: {why}")))?;
    Ok(Rehearsal(copy))
}

/// A copy of the regimen at `regimen` on the canned substrate its rung's
/// kind speaks: the hosted one for an `api` entry, else the local one, and
/// a canned substrate as it is (#942, the sandbox probe).
fn on_canned(regimen: &std::path::Path) -> Result<Rehearsal, Failure> {
    use crate::drive::registry::{REGISTRY, identity};
    let text = std::fs::read_to_string(regimen)
        .map_err(|why| Failure(format!("{} cannot be read: {why}", regimen.display())))?;
    let read = crate::formats::regimen::parse(&text)
        .map_err(|why| Failure(format!("{}: {why:?}", regimen.display())))?;
    let Some(crate::formats::regimen::Value::String(rung)) = read.get("substrate") else {
        return Err(Failure(format!("{} names no substrate", regimen.display())));
    };
    let onto = if rung == REHEARSED_ON || rung == CANNED_LOCAL {
        rung.as_str()
    } else if identity(REGISTRY, rung)
        .map_err(Failure)?
        .server_kind
        .as_deref()
        == Some("api")
    {
        REHEARSED_ON
    } else {
        CANNED_LOCAL
    };
    swapped_onto(&text, onto)
}

impl Intended {
    /// The plan the rehearsal's log `written` says a live run on this rung
    /// would spend (#555): its requests by lane and by fork kind, their
    /// sized prompts summed, their output at `max_tokens`, the retries,
    /// whether the cap was met; and the worst case the limits allow,
    /// labelled a bound.
    ///
    /// # Errors
    ///
    /// A log that does not read.
    #[allow(clippy::too_many_lines)]
    fn plan(&self, written: &str) -> Result<serde_json::Value, Failure> {
        use crate::formats::log::{Event, FailReason};
        let lines = crate::formats::log::parse(written)
            .map_err(|why| Failure(format!("the rehearsal's log does not read: {why}")))?;
        let forks: BTreeMap<u64, &str> = lines
            .iter()
            .filter_map(|line| match &line.event {
                Event::Fork { why, .. } => Some((line.seq, why.tag())),
                _ => None,
            })
            .collect();
        let (mut requests, mut retries, mut sized, mut output) = (0_u64, 0_u64, 0_u64, 0_u64);
        let mut by_lane: BTreeMap<String, (u64, u64, u64)> = BTreeMap::new();
        let mut by_fork_kind: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
        let mut capped = false;
        let mut widest = 0_u64;
        for line in &lines {
            match &line.event {
                Event::Request {
                    lane,
                    fork,
                    max_tokens,
                    sized_tokens,
                    ..
                } => {
                    requests += 1;
                    widest = widest.max(max_tokens.unwrap_or(0));
                    sized += sized_tokens.unwrap_or(0);
                    output += max_tokens.unwrap_or(0);
                    let lane_total = by_lane.entry(lane.tag().to_owned()).or_default();
                    lane_total.0 += 1;
                    lane_total.1 += sized_tokens.unwrap_or(0);
                    lane_total.2 += max_tokens.unwrap_or(0);
                    if let Some(kind) = fork.and_then(|fork| forks.get(&fork)) {
                        *by_fork_kind
                            .entry(lane.tag().to_owned())
                            .or_default()
                            .entry((*kind).to_owned())
                            .or_default() += 1;
                    }
                }
                Event::RequestRetried { .. } => retries += 1,
                Event::RequestFailed { reason, .. } if *reason == FailReason::ApiCap => {
                    capped = true;
                }
                _ => {}
            }
        }
        let bound = match (
            &self.limits,
            self.api.serving_context,
            self.api.max_output_tokens,
        ) {
            // A local rung (#942): what the rehearsal sent, each at its fullest.
            (None, Some(window), _) => serde_json::json!({
                "tokens": requests.saturating_mul(window + widest),
                "is": "a bound, not an estimate: the rehearsal's requests x (the rung's window + \
                       the widest max_tokens they asked), every request at its fullest; a local \
                       rung spends no money",
            }),
            (None, None, _) => serde_json::json!({
                "tokens": null,
                "is": "no bound: the rung declares no window",
            }),
            (Some(limits), Some(window), Some(cap)) => serde_json::json!({
                "tokens": limits.max_api_requests.saturating_mul(window + cap),
                "is": "a bound, not an estimate: max_api_requests x (the rung's window + its \
                       output cap), every request at its fullest",
            }),
            _ => serde_json::json!({
                "tokens": null,
                "is": "no bound: the rung declares no window or no output cap",
            }),
        };
        Ok(serde_json::json!({
            "rung": self.rung,
            "rung_kind": if self.limits.is_some() { "api" } else { "local" },
            "model": self.model,
            "rehearsed_on": self.rehearsed_on,
            "limits": self.limits.as_ref().map(|limits| serde_json::json!({
                "max_api_requests": limits.max_api_requests,
                "api_retries": limits.api_retries,
                "api_retry_after_max_seconds": limits.api_retry_after_max_seconds,
            })),
            "rehearsal": {
                "requests": requests,
                "retries": retries,
                "attempts": requests + retries,
                "by_lane": by_lane
                    .iter()
                    .map(|(lane, (count, sized, output))| {
                        (
                            lane.clone(),
                            serde_json::json!({
                                "requests": count,
                                "input_sized": sized,
                                "output_at_max_tokens": output,
                            }),
                        )
                    })
                    .collect::<serde_json::Map<_, _>>(),
                // Where the cache would place each request's input is not
                // estimated: the session keeps no account of what an
                // earlier request wrote, and modelling the cache here would
                // be a second estimator.
                "input_split": {
                    "of_which_cache_read": null,
                    "cache_write_5m": null,
                    "cache_write_1h": null,
                    "uncached": null,
                    "is": "unestimated: the session keeps no account of the prefix an earlier \
                           request wrote to the cache, so `input_sized` is not split by where the \
                           cache would place it; price it as uncached for the upper bound",
                },
                "by_fork_kind": by_fork_kind,
                "sized_tokens": sized,
                "output_tokens_at_max_tokens": output,
                "api_cap_met": capped,
            },
            "bound": bound,
            "api_unchecked": self.api.unchecked(),
        }))
    }
}

/// The key a canned hosted run sends (#555): a rehearsal's, never a
/// provider's.
pub const REHEARSAL_KEY: &str = "sk-rehearsal";

/// Whether `substrate` is a hosted entry (#555), an `api` one whose replies
/// are served as Anthropic's Messages API serves them; a reply that thinks,
/// counts the cache or answers with an error on any other is refused.
fn served_hosted(substrate: &str, replies: &[Reply]) -> Result<bool, Failure> {
    let hosted = crate::drive::registry::identity(crate::drive::registry::REGISTRY, substrate)
        .is_ok_and(|identity| identity.server_kind.as_deref() == Some("api"));
    if !hosted && replies.iter().any(Reply::is_hosted_only) {
        return Err(Failure(format!(
            "a reply's `thinking`, cache counts or `status` are a hosted substrate's, and \
             {substrate} is not one"
        )));
    }
    Ok(hosted)
}

/// Why a scenario does not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScenarioError {
    /// Not JSON Lines in the record's value space.
    Syntax(String),
    /// A line, by its number from 1, and what is wrong with it.
    Line(usize, String),
}

impl fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(why) => write!(f, "the scenario does not read: {why}"),
            Self::Line(at, why) => write!(f, "scenario line {at}: {why}"),
        }
    }
}

impl std::error::Error for ScenarioError {}

/// Read a scenario.
///
/// # Errors
///
/// Returns [`ScenarioError`] for text that is not JSON Lines in the
/// record's value space, a first line that is not the header, or any line
/// with a key it does not know, without one it needs, or of the wrong type.
pub fn parse(text: &str) -> Result<Scenario, ScenarioError> {
    let lines = json::objects(text).map_err(|why| ScenarioError::Syntax(why.to_string()))?;
    let mut lines = lines
        .into_iter()
        .enumerate()
        .map(|(at, line)| (at + 1, line));
    let (_, first) = lines
        .next()
        .ok_or_else(|| ScenarioError::Syntax("no header line".to_owned()))?;
    let mut scenario = header(Fields::of(first, 1))?;
    let mut names: Vec<String> = Vec::new();
    for (at, line) in lines {
        let mut fields = Fields::of(line, at);
        if fields.has("reply") {
            let reply = fields.object("reply")?;
            fields.done()?;
            scenario.replies.push(reply_of(Fields::of(reply, at))?);
        } else if fields.has("classify") {
            let rule = fields.object("classify")?;
            fields.done()?;
            scenario.classified.push(classify_of(Fields::of(rule, at))?);
        } else if fields.has("step") {
            let (step, patience) = step_of(fields)?;
            if let Some(seconds) = patience {
                scenario.patience.insert(scenario.steps.len(), seconds);
            }
            // A grade grades the ask just before it, which asks its task.
            if let Step::Grade(grading) = &step {
                match scenario.steps.last_mut() {
                    Some(Step::Ask {
                        refused: None,
                        task: task @ None,
                        ..
                    }) => *task = Some(grading.task.clone()),
                    Some(Step::Ask {
                        refused: None,
                        task: Some(asked),
                        ..
                    }) if *asked == grading.task => {}
                    _ => {
                        return Err(ScenarioError::Line(
                            at,
                            "wanted a `grade` right after the admitted `ask` of its task"
                                .to_owned(),
                        ));
                    }
                }
            }
            scenario.steps.push(step);
        } else if fields.has("expect") {
            let expect = expect_of(fields, &names)?;
            if let Some(name) = &expect.name {
                names.push(name.clone());
            }
            scenario.expects.push(expect);
        } else {
            return Err(fields.wrong("a `reply`, `classify`, `step` or `expect` line"));
        }
    }
    scenario.placeholders_are_declared()?;
    Ok(scenario)
}

/// The first line: `scenario`, `regimen`, `head`, and optionally
/// `live_regimen` and `approvals`.
#[allow(clippy::too_many_lines)]
fn header(mut fields: Fields) -> Result<Scenario, ScenarioError> {
    let name = fields.text("scenario")?;
    let regimen = fields.text("regimen")?;
    let head = fields.text("head")?;
    let live_regimen = fields.maybe_text("live_regimen")?;
    let approvals = fields.maybe_text("approvals")?;
    let fixed_asks = match fields.take("fixed_asks") {
        None => Vec::new(),
        Some(Value::Array(indices)) => indices
            .into_iter()
            .map(|index| match index {
                Value::Integer(n) if n >= 0 => Ok(n.unsigned_abs()),
                _ => Err(fields.wrong("each of `fixed_asks` an ask's place, 0 or more")),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(fields.wrong("`fixed_asks` a list")),
    };
    let seed = match fields.take("seed") {
        None => BTreeMap::new(),
        Some(Value::Object(seed)) => seed
            .into_iter()
            .map(|(path, source)| match source {
                Value::String(source) if is_relative(&path) && is_relative(&source) => {
                    Ok((path, source))
                }
                _ => Err(fields.wrong("`seed` mapping relative worktree paths to relative files")),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(fields.wrong("`seed` an object")),
    };
    let ports = match fields.take("ports") {
        None => Vec::new(),
        Some(Value::Array(names)) => {
            let mut ports: Vec<String> = Vec::new();
            for name in names {
                match name {
                    Value::String(name)
                        if crate::drive::services::is_a_name(&name) && !ports.contains(&name) =>
                    {
                        ports.push(name);
                    }
                    _ => return Err(fields.wrong("`ports` a list of distinct names")),
                }
            }
            ports
        }
        Some(_) => return Err(fields.wrong("`ports` a list")),
    };
    let services = match fields.take("services") {
        None => Vec::new(),
        Some(Value::Array(declared)) => crate::drive::services::services_of(declared, &ports)
            .map_err(|why| fields.wrong(&format!("`services` as declared: {why}")))?,
        Some(_) => return Err(fields.wrong("`services` a list")),
    };
    let sandbox_unix_sockets = match fields.take("sandbox_unix_sockets") {
        None => Vec::new(),
        Some(Value::Array(named)) => named
            .into_iter()
            .map(|named| match named {
                Value::String(named) => match named.split('.').collect::<Vec<_>>().as_slice() {
                    ["service", name, key]
                        if services.iter().any(|service| service.name == *name)
                            && crate::drive::services::is_a_name(key) =>
                    {
                        Ok(((*name).to_owned(), (*key).to_owned()))
                    }
                    _ => Err(fields.wrong(
                        "each of `sandbox_unix_sockets` a declared service's value,                          `service.NAME.KEY`",
                    )),
                },
                _ => Err(fields.wrong("each of `sandbox_unix_sockets` text")),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(fields.wrong("`sandbox_unix_sockets` a list")),
    };
    let service_values = |fields: &Fields, key: &str, named: Vec<Value>| {
        named
            .into_iter()
            .map(|named| match named {
                Value::String(named) => match named.split('.').collect::<Vec<_>>().as_slice() {
                    ["service", name, value]
                        if services.iter().any(|service| service.name == *name)
                            && crate::drive::services::is_a_name(value) =>
                    {
                        Ok(((*name).to_owned(), (*value).to_owned()))
                    }
                    _ => Err(fields.wrong(&format!(
                        "each of `{key}` a declared service's value, `service.NAME.KEY`"
                    ))),
                },
                _ => Err(fields.wrong(&format!("each of `{key}` text"))),
            })
            .collect::<Result<Vec<_>, _>>()
    };
    let seal = match fields.take("seal") {
        None => Vec::new(),
        Some(Value::Array(named)) => service_values(&fields, "seal", named)?,
        Some(_) => return Err(fields.wrong("`seal` a list")),
    };
    let seal_probe = match fields.take("seal_probe") {
        None => Vec::new(),
        Some(Value::Array(texts)) => texts
            .into_iter()
            .map(|text| match text {
                Value::String(text) => Ok(text),
                _ => Err(fields.wrong("each of `seal_probe` text")),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(fields.wrong("`seal_probe` a list")),
    };
    let serve_env = match fields.take("serve_env") {
        None => BTreeMap::new(),
        Some(Value::Object(named)) => named
            .into_iter()
            .map(|(name, text)| match text {
                Value::String(text) if is_an_environment_name(&name) => Ok((name, text)),
                _ => Err(fields.wrong("`serve_env` mapping environment variable names to text")),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(fields.wrong("`serve_env` an object")),
    };
    if let Some(scope) = &approvals
        && !crate::drive::tool_loop::Decision::ALL
            .iter()
            .any(|decision| decision.tag() == scope)
    {
        return Err(fields.wrong("`approvals` naming a `POST /approve` scope"));
    }
    fields.done()?;
    Ok(Scenario {
        name,
        regimen,
        live_regimen,
        head,
        approvals,
        seed,
        fixed_asks,
        replies: Vec::new(),
        classified: Vec::new(),
        steps: Vec::new(),
        patience: BTreeMap::new(),
        expects: Vec::new(),
        ports,
        services,
        sandbox_unix_sockets,
        serve_env,
        seal,
        seal_probe,
    })
}

/// A variable's name: a letter or `_`, then letters, digits and `_`.
fn is_an_environment_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A path that stays where it is put: relative, with no `..`.
fn is_relative(path: &str) -> bool {
    let path = std::path::Path::new(path);
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
}

fn classify_of(mut fields: Fields) -> Result<Classify, ScenarioError> {
    let when = fields.text("when")?;
    let hypothesis = fields.text("hypothesis")?;
    let score = fields.text("score")?;
    let decimal = score.split_once('.').is_some_and(|(whole, part)| {
        !whole.is_empty()
            && !part.is_empty()
            && whole
                .bytes()
                .chain(part.bytes())
                .all(|b| b.is_ascii_digit())
    });
    if !decimal
        || !score
            .parse::<f64>()
            .is_ok_and(|value| (0.0..=1.0).contains(&value))
    {
        return Err(fields.wrong("`score` a decimal from 0 to 1, written as text"));
    }
    fields.done()?;
    Ok(Classify {
        when,
        hypothesis,
        score,
    })
}

fn reply_of(mut fields: Fields) -> Result<Reply, ScenarioError> {
    let when = fields.maybe_text("when")?;
    let status = match fields.maybe_count("status")? {
        None => None,
        Some(code @ 400..=599) => Some((
            u16::try_from(code).map_err(|_| fields.wrong("`status` an HTTP error status"))?,
            fields.maybe_count("retry_after")?,
        )),
        Some(_) => return Err(fields.wrong("`status` an HTTP error status, 400 to 599")),
    };
    // An error answers nothing: its text is the scenario's to omit.
    let text = match status {
        Some(_) => fields.maybe_text("text")?.unwrap_or_default(),
        None => fields.text("text")?,
    };
    let calls = match fields.take("calls") {
        None => Vec::new(),
        Some(Value::Array(calls)) => calls
            .into_iter()
            .map(|call| match call {
                Value::Object(call) => {
                    let mut call = Fields::of(call, fields.at);
                    let read = Call {
                        id: call.text("id")?,
                        name: call.text("name")?,
                        arguments: call.text("arguments")?,
                    };
                    call.done()?;
                    Ok(read)
                }
                _ => Err(fields.wrong("each call an object")),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(fields.wrong("`calls` a list")),
    };
    let delay_ms = fields.maybe_count("delay_ms")?;
    let prompt_tokens = fields.maybe_count("prompt_tokens")?;
    let thinking = fields.maybe_text("thinking")?;
    let cache_read_tokens = fields.maybe_count("cache_read_tokens")?;
    let cache_write_tokens = fields.maybe_count("cache_write_tokens")?;
    fields.done()?;
    Ok(Reply {
        when,
        text,
        calls,
        delay_ms,
        prompt_tokens,
        thinking,
        cache_read_tokens,
        cache_write_tokens,
        status,
    })
}

/// One `step` line, and the `patience` it names: whole seconds above 0, on a
/// step that waits for the session to settle and on no other.
fn step_of(mut fields: Fields) -> Result<(Step, Option<u64>), ScenarioError> {
    let patience = fields.maybe_count("patience")?;
    // Where an operator script's move was made (#908): for a reader, never
    // for the runner.
    match fields.take("place") {
        None | Some(Value::Object(_)) => {}
        Some(_) => return Err(fields.wrong("`place` an object")),
    }
    if patience == Some(0) {
        return Err(fields.wrong("`patience` whole seconds above 0"));
    }
    let step = match fields.text("step")?.as_str() {
        "ask" => Step::Ask {
            text: fields.text("text")?,
            scoping: fields.maybe_bool("scoping")?.unwrap_or(false),
            refused: fields.maybe_text("refused")?,
            attach: match fields.take("attach") {
                None => Vec::new(),
                Some(Value::Array(paths)) => paths
                    .into_iter()
                    .map(|path| match path {
                        Value::String(path) if is_relative(&path) => Ok(path),
                        _ => Err(fields.wrong("each of `attach` a relative worktree path")),
                    })
                    .collect::<Result<_, _>>()?,
                Some(_) => return Err(fields.wrong("`attach` a list")),
            },
            task: fields.maybe_text("task")?,
        },
        "grade" => Step::Grade(Box::new(grading_of(&mut fields)?)),
        "start-phase" => Step::StartPhase {
            phase: fields.text("phase")?,
        },
        "propose-phase" => Step::ProposePhase {
            to: fields.text("to")?,
            meditate: fields.maybe_bool("meditate")?,
        },
        "move-phase" => Step::MovePhase {
            to: fields.text("to")?,
            meditate: fields.maybe_bool("meditate")?,
        },
        "open-tangent" => Step::OpenTangent {
            id: fields.text("id")?,
        },
        "close-tangent" => Step::CloseTangent {
            each: match fields.text("each")?.as_str() {
                word @ ("keep" | "drop" | "park") => word.to_owned(),
                _ => return Err(fields.wrong("`each` keep, drop or park")),
            },
        },
        "flag" => Step::Flag {
            pick: fields.pick()?,
            note: fields.text("note")?,
            bind: fields.maybe_text("as")?,
        },
        "edit" => Step::Edit {
            pick: fields.pick()?,
            text: fields.text("text")?,
            bind: fields.maybe_text("as")?,
        },
        "declare-meditation" => Step::DeclareMeditation,
        "refill-anyway" => Step::RefillAnyway,
        "rule-phase" => Step::RulePhase {
            call: fields.text("call")?,
            choice: fields.text("choice")?,
        },
        "wait" => {
            let until = fields.text("until")?;
            let matches = where_of(&mut fields)?;
            if until == "idle" && !matches.is_empty() {
                return Err(fields.wrong("`where` only on a wait for a line's kind"));
            }
            Step::Wait {
                until,
                timeout: fields.maybe_count("timeout")?,
                matches,
            }
        }
        "end" => Step::End,
        _ => return Err(fields.wrong("a step the runner knows")),
    };
    if patience.is_some() && !step.settles() {
        return Err(fields.wrong("`patience` only on a step that waits for the session to settle"));
    }
    fields.done()?;
    Ok((step, patience))
}

/// A `grade` step's fields (#942).
fn grading_of(fields: &mut Fields) -> Result<Grading, ScenarioError> {
    let attempts = fields.maybe_count("attempts")?.unwrap_or(GRADE_ATTEMPTS);
    if attempts == 0 {
        return Err(fields.wrong("`attempts` 1 or more"));
    }
    let timeout = fields.maybe_count("timeout_s")?.unwrap_or(GRADE_TIMEOUT_S);
    if timeout == 0 {
        return Err(fields.wrong("`timeout_s` whole seconds above 0"));
    }
    let task = fields.text("task")?;
    if task.trim().is_empty() {
        return Err(fields.wrong("`task` a name"));
    }
    Ok(Grading {
        task,
        check: fields.text("check")?,
        attempts,
        wrong: fields.text("wrong")?,
        last: fields.maybe_text("last")?,
        right: fields.maybe_text("right")?,
        before: fields.maybe_text("before")?,
        timeout: std::time::Duration::from_secs(timeout),
    })
}

/// A line's `where`: each field and what it must be, `key` equal to the
/// text and `key~` holding it. Empty when it has none.
fn where_of(fields: &mut Fields) -> Result<Vec<(String, Match)>, ScenarioError> {
    let mut matches = Vec::new();
    if let Some(wanted) = fields.take("where") {
        let Value::Object(wanted) = wanted else {
            return Err(fields.wrong("`where` an object"));
        };
        for (key, value) in wanted {
            let Value::String(text) = value else {
                return Err(fields.wrong("each `where` value text"));
            };
            matches.push(match key.strip_suffix('~') {
                Some(key) => (key.to_owned(), Match::Contains(text)),
                None => (key, Match::Equals(text)),
            });
        }
    }
    Ok(matches)
}

fn expect_of(mut fields: Fields, names: &[String]) -> Result<Expect, ScenarioError> {
    let kind = fields.text("expect")?;
    let matches = where_of(&mut fields)?;
    let at_least = fields.maybe_count("at_least")?.unwrap_or(1);
    if at_least == 0 {
        return Err(fields.wrong("`at_least` 1 or more"));
    }
    let name = fields.maybe_text("name")?;
    let after = fields.maybe_text("after")?;
    if after.as_ref().is_some_and(|after| !names.contains(after)) {
        return Err(fields.wrong("`after` naming an expectation above it"));
    }
    fields.done()?;
    Ok(Expect {
        kind,
        fields: matches,
        at_least,
        name,
        after,
    })
}

/// One line's fields, taken one by one; any left over is refused.
struct Fields {
    at: usize,
    left: BTreeMap<String, Value>,
}

impl Fields {
    fn of(left: BTreeMap<String, Value>, at: usize) -> Self {
        Self { at, left }
    }

    fn wrong(&self, wanted: &str) -> ScenarioError {
        ScenarioError::Line(self.at, format!("wanted {wanted}"))
    }

    fn has(&self, key: &str) -> bool {
        self.left.contains_key(key)
    }

    fn take(&mut self, key: &str) -> Option<Value> {
        self.left.remove(key)
    }

    fn maybe_text(&mut self, key: &str) -> Result<Option<String>, ScenarioError> {
        match self.take(key) {
            None => Ok(None),
            Some(Value::String(text)) => Ok(Some(text)),
            Some(_) => Err(self.wrong(&format!("`{key}` text"))),
        }
    }

    fn text(&mut self, key: &str) -> Result<String, ScenarioError> {
        self.maybe_text(key)?
            .ok_or_else(|| self.wrong(&format!("`{key}`")))
    }

    fn maybe_bool(&mut self, key: &str) -> Result<Option<bool>, ScenarioError> {
        match self.take(key) {
            None => Ok(None),
            Some(Value::Boolean(flag)) => Ok(Some(flag)),
            Some(_) => Err(self.wrong(&format!("`{key}` true or false"))),
        }
    }

    fn maybe_count(&mut self, key: &str) -> Result<Option<u64>, ScenarioError> {
        match self.take(key) {
            None => Ok(None),
            Some(Value::Integer(n)) => u64::try_from(n)
                .map(Some)
                .map_err(|_| self.wrong(&format!("`{key}` 0 or more"))),
            Some(_) => Err(self.wrong(&format!("`{key}` a count"))),
        }
    }

    fn object(&mut self, key: &str) -> Result<BTreeMap<String, Value>, ScenarioError> {
        match self.take(key) {
            Some(Value::Object(object)) => Ok(object),
            _ => Err(self.wrong(&format!("`{key}` an object"))),
        }
    }

    /// `pick`: a place among the live entries, or text one holds.
    fn pick(&mut self) -> Result<Pick, ScenarioError> {
        match self.take("pick") {
            Some(Value::Integer(n)) => usize::try_from(n)
                .map(Pick::Index)
                .map_err(|_| self.wrong("`pick` 0 or more")),
            Some(Value::String(text)) if !text.is_empty() => Ok(Pick::Containing(text)),
            _ => Err(self.wrong("`pick`, a place or some text")),
        }
    }

    fn done(self) -> Result<(), ScenarioError> {
        match self.left.keys().next() {
            None => Ok(()),
            Some(key) => Err(ScenarioError::Line(
                self.at,
                format!("`{key}` is not a key this line takes"),
            )),
        }
    }
}

/// A serve this runner drives over its HTTP API (#756): its address, and
/// the Basic credential every request presents.
#[derive(Debug, Clone)]
pub struct Driver {
    address: String,
    authorization: Option<String>,
    /// Where the model's commands run: what an ask's `attach` reads from.
    worktree: Option<std::path::PathBuf>,
    /// The session's log, which a `grade` step reads its turn from (#942).
    log: Option<std::path::PathBuf>,
    /// The scenario's directory, named to a grade's commands (#942).
    scenario_dir: Option<std::path::PathBuf>,
    /// What a grade's commands are told of the run's services (#942).
    services_env: Vec<(String, String)>,
    /// Where each attempt's grading output is kept, when it is (#942).
    grades_out: Option<std::path::PathBuf>,
    /// Why the drive failed void, when it did for a reason outside the
    /// model (#910, #942): a check that crashed.
    voided: std::cell::Cell<Option<&'static str>>,
}

/// Why a drive stopped short of its last step, or a run failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure(pub String);

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// How long one step may take to settle before the drive fails, unless the
/// step names its own `patience`.
pub const STEP_PATIENCE: std::time::Duration = std::time::Duration::from_secs(900);

/// The names steps bound, each to an entry id.
pub type Bindings = BTreeMap<String, String>;

impl Driver {
    /// A driver for the serve listening at `address` (`host:port`), with
    /// `user_password` its `--auth-file`'s content when it has one.
    #[must_use]
    pub fn new(address: &str, user_password: Option<&str>) -> Self {
        Self {
            address: address.to_owned(),
            authorization: user_password
                .map(|pair| format!("Basic {}", crate::drive::serve::base64(pair.as_bytes()))),
            worktree: None,
            log: None,
            scenario_dir: None,
            services_env: Vec::new(),
            grades_out: None,
            voided: std::cell::Cell::new(None),
        }
    }

    /// This driver, a `grade` step reading its turn from the log at `log`.
    #[must_use]
    pub fn logging_to(mut self, log: std::path::PathBuf) -> Self {
        self.log = Some(log);
        self
    }

    /// This driver, naming `dir` to a grade's commands as
    /// `DIET_GRADE_SCENARIO_DIR` (#942).
    #[must_use]
    pub fn for_scenario_in(mut self, dir: std::path::PathBuf) -> Self {
        self.scenario_dir = Some(dir);
        self
    }

    /// This driver, telling a grade's commands of the run's services
    /// (#942): their ports and values.
    #[must_use]
    pub fn telling(mut self, env: Vec<(String, String)>) -> Self {
        self.services_env = env;
        self
    }

    /// This driver, keeping each attempt's grading output under `dir`, as
    /// `<task>/attempt-<n>/`, named to the check as `DIET_GRADE_OUT` (#942).
    #[must_use]
    pub fn grading_out_to(mut self, dir: std::path::PathBuf) -> Self {
        self.grades_out = Some(dir);
        self
    }

    /// What every grade command is told: the scenario's directory, and the
    /// run's services' ports and values.
    fn grade_env(&self) -> Vec<(&str, String)> {
        let mut env: Vec<(&str, String)> = self
            .scenario_dir
            .iter()
            .map(|dir| ("DIET_GRADE_SCENARIO_DIR", dir.display().to_string()))
            .collect();
        env.extend(
            self.services_env
                .iter()
                .map(|(key, value)| (key.as_str(), value.clone())),
        );
        env
    }

    /// Why the drive failed void, when it did (#942).
    #[must_use]
    pub fn voided(&self) -> Option<&'static str> {
        self.voided.get()
    }

    /// `why`, the drive marked void for `cause`.
    fn void(&self, cause: &'static str, why: String) -> Failure {
        self.voided.set(Some(cause));
        Failure(why)
    }

    /// The log's lines, once it holds every line `/state` names.
    fn logged(&self) -> Result<Vec<crate::formats::log::Line>, Failure> {
        let path = self.log.as_ref().ok_or_else(|| {
            Failure("a grade step reads the session's log, and none is kept".to_owned())
        })?;
        let last = self.last_seq()?;
        let give_up = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let text = std::fs::read_to_string(path)
                .map_err(|why| Failure(format!("{} cannot be read: {why}", path.display())))?;
            let read = crate::formats::log::read(&text)
                .map_err(|why| Failure(format!("{}: {why}", path.display())))?;
            if read.lines.last().is_some_and(|line| line.seq >= last) {
                return Ok(read.lines);
            }
            if std::time::Instant::now() >= give_up {
                return Err(Failure(format!(
                    "{} did not reach seq {last} in 10 s",
                    path.display()
                )));
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    /// The worktree a grade's commands run in.
    fn grading_in(&self) -> Result<&std::path::Path, Failure> {
        self.worktree.as_deref().ok_or_else(|| {
            Failure(
                "a grade step runs in the worktree, and the regimen runs no commands".to_owned(),
            )
        })
    }

    /// A grade's `before` command's stdout, run guarded in the worktree.
    fn before(&self, grading: &Grading) -> Result<Option<String>, Failure> {
        let Some(command) = &grading.before else {
            return Ok(None);
        };
        let ran = crate::drive::grade::guarded(
            command,
            self.grading_in()?,
            &self.grade_env(),
            grading.timeout,
        )
        .map_err(|why| self.void("grader", why))?;
        if ran.timed_out {
            return Err(self.void(
                "grader",
                format!("the grade's `before` ran out of time: {command}"),
            ));
        }
        Ok(Some(ran.stdout))
    }

    /// Grade the turn just settled (#942): run the check, log its verdict,
    /// and send the feedback ask its template names, each wrong attempt
    /// with attempts left asked again, until it is right or the attempts
    /// run out.
    fn graded(
        &self,
        grading: &Grading,
        mut before: Option<String>,
        (approvals, patience): (Option<&str>, std::time::Duration),
    ) -> Result<(), Failure> {
        use crate::drive::grade::{Verdict, feedback, guarded, turn_of};
        let worktree = self.grading_in()?.to_path_buf();
        let check_sha256 = crate::digest::sha256_hex(grading.check.as_bytes());
        for attempt in 1..=grading.attempts {
            let turn = turn_of(&self.logged()?, (&grading.task, attempt));
            // The check's inputs, outside the worktree, gone once it ran.
            let inputs = fresh("grade")?;
            let turn_file = inputs.join("turn.json");
            std::fs::write(&turn_file, format!("{turn:#}\n"))
                .map_err(|why| Failure(format!("the turn file cannot be written: {why}")))?;
            let mut env = self.grade_env();
            env.extend([
                ("DIET_GRADE_TASK", grading.task.clone()),
                ("DIET_GRADE_ATTEMPT", attempt.to_string()),
                ("DIET_GRADE_TURN", turn_file.display().to_string()),
            ]);
            if let Some(out) = &self.grades_out {
                let kept = out.join(&grading.task).join(format!("attempt-{attempt}"));
                std::fs::create_dir_all(&kept)
                    .map_err(|why| Failure(format!("{} cannot be made: {why}", kept.display())))?;
                env.push(("DIET_GRADE_OUT", kept.display().to_string()));
            }
            if let Some(text) = &before {
                let file = inputs.join("before.txt");
                std::fs::write(&file, text)
                    .map_err(|why| Failure(format!("the before file cannot be written: {why}")))?;
                env.push(("DIET_GRADE_BEFORE", file.display().to_string()));
            }
            let ran = guarded(&grading.check, &worktree, &env, grading.timeout);
            let _ = std::fs::remove_dir_all(&inputs);
            let ran = ran.map_err(|why| self.void("grader", why))?;
            let verdict = if ran.timed_out {
                Verdict {
                    right: false,
                    expected: String::new(),
                    got: "(the check timed out)".to_owned(),
                }
            } else {
                Verdict::read(&ran.stdout).map_err(|why| {
                    self.void(
                        "grader",
                        format!("{why}; its stderr: {}", ran.stderr.trim()),
                    )
                })?
            };
            let mut grade = serde_json::json!({
                "kind": "grade",
                "task": grading.task,
                "attempt": attempt,
                "attempts": grading.attempts,
                "right": verdict.right,
                "check_sha256": check_sha256,
            });
            if ran.timed_out {
                grade["timed_out"] = serde_json::Value::Bool(true);
            }
            self.command(&grade)?;
            let again = !verdict.right && attempt < grading.attempts;
            let template = if verdict.right {
                grading.right.as_ref()
            } else if again {
                Some(&grading.wrong)
            } else {
                grading.last.as_ref()
            };
            if again {
                before = self.before(grading)?;
            }
            if let Some(template) = template {
                let text = feedback(
                    template,
                    &verdict,
                    &grading.task,
                    (attempt, grading.attempts),
                );
                self.asked(
                    (&text, false, &[], Some(&grading.task)),
                    None,
                    (approvals, patience),
                )?;
            }
            if !again {
                break;
            }
        }
        Ok(())
    }

    /// This driver, reading an ask's attachments from `worktree`.
    #[must_use]
    pub fn in_worktree(mut self, worktree: std::path::PathBuf) -> Self {
        self.worktree = Some(worktree);
        self
    }

    /// One request, answered with a close: its status and its body.
    fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<(u16, String), Failure> {
        self.sent(
            method,
            path,
            body.map(|body| ("application/json", body.as_bytes())),
        )
    }

    /// One request whose body is `body`'s bytes, of its media type.
    fn sent(
        &self,
        method: &str,
        path: &str,
        body: Option<(&str, &[u8])>,
    ) -> Result<(u16, String), Failure> {
        use std::io::{Read as _, Write as _};
        let failed = |why: std::io::Error| Failure(format!("{method} {path}: {why}"));
        let mut stream = std::net::TcpStream::connect(&self.address).map_err(failed)?;
        let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {}\r\n", self.address);
        if let Some(authorization) = &self.authorization {
            let _ = write!(head, "Authorization: {authorization}\r\n");
        }
        if let Some((media_type, body)) = body {
            let _ = write!(
                head,
                "Content-Type: {media_type}\r\nContent-Length: {}\r\n",
                body.len()
            );
        }
        head.push_str("\r\n");
        let mut bytes = head.into_bytes();
        bytes.extend_from_slice(body.map_or(&[][..], |(_, body)| body));
        stream.write_all(&bytes).map_err(failed)?;
        let mut reply = String::new();
        stream.read_to_string(&mut reply).map_err(failed)?;
        let status = reply
            .split(' ')
            .nth(1)
            .and_then(|code| code.parse().ok())
            .ok_or_else(|| Failure(format!("{method} {path}: not an HTTP reply")))?;
        let body = reply.split_once("\r\n\r\n").map_or("", |(_, body)| body);
        Ok((status, body.to_owned()))
    }

    fn get(&self, path: &str) -> Result<serde_json::Value, Failure> {
        match self.request("GET", path, None)? {
            (200, body) => {
                serde_json::from_str(&body).map_err(|why| Failure(format!("GET {path}: {why}")))
            }
            (status, body) => Err(Failure(format!("GET {path}: {status} {body}"))),
        }
    }

    fn post(&self, path: &str, body: &serde_json::Value) -> Result<(u16, String), Failure> {
        self.request("POST", path, Some(&body.to_string()))
    }

    /// A command that must be admitted.
    fn command(&self, body: &serde_json::Value) -> Result<(), Failure> {
        match self.post("/commands", body)? {
            (200, _) => Ok(()),
            (status, reply) => Err(Failure(format!("{body} was answered {status} {reply}"))),
        }
    }

    /// Wait for a line of kind `until` after `mark` that has every field
    /// `matches` names -- one of the kind that does not is waited past --
    /// or for `idle`; a failure when `seconds` pass first.
    fn waited(
        &self,
        (until, matches): (&str, &[(String, Match)]),
        mark: u64,
        seconds: u64,
    ) -> Result<(), Failure> {
        use crate::formats::log_query::Filter;
        let filters: Vec<Filter> = matches
            .iter()
            .map(|(key, wanted)| match wanted {
                Match::Equals(text) => Filter::Is(key.clone(), text.clone()),
                Match::Contains(text) => Filter::Holds(key.clone(), text.clone()),
            })
            .collect();
        let give_up = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
        let mut after = mark;
        loop {
            let left = give_up
                .saturating_duration_since(std::time::Instant::now())
                .as_secs();
            let state = self.get(&format!("/wait?until={until}&after={after}&timeout={left}"))?;
            if state["timed_out"] == true || left == 0 {
                return Err(Failure(format!("no `{until}` within {seconds} s: {state}")));
            }
            if filters.is_empty() {
                return Ok(());
            }
            let event = &state["event"];
            let line = crate::formats::record::json::line(&event.to_string())
                .map(crate::formats::record::json::Value::Object)
                .map_err(|why| Failure(format!("/wait's event does not read: {why}")))?;
            if filters.iter().all(|filter| filter.passes(&line)) {
                return Ok(());
            }
            after = event["seq"]
                .as_u64()
                .ok_or_else(|| Failure(format!("/wait's event names no seq: {event}")))?;
        }
    }

    /// The last `seq` logged.
    fn last_seq(&self) -> Result<u64, Failure> {
        self.get("/state")?["last_seq"]
            .as_u64()
            .ok_or_else(|| Failure("/state named no last_seq".to_owned()))
    }

    /// Wait until the session is idle, answering each prompt that waits on
    /// the operator with `approvals`' scope; a prompt with no scope to
    /// answer it fails the drive, as does `patience` passing.
    fn settle(
        &self,
        approvals: Option<&str>,
        patience: std::time::Duration,
    ) -> Result<serde_json::Value, Failure> {
        let give_up = std::time::Instant::now() + patience;
        loop {
            // Each wait asks no longer than the patience left, so a short
            // patience is not outlasted by one long wait.
            let left = give_up
                .saturating_duration_since(std::time::Instant::now())
                .as_secs()
                .clamp(1, 30);
            let state = self.get(&format!("/wait?until=idle&timeout={left}"))?;
            if state["idle"] == true {
                return Ok(state);
            }
            if let Some(prompt) = state.get("approval") {
                let Some(scope) = approvals else {
                    return Err(Failure(format!(
                        "a command waited on the operator and the scenario names no approvals: {prompt}"
                    )));
                };
                let answer = serde_json::json!({"call": prompt["id"], "scope": scope});
                match self.post("/approve", &answer)? {
                    (204 | 409, _) => {}
                    (status, body) => {
                        return Err(Failure(format!("{answer} was answered {status} {body}")));
                    }
                }
            }
            if std::time::Instant::now() >= give_up {
                return Err(Failure(format!("not idle after {patience:?}: {state}")));
            }
        }
    }

    /// The digest `POST /files` answers for the worktree file `path`.
    fn uploaded(&self, path: &str) -> Result<String, Failure> {
        let worktree = self.worktree.as_ref().ok_or_else(|| {
            Failure(format!(
                "`{path}` is attached and nothing runs in a worktree"
            ))
        })?;
        let bytes = std::fs::read(worktree.join(path))
            .map_err(|why| Failure(format!("`{path}` cannot be read from the worktree: {why}")))?;
        match self.sent("POST", "/files", Some(("image/png", &bytes)))? {
            (200, body) => serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|reply| reply["sha256"].as_str().map(str::to_owned))
                .ok_or_else(|| Failure(format!("POST /files answered {body}"))),
            (status, body) => Err(Failure(format!(
                "POST /files for `{path}`: {status} {body}"
            ))),
        }
    }

    /// Close the tangent `id` (#22), ruling `each` on every live entry born
    /// under it, read from `GET /memory`.
    fn closed(&self, id: &str, each: &str) -> Result<(), Failure> {
        let memory = self.get("/memory")?;
        let dispositions: serde_json::Map<String, serde_json::Value> = memory["entries"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|entry| entry["provenances"][0]["tangent"] == id)
            .filter_map(|entry| entry["id"].as_str().map(str::to_owned))
            .map(|entry| (entry, serde_json::Value::String(each.to_owned())))
            .collect();
        self.command(&serde_json::json!({"kind": "close-tangent", "dispositions": dispositions}))
    }

    /// Post an ask -- `text`, its scoping mark, and the worktree files it
    /// attaches -- and wait until the session is idle; or, with `refused`,
    /// see it refused with that tag.
    fn asked(
        &self,
        (text, scoping, attach, task): (&str, bool, &[String], Option<&str>),
        refused: Option<&str>,
        (approvals, patience): (Option<&str>, std::time::Duration),
    ) -> Result<(), Failure> {
        let mut ask = serde_json::json!({"kind": "ask", "text": text});
        if scoping {
            ask["scoping"] = serde_json::Value::Bool(true);
        }
        if let Some(task) = task {
            ask["task"] = serde_json::Value::String(task.to_owned());
        }
        if !attach.is_empty() {
            let files = attach
                .iter()
                .map(|path| self.uploaded(path))
                .collect::<Result<Vec<_>, _>>()?;
            ask["files"] = serde_json::json!(files);
        }
        let Some(refused) = refused else {
            self.command(&ask)?;
            return self.settle(approvals, patience).map(|_| ());
        };
        let (status, body) = self.post("/commands", &ask)?;
        let tag = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|reply| reply["refused"].as_str().map(str::to_owned));
        if status != 409 || tag.as_deref() != Some(refused) {
            return Err(Failure(format!(
                "the ask was to be refused `{refused}` and was answered {status} {body}"
            )));
        }
        Ok(())
    }

    /// The live entry `pick` names, from `GET /memory`.
    fn picked(&self, pick: &Pick) -> Result<String, Failure> {
        let memory = self.get("/memory")?;
        let entries = memory["entries"].as_array().cloned().unwrap_or_default();
        let found = match pick {
            Pick::Index(at) => entries.get(*at),
            Pick::Containing(text) => entries.iter().find(|entry| {
                entry["content"]
                    .as_str()
                    .is_some_and(|c| c.contains(text.as_str()))
            }),
        };
        found
            .and_then(|entry| entry["id"].as_str())
            .map(str::to_owned)
            .ok_or_else(|| Failure(format!("no live entry is {pick:?}: {memory}")))
    }

    /// The live entry that superseded `id`.
    fn superseding(&self, id: &str) -> Result<String, Failure> {
        let memory = self.get("/memory")?;
        memory["entries"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|entry| entry["supersedes"] == id)
            .and_then(|entry| entry["id"].as_str())
            .map(str::to_owned)
            .ok_or_else(|| Failure(format!("no live entry supersedes {id}")))
    }
}

/// Drive `scenario`'s steps through `driver`, in order (#760): each command
/// posted and, once admitted, waited on until the session is idle; the
/// entries steps bound, by name.
///
/// # Errors
///
/// Returns the [`Failure`] of the first step that did not go as written: a
/// command refused that was to be admitted or admitted that was to be
/// refused, a pick that names no live entry, a prompt with no approvals to
/// answer it, a wait that timed out, or a session that never settled.
pub fn drive(scenario: &Scenario, driver: &Driver) -> Result<Bindings, Failure> {
    drive_from(scenario, driver, (0, false)).map(|(bound, _)| bound)
}

/// [`drive`], from the step at `from`; with `reseat`, stopping after the
/// first declared meditation it settles (#948), and naming the step after
/// it, where the next session takes up the script.
///
/// # Errors
///
/// As [`drive`].
#[allow(clippy::too_many_lines)]
pub fn drive_from(
    scenario: &Scenario,
    driver: &Driver,
    (from, reseat): (usize, bool),
) -> Result<(Bindings, Option<usize>), Failure> {
    let approvals = scenario.approvals.as_deref();
    let mut bound = Bindings::new();
    let mut mark = driver.last_seq()?;
    let mut tangent: Option<&str> = None;
    let mut before: Option<String> = None;
    for (at, step) in scenario.steps.iter().enumerate().skip(from) {
        let failed = |why: Failure| Failure(format!("step {}: {why}", at + 1));
        match step {
            Step::Ask {
                text,
                scoping,
                refused,
                attach,
                task,
            } => {
                // A grade's `before` runs before its task's first ask (#942).
                before = match scenario.steps.get(at + 1) {
                    Some(Step::Grade(grading)) => driver.before(grading).map_err(failed)?,
                    _ => None,
                };
                mark = driver.last_seq().map_err(failed)?;
                driver
                    .asked(
                        (text, *scoping, attach, task.as_deref()),
                        refused.as_deref(),
                        (approvals, scenario.patience(at)),
                    )
                    .map_err(failed)?;
            }
            Step::Grade(grading) => {
                mark = driver.last_seq().map_err(failed)?;
                driver
                    .graded(grading, before.take(), (approvals, scenario.patience(at)))
                    .map_err(failed)?;
            }
            Step::Flag { pick, note, bind } => {
                let id = driver.picked(pick).map_err(failed)?;
                driver
                    .command(&serde_json::json!({"kind": "flag-entry", "id": id, "note": note}))
                    .map_err(failed)?;
                if let Some(name) = bind {
                    bound.insert(name.clone(), id);
                }
            }
            Step::Edit { pick, text, bind } => {
                let id = driver.picked(pick).map_err(failed)?;
                driver
                    .command(&serde_json::json!({"kind": "edit-entry", "id": id, "content": text}))
                    .map_err(failed)?;
                if let Some(name) = bind {
                    let theirs = driver.superseding(&id).map_err(failed)?;
                    bound.insert(name.clone(), theirs);
                }
            }
            Step::DeclareMeditation if reseat => {
                driver.command(&step.command()).map_err(failed)?;
                driver
                    .settle(approvals, scenario.patience(at))
                    .map_err(failed)?;
                return Ok((bound, Some(at + 1)));
            }
            Step::DeclareMeditation
            | Step::RefillAnyway
            | Step::RulePhase { .. }
            | Step::StartPhase { .. }
            | Step::ProposePhase { .. }
            | Step::MovePhase { .. } => {
                mark = driver.last_seq().map_err(failed)?;
                driver.command(&step.command()).map_err(failed)?;
                driver
                    .settle(approvals, scenario.patience(at))
                    .map_err(failed)?;
            }
            Step::OpenTangent { id } => {
                driver
                    .command(&serde_json::json!({"kind": "open-tangent", "id": id}))
                    .map_err(failed)?;
                tangent = Some(id);
            }
            Step::CloseTangent { each } => {
                let id = tangent
                    .take()
                    .ok_or_else(|| failed(Failure("no tangent is open to close".to_owned())))?;
                driver.closed(id, each).map_err(failed)?;
            }
            Step::Wait {
                until,
                timeout,
                matches,
            } => {
                driver
                    .waited((until, matches), mark, timeout.unwrap_or(300))
                    .map_err(failed)?;
            }
            Step::End => {
                driver
                    .command(&serde_json::json!({"kind": "end"}))
                    .map_err(failed)?;
            }
        }
    }
    Ok((bound, None))
}

/// Each of `scenario`'s expectations `log` -- the session's log -- does not
/// meet, with `bound`'s names in place of each `$name`; empty when it meets
/// every one. Matched by [`crate::formats::log_query::find_terms`], the one
/// matcher `diet log find` calls too: `kind=<kind>`, then each field as
/// `key=value` or `key~text`.
#[must_use]
pub fn unmet(scenario: &Scenario, log: &str, bound: &Bindings) -> Vec<String> {
    let bind = |wanted: &str| {
        bound.iter().fold(wanted.to_owned(), |out, (name, id)| {
            out.replace(&format!("${name}"), id)
        })
    };
    let mut first: BTreeMap<&str, i64> = BTreeMap::new();
    let mut missed = Vec::new();
    for expect in &scenario.expects {
        let floor = expect
            .after
            .as_deref()
            .and_then(|name| first.get(name).copied());
        if expect.after.is_some() && floor.is_none() {
            missed.push(format!(
                "{}: `after` names {:?}, which matched nothing",
                expect.kind, expect.after
            ));
            continue;
        }
        let mut terms = vec![format!("kind={}", expect.kind)];
        terms.extend(expect.fields.iter().map(|(key, wanted)| match wanted {
            Match::Equals(text) => format!("{key}={}", bind(text)),
            Match::Contains(text) => format!("{key}~{}", bind(text)),
        }));
        let found = match crate::formats::log_query::find_terms(log, &terms) {
            Ok(found) => found,
            Err(why) => {
                missed.push(format!("{terms:?}: {why}"));
                continue;
            }
        };
        let seqs: Vec<i64> = found
            .iter()
            .filter_map(|line| match line {
                Value::Object(line) => match line.get("seq") {
                    Some(Value::Integer(seq)) => Some(*seq),
                    _ => None,
                },
                _ => None,
            })
            .filter(|seq| floor.is_none_or(|floor| *seq > floor))
            .collect();
        if let (Some(name), Some(seq)) = (&expect.name, seqs.first()) {
            first.insert(name, *seq);
        }
        if (seqs.len() as u64) < expect.at_least {
            missed.push(format!(
                "{terms:?}: {} line(s) matched, {} wanted",
                seqs.len(),
                expect.at_least
            ));
        }
    }
    missed
}

/// What `diet-drive scenario` was told (#760).
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// The scenario file.
    pub file: std::path::PathBuf,
    /// Where the run's outputs go: the assembled run, or with `assemble`
    /// off, the log and the record themselves.
    pub out: std::path::PathBuf,
    /// A named endpoint, for a live re-fire; absent, the canned substrate.
    pub endpoint: Option<String>,
    /// The served model's name on that endpoint.
    pub model: Option<String>,
    /// The endpoint's key file, passed to `serve` as it is.
    pub key_file: Option<String>,
    /// A regimen that beats the scenario's.
    pub regimen: Option<std::path::PathBuf>,
    /// Where the model's commands run; absent, a fresh directory.
    pub worktree: Option<std::path::PathBuf>,
    /// Whether the outputs go to `scripts/assemble-run.py` once the
    /// expectations pass.
    pub assemble: bool,
    /// A dry run (#555): the scenario rehearsed on `canned-scenario-hosted`
    /// under the regimen the live run would take -- `regimen`, else the
    /// scenario's `live_regimen` -- its substrate alone swapped, and the
    /// plan it would spend reported. Nothing leaves loopback.
    pub dry_run: bool,
    /// Which repeat of which script this run is (#910), passed to serve as
    /// `--run`. A counted repeat keeps its outputs, assembled, whether or
    /// not its expectations were met.
    pub run: Option<crate::formats::log::Run>,
    /// The session this one is carried from (#948): the first session's
    /// log, passed to serve as `--carry`. Absent, it starts from its head.
    pub carry: Option<std::path::PathBuf>,
    /// The step this session's drive starts at (#948): a carried session
    /// takes up the script after the meditation the first one ended at.
    pub from_step: usize,
    /// Whether this is a later session of its run, which the regimen's
    /// `refill_session` started (#948): it is never reseated again.
    pub carried: bool,
    /// Paths sealed from this session's commands besides the deny list
    /// (#948, `refill_session = "sealed"`): passed to serve as
    /// `--sandbox-secret`, and probed.
    pub seal: Vec<String>,
    /// Files the probe must fail to read besides `DIET_SANDBOX_PROBE_FILE`
    /// (#948): the scenario's `seal_probe`, filled.
    pub seal_probe: Vec<String>,
}

/// The regimen key for what a declared meditation does to its session
/// (#948).
pub const REFILL_SESSION: &str = "refill_session";

/// What a declared meditation does to its session (#948).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefillSession {
    /// The session goes on: the refill replaces its trunk (the default).
    Same,
    /// The session ends at the refill, and the next starts from its carry:
    /// the refilled trunk and the working object, exactly.
    Fresh,
    /// [`Self::Fresh`], the next session also denied the scenario's `seal`.
    Sealed,
    /// The session ends at the refill, and the next starts from its head
    /// alone, carrying nothing.
    Blank,
}

impl RefillSession {
    /// Its word in a regimen and on the start's levers.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Same => "same",
            Self::Fresh => "fresh",
            Self::Sealed => "sealed",
            Self::Blank => "blank",
        }
    }
}

/// [`REFILL_SESSION`] as `regimen` declares it.
///
/// # Errors
///
/// When it names none of `same`, `fresh`, `sealed` and `blank`.
pub fn refill_session(regimen: &crate::formats::regimen::Regimen) -> Result<RefillSession, String> {
    match regimen.get(REFILL_SESSION) {
        None => Ok(RefillSession::Same),
        Some(crate::formats::regimen::Value::String(word)) => [
            RefillSession::Same,
            RefillSession::Fresh,
            RefillSession::Sealed,
            RefillSession::Blank,
        ]
        .into_iter()
        .find(|state| state.tag() == word)
        .ok_or_else(|| {
            format!("`{REFILL_SESSION}` takes \"same\", \"fresh\", \"sealed\" or \"blank\", not {word:?}")
        }),
        Some(other) => Err(format!("`{REFILL_SESSION}` is a word, not {other:?}")),
    }
}

/// Run a scenario (#760): `serve` started as a child of `drive` -- this
/// binary -- on the canned substrate, its replies the scenario's, or on
/// `options.endpoint`; driven through its HTTP API; its log checked against
/// the expectations; and, when they pass, its outputs handed to the
/// assembler. The outcome as one JSON object, and whether it passed.
#[must_use]
pub fn run(drive: &std::path::Path, options: &RunOptions) -> (serde_json::Value, bool) {
    let mut said = serde_json::json!({"ok": false});
    let passed = match running(drive, options, &mut said) {
        Ok(passed) => passed,
        Err(Failure(why)) => {
            said["failure"] = serde_json::Value::String(why);
            false
        }
    };
    said["ok"] = serde_json::Value::Bool(passed);
    (said, passed)
}

/// How `diet-drive scenario --repeat` repeats (#910).
#[derive(Debug, Clone)]
pub struct Repeats {
    /// How many repeats: each ends before the next starts.
    pub count: u64,
    /// The file whose existence stops the batch between repeats; absent,
    /// `STOP` in the batch's `--out`.
    pub stop_file: Option<std::path::PathBuf>,
    /// How many times one slot's void repeat is re-run before the batch
    /// stops and names it.
    pub void_cap: u64,
}

/// `--void-cap`'s default (#910).
pub const VOID_CAP: u64 = 2;

/// A repeat's directory under the batch's `--out`: `repeat-NN`, from 1.
#[must_use]
pub fn repeat_dir(out: &std::path::Path, repeat: u64) -> std::path::PathBuf {
    out.join(format!("repeat-{repeat:02}"))
}

/// Run a scenario `repeats.count` times (#910), one after another, each its
/// own session: a fresh worktree, its own `repeat-NN/` under `--out`, and
/// serve told which repeat it is (`--run`), so its log's `session.start`
/// and its record's start say so. `emit` is given one line per repeat as
/// it ends. A void repeat -- serve could not run it to its end for a reason
/// outside the model -- is re-run at its slot, its outputs kept beside as
/// `repeat-NN.void-M/`, up to `void_cap` times; then the batch stops and
/// names it. Between repeats the stop file stops it. Unmet expectations
/// are data: they never fail the batch. The batch's line, and whether it
/// ran to its end.
pub fn repeat(
    drive: &std::path::Path,
    options: &RunOptions,
    repeats: &Repeats,
    emit: &mut dyn FnMut(&serde_json::Value),
) -> (serde_json::Value, bool) {
    let mut said =
        serde_json::json!({"batch": options.file.display().to_string(), "of": repeats.count});
    // A scenario that does not read is refused before any repeat runs.
    let script_sha256 = match script_digest(&options.file) {
        Ok(digest) => digest,
        Err(why) => {
            said["failure"] = serde_json::Value::String(why);
            said["ok"] = serde_json::Value::Bool(false);
            return (said, false);
        }
    };
    said["script_sha256"] = serde_json::Value::String(script_sha256.clone());
    if let Err(why) = std::fs::create_dir_all(&options.out) {
        said["failure"] =
            serde_json::Value::String(format!("{} cannot be made: {why}", options.out.display()));
        said["ok"] = serde_json::Value::Bool(false);
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
            said["ok"] = serde_json::Value::Bool(false);
            return (said, false);
        }
    };
    let mut kept = |line: &serde_json::Value| {
        runs.keep(line);
        emit(line);
    };
    let batch = batch_id();
    said["batch"] = serde_json::Value::String(batch.clone());
    let mut ran = 0;
    let mut ok = true;
    for repeat in 1..=repeats.count {
        let run = crate::formats::log::Run {
            script_sha256: script_sha256.clone(),
            repeat,
            of: repeats.count,
            paired: None,
            batch: Some(batch.clone()),
        };
        match slot(
            drive,
            options,
            &run,
            (&stop_file, repeats.void_cap),
            &mut kept,
        ) {
            Slot::Ran => ran += 1,
            Slot::Stopped => {
                said["stopped_at"] = serde_json::json!(repeat);
                said["stopped_by"] = serde_json::Value::String(stop_file.display().to_string());
                break;
            }
            Slot::Voided(voids) => {
                said["void_cap_reached"] = serde_json::json!({"repeat": repeat, "voids": voids});
                ok = false;
                break;
            }
        }
    }
    let ok = runs.report(&mut said) && ok;
    said["ran"] = serde_json::json!(ran);
    said["ok"] = serde_json::Value::Bool(ok);
    (said, ok)
}

/// The sha256 of a scenario file's bytes, once it reads as a scenario.
///
/// # Errors
///
/// When it cannot be read, or is not a scenario.
pub fn script_digest(file: &std::path::Path) -> Result<String, String> {
    std::fs::read_to_string(file)
        .map_err(|why| format!("{} cannot be read: {why}", file.display()))
        .and_then(|text| {
            parse(&text)
                .map(|_| crate::digest::sha256_hex(text.as_bytes()))
                .map_err(|why| why.to_string())
        })
}

/// A batch's `runs.jsonl` (#914): every line it emits, one per attempt,
/// kept beside its runs, where `diet-drive table` reads each run's unmet
/// expectations and whether it was void.
pub(crate) struct Runs {
    path: std::path::PathBuf,
    file: std::fs::File,
    failed: Option<String>,
}

impl Runs {
    /// `runs.jsonl` in `out`, emptied.
    pub(crate) fn create(out: &std::path::Path) -> Result<Self, String> {
        let path = out.join(RUNS);
        let file = std::fs::File::create(&path)
            .map_err(|why| format!("{} cannot be made: {why}", path.display()))?;
        Ok(Self {
            path,
            file,
            failed: None,
        })
    }

    /// Keep `line`; the first write that fails is reported, never skipped.
    pub(crate) fn keep(&mut self, line: &serde_json::Value) {
        use std::io::Write as _;
        if self.failed.is_none()
            && let Err(why) = writeln!(self.file, "{line}")
        {
            self.failed = Some(format!("{} cannot be written: {why}", self.path.display()));
        }
    }

    /// Name the file on the batch's line, and whether every line was kept.
    pub(crate) fn report(self, said: &mut serde_json::Value) -> bool {
        said["runs"] = serde_json::Value::String(self.path.display().to_string());
        match self.failed {
            None => true,
            Some(why) => {
                said["runs_failed"] = serde_json::Value::String(why);
                false
            }
        }
    }
}

/// The file a batch keeps its lines in, under its `--out` (#914).
pub const RUNS: &str = "runs.jsonl";

/// How one repeat's slot ended.
pub(crate) enum Slot {
    /// A repeat ran to its end, met or not.
    Ran,
    /// The stop file was there before it started.
    Stopped,
    /// It voided this many times: one more than the cap.
    Voided(u64),
}

/// One repeat's slot (#910): the run, re-run while it voids, up to `cap`
/// re-runs, the stop file read before each attempt.
pub(crate) fn slot(
    drive: &std::path::Path,
    options: &RunOptions,
    run: &crate::formats::log::Run,
    (stop_file, cap): (&std::path::Path, u64),
    emit: &mut dyn FnMut(&serde_json::Value),
) -> Slot {
    let dir = repeat_dir(&options.out, run.repeat);
    let mut voids = 0;
    loop {
        if stop_file.exists() {
            return Slot::Stopped;
        }
        let attempt = RunOptions {
            out: dir.clone(),
            worktree: None,
            run: Some(run.clone()),
            ..options.clone()
        };
        let (said, _) = self::run(drive, &attempt);
        let mut line = tallied(&said);
        line["repeat"] = serde_json::json!(run.repeat);
        line["of"] = serde_json::json!(run.of);
        line["attempt"] = serde_json::json!(voids + 1);
        if let Some((arm, _)) = &run.paired {
            line["arm"] = serde_json::Value::String(arm.clone());
        }
        if let Some(batch) = &run.batch {
            line["batch"] = serde_json::Value::String(batch.clone());
        }
        if line["void"].is_null() {
            emit(&line);
            return Slot::Ran;
        }
        voids += 1;
        // The void attempt's outputs are kept beside its slot, which the
        // re-run then has to itself.
        if dir.exists() {
            let kept = dir.with_file_name(format!(
                "{}.void-{voids}",
                dir.file_name()
                    .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
            ));
            line["kept"] = match std::fs::rename(&dir, &kept) {
                Ok(()) => serde_json::Value::String(kept.display().to_string()),
                Err(why) => serde_json::Value::String(format!("not kept: {why}")),
            };
        }
        emit(&line);
        if voids > cap {
            return Slot::Voided(voids);
        }
    }
}

/// A batch's id (v25, #932): drawn once per invocation of a repeating
/// runner, from this process and the clock, so two batches of one manifest
/// never share one. A void attempt's re-run at its slot is the same batch.
#[must_use]
pub fn batch_id() -> String {
    crate::drive::regimen::nonce_of(&format!(
        "batch-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
    ))
}

/// One repeat's line (#910), from what [`run`] said and the log it names:
/// whether it voided and why, its turns, tokens, captures, refusals,
/// settles by reason, unmet expectations and record.
fn tallied(said: &serde_json::Value) -> serde_json::Value {
    use crate::formats::log::{Event, FailReason};
    let mut line = serde_json::json!({
        "void": said["void"],
        "ok": said["ok"],
        "unmet": said.get("unmet").cloned().unwrap_or_else(|| serde_json::json!([])),
        "record": said["record"],
    });
    if let Some(failure) = said.get("failure") {
        line["failure"] = failure.clone();
    }
    // The run's deny list and its probe (#942): counts, never a path.
    for key in ["sandbox_secrets", "sandbox_probe"] {
        if let Some(value) = said.get(key) {
            line[key] = value.clone();
        }
    }
    // The session the run went on in (#948), tallied as this one is: its
    // void and failure are the run's to show.
    if let Some(next) = said.get("next") {
        line["next"] = tallied(next);
    }
    let read = said["log"]
        .as_str()
        .and_then(|log| std::fs::read_to_string(log).ok())
        .map(|text| crate::formats::log::parse(&text));
    let lines = match read {
        Some(Ok(lines)) => lines,
        Some(Err(why)) => {
            line["log"] = serde_json::Value::String(format!("unreadable: {why}"));
            return line;
        }
        None => {
            line["log"] = serde_json::Value::String("none".to_owned());
            return line;
        }
    };
    let (mut turns, mut tokens, mut captures, mut refusals) = (0u64, 0u64, 0u64, 0u64);
    let mut settles: BTreeMap<String, u64> = BTreeMap::new();
    for logged in &lines {
        match &logged.event {
            Event::TurnSettled { reason, .. } => {
                turns += 1;
                *settles.entry(reason.tag().to_owned()).or_default() += 1;
            }
            Event::Response {
                usage: Some(usage), ..
            } => tokens += usage.prompt_tokens + usage.completion_tokens,
            Event::Patch { .. } => captures += 1,
            Event::Refused { .. } => refusals += 1,
            // A transport failure at any point is no answer from the model
            // (#910): the connection failed, the call's thread crashed, or
            // it timed out with nothing arriving.
            Event::RequestFailed {
                reason, partial, ..
            } if line["void"].is_null() => {
                let cause = match reason {
                    FailReason::Transport => Some("transport"),
                    FailReason::Crashed => Some("crashed"),
                    FailReason::Timeout if partial.as_deref().is_none_or(str::is_empty) => {
                        Some("timeout")
                    }
                    _ => None,
                };
                if let Some(cause) = cause {
                    line["void"] = serde_json::Value::String(cause.to_owned());
                }
            }
            _ => {}
        }
    }
    line["turns"] = serde_json::json!(turns);
    line["tokens"] = serde_json::json!(tokens);
    line["captures"] = serde_json::json!(captures);
    line["refusals"] = serde_json::json!(refusals);
    line["settles"] = serde_json::json!(settles);
    line
}

/// A file beside the scenario, or as written when absolute.
fn beside(file: &std::path::Path, named: &str) -> std::path::PathBuf {
    file.parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(named)
}

/// Copy each of `seed`'s files, beside the scenario `file`, into `worktree`
/// at its path there.
fn seeded(
    worktree: &std::path::Path,
    file: &std::path::Path,
    seed: &BTreeMap<String, String>,
) -> Result<(), Failure> {
    for (path, source) in seed {
        let to = worktree.join(path);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|why| Failure(format!("{} cannot be made: {why}", parent.display())))?;
        }
        std::fs::copy(beside(file, source), &to)
            .map_err(|why| Failure(format!("the seed `{source}` cannot be copied: {why}")))?;
    }
    Ok(())
}

/// What a canned scenario's model is shown for its recording's directory,
/// wherever that really is.
pub const SHOWN_RECORDING: &str = "/scenario/recording";
/// ... and for its worktree.
pub const SHOWN_WORKTREE: &str = "/scenario/worktree";

/// A directory no other run uses, under the system's temporary one.
fn fresh(what: &str) -> Result<std::path::PathBuf, Failure> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let dir = std::env::temp_dir().join(named(what, std::process::id(), stamp));
    std::fs::create_dir_all(&dir)
        .map_err(|why| Failure(format!("{} cannot be made: {why}", dir.display())))?;
    Ok(dir)
}

/// A fresh directory's name, from this process's id and the time, each
/// zero-padded to its widest (a `u32`'s ten digits; nanoseconds since the
/// epoch, nineteen today, given twenty), so the name -- printed into the
/// model's context wherever a kept output is "saved at" it -- is one length
/// for every run.
fn named(what: &str, pid: u32, stamp: u128) -> String {
    format!("diet-scenario-{what}-{pid:010}-{stamp:020}")
}

/// Copy `work/files/` into `out/files/`, name for name: the recording's
/// copies of the files its asks attached. A partial copy serve never
/// renamed into place (a dot-named temporary) is not one.
fn kept_files(work: &std::path::Path, out: &std::path::Path) -> Result<(), Failure> {
    let from = work.join("files");
    let entries = match std::fs::read_dir(&from) {
        Ok(entries) => entries,
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(why) => return Err(Failure(format!("{} cannot be read: {why}", from.display()))),
    };
    let to = out.join("files");
    std::fs::create_dir_all(&to)
        .map_err(|why| Failure(format!("{} cannot be made: {why}", to.display())))?;
    for entry in entries {
        let entry =
            entry.map_err(|why| Failure(format!("{} cannot be read: {why}", from.display())))?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') || !entry.path().is_file() {
            continue;
        }
        std::fs::copy(entry.path(), to.join(&name))
            .map_err(|why| Failure(format!("{} cannot be kept: {why}", entry.path().display())))?;
    }
    Ok(())
}

/// `failure`, marked void (#910): serve could not run the session to its
/// end for a reason outside the model, `cause` naming which.
fn voided(said: &mut serde_json::Value, cause: &str, failure: Failure) -> Failure {
    said["void"] = serde_json::Value::String(cause.to_owned());
    failure
}

#[allow(clippy::too_many_lines)]
fn running(
    drive: &std::path::Path,
    options: &RunOptions,
    said: &mut serde_json::Value,
) -> Result<bool, Failure> {
    use std::io::BufRead as _;
    let text = std::fs::read_to_string(&options.file)
        .map_err(|why| Failure(format!("{} cannot be read: {why}", options.file.display())))?;
    let scenario = parse(&text).map_err(|why| Failure(why.to_string()))?;
    said["scenario"] = serde_json::Value::String(scenario.name.clone());
    let live = options.endpoint.is_some();
    let regimen = match (
        &options.regimen,
        live || options.dry_run,
        &scenario.live_regimen,
    ) {
        (Some(regimen), _, _) => regimen.clone(),
        (None, false, _) => beside(&options.file, &scenario.regimen),
        (None, true, Some(live_regimen)) => beside(&options.file, live_regimen),
        (None, true, None) => {
            return Err(Failure(
                "a re-fire on an endpoint, or a dry run, needs the scenario's `live_regimen` \
                 or --regimen"
                    .to_owned(),
            ));
        }
    };
    // A dry run (#555): the regimen the live run would take, rehearsed on the
    // canned hosted substrate -- a copy with its substrate alone swapped.
    let (regimen, intended) = if options.dry_run {
        let (copy, intended) = rehearsing(&regimen)?;
        (copy.0.clone(), Some((intended, copy)))
    } else {
        (regimen, None)
    };
    let read = std::fs::read_to_string(&regimen)
        .map_err(|why| Failure(format!("{} cannot be read: {why}", regimen.display())))
        .and_then(|text| {
            crate::formats::regimen::parse(&text)
                .map_err(|why| Failure(format!("{}: {why:?}", regimen.display())))
        })?;
    let substrate = match read.get("substrate") {
        Some(crate::formats::regimen::Value::String(id)) => id.clone(),
        _ => return Err(Failure(format!("{} names no substrate", regimen.display()))),
    };
    said["substrate"] = serde_json::Value::String(substrate.clone());
    // A hosted substrate (#555): an `api` entry, whose replies are served as
    // Anthropic's Messages API serves them. A reply that thinks, counts the
    // cache or answers with an error is said in that wire alone.
    let hosted = served_hosted(&substrate, &scenario.replies)?;
    // Serve's own reading of whether the regimen runs commands.
    let runs_commands = crate::drive::tool_loop::declared(&read)
        .map_err(|why| Failure(format!("{}: {why}", regimen.display())))?
        .is_some();
    // What a declared meditation does to the session (#948).
    let refill =
        refill_session(&read).map_err(|why| Failure(format!("{}: {why}", regimen.display())))?;
    // A seal matters only where a meditation ends the session: a scenario
    // that declares none (a sandbox probe under the run's regimen) runs as
    // `same` would.
    let meditates = scenario
        .steps
        .iter()
        .any(|step| matches!(step, Step::DeclareMeditation));
    if refill == RefillSession::Sealed && meditates && scenario.seal.is_empty() {
        return Err(Failure(format!(
            "{} seals the session after the refill, and the scenario names no `seal`",
            regimen.display()
        )));
    }
    let reseat = refill != RefillSession::Same && !options.carried;
    said["refill_session"] = serde_json::Value::String(refill.tag().to_owned());
    // The run's deny list (#942): every live run or dry run of a regimen that
    // runs commands names one, or waives it, and proves it before it starts.
    let deny =
        crate::drive::probe::deny_of(std::env::var(crate::drive::probe::SECRETS).ok().as_deref())
            .map_err(|why| voided(said, "sandbox", Failure(why)))?;
    let denied = match (&deny, runs_commands && (live || options.dry_run)) {
        (crate::drive::probe::Deny::Unset, true) => {
            return Err(Failure(format!(
                "a live run or a dry run whose regimen runs commands needs {}: the paths its \
                 commands must not read (the private runs, the checkout container, the agents' \
                 transcripts and memory, the operator's keys), colon-separated, or `none`",
                crate::drive::probe::SECRETS
            )));
        }
        (crate::drive::probe::Deny::Paths(paths), _) if runs_commands => paths.clone(),
        _ => Vec::new(),
    };
    // A sealed session's seal (#948) is denied beside the deny list.
    let denied: Vec<String> = denied
        .into_iter()
        .chain(options.seal.iter().cloned())
        .collect();
    said["sandbox_secrets"] = match &deny {
        crate::drive::probe::Deny::Waived => serde_json::json!("waived"),
        _ => serde_json::json!({"count": denied.len()}),
    };
    if !denied.is_empty() && (live || options.dry_run) {
        // Once per batch (#942): a regimen and a deny list already proven
        // in this process are not probed again, and the line says so.
        let key = (
            crate::digest::sha256_hex(&std::fs::read(&regimen).unwrap_or_default()),
            crate::digest::sha256_hex(
                {
                    let mut sorted = denied.clone();
                    sorted.sort();
                    sorted.join("\n")
                }
                .as_bytes(),
            ),
        );
        let cached = PROBED
            .lock()
            .ok()
            .and_then(|probed| probed.get(&key).cloned());
        let probed = match cached {
            Some(mut verdict) => {
                verdict["reused"] = serde_json::Value::Bool(true);
                Ok(verdict)
            }
            None => probed(
                drive,
                &regimen,
                &beside(&options.file, &scenario.head),
                (&denied, &options.seal_probe),
                &options.out,
            ),
        }
        .map_err(|failure| voided(said, "sandbox", failure))?;
        let mut probed = probed;
        // Which regimen it proved (its bytes as probed), so a batch's line
        // shows one proof per regimen and list.
        probed["regimen_sha256"] = serde_json::Value::String(key.0.clone());
        let ok = probed["ok"] == true;
        if ok
            && probed.get("reused").is_none()
            && let Ok(mut cache) = PROBED.lock()
        {
            cache.insert(key, probed.clone());
        }
        said["sandbox_probe"] = probed;
        if !ok {
            return Err(voided(
                said,
                "sandbox",
                Failure(
                    "the sandbox probe read what the deny list denies: refusing the run".to_owned(),
                ),
            ));
        }
    }
    // The warrant gate's classifier, when the regimen gates by one: on the
    // canned substrate, the scenario's own (#862), reporting the model and
    // compute units the regimen declares.
    let classifier = match crate::drive::warrant_gate::read(&read)
        .map_err(|why| Failure(format!("{}: {why}", regimen.display())))?
    {
        crate::drive::warrant_gate::WarrantGate::Classifier(classifier) => Some(classifier),
        crate::drive::warrant_gate::WarrantGate::Regex => None,
    };
    if classifier.is_none() && !scenario.classified.is_empty() {
        return Err(Failure(format!(
            "the scenario scores `classify` lines and {} gates no step by a classifier",
            regimen.display()
        )));
    }
    // The canned substrate: the scenario's own replies, routed.
    let stub = if live {
        None
    } else {
        said["replies_sha256"] = serde_json::Value::String(scenario.replies_digest());
        let build_info = crate::drive::canned::scenario_build_info();
        let listening = match &classifier {
            None => crate::client::stub::Stub::routing(scenario.routes_on(hosted), &build_info, 0),
            Some(classifier) => crate::client::stub::Stub::routing_classifying(
                scenario.routes_on(hosted),
                &build_info,
                0,
                scenario.classifying(classifier),
            ),
        };
        Some(
            listening
                .map_err(|why| Failure(format!("the canned substrate cannot listen: {why}")))?,
        )
    };
    let endpoint = match (&options.endpoint, &stub) {
        (Some(endpoint), _) => endpoint.clone(),
        // The Messages API's path, as a hosted entry is spoken to (#555).
        (None, Some(stub)) if hosted => format!("http://{}/v1/messages", stub.address()),
        (None, Some(stub)) => stub.url(),
        (None, None) => unreachable!("a run is live or canned"),
    };
    let work = if options.assemble {
        fresh("work").map_err(|failure| voided(said, "setup", failure))?
    } else {
        std::fs::create_dir_all(&options.out).map_err(|why| {
            voided(
                said,
                "setup",
                Failure(format!("{} cannot be made: {why}", options.out.display())),
            )
        })?;
        options.out.clone()
    };
    let (log, record) = (work.join("session.log"), work.join("session.record.jsonl"));
    said["log"] = serde_json::Value::String(log.display().to_string());
    said["record"] = serde_json::Value::String(record.display().to_string());
    // Every route asks for this credential; it never reaches the outputs.
    let secrets = fresh("auth").map_err(|failure| voided(said, "setup", failure))?;
    let user_password = format!(
        "scenario:{}",
        crate::digest::sha256_hex(
            format!("{:?}{}", std::time::SystemTime::now(), std::process::id()).as_bytes()
        )
    );
    let auth_file = secrets.join("drive.auth");
    std::fs::write(&auth_file, &user_password).map_err(|why| {
        voided(
            said,
            "setup",
            Failure(format!("the auth file cannot be written: {why}")),
        )
    })?;
    let mut serve = std::process::Command::new(drive);
    serve
        .arg("serve")
        .arg("--endpoint")
        .arg(&endpoint)
        .arg("--model")
        .arg(options.model.as_deref().unwrap_or("scenario"))
        .arg("--head")
        .arg(beside(&options.file, &scenario.head))
        .arg("--regimen")
        .arg(&regimen)
        .arg("--port")
        .arg("0")
        .arg("--auth-file")
        .arg(&auth_file)
        .arg("--log")
        .arg(&log)
        .arg("--record")
        .arg(&record)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped());
    // A dry run rehearses under its rung's declarations (#926).
    if let Some((intended, _)) = &intended
        && intended.limits.is_some()
    {
        serve.arg("--rehearse-as").arg(&intended.rung);
    }
    if let Some(run) = &options.run {
        serve.arg("--run").arg(crate::formats::log::run_text(run));
    }
    // A carried session (#948) starts from the first one's kept carry.
    if let Some(carry) = &options.carry {
        serve.arg("--carry").arg(carry);
    }
    if let Some(key_file) = &options.key_file {
        serve.arg("--key-file").arg(key_file);
    } else if hosted && !live {
        // A canned hosted run sends a rehearsal key (#555), written beside
        // the run's credential, and never the operator's: the environment's
        // key is withheld from serve, so no rehearsal can carry it.
        let key_file = secrets.join("rehearsal.key");
        std::fs::write(&key_file, REHEARSAL_KEY)
            .map_err(|why| Failure(format!("the rehearsal key cannot be written: {why}")))?;
        serve.arg("--key-file").arg(&key_file);
        serve.env_remove(crate::drive::hosted::ANTHROPIC_API_KEY);
    }
    let mut worktree = None;
    if runs_commands {
        let dir = match &options.worktree {
            Some(worktree) => worktree.clone(),
            None => fresh("worktree").map_err(|failure| voided(said, "worktree", failure))?,
        };
        seeded(&dir, &options.file, &scenario.seed)
            .map_err(|failure| voided(said, "worktree", failure))?;
        serve.arg("--worktree").arg(&dir);
        for secret in &denied {
            serve.arg("--sandbox-secret").arg(secret);
        }
        worktree = Some(dir);
    } else if !scenario.seed.is_empty() {
        return Err(Failure(
            "the scenario seeds a worktree and its regimen runs no commands".to_owned(),
        ));
    }
    // Path-free outputs: on the canned substrate the model is shown fixed
    // placeholders for the recording and the worktree, so a run shows the
    // same bytes on any machine. A re-fire on an endpoint is shown where it
    // ran, and serve refuses the roots off a canned substrate anyway.
    if !live {
        serve.arg("--shown-root").arg(format!(
            "{}={SHOWN_RECORDING}",
            std::path::absolute(&work)
                .unwrap_or_else(|_| work.clone())
                .display()
        ));
        if let Some(dir) = &worktree {
            serve
                .arg("--shown-root")
                .arg(format!("{}={SHOWN_WORKTREE}", dir.display()));
        }
    }
    // The canned classifier answers on the canned substrate's own port.
    if let (Some(stub), Some(_)) = (&stub, &classifier) {
        serve
            .arg("--classifier-endpoint")
            .arg(format!("http://{}", stub.address()));
    }
    // Its services (#942): started before serve, outside the sandbox, their
    // ports and values in serve's environment and the run's texts.
    let scenario_dir = std::path::absolute(&options.file)
        .ok()
        .and_then(|file| file.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let mut services = None;
    let scenario = if scenario.services.is_empty()
        && scenario.ports.is_empty()
        && scenario.serve_env.is_empty()
    {
        scenario
    } else {
        let started = crate::drive::services::Running::start(
            &scenario.services,
            &scenario.ports,
            (
                &options.out.join("services"),
                &scenario_dir,
                worktree.as_deref(),
            ),
            &[],
        );
        let mut running = match started {
            Ok(running) => running,
            Err((did, why)) => {
                said["services"] = did;
                return Err(voided(said, "services", Failure(why)));
            }
        };
        let mut sockets = Vec::new();
        let mut refused = None;
        for (name, key) in &scenario.sandbox_unix_sockets {
            let Some(socket) = running.value(&["service", name, key]) else {
                refused = Some(format!(
                    "service `{name}` gave no `{key}` for the sandbox's socket"
                ));
                break;
            };
            sockets.push(socket);
        }
        let mut serve_env = Vec::new();
        for (name, text) in &scenario.serve_env {
            match running.filled(text) {
                Ok(value) => serve_env.push((name.clone(), value)),
                Err(why) => {
                    refused.get_or_insert(format!("`serve_env` `{name}`: {why}"));
                }
            }
        }
        let filled = match refused {
            Some(why) => Err(why),
            None => scenario.filled(&running),
        };
        let filled = match filled {
            Ok(filled) => filled,
            Err(why) => {
                running.stop(&scenario_dir, &[]);
                said["services"] = running.said();
                return Err(voided(said, "services", Failure(why)));
            }
        };
        for socket in sockets {
            serve.arg("--sandbox-unix-socket").arg(socket);
        }
        serve.envs(running.env());
        // Named with their sources, never their values: a run's facts.
        serve.envs(serve_env);
        if !scenario.serve_env.is_empty() {
            said["serve_env"] = serde_json::json!(scenario.serve_env);
        }
        services = Some(running);
        filled
    };
    let mut child = serve
        .spawn()
        .map_err(|why| voided(said, "serve", Failure(format!("serve cannot start: {why}"))))?;
    let stdout = child.stdout.take().expect("piped");
    let mut lines = std::io::BufReader::new(stdout).lines();
    let first = lines.next().and_then(Result::ok).unwrap_or_default();
    // The rest of serve's stdout is read as it comes, so it never blocks.
    let drained = std::thread::spawn(move || lines.map_while(Result::ok).collect::<Vec<_>>());
    let started: serde_json::Value = serde_json::from_str(&first).unwrap_or_default();
    let Some(listening) = started["listening"].as_str() else {
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&secrets);
        if let Some(mut running) = services.take() {
            running.stop(&scenario_dir, &[]);
            said["services"] = running.said();
        }
        return Err(voided(
            said,
            "serve",
            Failure(format!("serve did not start: {first}")),
        ));
    };
    let mut driver = Driver::new(listening, Some(&user_password))
        .logging_to(log.clone())
        .for_scenario_in(scenario_dir.clone())
        .grading_out_to(options.out.join("grades"));
    if let Some(running) = &services {
        driver = driver.telling(running.env());
    }
    if let Some(worktree) = worktree {
        driver = driver.in_worktree(worktree);
    }
    let outcome = self::drive_from(&scenario, &driver, (options.from_step, reseat));
    let next = outcome.as_ref().ok().and_then(|(_, next)| *next);
    // A check that crashed (#942): void, whatever else the run says.
    if let Some(cause) = driver.voided() {
        said["void"] = serde_json::Value::String(cause.to_owned());
    }
    if outcome.is_err() || next.is_some() {
        let _ = driver.command(&serde_json::json!({"kind": "end"}));
    }
    // Serve exits once the session ends and its record is written.
    let give_up = std::time::Instant::now() + std::time::Duration::from_secs(120);
    let exited = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if std::time::Instant::now() < give_up => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let _ = drained.join();
    let _ = std::fs::remove_dir_all(&secrets);
    // A sealed next session's seal (#948), from this run's services' values,
    // read before they stop: the same checkout the next one's will check.
    let sealed = match (next, refill) {
        (Some(_), RefillSession::Sealed) => Some(seal_of(&scenario, services.as_ref())?),
        _ => None,
    };
    if let Some(mut running) = services.take() {
        running.stop(&scenario_dir, &[]);
        said["services"] = running.said();
    }
    let unmatched = stub
        .as_ref()
        .map(crate::client::stub::Stub::unmatched)
        .unwrap_or_default();
    said["unmatched"] = serde_json::json!(unmatched);
    // Serve crashed, or never exited (#910): void, whatever the drive said.
    let clean = exited.is_some_and(|status| status.success());
    if !clean {
        said["void"] = serde_json::Value::String("serve".to_owned());
    }
    // A dry run's plan, from whatever the log holds (#926): complete only
    // when the drive ran its last step and serve exited cleanly.
    if let Some((intended, _)) = &intended {
        // A log that does not read has no plan, and says why.
        let mut plan = std::fs::read_to_string(&log)
            .map_err(|why| Failure(format!("the log cannot be read: {why}")))
            .and_then(|written| intended.plan(&written))
            .unwrap_or_else(|Failure(why)| serde_json::json!({"unplanned": why}));
        plan["complete"] =
            serde_json::Value::Bool(outcome.is_ok() && clean && plan.get("unplanned").is_none());
        // Its grades (#942): the graders graded canned replies, so every
        // attempt ran; they bound the requests and say nothing of a task.
        let grades: Vec<serde_json::Value> = std::fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|line| line["kind"] == "grade")
            .collect();
        if !grades.is_empty() {
            plan["grades"] = serde_json::json!({
                "graded": grades.len(),
                "right": grades.iter().filter(|grade| grade["right"] == true).count(),
                "is": "canned: the graders graded the canned replies, so every attempt ran; an \
                       upper bound on the requests, not a result",
            });
        }
        if let Err(Failure(why)) = &outcome {
            plan["stopped_by"] = serde_json::Value::String(why.clone());
        }
        said["plan"] = plan;
    }
    let (bound, _) = outcome?;
    if !clean {
        return Err(Failure(format!("serve did not exit cleanly: {exited:?}")));
    }
    // The next session of this run (#948): started from this one's log and
    // recording, before they are assembled away, in `next/`, the script
    // taken up after the meditation. Its outcome is the run's, and its log
    // meets the scenario's expectations.
    let next_met = match next {
        Some(from_step) => {
            let (seal, seal_probe) = sealed.unwrap_or_default();
            let carried = RunOptions {
                out: options.out.join("next"),
                worktree: None,
                carry: (refill != RefillSession::Blank).then(|| log.clone()),
                from_step,
                carried: true,
                seal,
                seal_probe,
                ..options.clone()
            };
            let mut next_said = serde_json::json!({"ok": false});
            let met = running(drive, &carried, &mut next_said).unwrap_or_else(|Failure(why)| {
                next_said["failure"] = serde_json::Value::String(why);
                false
            });
            next_said["ok"] = serde_json::Value::Bool(met);
            if let Some(cause) = next_said.get("void").cloned() {
                said["void"] = cause;
            }
            said["next"] = next_said;
            Some(met)
        }
        None => None,
    };
    let written = std::fs::read_to_string(&log)
        .map_err(|why| Failure(format!("the log cannot be read: {why}")))?;
    let missed = if next_met.is_some() {
        Vec::new()
    } else {
        unmet(&scenario, &written, &bound)
    };
    said["unmet"] = serde_json::json!(missed);
    let met = missed.is_empty() && unmatched.is_empty();
    // A counted repeat is data either way (#910): it is assembled.
    if !met && options.run.is_none() {
        return Ok(false);
    }
    if options.assemble {
        let mut driven_by = format!("scripted: scenario {}", scenario.name);
        if live {
            let _ = write!(driven_by, ", re-fired on {substrate}");
        } else {
            let _ = write!(
                driven_by,
                ", canned replies sha256 {}",
                scenario.replies_digest()
            );
        }
        // What the run already wrote in `out` -- its services', its
        // grader's, a carried session's -- waits beside it: the assembler
        // builds `out` whole and refuses one that exists. It goes back in
        // after, whatever the assembler answered.
        let aside = set_aside(&options.out)?;
        let assembled = std::process::Command::new("python3")
            .arg("scripts/assemble-run.py")
            .arg("--log")
            .arg(&log)
            .arg("--record")
            .arg(&record)
            .arg("--regimen")
            .arg(&regimen)
            .arg("--out")
            .arg(&options.out)
            .arg("--driven-by")
            .arg(&driven_by)
            .output();
        put_back(aside.as_deref(), &options.out)?;
        let assembled = assembled.map_err(|why| {
            voided(
                said,
                "assembler",
                Failure(format!("the assembler cannot run: {why}")),
            )
        })?;
        let reply = String::from_utf8_lossy(&assembled.stdout);
        said["assembled"] = serde_json::from_str(reply.lines().last().unwrap_or(""))
            .unwrap_or_else(|_| serde_json::Value::String(reply.into_owned()));
        if !assembled.status.success() {
            // A refusal is the assembler's answer; anything else, a crash.
            if said["assembled"].get("refused").is_none() {
                said["void"] = serde_json::Value::String("assembler".to_owned());
            }
            return Ok(false);
        }
        // The copies of the files its asks attached are the run's too, kept
        // as serve's recording keeps them (`files/<sha256>`, #372): the log
        // names them there and a replay fetches them from the run.
        kept_files(&work, &options.out)?;
        // The assembled copies, scrubbed, are the run's; the work goes.
        said["log"] =
            serde_json::Value::String(options.out.join("session.log").display().to_string());
        said["record"] =
            serde_json::Value::String(options.out.join("record.jsonl").display().to_string());
        let _ = std::fs::remove_dir_all(&work);
    }
    Ok(next_met.unwrap_or(met))
}

/// `out` moved aside, when the run has already written in it, to a sibling
/// of its own; `None` when there is nothing to move.
fn set_aside(out: &std::path::Path) -> Result<Option<std::path::PathBuf>, Failure> {
    let holds = std::fs::read_dir(out).is_ok_and(|mut entries| entries.next().is_some());
    if !holds {
        // An empty `out` is the assembler's to make.
        let _ = std::fs::remove_dir(out);
        return Ok(None);
    }
    let aside = out.with_file_name(format!(
        ".{}.parts",
        out.file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
    ));
    std::fs::rename(out, &aside).map_err(|why| {
        Failure(format!(
            "{} cannot be set aside for the assembler: {why}",
            out.display()
        ))
    })?;
    Ok(Some(aside))
}

/// What [`set_aside`] moved, back into `out` beside what the assembler made
/// there, or into `out` alone when it made nothing. A name both hold is
/// refused rather than overwritten.
fn put_back(aside: Option<&std::path::Path>, out: &std::path::Path) -> Result<(), Failure> {
    let Some(aside) = aside else {
        return Ok(());
    };
    let failed = |why: String| Failure(format!("the run's own outputs cannot go back in: {why}"));
    std::fs::create_dir_all(out).map_err(|why| failed(format!("{}: {why}", out.display())))?;
    let entries =
        std::fs::read_dir(aside).map_err(|why| failed(format!("{}: {why}", aside.display())))?;
    for entry in entries {
        let entry = entry.map_err(|why| failed(why.to_string()))?;
        let to = out.join(entry.file_name());
        if to.exists() {
            return Err(failed(format!(
                "{} is both the run's and the assembler's",
                to.display()
            )));
        }
        std::fs::rename(entry.path(), &to)
            .map_err(|why| failed(format!("{}: {why}", to.display())))?;
    }
    std::fs::remove_dir(aside).map_err(|why| failed(format!("{}: {why}", aside.display())))
}

/// A sealed session's seal (#948): the scenario's `seal`, each a service's
/// value, and its `seal_probe`, filled, from this run's services.
fn seal_of(
    scenario: &Scenario,
    services: Option<&crate::drive::services::Running>,
) -> Result<(Vec<String>, Vec<String>), Failure> {
    let running = services.ok_or_else(|| {
        Failure("the scenario seals service values and starts no service".to_owned())
    })?;
    let seal = scenario
        .seal
        .iter()
        .map(|(name, key)| {
            running
                .value(&["service", name, key])
                .ok_or_else(|| Failure(format!("service `{name}` gave no `{key}` to seal")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let probe = scenario
        .seal_probe
        .iter()
        .map(|text| {
            running
                .filled(text)
                .map_err(|why| Failure(format!("`seal_probe`: {why}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((seal, probe))
}

/// The probes passed in this process (#942), by the digests of the
/// regimen they ran under and of the sorted deny list: a batch probes each
/// pair once.
static PROBED: std::sync::Mutex<BTreeMap<(String, String), serde_json::Value>> =
    std::sync::Mutex::new(BTreeMap::new());

/// Run the sandbox probe (#942) under `regimen`, its substrate swapped for
/// the canned hosted one unless it is a rehearsal already, with `secrets`
/// denied: its verdict ([`crate::drive::probe::verdict`]).
fn probed(
    drive: &std::path::Path,
    regimen: &std::path::Path,
    head: &std::path::Path,
    (secrets, sealed_files): (&[String], &[String]),
    kept_in: &std::path::Path,
) -> Result<serde_json::Value, Failure> {
    use crate::drive::probe::{PROBE_FILE, PROBE_PHRASE};
    let needed = |name: &str| {
        std::env::var(name).map_err(|_| {
            Failure(format!(
                "the sandbox probe needs {name}: {}",
                if name == PROBE_FILE {
                    "a file the model must not read"
                } else {
                    "text only the denied files hold"
                }
            ))
        })
    };
    let (file, phrase) = (needed(PROBE_FILE)?, needed(PROBE_PHRASE)?);
    let home = std::env::var("HOME")
        .map_err(|_| Failure("the probe greps from $HOME, and it is unset".to_owned()))?;
    let grep_in = crate::drive::probe::grep_roots(
        std::path::Path::new(&home),
        secrets,
        Some(&std::env::temp_dir()),
    );
    let rehearsal = on_canned(regimen)?;
    let dir = fresh("probe")?;
    // The canary (#942): a string only this file holds, in the temporary
    // directory the search covers, which a search that ran must list.
    let canary = format!(
        "diet-probe-canary-{}",
        crate::digest::sha256_hex(
            format!("{:?}{}", std::time::SystemTime::now(), std::process::id()).as_bytes()
        )
    );
    let canary_file = dir.join("canary.txt");
    std::fs::write(&canary_file, &canary)
        .map_err(|why| Failure(format!("the probe's canary cannot be written: {why}")))?;
    let file_path = dir.join("probe.jsonl");
    let text = crate::drive::probe::scenario(
        &rehearsal.0,
        &std::path::absolute(head).unwrap_or_else(|_| head.to_path_buf()),
        secrets,
        (
            &std::iter::once(file.clone())
                .chain(sealed_files.iter().cloned())
                .collect::<Vec<_>>(),
            &phrase,
            &grep_in,
        ),
        &canary,
    );
    std::fs::write(&file_path, text)
        .map_err(|why| Failure(format!("the probe cannot be written: {why}")))?;
    let out = dir.join("out");
    let ran = std::process::Command::new(drive)
        .arg("scenario")
        .arg(&file_path)
        .arg("--out")
        .arg(&out)
        .arg("--no-assemble")
        .env(crate::drive::probe::SECRETS, secrets.join(":"))
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|why| Failure(format!("the probe cannot run: {why}")))?;
    let log = std::fs::read_to_string(out.join("session.log")).map_err(|why| {
        Failure(format!(
            "the probe left no log ({why}): {}",
            String::from_utf8_lossy(&ran.stdout)
                .lines()
                .last()
                .unwrap_or("")
        ))
    })?;
    let verdict = crate::drive::probe::verdict(
        &log,
        (secrets.len(), 1 + sealed_files.len()),
        &canary_file.display().to_string(),
    );
    // A failed probe's log is kept beside the run, for whoever reads why.
    if verdict["ok"] != true {
        let _ = std::fs::create_dir_all(kept_in);
        let _ = std::fs::write(kept_in.join("sandbox-probe.log"), &log);
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok(verdict)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = include_str!("../../scenarios/fixtures/minimal.jsonl");

    #[test]
    #[allow(clippy::too_many_lines)]
    fn the_minimal_fixture_reads_line_for_line() {
        let read = parse(MINIMAL).expect("it reads");
        assert_eq!(read.name, "minimal");
        assert_eq!(read.regimen, "minimal.toml");
        assert_eq!(read.live_regimen.as_deref(), Some("../drive/floor.toml"));
        assert_eq!(read.head, "../drive/heads/floor.md");
        assert_eq!(read.approvals.as_deref(), Some("session"));
        assert_eq!(
            read.seed,
            BTreeMap::from([("AGENTS.md".to_owned(), "minimal.agents.md".to_owned())])
        );
        assert_eq!(
            read.replies,
            [
                Reply {
                    when: Some("Record the decisions".to_owned()),
                    text: "DECISION: a tracker".to_owned(),
                    calls: Vec::new(),
                    delay_ms: None,
                    prompt_tokens: None,
                    thinking: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    status: None,
                },
                Reply {
                    when: None,
                    text: "Writing it.".to_owned(),
                    calls: vec![Call {
                        id: "call-w".to_owned(),
                        name: "write".to_owned(),
                        arguments: r#"{"path":"a.py","content":"x"}"#.to_owned(),
                    }],
                    delay_ms: None,
                    prompt_tokens: None,
                    thinking: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    status: None,
                },
                Reply {
                    when: None,
                    text: "Done.".to_owned(),
                    calls: Vec::new(),
                    delay_ms: Some(250),
                    prompt_tokens: Some(1200),
                    thinking: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    status: None,
                },
            ]
        );
        assert_eq!(
            read.classified,
            [Classify {
                when: "No such file".to_owned(),
                hypothesis: "went wrong".to_owned(),
                score: "0.93".to_owned(),
            }]
        );
        assert_eq!(
            read.steps,
            [
                Step::StartPhase {
                    phase: "orient".to_owned(),
                },
                Step::Ask {
                    text: "write a.py".to_owned(),
                    scoping: true,
                    refused: None,
                    attach: Vec::new(),
                    task: None,
                },
                Step::Ask {
                    text: "what is in shot.png".to_owned(),
                    scoping: false,
                    refused: None,
                    attach: vec!["shot.png".to_owned()],
                    task: None,
                },
                Step::OpenTangent {
                    id: "t/1".to_owned(),
                },
                Step::CloseTangent {
                    each: "keep".to_owned(),
                },
                Step::Flag {
                    pick: Pick::Index(0),
                    note: "stale?".to_owned(),
                    bind: Some("flagged".to_owned()),
                },
                Step::Edit {
                    pick: Pick::Containing("tracker".to_owned()),
                    text: "decision: a ledger".to_owned(),
                    bind: Some("edited".to_owned()),
                },
                Step::DeclareMeditation,
                Step::Wait {
                    until: "meditation.held".to_owned(),
                    timeout: Some(60),
                    matches: Vec::new(),
                },
                Step::Ask {
                    text: "go on".to_owned(),
                    scoping: false,
                    refused: Some("meditation-held".to_owned()),
                    attach: Vec::new(),
                    task: None,
                },
                Step::RefillAnyway,
                Step::RulePhase {
                    call: "call-p".to_owned(),
                    choice: "keep".to_owned(),
                },
                Step::ProposePhase {
                    to: "build".to_owned(),
                    meditate: Some(false),
                },
                Step::MovePhase {
                    to: "plan".to_owned(),
                    meditate: None,
                },
                Step::End,
            ]
        );
        assert_eq!(
            read.expects,
            [
                Expect {
                    kind: "fork".to_owned(),
                    fields: vec![
                        ("trigger".to_owned(), Match::Contains("call:".to_owned())),
                        ("why".to_owned(), Match::Equals("read".to_owned())),
                    ],
                    at_least: 2,
                    name: Some("fork".to_owned()),
                    after: None,
                },
                Expect {
                    kind: "meditation.held".to_owned(),
                    fields: vec![(
                        "unaddressed".to_owned(),
                        Match::Contains("$flagged".to_owned())
                    )],
                    at_least: 1,
                    name: None,
                    after: Some("fork".to_owned()),
                },
            ]
        );
    }

    /// A fresh directory's name is as long whatever the process id and the
    /// time: the work directory's path is printed into the model's context
    /// (a kept output is "saved at" it), so a longer name moved `reserved`
    /// and every `*_tokens` and `*_bytes` after it by one or two.
    #[test]
    fn a_fresh_directorys_name_is_as_long_whatever_its_pid_and_time() {
        let names = [
            named("work", 7, 0),
            named("work", 78_357, 1_792_012_742_438_612_000),
            named("work", 123_456, 1_792_012_742_438_612_000),
            named("work", u32::MAX, u128::from(u64::MAX)),
        ];
        let lengths: Vec<usize> = names.iter().map(String::len).collect();
        assert!(lengths.iter().all(|&l| l == lengths[0]), "{names:?}");
        assert_ne!(names[1], names[2], "distinct processes, distinct names");
    }

    /// The replies' digest moves with any reply and with nothing else.
    #[test]
    fn the_replies_digest_is_of_the_replies_alone() {
        let read = parse(MINIMAL).expect("it reads");
        let digest = read.replies_digest();
        assert_eq!(digest.len(), 64);
        let mut fewer_steps = read.clone();
        fewer_steps.steps.pop();
        fewer_steps.expects.clear();
        assert_eq!(fewer_steps.replies_digest(), digest);
        let mut other = read.clone();
        other.replies[2].text.push('!');
        assert_ne!(other.replies_digest(), digest);
        // The canned classifier's rules are what it plays too (#862).
        let mut rescored = read.clone();
        rescored.classified[0].score = "0.5".to_owned();
        assert_ne!(rescored.replies_digest(), digest);
        assert_eq!(read.routes().len(), 3);
    }

    /// #555 step 4c: a dry run's plan counts every request of the log by
    /// its lane and, for a fork's, by the fork's warrant; sums their sized
    /// prompts and their `max_tokens`; and counts the retries -- each count
    /// what the log's own lines say, read here a second way. The kitchen
    /// sink's example carries every lane: trunk, interview, audit, warm.
    #[test]
    fn a_dry_runs_plan_counts_every_lane_and_fork_kind_its_log_holds() {
        let example: serde_json::Value = serde_json::from_str(include_str!(
            "../../../exercise/src/drive/examples/kitchen-sink.json"
        ))
        .expect("the example");
        let lines = example["log"].as_array().expect("its log");
        let written = lines
            .iter()
            .map(serde_json::Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        let intended = Intended {
            rung: "a-rung".to_owned(),
            model: Some("a-model".to_owned()),
            limits: Some(crate::drive::hosted::RequestLimits {
                max_api_requests: 9,
                api_retries: 1,
                api_retry_after_max_seconds: 5,
            }),
            rehearsed_on: REHEARSED_ON,
            api: crate::drive::registry::ApiDeclared {
                serving_context: Some(1_000),
                max_output_tokens: Some(100),
                ..crate::drive::registry::ApiDeclared::default()
            },
        };
        let plan = intended.plan(&written).expect("a plan");
        let rehearsal = &plan["rehearsal"];
        let requests: Vec<&serde_json::Value> = lines
            .iter()
            .filter(|line| line["kind"] == "request")
            .collect();
        let mut by_lane: BTreeMap<String, u64> = BTreeMap::new();
        for request in &requests {
            *by_lane
                .entry(request["lane"].as_str().expect("a lane").to_owned())
                .or_default() += 1;
        }
        for lane in ["trunk", "interview", "audit", "warm"] {
            assert!(
                by_lane.contains_key(lane),
                "the example has no {lane}: {by_lane:?}"
            );
        }
        for (lane, count) in &by_lane {
            assert_eq!(rehearsal["by_lane"][lane]["requests"], *count, "{lane}");
            let of = |key: &str| -> u64 {
                requests
                    .iter()
                    .filter(|request| request["lane"] == lane.as_str())
                    .filter_map(|request| request[key].as_u64())
                    .sum()
            };
            assert_eq!(
                rehearsal["by_lane"][lane]["input_sized"],
                of("sized_tokens")
            );
            assert_eq!(
                rehearsal["by_lane"][lane]["output_at_max_tokens"],
                of("max_tokens")
            );
        }
        assert_eq!(
            rehearsal["input_split"]["of_which_cache_read"],
            serde_json::Value::Null
        );
        assert!(
            rehearsal["input_split"]["is"]
                .as_str()
                .is_some_and(|text| text.starts_with("unestimated"))
        );
        assert_eq!(rehearsal["requests"], requests.len());
        for lane in ["interview", "audit"] {
            let kinds = rehearsal["by_fork_kind"][lane]
                .as_object()
                .unwrap_or_else(|| panic!("{lane}'s kinds: {plan}"));
            let summed: u64 = kinds.values().filter_map(serde_json::Value::as_u64).sum();
            assert_eq!(
                summed, by_lane[lane],
                "{lane}: every request names its fork"
            );
        }
        let sum = |key: &str| -> u64 {
            requests
                .iter()
                .filter_map(|request| request[key].as_u64())
                .sum()
        };
        assert_eq!(rehearsal["sized_tokens"], sum("sized_tokens"));
        assert_eq!(rehearsal["output_tokens_at_max_tokens"], sum("max_tokens"));
        assert_eq!(plan["bound"]["tokens"], 9 * 1_100);
    }

    /// A hosted reply's fields (#555): an error status takes its
    /// `retry-after` and needs no text; thinking and cache counts are read;
    /// a substrate that is not hosted refuses them, and a hosted one serves
    /// them.
    #[test]
    fn a_hosted_reply_reads_and_only_a_hosted_substrate_takes_it() {
        let text = concat!(
            r#"{"scenario":"s","regimen":"r.toml","head":"h.md"}"#,
            "\n",
            r#"{"reply":{"thinking":"t","text":"a","cache_read_tokens":512,"cache_write_tokens":64}}"#,
            "\n",
            r#"{"reply":{"status":529,"retry_after":7}}"#,
            "\n",
            r#"{"reply":{"text":"plain"}}"#,
            "\n",
        );
        let read = parse(text).expect("it reads");
        assert_eq!(read.replies[0].thinking.as_deref(), Some("t"));
        assert_eq!(read.replies[0].cache_read_tokens, Some(512));
        assert_eq!(read.replies[0].cache_write_tokens, Some(64));
        assert_eq!(read.replies[0].status, None);
        assert_eq!(read.replies[1].status, Some((529, Some(7))));
        assert_eq!(read.replies[1].text, "");
        assert!(read.replies[0].is_hosted_only() && read.replies[1].is_hosted_only());
        assert!(!read.replies[2].is_hosted_only());
        let refused = served_hosted("canned-scenario", &read.replies).expect_err("refused");
        assert!(
            refused.0.contains("canned-scenario is not one"),
            "{refused}"
        );
        assert_eq!(
            served_hosted("canned-scenario", &read.replies[2..]).map_err(|f| f.0),
            Ok(false)
        );
        assert_eq!(
            served_hosted("canned-scenario-hosted", &read.replies).map_err(|f| f.0),
            Ok(true)
        );
        // The digest moves with a hosted field, and a scenario without them
        // digests as before.
        let mut other = read.clone();
        other.replies[1].status = Some((429, Some(7)));
        assert_ne!(other.replies_digest(), read.replies_digest());
    }

    /// Every way a line can be wrong is refused, naming its line.
    #[test]
    fn a_line_it_cannot_read_is_refused_by_its_number() {
        let header = r#"{"scenario":"s","regimen":"r.toml","head":"h.md"}"#;
        for (body, at) in [
            (r#"{"step":"ask"}"#, 2),
            (r#"{"step":"dance"}"#, 2),
            (r#"{"step":"ask","text":"a","loud":true}"#, 2),
            (r#"{"step":"flag","pick":-1,"note":"n"}"#, 2),
            (r#"{"reply":{"when":"x"}}"#, 2),
            (r#"{"expect":"fork","where":{"a":1}}"#, 2),
            (r#"{"expect":"fork","after":"nobody"}"#, 2),
            (r#"{"expect":"fork","at_least":0}"#, 2),
            (r#"{"step":"end","reply":{"text":"x"}}"#, 2),
            (r#"{"step":"wait"}"#, 2),
            (r#"{"step":"ask","text":"a","patience":0}"#, 2),
            (r#"{"step":"ask","text":"a","patience":"long"}"#, 2),
            (r#"{"step":"flag","pick":0,"note":"n","patience":60}"#, 2),
            (r#"{"step":"wait","until":"idle","patience":60}"#, 2),
            (r#"{"step":"end","patience":60}"#, 2),
            (r#"{"classify":{"when":"x","hypothesis":"y"}}"#, 2),
            (
                r#"{"classify":{"when":"x","hypothesis":"y","score":"1.5"}}"#,
                2,
            ),
            (
                r#"{"classify":{"when":"x","hypothesis":"y","score":"high"}}"#,
                2,
            ),
            (
                r#"{"classify":{"when":"x","hypothesis":"y","score":"0.5","loud":true}}"#,
                2,
            ),
            (r#"{"reply":{"status":200}}"#, 2),
            (r#"{"reply":{"status":600}}"#, 2),
            (r#"{"reply":{"text":"x","retry_after":7}}"#, 2),
            (r#"{"reply":{"text":"x","cache_read_tokens":"512"}}"#, 2),
        ] {
            let text = format!("{header}\n{body}\n");
            assert!(
                matches!(parse(&text), Err(ScenarioError::Line(line, _)) if line == at),
                "{body}: {:?}",
                parse(&text)
            );
        }
        assert!(matches!(
            parse(r#"{"step":"end"}"#),
            Err(ScenarioError::Line(1, _))
        ));
        assert!(matches!(
            parse(&format!("{header}\n{{\"step\":\"end\"}}\n{header}\n")),
            Err(ScenarioError::Line(3, _))
        ));
        assert!(matches!(
            parse("{\"scenario\":\n\"s\"}"),
            Err(ScenarioError::Syntax(_))
        ));
        assert!(matches!(
            parse(&format!(
                "{}\n",
                r#"{"scenario":"s","regimen":"r.toml","head":"h.md","approvals":"always"}"#
            )),
            Err(ScenarioError::Line(1, _))
        ));
    }

    /// A step that waits for the session to settle takes its own
    /// `patience`, in seconds; one that names none keeps
    /// [`STEP_PATIENCE`].
    #[test]
    fn a_settling_step_reads_its_patience_and_the_rest_keep_the_default() {
        let text = concat!(
            r#"{"scenario":"s","regimen":"r.toml","head":"h.md"}"#,
            "\n",
            r#"{"step":"ask","text":"a","patience":3600}"#,
            "\n",
            r#"{"step":"declare-meditation","patience":120}"#,
            "\n",
            r#"{"step":"ask","text":"b"}"#,
            "\n",
            r#"{"step":"end"}"#,
            "\n",
        );
        let read = parse(text).expect("it reads");
        assert_eq!(read.steps.len(), 4);
        assert_eq!(read.patience(0), std::time::Duration::from_secs(3600));
        assert_eq!(read.patience(1), std::time::Duration::from_secs(120));
        assert_eq!(read.patience(2), STEP_PATIENCE);
        assert_eq!(read.patience(3), STEP_PATIENCE);
    }

    /// #910: a repeat's line reads a transport failure at any point off its
    /// log as void -- the connection failed, the call's thread crashed, or
    /// it timed out with nothing arriving -- and an HTTP error the server
    /// returned, or a timeout after something arrived, as data.
    #[test]
    fn a_repeats_line_calls_a_transport_failure_void_and_an_answer_data() {
        const FAILED: &str =
            include_str!("../../formats/log/fixtures/valid/a-turn-whose-connection-failed.jsonl");
        let silent_timeout = FAILED.replace(
            r#""message":"could not connect: refused","reason":"transport""#,
            r#""message":"the server did not finish in 5s","reason":"timeout""#,
        );
        assert_ne!(
            silent_timeout, FAILED,
            "the timeout is made from the fixture"
        );
        let dir = fresh("tallied").expect("a scratch directory");
        for (named, text, void) in [
            (
                "connection",
                FAILED.to_owned(),
                serde_json::json!("transport"),
            ),
            (
                "crashed",
                include_str!("../../formats/log/fixtures/valid/a-turn-whose-thread-crashed.jsonl")
                    .to_owned(),
                serde_json::json!("crashed"),
            ),
            (
                "silent-timeout",
                silent_timeout,
                serde_json::json!("timeout"),
            ),
            (
                "timeout-after-text",
                include_str!("../../formats/log/fixtures/valid/a-turn-that-ran-out-of-time.jsonl")
                    .to_owned(),
                serde_json::Value::Null,
            ),
            (
                "server",
                include_str!("../../formats/log/fixtures/valid/a-turn-the-server-refused.jsonl")
                    .to_owned(),
                serde_json::Value::Null,
            ),
        ] {
            let log = dir.join(format!("{named}.log"));
            std::fs::write(&log, text).expect("the log");
            let line = tallied(&serde_json::json!({"log": log.display().to_string(), "ok": false}));
            assert_eq!(line["void"], void, "{named}: {line}");
            assert_eq!(line["turns"], 1, "{named}: {line}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A run's own outputs go back beside the assembled run; a name both
    /// hold is refused, and the assembler's file is left as it made it.
    #[test]
    fn a_runs_own_outputs_go_back_beside_the_assembled_run_and_never_over_it() {
        let dir = std::env::temp_dir().join(format!("diet-put-back-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let out = dir.join("repeat-01");
        std::fs::create_dir_all(out.join("grades")).expect("the run's outputs");
        std::fs::write(out.join("grades").join("seen.txt"), "graded").expect("a grade's file");
        let aside = set_aside(&out)
            .expect("set aside")
            .expect("something to set aside");
        assert!(!out.exists(), "out is the assembler's to make");
        std::fs::create_dir_all(&out).expect("the assembled run");
        std::fs::write(out.join("record.jsonl"), "assembled").expect("its record");
        put_back(Some(&aside), &out).expect("put back");
        assert_eq!(
            std::fs::read_to_string(out.join("grades").join("seen.txt"))
                .ok()
                .as_deref(),
            Some("graded")
        );
        assert!(!aside.exists(), "nothing left aside");
        std::fs::create_dir_all(&aside).expect("aside again");
        std::fs::write(aside.join("record.jsonl"), "the run's").expect("a clash");
        assert!(
            put_back(Some(&aside), &out).is_err(),
            "a name both hold is refused"
        );
        assert_eq!(
            std::fs::read_to_string(out.join("record.jsonl"))
                .ok()
                .as_deref(),
            Some("assembled"),
            "the assembler's file is left as it made it"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #948: a run that went on in a second session shows it on its line,
    /// tallied as the first is, with the void and failure that ended it.
    #[test]
    fn a_repeats_line_tallies_the_session_it_went_on_in() {
        let line = tallied(&serde_json::json!({
            "ok": false,
            "void": "sandbox",
            "next": {"ok": false, "void": "sandbox", "failure": "the sandbox probe needs a file"},
        }));
        assert_eq!(line["next"]["void"], "sandbox", "{line}");
        assert_eq!(
            line["next"]["failure"], "the sandbox probe needs a file",
            "{line}"
        );
        assert!(
            tallied(&serde_json::json!({"ok": true}))
                .get("next")
                .is_none()
        );
    }

    /// #910: a void [`run`] marked at its cause stays void on the repeat's
    /// line whatever its log says.
    #[test]
    fn a_repeats_line_keeps_the_void_its_run_marked() {
        let line = tallied(
            &serde_json::json!({"void": "serve", "ok": false, "failure": "serve did not start"}),
        );
        assert_eq!(line["void"], "serve", "{line}");
        assert_eq!(line["failure"], "serve did not start", "{line}");
        assert_eq!(line["log"], "none", "{line}");
    }

    /// #942: a grade grades the admitted ask just before it, which then
    /// asks its task; anywhere else, or after an ask of another task, it is
    /// refused, naming its line.
    #[test]
    fn a_grade_step_follows_the_ask_of_its_task() {
        let header = r#"{"scenario":"g","regimen":"r.toml","head":"h.md"}"#;
        let grade = r#"{"step":"grade","task":"T1","check":"true","wrong":"no"}"#;
        let read = parse(&format!(
            "{header}\n{{\"step\":\"ask\",\"text\":\"go\"}}\n{grade}\n"
        ))
        .expect("a grade after its ask");
        assert!(matches!(&read.steps[0], Step::Ask { task: Some(task), .. } if task == "T1"));
        let Step::Grade(grading) = &read.steps[1] else {
            panic!("a grade step: {:?}", read.steps[1]);
        };
        assert_eq!(
            (grading.attempts, grading.timeout.as_secs()),
            (GRADE_ATTEMPTS, GRADE_TIMEOUT_S)
        );
        for (before, why) in [
            ("", "first"),
            (r#"{"step":"end"}"#, "after an end"),
            (
                r#"{"step":"ask","text":"go","task":"T2"}"#,
                "after another task's ask",
            ),
            (
                r#"{"step":"ask","text":"go","refused":"in-flight"}"#,
                "after a refused ask",
            ),
        ] {
            let refused = parse(&format!("{header}\n{before}\n{grade}\n").replace("\n\n", "\n"))
                .expect_err(why);
            assert!(
                refused
                    .to_string()
                    .contains("right after the admitted `ask`"),
                "{why}: {refused}"
            );
        }
        for (bad, why) in [
            (
                r#"{"step":"grade","task":"T1","check":"true","wrong":"no","attempts":0}"#,
                "no attempts",
            ),
            (
                r#"{"step":"grade","task":" ","check":"true","wrong":"no"}"#,
                "a blank task",
            ),
            (r#"{"step":"grade","task":"T1","check":"true"}"#, "no wrong"),
        ] {
            assert!(
                parse(&format!(
                    "{header}\n{{\"step\":\"ask\",\"text\":\"go\"}}\n{bad}\n"
                ))
                .is_err(),
                "{why}"
            );
        }
    }
}
