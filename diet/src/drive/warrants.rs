//! What a step produced, read for the interview warrants that look at it
//! (#783): a surprise in what the model said, and an error in what a call
//! showed it.
//!
//! The gates, their numbers and their order are the predecessor's, its
//! intent: its `capture/templates.py`, `gates_for_step`, and the loop that
//! counts its steps, `capture/replay_capture.py` 403-452, both at 15f4874
//! (`templates.py` last changed in aabfcf9, 2026-08-09). Its two patterns
//! are copied verbatim as [`SURPRISE_PATTERN`]
//! and [`ERROR_PATTERN`]. This crate has no regex engine, so each is matched
//! by hand-written code; `diet/tests/fixtures/step-warrants/vectors.jsonl`,
//! judged by Python's `re` with the same two patterns (its generator beside
//! it), is what the code must agree with, line for line. A change to a
//! pattern is a change to the code, the constant and the generator together.

/// The predecessor's `SURPRISE_RE`, verbatim, matched case-insensitively
/// against a step's reasoning and text, never a call's output.
pub const SURPRISE_PATTERN: &str = r"\b(actually|instead|turns out|however|wait[,.]|doesn't support|not supported|unsupported|can't|cannot|must (?:be|use|not)|expects?|requires?|error|failed|failure|mismatch|deprecated|confirmed via|it's called|renamed)\b";

/// The predecessor's `ERROR_RE`, verbatim, matched case-sensitively against
/// a call's shown output.
pub const ERROR_PATTERN: &str =
    r"(error|Error|ERROR|Traceback|failed|FAILED|Cannot find|not found|EEXIST|ENOENT|TS\d{4})";

/// Whether `text` -- a step's reasoning, a line break, then its text --
/// matches [`SURPRISE_PATTERN`].
#[must_use]
pub fn surprises(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    (0..=chars.len()).any(|at| {
        boundary(&chars, at)
            && SURPRISES.iter().any(|word| {
                let word: Vec<char> = word.chars().collect();
                folded_at(&chars, at, &word) && boundary(&chars, at + word.len())
            })
    })
}

/// [`SURPRISE_PATTERN`]'s alternatives, each optional `s` and each
/// `(?:be|use|not)` spelled out: for whether the pattern matches anywhere,
/// the same set. `wait[,.]` is kept as the pattern has it, with its `\b`
/// after the punctuation: "wait,x" matches and "wait, x" does not, as the
/// predecessor's did -- its behaviour, not a reading of its intent.
const SURPRISES: &[&str] = &[
    "actually",
    "instead",
    "turns out",
    "however",
    "wait,",
    "wait.",
    "doesn't support",
    "not supported",
    "unsupported",
    "can't",
    "cannot",
    "must be",
    "must use",
    "must not",
    "expect",
    "expects",
    "require",
    "requires",
    "error",
    "failed",
    "failure",
    "mismatch",
    "deprecated",
    "confirmed via",
    "it's called",
    "renamed",
];

/// Python's `\w` on text: a letter, a digit, or `_`.
fn word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Python's `\b` at `at`: a word character on exactly one side of it.
fn boundary(chars: &[char], at: usize) -> bool {
    let before = at > 0 && chars.get(at - 1).is_some_and(|c| word(*c));
    let after = chars.get(at).is_some_and(|c| word(*c));
    before != after
}

/// Whether `word`, lower case, is at `at` in `chars`, case folded as `re.I`
/// folds it for these ASCII patterns: each character's lower case, when it
/// is one character.
fn folded_at(chars: &[char], at: usize, word: &[char]) -> bool {
    chars.get(at..at + word.len()).is_some_and(|here| {
        here.iter().zip(word).all(|(c, w)| {
            let mut lower = c.to_lowercase();
            lower.next() == Some(*w) && lower.next().is_none()
        })
    })
}

/// Whether a call's shown output `text` matches [`ERROR_PATTERN`].
#[must_use]
pub fn shows_an_error(text: &str) -> bool {
    const LITERALS: &[&str] = &[
        "error",
        "Error",
        "ERROR",
        "Traceback",
        "failed",
        "FAILED",
        "Cannot find",
        "not found",
        "EEXIST",
        "ENOENT",
    ];
    LITERALS.iter().any(|literal| text.contains(literal))
        || text.match_indices("TS").any(|(at, _)| {
            // `TS\d{4}`: Python's `\d` is any decimal digit, Unicode's too.
            let digits = text[at + 2..].chars().take(4);
            digits.clone().count() == 4 && digits.into_iter().all(char::is_numeric)
        })
}

