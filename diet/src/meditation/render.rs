//! The render: the working object turned into the prompt a session continues
//! from.
//!
//! This is the architecture's single deliberate prefill event, and its
//! economics are the reason everything else exists. A 50K-token prefix
//! re-sent identically costs 0.10 s warm against 46 s cold -- 444x -- and any
//! edit to the prefix forces a full re-prefill, because reuse snaps to the
//! engine's checkpoint boundaries. (One model, one engine, one card;
//! re-measure per substrate.) So the prefix stays byte-identical during a
//! phase, state accumulates outside the context, and this function runs only
//! at a meditation.
//!
//! It is a **pure function** of the object and the phase. Nothing here reads
//! a clock, a counter, or anything that moves: two renders of the same object
//! in the same phase are the same bytes, and the byte-identity claim the meditation
//! controller makes is only checkable because that is true.

use std::fmt::Write as _;

use crate::client::shape::{Message, Role};
use crate::object::WorkingObject;

/// Which frame this module renders with, named in the render itself and
/// logged as each meditation's `frame`, so renders from before and after a
/// change to the frame are told apart.
///
/// `1` (#1050): each entry's line carries its source -- who wrote it, and
/// from what -- between its id and its text, and a keep's tag after the
/// source. Every render before it said `placeholder`, the frame no one had
/// authored, with the id and the text alone.
///
/// `2` (#1086, the maintainer's legend form): one legend line under the
/// working set's heading names the default source, [`DEFAULT_SOURCE`], and a
/// line with that source carries no cell -- unless a live note checks it,
/// when its cell carries the check. Every other line keeps frame 1's. Off
/// the trunk the column is the object's dump's, from which mining computes
/// it, and [`source`] gives every entry's phrase, the default included.
pub const FRAME_VERSION: &str = "2";

/// The source frame 2 leaves unmarked (#1086): fixed, not chosen per render,
/// so the legend means the same at every refill -- the harness's mechanical
/// entries, most of a recorded refill's lines.
pub const DEFAULT_SOURCE: &str = "the harness noted, from your commands";

/// Frame 2's legend line (#1086), under the working set's heading: what an
/// unmarked line's source is. A draft; the words are the maintainer's.
pub const LEGEND: &str =
    "A line with no source between its id and its text: the harness noted, from your commands.";

/// What the render does past its budget (#565).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverBudget {
    /// An older entry keeps its first line, cut at [`TIER_CHARS`].
    Tier,
    /// An older entry keeps only its id.
    Elide,
}

/// The render's budget (#565): estimated tokens (chars/4, as #552's depth),
/// and what becomes of the older entries past it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// The estimated tokens the whole render may run to.
    pub tokens: u64,
    /// What becomes of an entry past it.
    pub over: OverBudget,
}

/// A render, and what its budget did to it (#565).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// The render.
    pub text: String,
    /// Its estimated tokens.
    pub tokens: u64,
    /// How many entries it shortened or elided.
    pub reduced: u64,
    /// Whether it came in at or under its budget; with none, it does. A
    /// render that does not, with everything reducible reduced, says so.
    pub fits: bool,
}

/// The most characters a tiered entry keeps of its first line.
pub const TIER_CHARS: usize = 80;

/// The entry kinds that are the brief and always render in full (#565):
/// the goal and the constraints, by the fold's `<kind>: ` prefix.
const BRIEF: &[&str] = &["goal: ", "constraint: "];

/// The entries the render shows, in id order: every live one, and every
/// voided one a refutation voided (#878), which stays as what was believed.
fn shown(object: &WorkingObject) -> impl Iterator<Item = &crate::object::Entry> {
    object
        .entries()
        .filter(|entry| entry.state.is_live() || entry.refuted.is_some())
}

/// What `entry`'s line says: its content; or, refuted (#878), the claim as it
/// was held, why it is wrong, and where that was shown.
fn said(entry: &crate::object::Entry) -> std::borrow::Cow<'_, str> {
    match &entry.refuted {
        Some(refuted) if !entry.state.is_live() => std::borrow::Cow::Owned(format!(
            "was: {} (held at {}/10), wrong because {} [{}]",
            entry.content,
            refuted.confidence.get(),
            refuted.because,
            refuted.pointer
        )),
        _ => std::borrow::Cow::Borrowed(&entry.content),
    }
}

/// The working object rendered as the prompt's prefix.
///
/// The regime is carried at the top, because a prompt whose record cannot say
/// which substrate and which dogma it was built for is a prompt that does not
/// transfer.
#[must_use]
pub fn render(object: &WorkingObject, phase: Option<&str>) -> String {
    rendered(object, phase, None).text
}

/// What a note's line carries after its text (#150): the operator's
/// markers, by entry -- [`EDITED`], and [`flagged`] for each flag.
pub type Marks = std::collections::BTreeMap<crate::object::EntryId, String>;

/// The marker on a note the operator's edit wrote (#150).
pub const EDITED: &str = " [edited by the operator]";

/// The marker on a note the operator flagged with `note` (#150).
#[must_use]
pub fn flagged(note: &str) -> String {
    format!(" [flagged: {}]", one_line(note))
}

/// [`render`] under a `budget` (#565), still a pure function of the object,
/// the phase and the budget. With none, or with everything inside it, the
/// bytes are [`render`]'s. Past it, the goal and the constraints still
/// render whole; every other entry is kept whole newest first -- newest by
/// its latest patch, the greatest of its provenances -- until the next would
/// pass the budget, and that one and every older one is tiered or elided in
/// place, with one line after the working set counting them.
#[must_use]
pub fn rendered(object: &WorkingObject, phase: Option<&str>, budget: Option<Budget>) -> Rendered {
    rendered_marked(object, phase, budget, &Marks::new())
}

