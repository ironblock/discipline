//! Archive recall (#566): what left the trunk at a seam, kept in memory, and
//! the lever that recalls it.
//!
//! **The archive** is one in-memory index per session. A seam adds every
//! message its refill dropped from the trunk -- the operator's asks, the
//! model's answers and the tool results it was shown -- and every working
//! memory entry that is no longer live (voided, resolved, retired, parked).
//! Nothing here is written to the log but by reference: an item's key and
//! its digest.
//!
//! **The lever,** `archive_recall`: `off` (the default) or `literal`. Pi and
//! `OpenCode` 2 recall nothing after compaction beyond a pointer to a saved
//! tool output, so `off` is their convention and the default; Qwen Code
//! recalls automatically, every turn, by keyword match, into a reminder on
//! the user turn (`qc:packages/core/src/memory/recall.ts:379-405`,
//! `core/client.ts:4589-4600`), and that is the shape `literal` takes here.
//! Any other value -- `embedding` among them, which is parked out of v0.1.0
//! -- reads as `off`, and the start row names the value it fell back from.
//! The embedding seam stays where it is (`capture::sense::Embedder`),
//! unwired.
//!
//! **When it fires:** at each ask once a seam has archived something. The
//! ask is the query; the [`TOP`] best items go as one note after the ask, in
//! Qwen Code's "Relevant memory" shape (`memory/recall.ts:452-478`) wrapped
//! as its system reminder (`core/environmentContext.ts:25-26,116-117`). The
//! note is a user message: it never touches the system prompt, and it stays
//! on the trunk, so the prefix holds.
//!
//! **Literal matching** is this crate's own: the ask's anchors --
//! identifiers, paths and quoted spans (`capture::collector::literal::anchors`)
//! -- each found as a whole token in an item's title or text, an item scored
//! by how many of them it holds, ties to the most recently archived.

use crate::capture::collector::literal;

/// The regimen key for the lever.
pub const ARCHIVE_RECALL: &str = "archive_recall";

/// How many items a recall carries, at most.
pub const TOP: usize = 3;

/// An item's text, in characters, past which the note truncates it (Qwen
/// Code's `MAX_DOC_BODY_CHARS`, `memory/recall.ts:44`).
pub const MAX_BODY_CHARS: usize = 1_200;

/// The lever's state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Recall {
    /// Nothing is recalled.
    #[default]
    Off,
    /// By the ask's anchors, matched whole.
    Literal,
}

impl Recall {
    /// The state `regimen` declares, read leniently: `literal`, and anything
    /// else, or nothing, `off`.
    #[must_use]
    pub fn of(regimen: &crate::formats::regimen::Regimen) -> Self {
        match regimen.get(ARCHIVE_RECALL) {
            Some(crate::formats::regimen::Value::String(state)) if state == "literal" => {
                Self::Literal
            }
            _ => Self::Off,
        }
    }

    /// The start row's word for the state `regimen` declares: `off` or
    /// `literal`, and for a value read as `off` that is not, the value it
    /// fell back from (`off:unknown:embedding`).
    #[must_use]
    pub fn lever(regimen: &crate::formats::regimen::Regimen) -> String {
        match (Self::of(regimen), regimen.get(ARCHIVE_RECALL)) {
            (Self::Literal, _) => "literal".to_owned(),
            (Self::Off, None) => "off".to_owned(),
            (Self::Off, Some(crate::formats::regimen::Value::String(state))) if state == "off" => {
                "off".to_owned()
            }
            (Self::Off, Some(crate::formats::regimen::Value::String(state))) => {
                format!("off:unknown:{state}")
            }
            (Self::Off, Some(_)) => "off:unknown".to_owned(),
        }
    }
}

/// One archived item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// What names it: `seam-<turn>/message-<n>` for a dropped message,
    /// `entry/<id>` for a working-memory entry.
    pub key: String,
    /// What it is, in a few words: the note's heading.
    pub title: String,
    /// Its text, whole.
    pub text: String,
}