/// The warrants that look at what a step produced, and their numbers (#783):
/// each the predecessor's default unless the regimen says otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepWarrants {
    /// `evidence`: a large output.
    pub evidence: bool,
    /// `trouble`: an error, or a surprise in what the model said.
    pub trouble: bool,
    /// `open`: a cadence floor.
    pub open: bool,
    /// The shown output, in characters, an `evidence` fork needs.
    pub evidence_min_chars: u64,
    /// The `evidence` forks one step fires at most.
    pub evidence_per_step: usize,
    /// The steps the surprise path waits after the trio fired.
    pub trouble_cooldown_steps: u32,
    /// The steps between `open` forks.
    pub open_every_steps: u32,
}

impl StepWarrants {
    /// The predecessor's numbers: 3000 characters, 2 per step, a 3-step
    /// cooldown, every 10 steps.
    pub const DEFAULTS: Self = Self {
        evidence: false,
        trouble: false,
        open: false,
        evidence_min_chars: 3000,
        evidence_per_step: 2,
        trouble_cooldown_steps: 3,
        open_every_steps: 10,
    };

    /// Whether any of them is on.
    #[must_use]
    pub fn any(&self) -> bool {
        self.evidence || self.trouble || self.open
    }
}

/// What the gates carry from step to step: the steps since `open` and the
/// trio last fired, counted across the session -- a meditation resets
/// neither -- and the outputs an `evidence` fork already covered this turn,
/// by digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counters {
    /// Steps since `open` fired.
    pub since_open: u32,
    /// Steps since the trio fired; the predecessor's start, 99, is no
    /// cooldown.
    pub since_trio: u32,
    /// This turn's outputs an `evidence` fork covered.
    pub covered: std::collections::BTreeSet<String>,
}

impl Default for Counters {
    fn default() -> Self {
        Self {
            since_open: 0,
            since_trio: 99,
            covered: std::collections::BTreeSet::new(),
        }
    }
}

/// One step, as the gates read it: its reasoning and text, joined as the
/// predecessor joined them, and its calls.
#[derive(Debug, Clone, Default)]
pub struct Step {
    /// `reasoning + "\n" + text`.
    pub said: String,
    /// Its calls, in call order.
    pub calls: Vec<StepCall>,
}

/// One call of a step, as the gates read it.
#[derive(Debug, Clone, Default)]
pub struct StepCall {
    /// Its id.
    pub id: String,
    /// The tool.
    pub tool: String,
    /// The predecessor's `_tool_ident`: the tool, then ` of ` and the first
    /// 80 characters of what it was pointed at, when it names one.
    pub ident: String,
    /// What the model was shown of its result.
    pub shown: String,
    /// It ran and exited non-zero, or its timeout ended it.
    pub errored: bool,
    /// The gate refused it or the operator declined it, or it never ran
    /// (cancelled): no output of its own.
    pub excluded: bool,
    /// The router reads it as a directory listing, which never forks.
    pub listing: bool,
}

/// What a step's gates fired, in the predecessor's order: `evidence`, then
/// the trio, then `open`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    /// An `evidence` fork on the call `id`, filled with its `source` and
    /// `size`.
    Evidence {
        /// The call's id.
        id: String,
        /// The material's name.
        source: String,
        /// Its size, in characters.
        size: u64,
    },
    /// The trio: GOTCHAS, DECISIONS and CONSTRAINTS.
    Trouble,
    /// Those of the trio a classifier fired, each once, in the trio's
    /// order: the fork asks only these.
    TroubleOf(Vec<crate::drive::warrant_gate::GateClass>),
    /// `OPEN_NEXT`.
    Open,
}

/// The tools whose output an `evidence` fork reads: the predecessor's
/// `read`, `webfetch` and `bash`, and `fetch`, the name a fetch tool here
/// would take.
const EVIDENCE_TOOLS: &[&str] = &["read", "bash", "webfetch", "fetch"];