/// [`rendered`], each entry in `marks` carrying its marker after its line
/// (#150) -- whole, tiered or elided. With no mark, [`rendered`]'s bytes.
#[must_use]
pub fn rendered_marked(
    object: &WorkingObject,
    phase: Option<&str>,
    budget: Option<Budget>,
    marks: &Marks,
) -> Rendered {
    let head = head(object, phase);
    let mark = |id: &crate::object::EntryId| marks.get(id).map_or("", String::as_str);
    // Frame 2 (#1086): the source cell, and its tab, unless the legend says it.
    let line = |entry: &crate::object::Entry, content: &str| {
        let cell = cell(object, entry)
            .map(|cell| format!("{cell}\t"))
            .unwrap_or_default();
        format!(
            "{}\t{cell}{}{}\n",
            one_line(entry.id.as_str()),
            one_line(content),
            mark(&entry.id)
        )
    };
    let over = budget.map_or(OverBudget::Tier, |budget| budget.over);
    // The render with `reduced` shortened or elided, and how many were.
    let compose = |reduced: &std::collections::BTreeSet<&crate::object::EntryId>| {
        let mut out = head.clone();
        let mut count = 0_u64;
        for entry in shown(object) {
            if !reduced.contains(&entry.id) {
                out.push_str(&line(entry, &said(entry)));
                continue;
            }
            match over {
                OverBudget::Tier => {
                    let first = entry.content.split('\n').next().unwrap_or_default();
                    let mut cut: String = first.chars().take(TIER_CHARS).collect();
                    // An entry already one short line is left as it is, and
                    // not counted: tiering did nothing to it.
                    if cut.chars().count() < entry.content.chars().count() {
                        cut.push('…');
                        count += 1;
                    }
                    out.push_str(&line(entry, &cut));
                }
                OverBudget::Elide => {
                    out.push_str(&one_line(entry.id.as_str()));
                    out.push_str(mark(&entry.id));
                    out.push('\n');
                    count += 1;
                }
            }
        }
        if count > 0 {
            let what = match over {
                OverBudget::Tier => "shortened",
                OverBudget::Elide => "elided",
            };
            let _ = writeln!(out, "{count} older entries {what}");
        }
        (out, count)
    };
    let Some(budget) = budget else {
        let (text, reduced) = compose(&std::collections::BTreeSet::new());
        return Rendered {
            tokens: estimate(&text),
            text,
            reduced,
            fits: true,
        };
    };
    // The brief is never reduced; the rest are, oldest first -- newest by
    // latest patch. The newest are kept whole while their whole lines fit
    // beside the header and the brief; then, while the render with its
    // reduced lines and its count still does not fit, one more is reduced,
    // until it fits or nothing is left to reduce (levers run A: a render
    // that counted only the whole lines overshot its budget).
    // A refuted claim is kept whole, as the brief is (#878).
    let brief = |entry: &crate::object::Entry| {
        BRIEF.iter().any(|kind| entry.content.starts_with(kind)) || !entry.state.is_live()
    };
    let mut rest: Vec<_> = shown(object).filter(|entry| !brief(entry)).collect();
    rest.sort_by(|a, b| {
        b.provenances
            .iter()
            .max()
            .cmp(&a.provenances.iter().max())
            .then_with(|| b.id.cmp(&a.id))
    });
    // Counted in characters and rounded once, as the render's own estimate
    // is (#732): rounding each piece up overcounted a render that fits
    // exactly, and reduced it.
    let chars = |text: &str| text.chars().count() as u64;
    let mut spent = chars(&head)
        + shown(object)
            .filter(|entry| brief(entry))
            .map(|entry| chars(&line(entry, &said(entry))))
            .sum::<u64>();
    let mut whole = 0;
    for entry in &rest {
        let cost = chars(&line(entry, &said(entry)));
        if (spent + cost).div_ceil(4) > budget.tokens {
            break;
        }
        spent += cost;
        whole += 1;
    }
    loop {
        let reduced = rest[whole..].iter().map(|entry| &entry.id).collect();
        let (text, count) = compose(&reduced);
        let tokens = estimate(&text);
        let fits = tokens <= budget.tokens;
        if fits || whole == 0 {
            return Rendered {
                tokens,
                text,
                reduced: count,
                fits,
            };
        }
        whole -= 1;
    }
}

/// Estimated tokens of `text`: its characters over four, rounded up, as
/// [`estimated_tokens`] counts a message.
fn estimate(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(4)
}

/// The render's header: the regime, the phase, the frame, and the working
/// set's heading.
fn head(object: &WorkingObject, phase: Option<&str>) -> String {
    let regime = object.regime();
    let mut out = String::new();

    out.push_str("# regime\n");
    // No arm and no substrate (the maintainer's ruling, 2026-10-23): the
    // model reads this header, and an arm's name or a substrate's id is the
    // experiment's own bookkeeping, not anything the model works from. Both
    // stay in the log and the record, beside the render.
    out.push_str("dogma_version: ");
    out.push_str(&regime.dogma_version.to_string());
    out.push_str("\nphase: ");
    // A session with no phase graph renders `-` rather than omitting the
    // line. A field that disappears when it is empty makes two prompts differ
    // in shape as well as in content, and the byte-identity claim is about
    // shape as much as anything.
    out.push_str(phase.unwrap_or("-"));
    out.push_str("\nframe_version: ");
    out.push_str(FRAME_VERSION);
    out.push_str("\n\n");
    out.push_str(WORKING_SET_HEADER);
    out.push_str(LEGEND);
    out.push('\n');
    out
}

/// An entry's source cell (#1086): [`source_in`], or none when that is
/// exactly the default source -- the legend says it.
fn cell(object: &WorkingObject, entry: &crate::object::Entry) -> Option<String> {
    Some(source_in(object, entry)).filter(|cell| cell != DEFAULT_SOURCE)
}

/// The working record's live entries, one `<id>\t<content>` line each:
/// what an ask that shows the record carries (#595), in the two columns its
/// pinned words describe -- the render's own lines carry the source between
/// them since frame 1 (#1050).
#[must_use]
pub fn entries(object: &WorkingObject) -> String {
    let mut lines = String::new();
    for entry in object.live() {
        lines.push_str(&one_line(entry.id.as_str()));
        lines.push('\t');
        lines.push_str(&one_line(&entry.content));
        lines.push('\n');
    }
    lines
}