impl Item {
    /// The sha256 of its text: what the log names it by.
    #[must_use]
    pub fn sha256(&self) -> String {
        crate::digest::sha256_hex(self.text.as_bytes())
    }
}

/// The archive: items in the order they were archived.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Archive {
    items: Vec<Item>,
}

impl Archive {
    /// Whether anything is archived.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Archive `item`, unless an item with its key already is.
    pub fn push(&mut self, item: Item) {
        if !item.text.trim().is_empty() && !self.items.iter().any(|held| held.key == item.key) {
            self.items.push(item);
        }
    }

    /// The [`TOP`] items `query`'s anchors find, best first, each with how
    /// many anchors it holds and the byte offset its note centres on.
    ///
    /// **Ranked, in order, by:** the distinct anchors it holds (in its text
    /// or title); then whether its text DEFINES one -- the anchor right
    /// after a definition keyword (`def`, `fn`, `class`, ...), the item a
    /// question about that name most wants; then how often the anchors
    /// occur in it; then recency. Recency first among equals let newer
    /// mentions crowd out an older archive's definition (seam smoke run 3:
    /// turn 3's `def heading_slug` lost to newer items holding the same
    /// two anchors).
    ///
    /// **Centred on** the densest cluster: the hit with the most distinct
    /// anchors in the window a note around it would keep, a definition
    /// among equals, then the earliest. Not the rarest anchor: rarity is a
    /// fact about the whole archive, and the window is about this item --
    /// the region that answers most of the question. Empty when the query
    /// has no anchor or no item holds one.
    #[must_use]
    pub fn literal(&self, query: &str) -> Vec<Found<'_>> {
        let anchors = literal::anchors(query);
        if anchors.is_empty() {
            return Vec::new();
        }
        let mut scored: Vec<(usize, Scored<'_>)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(at, item)| scored(item, &anchors).map(|scored| (at, scored)))
            .collect();
        scored.sort_by(|(a_at, a), (b_at, b)| {
            b.held
                .cmp(&a.held)
                .then(b.defines.cmp(&a.defines))
                .then(b.hits.len().cmp(&a.hits.len()))
                .then(b_at.cmp(a_at))
        });
        scored
            .into_iter()
            .take(TOP)
            .map(|(_, scored)| Found {
                item: scored.item,
                score: scored.held,
                at: centre(&scored.hits),
            })
            .collect()
    }
}

/// The keywords a name is defined after, in the languages a session's
/// tools show: `def` and `class` (Python), `fn`, `struct`, `enum`, `trait`
/// (Rust), `function`, `class`, `interface`, `type`, `const` (TypeScript),
/// `func` (Go).
const DEFINING: &[&str] = &[
    "def",
    "class",
    "fn",
    "struct",
    "enum",
    "trait",
    "function",
    "interface",
    "type",
    "const",
    "func",
];

/// One anchor's occurrence in an item's text: where, which anchor, and
/// whether it is a definition.
#[derive(Debug, Clone, Copy)]
struct Hit {
    offset: usize,
    anchor: usize,
    defines: bool,
}

/// An item and what the query's anchors find in it.
struct Scored<'a> {
    item: &'a Item,
    held: u64,
    defines: bool,
    hits: Vec<Hit>,
}

/// What `anchors` find in `item`, or `None` when it holds none.
fn scored<'a>(item: &'a Item, anchors: &[literal::Anchor]) -> Option<Scored<'a>> {
    let mut hits = Vec::new();
    let mut held = 0;
    for (index, anchor) in anchors.iter().enumerate() {
        let in_text = literal::find(&anchor.text, &item.text);
        if !in_text.is_empty() || !literal::find(&anchor.text, &item.title).is_empty() {
            held += 1;
        }
        hits.extend(in_text.into_iter().map(|hit| Hit {
            offset: hit.offset,
            anchor: index,
            defines: defined_at(&item.text, hit.offset),
        }));
    }
    (held > 0).then(|| Scored {
        item,
        held,
        defines: hits.iter().any(|hit| hit.defines),
        hits,
    })
}