/// The gates one step fires (#783), as the predecessor's `gates_for_step`
/// fired them, its counters counted first as its loop counted them
/// (`replay_capture.py` 403-452 at 15f4874): every step counts, the
/// final answer included.
pub fn gates(warrants: &StepWarrants, counters: &mut Counters, step: &Step) -> Vec<Gate> {
    counters.since_open += 1;
    counters.since_trio += 1;
    let mut fired = Vec::new();
    if warrants.evidence {
        let big = step.calls.iter().filter(|call| {
            !call.excluded
                && !call.listing
                && EVIDENCE_TOOLS.contains(&call.tool.as_str())
                && call.shown.chars().count() as u64 >= warrants.evidence_min_chars
        });
        for call in big {
            if fired.len() >= warrants.evidence_per_step {
                break;
            }
            // No second fork for an output this turn's forks already covered.
            if !counters
                .covered
                .insert(crate::digest::sha256_hex(call.shown.as_bytes()))
            {
                continue;
            }
            fired.push(Gate::Evidence {
                id: call.id.clone(),
                source: call.ident.clone(),
                size: call.shown.chars().count() as u64,
            });
        }
    }
    if warrants.trouble {
        let saw_error = step
            .calls
            .iter()
            .filter(|call| !call.excluded)
            .any(|call| call.errored || shows_an_error(&call.shown));
        if saw_error
            || (surprises(&step.said) && counters.since_trio >= warrants.trouble_cooldown_steps)
        {
            fired.push(Gate::Trouble);
            counters.since_trio = 0;
        }
    }
    if warrants.open && counters.since_open >= warrants.open_every_steps {
        fired.push(Gate::Open);
        counters.since_open = 0;
    }
    fired
}

/// The gates one step fires under a classifier: `evidence` as [`gates`]
/// fires it; the trio's classes that `fired` (no cooldown -- the thresholds
/// govern), asking only those; and `open` when it `fired`, if a probe gates
/// it (`open_gated`), else on [`gates`]'s cadence. The counters count as
/// [`gates`] counts them.
pub fn classified_gates(
    warrants: &StepWarrants,
    counters: &mut Counters,
    step: &Step,
    (fired, open_gated): (&[crate::drive::warrant_gate::GateClass], bool),
) -> Vec<Gate> {
    use crate::drive::warrant_gate::GateClass;
    let patterned = StepWarrants {
        trouble: false,
        open: warrants.open && !open_gated,
        ..*warrants
    };
    let mut out: Vec<Gate> = gates(&patterned, counters, step);
    let cadence = out.contains(&Gate::Open);
    out.retain(|gate| *gate != Gate::Open);
    if warrants.trouble {
        let trio: Vec<GateClass> = [
            GateClass::Gotcha,
            GateClass::Decision,
            GateClass::Constraint,
        ]
        .into_iter()
        .filter(|class| fired.contains(class))
        .collect();
        if !trio.is_empty() {
            out.push(Gate::TroubleOf(trio));
            counters.since_trio = 0;
        }
    }
    if warrants.open && open_gated && fired.contains(&GateClass::Open) {
        out.push(Gate::Open);
        counters.since_open = 0;
    } else if cadence {
        out.push(Gate::Open);
    }
    out
}