/// The trunk a served session continues from after a meditation (#493): its
/// `head`, untouched, then [`refill_message`] carrying `render`, and nothing
/// of the old trunk.
///
/// A TOTAL compaction, by the maintainer's intent on #493: the refill is not
/// a summary of the old turns with the later ones kept, and it does not try
/// to make the model believe it is continuing the session. Each meditation hands it
/// a better starting point, built from working memory alone.
///
/// This function never changes the system message (#597). The one change
/// the program allows is the caller's: a meditation, which misses the cache
/// anyway, passes a `head` carrying the current phase's ground rules
/// ([`with_ground_rules`], #787; the maintainer, 2026-10-11: "only allowed
/// during an event that will cause a cache miss anyways"). The session and
/// the record's projection both rebuild the trunk through this one
/// function, so the projection's head check holds the session to it.
#[must_use]
pub fn refill(head: &[Message], render: &str) -> Vec<Message> {
    let mut trunk = head.to_vec();
    trunk.push(refill_message(render));
    trunk
}

/// [`refill`], with `after` -- a meditation's list of the background commands
/// still running (#614) -- following the summary in the same message, as
/// Qwen Code puts its list after the summary
/// (`chatCompressionService.ts:1139`).
#[must_use]
pub fn refill_with(head: &[Message], render: &str, after: Option<&str>) -> Vec<Message> {
    let mut trunk = refill(head, render);
    if let (Some(after), Some(message)) = (after, trunk.last_mut()) {
        message.content.push_str(after);
    }
    trunk
}

/// `head` with a phase's ground rules, when it is in one, after its system message (#787,
/// refining #597): what a meditation's refill opens with while the session
/// is in a phase. The head's own text is unchanged, the rules following it
/// after a blank line, and then, after another, the sentence naming the
/// phase's moves (v38, #1050: [`phase_moves`]); a head with no system
/// message is returned as it is, and its rules ride as a note instead
/// ([`phase_note`]).
#[must_use]
pub fn with_ground_rules(
    head: &[Message],
    document: Option<&crate::formats::log::PhaseDocument>,
) -> Vec<Message> {
    let mut head = with_ground_rules_before_v38(head, document);
    if let (Some(document), Some(first)) = (document, head.first_mut())
        && first.role == Role::System
        && let Some(moves) = phase_moves(&document.name, &document.transitions)
    {
        let _ = write!(first.content, "\n\n{moves}");
    }
    head
}

/// [`with_ground_rules`] as a log before v38 was sent it: the rules alone,
/// with no sentence naming the moves.
#[must_use]
pub fn with_ground_rules_before_v38(
    head: &[Message],
    document: Option<&crate::formats::log::PhaseDocument>,
) -> Vec<Message> {
    let mut head = head.to_vec();
    if let (Some(document), Some(first)) = (
        document,
        head.first_mut().filter(|first| first.role == Role::System),
    ) {
        let _ = write!(
            first.content,
            "\n\nGround rules for the {} phase:\n\n{}",
            document.name, document.ground_rules
        );
    }
    head
}

/// Whether [`with_ground_rules`] carries a phase's rules on `head`.
#[must_use]
pub fn carries_ground_rules(head: &[Message]) -> bool {
    head.first().is_some_and(|first| first.role == Role::System)
}

/// The note a phase's ground rules ride in on the next ask when the session
/// entered it with no meditation (#787): at its start, or on a `continue`;
/// then, after a blank line, the sentence naming its moves (#1050:
/// [`phase_moves`]).
#[must_use]
pub fn phase_note(
    name: &str,
    ground_rules: &str,
    transitions: &[crate::formats::log::PhaseOption],
) -> String {
    let mut note =
        format!("The session is now in the {name} phase. Its ground rules:\n\n{ground_rules}");
    if let Some(moves) = phase_moves(name, transitions) {
        let _ = write!(note, "\n\n{moves}");
    }
    note
}

/// The sentence naming the moves out of phase `name` (#1050, the
/// maintainer's wording): the usual next phase -- its `suggested` moves --
/// and what is also open -- its `allowed` ones; none when it lists no move.
#[must_use]
pub fn phase_moves(name: &str, transitions: &[crate::formats::log::PhaseOption]) -> Option<String> {
    let of = |kind: &str| -> Vec<&str> {
        transitions
            .iter()
            .filter(|option| option.kind == kind)
            .map(|option| option.to.as_str())
            .collect()
    };
    let (suggested, allowed) = (of("suggested"), of("allowed"));
    let open = |phases: &[&str]| {
        let verb = if phases.len() > 1 { "are" } else { "is" };
        format!("{} {verb}", listed(phases))
    };
    let usual = |phases: &[&str]| {
        if phases.len() > 1 {
            format!("the usual next phases are {}", listed(phases))
        } else {
            format!("the usual next phase is {}", listed(phases))
        }
    };
    match (suggested.is_empty(), allowed.is_empty()) {
        (true, true) => None,
        (false, true) => Some(format!("From {name}, {}.", usual(&suggested))),
        (true, false) => Some(format!("From {name}, {} open to you.", open(&allowed))),
        (false, false) => Some(format!(
            "From {name}, {}; {} also open to you.",
            usual(&suggested),
            open(&allowed)
        )),
    }
}

/// Phase names joined with commas and a final "or" (#1050).
fn listed(phases: &[&str]) -> String {
    match phases {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
    }
}

