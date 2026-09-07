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
    out.push_str("\n\n# working set\n");

    for entry in object.live() {
        out.push_str(entry.id.as_str());
        out.push('\t');
        out.push_str(&entry.content);
        out.push('\n');
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
        // A count that saturates is a count that stops being a measurement.
        // Nothing here can reach it -- an object of eighteen exabytes is not
        // a thing -- and saturating is still the right floor for a number
        // that decides a trigger.
        total = total
            .saturating_add(entry.id.as_str().len() as u64)
            .saturating_add(entry.content.len() as u64)
            .saturating_add(2);
    }
    total
}