/// The predecessor's `_tool_ident` for a call of `tool` with `arguments`:
/// the first of `url`, `filePath`, `file_path` and `command` it carries --
/// and `path`, this harness's own name for a file (#557) -- its first 80
/// characters after `<tool> of `; else the tool.
#[must_use]
pub fn ident(tool: &str, arguments: &str) -> String {
    let named = serde_json::from_str::<serde_json::Value>(arguments)
        .ok()
        .and_then(|value| {
            ["url", "filePath", "file_path", "command", "path"]
                .iter()
                .find_map(|key| match value.get(key) {
                    Some(serde_json::Value::String(text)) if !text.is_empty() => Some(text.clone()),
                    Some(other) if !other.is_null() && *other != false => Some(other.to_string()),
                    _ => None,
                })
        });
    match named {
        Some(text) => format!("{tool} of {}", text.chars().take(80).collect::<String>()),
        None => tool.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VECTORS: &str = include_str!("../../tests/fixtures/step-warrants/vectors.jsonl");
    const GENERATOR: &str = include_str!("../../tests/fixtures/step-warrants/generate.py");

    fn call(tool: &str, shown: &str) -> StepCall {
        StepCall {
            id: format!("call-{}", shown.len()),
            tool: tool.to_owned(),
            ident: tool.to_owned(),
            shown: shown.to_owned(),
            ..StepCall::default()
        }
    }

    fn all_on() -> StepWarrants {
        StepWarrants {
            evidence: true,
            trouble: true,
            open: true,
            ..StepWarrants::DEFAULTS
        }
    }

    /// `evidence` fires on an output of 3000 characters and not of 2999; on
    /// the first two in call order, never more; never on a listing, a
    /// refused call, a tool it does not read, or an output this turn's forks
    /// already covered.
    #[test]
    fn evidence_fires_at_its_threshold_twice_a_step_at_most() {
        let warrants = StepWarrants {
            evidence: true,
            ..StepWarrants::DEFAULTS
        };
        let big = |n: usize, c: char| c.to_string().repeat(n);
        let mut counters = Counters::default();
        let step = Step {
            said: String::new(),
            calls: vec![
                call("bash", &big(2999, 'a')),
                call("read", &big(3000, 'b')),
                call("write", &big(5000, 'c')),
                StepCall {
                    listing: true,
                    ..call("bash", &big(4000, 'd'))
                },
                StepCall {
                    excluded: true,
                    ..call("bash", &big(4000, 'e'))
                },
                call("bash", &big(3001, 'f')),
                call("read", &big(3002, 'g')),
            ],
        };
        let fired = gates(&warrants, &mut counters, &step);
        let sizes: Vec<u64> = fired
            .iter()
            .map(|gate| match gate {
                Gate::Evidence { size, .. } => *size,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(sizes, [3000, 3001], "the first two in call order");
        // The same output again this turn: covered.
        let again = Step {
            said: String::new(),
            calls: vec![call("read", &big(3000, 'b')), call("read", &big(3002, 'g'))],
        };
        let fired = gates(&warrants, &mut counters, &again);
        assert_eq!(
            fired,
            [Gate::Evidence {
                id: "call-3002".to_owned(),
                source: "read".to_owned(),
                size: 3002
            }]
        );
    }

    /// The trio fires on an error always, and on a surprise only once the
    /// cooldown has passed; a refused call's text is no error.
    #[test]
    fn trouble_fires_on_an_error_always_and_on_a_surprise_after_its_cooldown() {
        let warrants = StepWarrants {
            trouble: true,
            ..StepWarrants::DEFAULTS
        };
        let said = |text: &str| Step {
            said: text.to_owned(),
            calls: Vec::new(),
        };
        let mut counters = Counters::default();
        assert_eq!(
            gates(
                &warrants,
                &mut counters,
                &said("It turns out the API differs.")
            ),
            [Gate::Trouble]
        );
        // Inside the cooldown: steps 1 and 2 after it stay quiet...
        assert!(gates(&warrants, &mut counters, &said("Actually, no.")).is_empty());
        assert!(gates(&warrants, &mut counters, &said("However, wait,x")).is_empty());
        // ...an error fires regardless...
        let errored = Step {
            said: String::new(),
            calls: vec![StepCall {
                errored: true,
                ..call("bash", "")
            }],
        };
        assert_eq!(gates(&warrants, &mut counters, &errored), [Gate::Trouble]);
        // ...as does an error in what a call showed...
        let traceback = Step {
            said: String::new(),
            calls: vec![call("bash", "Traceback (most recent call last):")],
        };
        assert_eq!(gates(&warrants, &mut counters, &traceback), [Gate::Trouble]);
        // ...but not one the gate refused.
        let refused = Step {
            said: String::new(),
            calls: vec![StepCall {
                excluded: true,
                ..call("bash", "ERROR: not allowed")
            }],
        };
        assert!(gates(&warrants, &mut counters, &refused).is_empty());
        assert!(gates(&warrants, &mut counters, &said("plain")).is_empty());
        // Three steps after the trio: a surprise fires again.
        assert_eq!(
            gates(&warrants, &mut counters, &said("it's called foo")),
            [Gate::Trouble]
        );
        // The surprise path never reads a call's output.
        let quiet = Step {
            said: String::new(),
            calls: vec![call("bash", "actually renamed")],
        };
        let mut counters = Counters::default();
        assert!(gates(&warrants, &mut counters, &quiet).is_empty());
    }

    /// `open` fires on every tenth step, the final answer's counting, and
    /// not before; the order is evidence, the trio, then open.
    #[test]
    fn open_fires_every_tenth_step_after_the_rest() {
        let mut counters = Counters::default();
        let plain = Step::default();
        for step in 1..10 {
            assert!(
                gates(&all_on(), &mut counters, &plain).is_empty(),
                "step {step}"
            );
        }
        let busy = Step {
            said: "turns out".to_owned(),
            calls: vec![call("read", &"x".repeat(3000))],
        };
        let fired = gates(&all_on(), &mut counters, &busy);
        assert!(matches!(fired[0], Gate::Evidence { .. }), "{fired:?}");
        assert_eq!(fired[1..], [Gate::Trouble, Gate::Open]);
        for _ in 1..10 {
            assert!(!gates(&all_on(), &mut counters, &plain).contains(&Gate::Open));
        }
        assert_eq!(gates(&all_on(), &mut counters, &plain), [Gate::Open]);
    }

    /// The predecessor's `_tool_ident`, with this harness's `path`.
    #[test]
    fn the_ident_names_the_tool_and_what_it_was_pointed_at() {
        assert_eq!(
            ident("bash", r#"{"command":"cat notes.md"}"#),
            "bash of cat notes.md"
        );
        assert_eq!(
            ident("read", r#"{"path":"src/lib.rs"}"#),
            "read of src/lib.rs"
        );
        assert_eq!(ident("read", "{}"), "read");
        assert_eq!(
            ident("bash", &format!(r#"{{"command":"{}"}}"#, "y".repeat(100))),
            format!("bash of {}", "y".repeat(80))
        );
    }

    /// Every vector Python's `re` judged, judged the same here.
    #[test]
    fn the_matchers_agree_with_pythons_re_on_every_vector() {
        let mut disagreements = Vec::new();
        let mut lines = 0;
        for line in VECTORS.lines() {
            let vector: serde_json::Value = serde_json::from_str(line).expect("a vector");
            let text = vector["text"].as_str().expect("its text");
            let surprise = vector["surprise"].as_bool().expect("a verdict");
            let error = vector["error"].as_bool().expect("a verdict");
            if surprises(text) != surprise || shows_an_error(text) != error {
                disagreements.push(format!(
                    "{text:?}: re says surprise {surprise}, error {error}; here {}, {}",
                    surprises(text),
                    shows_an_error(text)
                ));
            }
            lines += 1;
        }
        assert!(lines > 100, "the corpus is {lines} lines");
        assert!(disagreements.is_empty(), "{disagreements:#?}");
    }

    /// The pinned strings are the generator's, so the corpus judges the
    /// patterns these constants name.
    #[test]
    fn the_pinned_patterns_are_the_generators() {
        let unbroken: String = GENERATOR
            .lines()
            .map(str::trim)
            .collect::<Vec<_>>()
            .join("")
            .replace("\"r\"", "")
            .replace("\" r\"", "");
        assert!(
            unbroken.contains(&format!("re.compile(r\"{SURPRISE_PATTERN}\", re.I)")),
            "SURPRISE_PATTERN has drifted from the generator's SURPRISE_RE"
        );
        assert!(
            unbroken.contains(&format!("re.compile(r\"{ERROR_PATTERN}\")")),
            "ERROR_PATTERN has drifted from the generator's ERROR_RE"
        );
    }
    /// Under a classifier, the trouble fork asks only the classes that
    /// fired, with no cooldown; `open` fires on its class when a probe gates
    /// it, else on the cadence; `evidence` is unchanged.
    #[test]
    fn under_a_classifier_the_asks_follow_the_fired_classes() {
        use crate::drive::warrant_gate::GateClass;
        let warrants = all_on();
        let mut counters = Counters::default();
        let plain = Step {
            said: "Read it.".to_owned(),
            calls: vec![call("bash", "ok")],
        };
        // Errors and surprises are the classifier's now: nothing fired.
        let errored = Step {
            said: "It turns out it failed.".to_owned(),
            calls: vec![StepCall {
                errored: true,
                ..call("bash", "Traceback (most recent call last):")
            }],
        };
        assert!(classified_gates(&warrants, &mut counters, &errored, (&[], true)).is_empty());
        assert_eq!(
            classified_gates(
                &warrants,
                &mut counters,
                &plain,
                (&[GateClass::Constraint, GateClass::Gotcha], true)
            ),
            [Gate::TroubleOf(vec![
                GateClass::Gotcha,
                GateClass::Constraint
            ])]
        );
        // No cooldown: the next step fires again.
        assert_eq!(
            classified_gates(
                &warrants,
                &mut counters,
                &plain,
                (&[GateClass::Decision], true)
            ),
            [Gate::TroubleOf(vec![GateClass::Decision])]
        );
        // A gated `open` fires on its class, never on the cadence.
        assert_eq!(
            classified_gates(&warrants, &mut counters, &plain, (&[GateClass::Open], true)),
            [Gate::Open]
        );
        let mut counters = Counters::default();
        for _ in 0..12 {
            assert!(classified_gates(&warrants, &mut counters, &plain, (&[], true)).is_empty());
        }
        // Ungated, `open` keeps its 10-step cadence.
        let mut counters = Counters::default();
        let cadence: Vec<usize> = (1..=20)
            .filter(|_| {
                classified_gates(&warrants, &mut counters, &plain, (&[], false)) == [Gate::Open]
            })
            .collect();
        assert_eq!(cadence, [10, 20]);
        // `evidence` as the patterns fire it.
        let big = Step {
            said: String::new(),
            calls: vec![call("read", &"x".repeat(3_000))],
        };
        assert!(matches!(
            classified_gates(&warrants, &mut Counters::default(), &big, (&[], true))[..],
            [Gate::Evidence { .. }]
        ));
    }
}