/// The message a meditation's refill rides in (#597), shaped as Pi and `OpenCode`
/// 2 carry a compaction summary: a user message, the text inside
/// `<summary>` tags, opened and closed on lines of their own (Pi
/// `core/messages.ts:11-17`, `OpenCode` 2 `session/runner/to-llm-message.ts:
/// 311-325`). Their lead sentences differ and Qwen Code's summary message
/// has none, so it has none; no harness but Qwen Code adds a trailer, so it
/// has none.
#[must_use]
pub fn refill_message(render: &str) -> Message {
    Message::new(Role::User, format!("<summary>\n{render}\n</summary>"))
}

/// The trunk after a meditation logged before #597: `render` appended to the
/// head's system message after a blank line, or a system message of its
/// own when the head has none. What those logs' heads were sent, so they
/// still rebuild.
#[must_use]
pub fn refill_in_the_system_message(head: &[Message], render: &str) -> Vec<Message> {
    let mut trunk = head.to_vec();
    match trunk.first_mut() {
        Some(first) if first.role == Role::System => {
            first.content.push_str("\n\n");
            first.content.push_str(render);
        }
        _ => trunk.insert(0, Message::new(Role::System, render)),
    }
    trunk
}

/// A message's estimated tokens: its characters -- content, reasoning, each
/// tool call's name and arguments, each note (#732) -- divided by four,
/// rounded up, and each image by its pixels
/// ([`crate::client::shape::Image::estimated_tokens`]), never its encoding.
/// The harnesses compared estimate text the same way, none with a tokenizer
/// (#552; Pi `compaction.ts` 298-349, `OpenCode` 2 `util/token.ts` 3-5); Pi's
/// rounding up, which never undercounts.
#[must_use]
pub fn estimated_tokens(message: &Message) -> u64 {
    let chars = message.content.chars().count()
        + message.reasoning.as_ref().map_or(0, |r| r.chars().count())
        + message
            .tool_calls
            .iter()
            .map(|call| call.name.chars().count() + call.arguments.chars().count())
            .sum::<usize>()
        // The notes an ask carries go on the wire before its words (#609),
        // so they are counted (#732).
        + message
            .notes
            .iter()
            .map(|note| note.chars().count())
            .sum::<usize>();
    let images: u64 = message
        .images
        .iter()
        .map(crate::client::shape::Image::estimated_tokens)
        .sum();
    u64::try_from(chars.div_ceil(4))
        .unwrap_or(u64::MAX)
        .saturating_add(images)
}

/// The recent tail of `turns` -- the trunk after its head -- a meditation keeps
/// after the refill (#552): whole turns, from the newest back, while their
/// estimated tokens stay within `budget`. A turn starts at its user message:
/// the tail never starts at an assistant reply or a tool result, so no call
/// is parted from its result, and the chat template driven here refuses a
/// conversation with no user message after the system one. The turn that
/// would cross the budget is not kept (the budget is a ceiling); with no
/// whole turn within it, or a budget of 0, the tail is empty -- the total
/// refill (#505).
#[must_use]
pub fn tail(turns: &[Message], budget: u64) -> &[Message] {
    let mut start = turns.len();
    let mut spent = 0_u64;
    let mut since = 0_u64;
    for (at, message) in turns.iter().enumerate().rev() {
        since = since.saturating_add(estimated_tokens(message));
        if message.role == Role::User {
            if spent.saturating_add(since) > budget {
                break;
            }
            spent += since;
            since = 0;
            start = at;
        }
    }
    &turns[start..]
}

/// The estimated tokens of the newest whole turn of `turns` -- from its
/// last user message on, as [`tail`] measures one -- or `None` when it holds
/// no turn (#693): what a tail that kept nothing could not fit.
#[must_use]
pub fn newest_turn_tokens(turns: &[Message]) -> Option<u64> {
    let start = turns
        .iter()
        .rposition(|message| message.role == Role::User)?;
    Some(turns[start..].iter().map(estimated_tokens).sum())
}

/// The last `n` whole turns of `turns` (#567), each starting at its user
/// message as [`tail`]'s do, so no call is parted from its result; all of
/// them when there are fewer, none when `n` is 0.
#[must_use]
pub fn last_turns(turns: &[Message], n: usize) -> &[Message] {
    if n == 0 {
        return &turns[turns.len()..];
    }
    let mut seen = 0;
    for (at, message) in turns.iter().enumerate().rev() {
        if message.role == Role::User {
            seen += 1;
            if seen == n {
                return &turns[at..];
            }
        }
    }
    let first = turns
        .iter()
        .position(|message| message.role == Role::User)
        .unwrap_or(turns.len());
    &turns[first..]
}

