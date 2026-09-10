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
//! at a seam.
//!
//! It is a **pure function** of the object and the phase. Nothing here reads
//! a clock, a counter, or anything that moves: two renders of the same object
//! in the same phase are the same bytes, and the byte-identity claim the seam
//! controller makes is only checkable because that is true.

use crate::object::WorkingObject;

/// Which frame this module renders with.
///
/// **A placeholder, and it says so in the render itself.** The frame is
/// prompt text the model reads, and prompt text in this program is
/// judgment-as-data: it is authored by the maintainer, pinned, and
/// byte-guarded like every other template. This one is not. Naming it in the
/// prompt means a drive under the placeholder is distinguishable from a drive
/// under the authored frame -- which matters because they are different
/// regimes, and a comparison across them that could not tell would be the
/// incomparable-regime class again.
pub const FRAME_VERSION: &str = "placeholder";

/// The working object rendered as the prompt's prefix.
///
/// The regime is carried at the top, because a prompt whose record cannot say
/// which substrate and which dogma it was built for is a prompt that does not
/// transfer.
#[must_use]
pub fn render(object: &WorkingObject, phase: Option<&str>) -> String {
    let regime = object.regime();
    let mut out = String::new();

    out.push_str("# regime\n");
    out.push_str("arm: ");
    out.push_str(&regime.arm);
    out.push_str("\nsubstrate: ");
    out.push_str(&regime.substrate.name);
    out.push_str("\ndogma_version: ");
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

    for entry in object.live() {
        out.push_str(&one_line(entry.id.as_str()));
        out.push('\t');
        out.push_str(&one_line(&entry.content));
        out.push('\n');
    }

    out
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
        total = total
            .saturating_add(one_line(entry.id.as_str()).len() as u64)
            .saturating_add(one_line(&entry.content).len() as u64)
            .saturating_add(2);
    }
    total
}

/// Where the working set begins in a render.
///
/// Public so a caller -- and the test that pins
/// [`working_set_bytes`] against what is actually emitted -- can find the
/// section without re-deriving the header's shape.
pub const WORKING_SET_HEADER: &str = "# working set\n";