/// Whether the word right before byte `at` of `text` is a [`DEFINING`]
/// keyword.
fn defined_at(text: &str, at: usize) -> bool {
    let before = text[..at].trim_end();
    let word = before
        .rsplit(|c: char| !(c.is_alphanumeric() || c == '_'))
        .next()
        .unwrap_or_default();
    before.len() < at && DEFINING.contains(&word)
}

/// The hit a note centres on: the most distinct anchors within the window
/// [`window`] keeps around it (a quarter of [`MAX_BODY_CHARS`] before,
/// the rest after, counted in bytes), a definition among equals, then the
/// earliest. `None` for no hit in the text.
fn centre(hits: &[Hit]) -> Option<usize> {
    let before = MAX_BODY_CHARS / 4;
    let after = MAX_BODY_CHARS - before;
    hits.iter()
        .map(|hit| {
            let near: std::collections::BTreeSet<usize> = hits
                .iter()
                .filter(|other| {
                    other.offset + before >= hit.offset && other.offset < hit.offset + after
                })
                .map(|other| other.anchor)
                .collect();
            (
                near.len(),
                hit.defines,
                std::cmp::Reverse(hit.offset),
                hit.offset,
            )
        })
        .max()
        .map(|(.., offset)| offset)
}

/// An item a literal recall found (#566).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found<'a> {
    /// The item.
    pub item: &'a Item,
    /// How many of the query's anchors it holds.
    pub score: u64,
    /// The byte offset in its text its note centres on ([`Archive::literal`]);
    /// `None` when it holds the anchors only in its title.
    pub at: Option<usize>,
}

/// The note a recall sends: Qwen Code's "Relevant memory" block, each item
/// under its title and key, wrapped as a system reminder; empty for no
/// items. A text past [`MAX_BODY_CHARS`] is cut to a window of that many
/// characters around its first match -- a quarter of it before, so the
/// match keeps its lead-in -- each elided end marked `[…]`. Cut from the
/// start, the match could fall outside what was sent (seam smoke run 2:
/// the matched definition was cut off, and the model rightly said it could
/// not confirm it existed).
#[must_use]
pub fn note(items: &[Found<'_>]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let mut lines = vec![
        "## Relevant memory".to_owned(),
        String::new(),
        "Use the following memories only when they are directly relevant to the current \
         request. Verify file/function claims before relying on them."
            .to_owned(),
        String::new(),
    ];
    for found in items {
        let item = found.item;
        let lead = item.text.len() - item.text.trim_start().len();
        let text = item.text.trim();
        let body = if text.chars().count() <= MAX_BODY_CHARS {
            text.to_owned()
        } else {
            format!(
                "{}\n\n> NOTE: Relevant memory truncated for prompt budget.",
                window(text, found.at.map(|at| at.saturating_sub(lead)))
            )
        };
        lines.push(format!("### {} ({})", item.title, item.key));
        lines.push(String::new());
        lines.push(body);
        lines.push(String::new());
    }
    format!(
        "<system-reminder>\n{}\n</system-reminder>",
        lines.join("\n")
    )
}