/// `text` with everything that could forge a row escaped out of it.
///
/// **The render owns the grammar of the prompt it emits, and it used to have
/// none.** A working-set row is `id \t content \n`, and entry content comes
/// from model output through the capture lanes -- so a single note containing
/// a newline, a tab and an identifier of the model's choosing wrote an extra
/// row into the next prefix, with whatever id it liked. Two different objects
/// rendered to the same bytes, which also means the object could move without
/// the prefix moving: the byte-identity claim with a hole in the other
/// direction.
///
/// It matters twice over, because the same text is numbered into the audit
/// ask, and all three pinned templates say *"reply with EXACTLY one line,
/// each starting with its number ... Output {n} lines and nothing else"*. An
/// unescaped newline there put an unnumbered line in the middle of the list,
/// and a ratifier folding answer line *k* onto item *k* then wrote a verdict
/// onto the wrong entry.
///
/// Escaped rather than refused: the content is already in the object by the
/// time it gets here, and a render that refused to draw an entry would be a
/// prompt silently missing a fact.
#[must_use]
pub fn one_line(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

/// How many bytes of working set the render would carry.
///
/// The budget trigger measures THIS rather than a count of entries, because
/// the thing approaching a ceiling is the prompt, and an entry is not a fixed
/// size. Deriving it from the object rather than taking it as an argument
/// means no caller can pass a number that disagrees with what would actually
/// be sent.
#[must_use]
pub fn working_set_bytes(object: &WorkingObject) -> u64 {
    // Frame 2's legend line is the heading's, not an entry's (#1086): the
    // trigger measures the entries, and an empty object is 0, as before.
    let mut total: u64 = 0;
    for entry in object.live() {
        // Measured through the same escaping the render uses, and counting
        // the same separators, so the two cannot disagree. They used to
        // differ by any proportional amount without a test noticing, because
        // the only bounds asserted were "more than zero" and "less than the
        // whole render" -- and the whole render carries a regime header worth
        // a hundred bytes of slack.
        //
        // A count that saturates is a count that stops being a measurement.
        // Nothing here can reach it -- an object of eighteen exabytes is not
        // a thing -- and saturating is still the right floor for a number
        // that decides a trigger.
        let cell = cell(object, entry).map_or(0, |cell| cell.len() as u64 + 1);
        total = total
            .saturating_add(one_line(entry.id.as_str()).len() as u64)
            .saturating_add(cell)
            .saturating_add(one_line(&entry.content).len() as u64)
            .saturating_add(2);
    }
    total
}

/// [`source`], and the note that checks the entry against tool output while
/// that note is live (#1050, c4's `confirms`): `…, checked by <note id>`,
/// the most recent such note when several are.
#[must_use]
pub fn source_in(object: &WorkingObject, entry: &crate::object::Entry) -> String {
    let mut said = source(entry);
    if let Some(check) = object.checks_of(&entry.id).first() {
        said.push_str(", checked by ");
        said.push_str(&one_line(check.id.as_str()));
    }
    said
}

/// What an entry's line says of where its words came from (#1050), in the
/// maintainer's phrases: an operator's keep by what it kept from, its tag in
/// brackets after; else by the lane that wrote it first -- the operator's own
/// words, a note in the model's and what vouches for it, a fork's answer, or
/// the harness's.
#[must_use]
pub fn source(entry: &crate::object::Entry) -> String {
    use crate::object::{KeptFrom, Origin, Vouched};
    if entry.origin == Some(Origin::Corrected) {
        return "you corrected, user accepted".to_owned();
    }
    if let Some(Origin::Kept { from, tag }) = &entry.origin {
        let said = match from {
            KeptFrom::Ask => "user said",
            KeptFrom::ToolResult => "user kept, from tool output",
            KeptFrom::Answer => "user kept, from your answer",
        };
        return match tag {
            Some(tag) => format!("{said} [{}]", one_line(tag)),
            None => said.to_owned(),
        };
    }
    let lane = entry
        .provenances
        .first()
        .map_or("", |provenance| provenance.lane.as_str());
    let said = match lane {
        crate::object::OPERATOR => "user said",
        crate::capture::tools::LANE => match &entry.origin {
            Some(Origin::Noted(Vouched::Observed)) => "you noted, from tool output",
            Some(Origin::Noted(Vouched::UserStated)) => "you noted, from the user's words",
            _ => "you noted",
        },
        "interview" | "audit" | "trunk" => "you said, when asked",
        crate::capture::mechanical::LANE => "the harness noted, from your commands",
        // A lane no served session writes -- the scripted drive's, or an old
        // log's: the weakest claim, as a field-kind note's.
        _ => "you noted",
    };
    said.to_owned()
}

/// Where the working set begins in a render.
///
/// Public so a caller -- and the test that pins
/// [`working_set_bytes`] against what is actually emitted -- can find the
/// section without re-deriving the header's shape.
pub const WORKING_SET_HEADER: &str = "# working set\n";

#[cfg(test)]
mod phase_moves_tests {
    use super::{
        Message, Role, phase_moves, phase_note, with_ground_rules, with_ground_rules_before_v38,
    };
    use crate::formats::log::{PhaseDocument, PhaseOption};

    fn moves(list: &[(&str, &str)]) -> Vec<PhaseOption> {
        list.iter()
            .map(|(to, kind)| PhaseOption {
                to: (*to).to_owned(),
                kind: (*kind).to_owned(),
            })
            .collect()
    }

    /// Every move shape #1050 names, in the maintainer's words.
    #[test]
    fn the_sentence_names_each_shape_of_moves() {
        type Case<'a> = (&'a [(&'a str, &'a str)], Option<&'a str>);
        let cases: &[Case<'_>] = &[
            (
                &[("build", "suggested"), ("orient", "allowed")],
                Some("From plan, the usual next phase is build; orient is also open to you."),
            ),
            (
                &[
                    ("build", "suggested"),
                    ("orient", "allowed"),
                    ("review", "allowed"),
                ],
                Some(
                    "From plan, the usual next phase is build; orient or review are also open to you.",
                ),
            ),
            (
                &[("build", "suggested")],
                Some("From plan, the usual next phase is build."),
            ),
            (
                &[("orient", "allowed")],
                Some("From plan, orient is open to you."),
            ),
            (
                &[
                    ("orient", "allowed"),
                    ("build", "allowed"),
                    ("review", "allowed"),
                ],
                Some("From plan, orient, build or review are open to you."),
            ),
            (
                &[("build", "suggested"), ("review", "suggested")],
                Some("From plan, the usual next phases are build or review."),
            ),
            (&[], None),
        ];
        for (list, expected) in cases {
            assert_eq!(
                phase_moves("plan", &moves(list)).as_deref(),
                *expected,
                "{list:?}"
            );
        }
    }

    /// The first-ask note closes with the sentence after a blank line, and a
    /// phase with no moves (`quick`) adds nothing.
    #[test]
    fn the_phase_note_closes_with_the_moves() {
        assert_eq!(
            phase_note("plan", "Plan first.", &moves(&[("build", "suggested")])),
            "The session is now in the plan phase. Its ground rules:\n\nPlan first.\n\nFrom plan, the usual next phase is build."
        );
        assert_eq!(
            phase_note("quick", "Be quick.", &[]),
            "The session is now in the quick phase. Its ground rules:\n\nBe quick."
        );
    }

    /// The refill's system message closes with the same sentence (v38); a
    /// log before v38 rebuilds without it.
    #[test]
    fn the_refill_closes_with_the_moves_from_v38() {
        let head = vec![Message::new(Role::System, "you are the trunk")];
        let document = PhaseDocument {
            name: "plan".to_owned(),
            digest: String::new(),
            transitions: moves(&[("build", "suggested"), ("orient", "allowed")]),
            ground_rules: "Plan first.".to_owned(),
        };
        let rules = "you are the trunk\n\nGround rules for the plan phase:\n\nPlan first.";
        assert_eq!(
            with_ground_rules(&head, Some(&document))[0].content,
            format!(
                "{rules}\n\nFrom plan, the usual next phase is build; orient is also open to you."
            )
        );
        assert_eq!(
            with_ground_rules_before_v38(&head, Some(&document))[0].content,
            rules
        );
    }
}

#[cfg(test)]
mod refill_tests {
    use super::*;

    /// The head is never changed (#597): the render rides in a user
    /// message after it, inside `<summary>` tags on lines of their own.
    #[test]
    fn a_refill_leaves_the_head_alone_and_carries_the_render_in_a_user_message() {
        let head = vec![Message::new(Role::System, "you are the trunk")];
        let trunk = refill(&head, "# regime\n");
        assert_eq!(trunk[..1], head[..]);
        assert_eq!(trunk.len(), 2);
        assert_eq!(trunk[1].role, Role::User);
        assert_eq!(trunk[1].content, "<summary>\n# regime\n\n</summary>");
        // Before #597, appended to the system message.
        let before = refill_in_the_system_message(&head, "# regime\n");
        assert_eq!(before.len(), 1);
        assert_eq!(before[0].content, "you are the trunk\n\n# regime\n");
    }
}

#[cfg(test)]
mod tail_tests {
    use super::{Message, Role, estimated_tokens, tail};
    use crate::client::shape::ToolCall;

    fn user(text: &str) -> Message {
        Message::new(Role::User, text)
    }

    fn turn(ask: &str, chars: usize) -> Vec<Message> {
        let mut call = Message::new(Role::Assistant, "");
        call.tool_calls = vec![ToolCall {
            id: "c".to_owned(),
            name: "bash".to_owned(),
            arguments: "{}".to_owned(),
        }];
        vec![
            user(ask),
            call,
            Message::tool_result("c".to_owned(), "x".repeat(chars)),
            Message::new(Role::Assistant, "done"),
        ]
    }

    fn tokens(messages: &[Message]) -> u64 {
        messages.iter().map(estimated_tokens).sum()
    }

    /// A message's estimate is its characters over four, rounded up, its
    /// tool calls' names and arguments counted.
    #[test]
    fn a_messages_tokens_are_its_characters_over_four_rounded_up() {
        assert_eq!(estimated_tokens(&user("abcde")), 2);
        let mut call = Message::new(Role::Assistant, "");
        call.tool_calls = vec![ToolCall {
            id: "c".to_owned(),
            name: "bash".to_owned(),
            arguments: "{\"command\":\"ls\"}".to_owned(),
        }];
        assert_eq!(estimated_tokens(&call), 5);
    }

    /// Whole turns from the newest back, within the budget; the turn that
    /// would cross it is not kept; a budget of 0, or one no whole turn fits,
    /// keeps nothing.
    #[test]
    fn the_tail_is_whole_turns_from_the_newest_back_within_the_budget() {
        let turns: Vec<Message> = [turn("one", 400), turn("two", 400), turn("three", 400)].concat();
        let one_turn = tokens(&turn("three", 400));
        assert!(tail(&turns, 0).is_empty());
        assert!(
            tail(&turns, one_turn - 1).is_empty(),
            "the crossing turn is not kept"
        );
        let kept = tail(&turns, one_turn);
        assert_eq!(kept.len(), 4);
        assert_eq!(kept[0], user("three"));
        let kept = tail(&turns, one_turn * 2 + 5);
        assert_eq!(kept.len(), 8);
        assert_eq!(kept[0], user("two"));
        assert_eq!(tail(&turns, u64::MAX).len(), turns.len());
    }

    /// Turns that start mid-way (a trunk whose first kept message is not an
    /// ask) are never cut into: the tail starts only at a user message.
    #[test]
    fn the_tail_never_starts_at_a_reply_or_a_tool_result() {
        let mut turns = turn("one", 10);
        turns.remove(0);
        assert!(
            tail(&turns, u64::MAX).is_empty(),
            "{:?}",
            tail(&turns, u64::MAX)
        );
    }
}

#[cfg(test)]
mod review_tests {
    use super::{Message, Role, estimated_tokens, tail};

    /// REVIEW: the notes an ask carries (#609) go on the wire, so a
    /// message's estimate counts them, and the tail's budget holds.
    #[test]
    fn review_an_asks_notes_are_counted() {
        let mut ask = Message::new(Role::User, "hi");
        ask.notes = vec!["x".repeat(4_000)];
        assert!(
            estimated_tokens(&ask) >= 1_000,
            "{}",
            estimated_tokens(&ask)
        );
        let turns = vec![ask, Message::new(Role::Assistant, "ok")];
        assert!(
            tail(&turns, 100).is_empty(),
            "a turn of over 1,000 tokens kept under a 100-token tail budget"
        );
    }
}

#[cfg(test)]
mod refutation_tests {
    use super::{Budget, OverBudget, rendered};
    use crate::object::{Confidence, EntryId, Patch, Provenance, Refutation, WorkingObject};

    fn object() -> WorkingObject {
        let source = r#"{"source":{"kind":"live"},"record":"start","regime":{"arm":"baseline","dogma_version":0,"substrates":[{"id":"local","engine":{"name":"a-runtime","version_or_digest":"1.0"},"weights":{"kind":"digest","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"hardware_fingerprint":"aaa9402664f1a41f40ebbc52c9993eb66aeb366602958fdfaa283b71e64db123","sampler_card":{"seed":0},"reasoning":"on"}]}}"#;
        let regime = crate::formats::record::parse(source)
            .expect("a record")
            .regime()
            .clone();
        let at = |turn: u32| Provenance {
            turn,
            lane: "interview".to_owned(),
            fork: None,
            tangent: None,
            index: 0,
        };
        let id = |text: &str| EntryId::new(text).expect("an id");
        let mut object = WorkingObject::open(regime);
        object
            .apply(&Patch::Add {
                id: id("e1"),
                content: "decision: the loader reads ./config.toml".to_owned(),
                provenance: at(1),
            })
            .expect("the claim");
        object
            .apply(&Patch::Add {
                id: id("e2"),
                content: format!("learned: {}", "a long note that goes on ".repeat(20)),
                provenance: at(2),
            })
            .expect("a long note");
        object
            .apply(&Patch::Supersede {
                id: id("e3"),
                content: "decision: the settings path is not known yet".to_owned(),
                voids: id("e1"),
                provenance: at(3),
                refuted: Some(Refutation {
                    confidence: Confidence::new(9).expect("in range"),
                    because: "cat config.toml failed".to_owned(),
                    pointer: "call call-c".to_owned(),
                }),
            })
            .expect("the refutation");
        object
    }

    const WAS: &str = "e1\tyou said, when asked\twas: decision: the loader reads ./config.toml (held at 9/10), \
                       wrong because cat config.toml failed [call call-c]\n";

    /// #878: a refuted claim stays in the refill, as what was believed, how
    /// firmly, why it is wrong and where that was shown.
    #[test]
    fn a_refuted_claim_renders_as_what_was_believed() {
        let text = rendered(&object(), None, None).text;
        assert!(text.contains(WAS), "{text}");
        assert!(
            text.contains(
                "e3\tyou said, when asked\tdecision: the settings path is not known yet\n"
            ),
            "{text}"
        );
    }

    /// And it is kept whole, as the brief is, whatever the budget.
    #[test]
    fn a_refuted_claim_is_never_reduced() {
        for over in [OverBudget::Tier, OverBudget::Elide] {
            let text = rendered(&object(), None, Some(Budget { tokens: 1, over })).text;
            assert!(text.contains(WAS), "{over:?}: {text}");
        }
    }
}

#[cfg(test)]
mod source_tests {
    use super::{render, source};
    use crate::object::{EntryId, KeptFrom, Origin, Patch, Provenance, Vouched, WorkingObject};

    fn regime() -> crate::formats::record::Regime {
        let start = r#"{"source":{"kind":"live"},"record":"start","regime":{"arm":"baseline","dogma_version":0,"substrates":[{"id":"local","engine":{"name":"a-runtime","version_or_digest":"1.0"},"weights":{"kind":"digest","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"hardware_fingerprint":"aaa9402664f1a41f40ebbc52c9993eb66aeb366602958fdfaa283b71e64db123","sampler_card":{"seed":0},"reasoning":"on"}]}}"#;
        crate::formats::record::parse(start)
            .expect("a record")
            .regime()
            .clone()
    }

    /// An object of one entry per source #1050 names, each written by its
    /// lane, with the origin a keep or a note's authority gives it.
    pub(super) fn every_source() -> WorkingObject {
        let mut object = WorkingObject::open(regime());
        let entries: &[(&str, &str, Option<Origin>)] = &[
            (
                "a-kept-ask",
                "operator",
                Some(Origin::Kept {
                    from: KeptFrom::Ask,
                    tag: None,
                }),
            ),
            (
                "b-kept-tool",
                "operator",
                Some(Origin::Kept {
                    from: KeptFrom::ToolResult,
                    tag: Some("port".to_owned()),
                }),
            ),
            (
                "c-kept-answer",
                "operator",
                Some(Origin::Kept {
                    from: KeptFrom::Answer,
                    tag: None,
                }),
            ),
            (
                "d-observed",
                "self-capture",
                Some(Origin::Noted(Vouched::Observed)),
            ),
            (
                "e-user-stated",
                "self-capture",
                Some(Origin::Noted(Vouched::UserStated)),
            ),
            (
                "f-self-stated",
                "self-capture",
                Some(Origin::Noted(Vouched::SelfStated)),
            ),
            ("g-field-kind", "self-capture", None),
            ("h-interview", "interview", None),
            ("i-audit", "audit", None),
            ("j-refutation", "trunk", None),
            ("k-mechanical", "mechanical", None),
            ("l-edited", "operator", None),
        ];
        for (index, (id, lane, origin)) in entries.iter().enumerate() {
            let id = EntryId::new(id).expect("an id");
            object
                .apply(&Patch::Add {
                    id: id.clone(),
                    content: format!("entry {index}"),
                    provenance: Provenance {
                        turn: 1,
                        lane: (*lane).to_owned(),
                        fork: None,
                        tangent: None,
                        index: u32::try_from(index).expect("few"),
                    },
                })
                .expect("it applies");
            if let Some(origin) = origin {
                object.originate(&id, origin.clone()).expect("it is held");
            }
        }
        object
    }

    /// #1050: each entry's line is its id, its source in the maintainer's
    /// phrase -- a keep's tag in brackets after -- and its text; since frame
    /// 2 (#1086) the default source's lines carry none, the legend saying it.
    #[test]
    fn each_entry_renders_its_source_between_its_id_and_its_text() {
        let text = render(&every_source(), None);
        assert!(text.contains("frame_version: 2\n"), "{text}");
        for line in [
            "a-kept-ask\tuser said\tentry 0\n",
            "b-kept-tool\tuser kept, from tool output [port]\tentry 1\n",
            "c-kept-answer\tuser kept, from your answer\tentry 2\n",
            "d-observed\tyou noted, from tool output\tentry 3\n",
            "e-user-stated\tyou noted, from the user's words\tentry 4\n",
            "f-self-stated\tyou noted\tentry 5\n",
            "g-field-kind\tyou noted\tentry 6\n",
            "h-interview\tyou said, when asked\tentry 7\n",
            "i-audit\tyou said, when asked\tentry 8\n",
            "j-refutation\tyou said, when asked\tentry 9\n",
            "k-mechanical\tentry 10\n",
            "l-edited\tuser said\tentry 11\n",
        ] {
            assert!(text.contains(line), "{line:?} in\n{text}");
        }
    }

    /// #1050: a keep that restates a model's note is the keep's -- the
    /// user's endorsement is the authority that counts -- and an entry two
    /// lanes wrote is its first writer's.
    #[test]
    fn a_keep_wins_and_otherwise_the_first_writer_does() {
        let mut object = every_source();
        let interview = EntryId::new("h-interview").expect("an id");
        let entry = |object: &WorkingObject| {
            object
                .entries()
                .find(|entry| entry.id == interview)
                .cloned()
                .expect("held")
        };
        assert_eq!(source(&entry(&object)), "you said, when asked");
        // A second lane writes the same words: deduped onto the first's.
        object
            .apply(&Patch::Add {
                id: EntryId::new("z-again").expect("an id"),
                content: "entry 7".to_owned(),
                provenance: Provenance {
                    turn: 2,
                    lane: "self-capture".to_owned(),
                    fork: None,
                    tangent: None,
                    index: 0,
                },
            })
            .expect("it dedupes");
        assert_eq!(entry(&object).provenances.len(), 2, "both writers");
        assert_eq!(
            source(&entry(&object)),
            "you said, when asked",
            "the first's"
        );
        object
            .originate(
                &interview,
                Origin::Kept {
                    from: KeptFrom::Answer,
                    tag: Some("why".to_owned()),
                },
            )
            .expect("it is held");
        assert_eq!(source(&entry(&object)), "user kept, from your answer [why]");
    }

    /// #1050: an entry's origin crosses a dump, as everything else of it does.
    #[test]
    fn an_origin_crosses_a_dump() {
        let mut object = every_source();
        // And a note's check of an entry (#1050, c4).
        object
            .confirm(
                &EntryId::new("d-observed").expect("an id"),
                &EntryId::new("h-interview").expect("an id"),
            )
            .expect("both held");
        assert!(
            render(&object, None)
                .contains("h-interview\tyou said, when asked, checked by d-observed\t")
        );
        let read = WorkingObject::from_dump(regime(), &object.dump()).expect("it reads");
        assert_eq!(read.dump(), object.dump());
        assert_eq!(render(&read, None), render(&object, None));
    }
}

#[cfg(test)]
mod legend_tests {
    use super::source_tests::every_source;
    use super::{DEFAULT_SOURCE, LEGEND, WORKING_SET_HEADER, render, source, source_in};
    use crate::object::{EntryId, Patch, Provenance, WorkingObject};

    /// `every_source`, with a second line the harness wrote: a mixed object,
    /// most of whose sources are not the default and two of which are.
    fn mixed() -> WorkingObject {
        let mut object = every_source();
        object
            .apply(&Patch::Add {
                id: EntryId::new("m-mechanical").expect("an id"),
                content: "entry 12".to_owned(),
                provenance: Provenance {
                    turn: 2,
                    lane: "mechanical".to_owned(),
                    fork: None,
                    tangent: None,
                    index: 0,
                },
            })
            .expect("it applies");
        object
    }

    /// #1086: frame 2 names the default source once, in the legend right
    /// under the working set's heading, and each line is `{id}\t{text}` when
    /// its [`source_in`] is exactly the default phrase, else
    /// `{id}\t{source_in}\t{text}`; the dump keeps every entry's origin and
    /// provenance, and [`source`] still names the default.
    #[test]
    fn frame_two_marks_only_the_lines_whose_source_is_not_the_default() {
        let object = mixed();
        let text = render(&object, None);
        assert!(text.contains("frame_version: 2\n"), "{text}");
        assert_eq!(text.matches(LEGEND).count(), 1, "{text}");
        assert!(
            text.contains(&format!("{WORKING_SET_HEADER}{LEGEND}\n")),
            "{text}"
        );
        let mut defaults = 0;
        for entry in object.live() {
            let cell = source_in(&object, entry);
            let line = if cell == DEFAULT_SOURCE {
                defaults += 1;
                format!("{}\t{}\n", entry.id.as_str(), entry.content)
            } else {
                format!("{}\t{cell}\t{}\n", entry.id.as_str(), entry.content)
            };
            assert!(text.contains(&line), "{line:?} in\n{text}");
        }
        assert_eq!(defaults, 2, "the fixture has two default-source lines");
        assert!(!text.contains(&format!("\t{DEFAULT_SOURCE}\t")), "{text}");
        // Off the trunk: the phrase, and the dump's origin and provenance.
        let mechanical = object
            .live()
            .find(|entry| entry.id.as_str() == "k-mechanical")
            .expect("held");
        assert_eq!(source(mechanical), DEFAULT_SOURCE);
        let dump = object.dump();
        assert!(
            dump.contains("k-mechanical") && dump.contains("mechanical"),
            "{dump}"
        );
        let read = WorkingObject::from_dump(object.regime().clone(), &dump).expect("it reads");
        assert_eq!(
            render(&read, None),
            text,
            "the dump carries what the render is computed from"
        );
    }

    /// #1086: a default-source line a live note checks against tool output
    /// keeps its cell, carrying the check.
    #[test]
    fn a_default_line_with_a_live_check_keeps_its_cell() {
        let mut object = mixed();
        object
            .confirm(
                &EntryId::new("d-observed").expect("an id"),
                &EntryId::new("k-mechanical").expect("an id"),
            )
            .expect("it confirms");
        let text = render(&object, None);
        assert!(
            text.contains(&format!(
                "k-mechanical\t{DEFAULT_SOURCE}, checked by d-observed\tentry 10\n"
            )),
            "{text}"
        );
        assert!(text.contains("m-mechanical\tentry 12\n"), "{text}");
    }
}