/// [`MAX_BODY_CHARS`] characters of `text` around the byte offset `at` --
/// from the start when there is none -- a quarter of them before it, the
/// window kept inside the text, each end that elides text marked `[…]`.
fn window(text: &str, at: Option<usize>) -> String {
    let chars: Vec<char> = text.chars().collect();
    let at = at
        .filter(|at| text.is_char_boundary(*at))
        .map_or(0, |at| text[..at].chars().count());
    let start = at
        .saturating_sub(MAX_BODY_CHARS / 4)
        .min(chars.len().saturating_sub(MAX_BODY_CHARS));
    let end = (start + MAX_BODY_CHARS).min(chars.len());
    let cut: String = chars[start..end].iter().collect();
    format!(
        "{}{}{}",
        if start > 0 { "[…] " } else { "" },
        cut.trim(),
        if end < chars.len() { " […]" } else { "" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(key: &str, title: &str, text: &str) -> Item {
        Item {
            key: key.to_owned(),
            title: title.to_owned(),
            text: text.to_owned(),
        }
    }

    #[test]
    fn literal_recall_ranks_by_anchors_held_then_recency_and_keeps_three() {
        let mut archive = Archive::default();
        archive.push(item(
            "a",
            "an answer",
            "we chose `parse_regimen` in src/regimen.rs",
        ));
        archive.push(item("b", "an answer", "parse_regimen is called from main"));
        archive.push(item("c", "an answer", "nothing relevant"));
        archive.push(item("d", "a tool result", "src/regimen.rs: 120 lines"));
        archive.push(item("e", "an answer", "parse_regimen again"));
        // A duplicate key is not archived twice; an empty text not at all.
        archive.push(item("a", "dup", "parse_regimen src/regimen.rs"));
        archive.push(item("f", "empty", "  "));
        let hits: Vec<(&str, u64)> = archive
            .literal("where is parse_regimen in src/regimen.rs")
            .into_iter()
            .map(|found| (found.item.key.as_str(), found.score))
            .collect();
        assert_eq!(hits, [("a", 2), ("e", 1), ("d", 1)]);
        assert!(
            archive.literal("what now?").is_empty(),
            "no anchor, no recall"
        );
    }

    #[test]
    fn the_note_is_qwen_codes_relevant_memory_block_as_a_reminder() {
        let long = "x".repeat(MAX_BODY_CHARS + 5);
        let first = item(
            "seam-2/message-1",
            "an answer",
            " we chose parse_regimen \n",
        );
        let second = item("entry/e-1", "a retired entry", &long);
        let found = |item, score| Found {
            item,
            score,
            at: None,
        };
        let text = note(&[found(&first, 1), found(&second, 1)]);
        assert_eq!(
            text,
            format!(
                "<system-reminder>\n## Relevant memory\n\nUse the following memories only when \
                 they are directly relevant to the current request. Verify file/function claims \
                 before relying on them.\n\n### an answer (seam-2/message-1)\n\nwe chose \
                 parse_regimen\n\n### a retired entry (entry/e-1)\n\n{} […]\n\n> NOTE: Relevant memory \
                 truncated for prompt budget.\n\n</system-reminder>",
                "x".repeat(MAX_BODY_CHARS)
            )
        );
        assert_eq!(note(&[]), "");
    }

    /// #566, seam smoke run 2: a match past the first [`MAX_BODY_CHARS`]
    /// characters is in the note -- a window around it, a quarter before,
    /// both elided ends marked -- not cut off with the rest.
    #[test]
    fn a_long_items_note_keeps_the_window_around_its_match() {
        let before = "a".repeat(MAX_BODY_CHARS * 2);
        let after = "z".repeat(MAX_BODY_CHARS);
        let mut archive = Archive::default();
        archive.push(item(
            "seam-1/message-3",
            "a tool result",
            &format!(
                "{before} fn local_path(root: &Path) -> PathBuf {{ root.join(\"x\") }} {after}"
            ),
        ));
        let found = archive.literal("does local_path exist?");
        let [hit] = found.as_slice() else {
            panic!("one item: {found:?}");
        };
        assert_eq!(hit.at, Some(MAX_BODY_CHARS * 2 + 4));
        let text = note(&found);
        assert!(
            text.contains("fn local_path(root: &Path) -> PathBuf"),
            "{text}"
        );
        let body = text
            .split("\n\n")
            .find(|part| part.contains("local_path"))
            .expect("the body");
        assert!(
            body.starts_with("[…] a") && body.ends_with("z […]"),
            "{body}"
        );
        let kept = body.trim_start_matches("[…] ").trim_end_matches(" […]");
        assert!(
            kept.chars().count() <= MAX_BODY_CHARS,
            "{}",
            kept.chars().count()
        );
        // The anchor itself sits a quarter of the window in.
        let lead = kept.find("local_path").expect("the match");
        assert_eq!(kept[..lead].chars().count(), MAX_BODY_CHARS / 4);
    }

    /// #566, seam smoke run 3: among items holding the same anchors, the
    /// one that defines the asked name comes first however old it is; then
    /// the one where the anchors occur most; recency only after both.
    #[test]
    fn a_tie_goes_to_the_definition_then_the_most_occurrences_then_recency() {
        let mut archive = Archive::default();
        archive.push(item(
            "seam-1/message-7",
            "a tool result",
            "render.py:\ndef heading_slug(text):\n    return text.lower()",
        ));
        archive.push(item(
            "seam-8/message-45",
            "an answer",
            "render.py calls heading_slug twice; heading_slug is unchanged",
        ));
        archive.push(item(
            "seam-8/message-46",
            "an answer",
            "render.py still uses heading_slug",
        ));
        archive.push(item(
            "seam-8/message-50",
            "an answer",
            "heading_slug in render.py is fine",
        ));
        let hits: Vec<(&str, u64)> = archive
            .literal("the helper heading_slug from the very first version of render.py")
            .into_iter()
            .map(|found| (found.item.key.as_str(), found.score))
            .collect();
        assert_eq!(
            hits,
            [
                ("seam-1/message-7", 2),
                ("seam-8/message-45", 2),
                ("seam-8/message-50", 2)
            ]
        );
    }

    /// #566, seam smoke run 3: the note centres on the region holding the
    /// most of the asked anchors, a definition among equals -- not on the
    /// first mention of any of them.
    #[test]
    fn a_note_centres_on_the_densest_region_not_the_first_mention() {
        let filler = "a ".repeat(MAX_BODY_CHARS);
        let text = format!(
            "see heading_slug below\n{filler}\nrender.py:\ndef heading_slug(text):\n{filler}"
        );
        let definition = text.find("heading_slug(").expect("the definition");
        let mut archive = Archive::default();
        archive.push(item("seam-1/message-7", "a tool result", &text));
        let found = archive.literal("heading_slug in render.py");
        let [hit] = found.as_slice() else {
            panic!("one item: {found:?}");
        };
        // render.py and the definition are in the window from either; of
        // the two densest hits, the definition is the centre.
        assert_eq!(hit.at, Some(definition));
        assert!(text.find("render.py") < Some(definition));
        assert!(note(&found).contains("def heading_slug(text)"));
        // Equal density: the definition, not the earlier mention.
        let mut archive = Archive::default();
        archive.push(item("seam-1/message-7", "a tool result", &text));
        let found = archive.literal("where is heading_slug defined");
        assert_eq!(found[0].at, Some(definition));
    }

    #[test]
    fn the_lever_reads_leniently() {
        let read =
            |text: &str| Recall::of(&crate::formats::regimen::parse(text).expect("a regimen"));
        assert_eq!(read("arm = \"a\"\n"), Recall::Off);
        assert_eq!(read("archive_recall = \"literal\"\n"), Recall::Literal);
        // Embedding is parked: an unknown value, read as off, and named.
        assert_eq!(read("archive_recall = \"embedding\"\n"), Recall::Off);
        assert_eq!(read("archive_recall = \"loud\"\n"), Recall::Off);
        let lever =
            |text: &str| Recall::lever(&crate::formats::regimen::parse(text).expect("a regimen"));
        assert_eq!(lever("arm = \"a\"\n"), "off");
        assert_eq!(lever("archive_recall = \"off\"\n"), "off");
        assert_eq!(lever("archive_recall = \"literal\"\n"), "literal");
        assert_eq!(
            lever("archive_recall = \"embedding\"\n"),
            "off:unknown:embedding"
        );
    }
}
