//! The files whose text the context shows (#1149, #1150): one rule, the
//! maintainer's of 2026-11-20 (Qwen Code's), read off what the context
//! actually sends, so a carried trunk, a resumed turn, a refill's tail and its
//! kept outputs are all counted the same way.
//!
//! A file is shown while the messages sent hold a result that shows its text:
//! a `read` result of any range of it, or the model's own successful `write`
//! or `edit` of it. Each result is paired with its call, as the log that
//! recorded the call has it -- this session's, or, for a carried call, the
//! first session's -- and a failed call shows no file, by the standard
//! surface's own reading of its result ([`super::standard::shows_file`]),
//! one rule for both, since the log records no failure of its own. A result
//! counts where it sits on its own, where the user's wrap left a kept span of
//! it (a partial read, as the ruling has it), and where a refill's outputs
//! section prints it or an excerpt of it. A wrap with no span clears it from
//! the request after the wrap is accepted; a prune, a refill that drops it,
//! or a reference line carries none of it. `bash` output never counts, so no
//! command is parsed.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::client::shape::{Message, Role};
use crate::meditation::outputs;

/// A call whose result shows a file's text: a successful `read`, `write` or
/// `edit`, as the log that recorded it has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Showing {
    /// The file it shows, as its arguments name it.
    pub file: String,
    /// Its id, which its result's message names.
    pub id: String,
    /// The turn it was made in, as its log counts turns.
    pub turn: u32,
    /// Its tool.
    pub name: String,
    /// Its arguments, as the model wrote them.
    pub arguments: String,
    /// Its result, as the context first carried it.
    pub shown: String,
    /// The span the user wrapped its result with (#1030), which takes the
    /// result's place.
    pub span: Option<String>,
    /// Whether the user wrapped it with no span (#1050): cleared from the
    /// request after the wrap is accepted.
    pub cleared: bool,
}

/// When a file was last shown: the call whose result shows it, and that
/// result's place among the messages sent. What a check of the file against
/// its showing (#1150's, should it ever compare the file's state since)
/// would start from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    /// The call's id.
    pub call: String,
    /// The place of the message that shows it, from 0.
    pub place: usize,
}

impl Showing {
    /// The call `id` made in `turn` with `name` and `arguments`, as its result
    /// `shown` was first carried, when that result shows a file
    /// ([`super::standard::shows_file`]); `None` otherwise.
    #[must_use]
    pub fn of(id: &str, turn: u32, (name, arguments): (&str, &str), shown: &str) -> Option<Self> {
        Some(Self {
            file: super::standard::shows_file(name, arguments, shown)?,
            id: id.to_owned(),
            turn,
            name: name.to_owned(),
            arguments: arguments.to_owned(),
            shown: shown.to_owned(),
            span: None,
            cleared: false,
        })
    }

    /// Whether `message`, one the context sends, shows this call's file:
    /// its result where it sits, or its kept span; or, in a refill's outputs
    /// section, its result or an excerpt of it. `rewrite` is what the
    /// section's paths went through.
    #[must_use]
    pub fn shown_in(&self, message: &Message, rewrite: &dyn Fn(&str) -> String) -> bool {
        match (message.role, message.tool_call_id.as_deref()) {
            (Role::Tool, Some(id)) => {
                id == self.id
                    && ((!self.cleared && message.content == self.shown)
                        || self.span.as_deref() == Some(message.content.as_str()))
            }
            _ => {
                !self.shown.is_empty()
                    && outputs::carries_text_of(
                        &message.content,
                        (self.turn, &self.name, &self.arguments),
                        rewrite,
                    )
            }
        }
    }
}

/// The files `calls` show in `sent`, the messages the context sends in
/// order, each by `key` of the file its call names, at its latest showing.
/// `rewrite` is what a refill's section went through
/// ([`super::shown::ShownRoots::text`]).
pub fn shown<'a>(
    sent: impl IntoIterator<Item = &'a Message>,
    calls: &[Showing],
    key: &dyn Fn(&str) -> PathBuf,
    rewrite: &dyn Fn(&str) -> String,
) -> BTreeMap<PathBuf, Shown> {
    let mut by_id: BTreeMap<&str, Vec<&Showing>> = BTreeMap::new();
    for call in calls {
        by_id.entry(call.id.as_str()).or_default().push(call);
    }
    let mut out = BTreeMap::new();
    for (place, message) in sent.into_iter().enumerate() {
        let candidates: Vec<&Showing> = match (message.role, message.tool_call_id.as_deref()) {
            (Role::Tool, Some(id)) => by_id.get(id).into_iter().flatten().copied().collect(),
            _ if message.content.contains(outputs::HEADER) => calls.iter().collect(),
            _ => Vec::new(),
        };
        for call in candidates
            .into_iter()
            .filter(|call| call.shown_in(message, rewrite))
        {
            out.insert(
                key(&call.file),
                Shown {
                    call: call.id.clone(),
                    place,
                },
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(id: &str, file: &str, shown: &str) -> Showing {
        let arguments = serde_json::json!({"path": file}).to_string();
        Showing::of(id, 1, (super::super::standard::READ, &arguments), shown).expect("a read")
    }

    fn result(id: &str, content: &str) -> Message {
        let mut message = Message::new(Role::Tool, content);
        message.tool_call_id = Some(id.to_owned());
        message
    }

    fn files(sent: &[Message], calls: &[Showing]) -> Vec<(String, usize)> {
        shown(sent, calls, &|file| PathBuf::from(file), &str::to_owned)
            .into_iter()
            .map(|(file, shown)| (file.to_string_lossy().into_owned(), shown.place))
            .collect()
    }

    /// A result shows its file where it sits, under its kept span, or in a
    /// section's text; not where a wrap cleared it, nor under another call's
    /// id, nor once its text is gone.
    #[test]
    fn a_result_shows_its_file_where_its_text_is_sent() {
        let a = read("c1", "a.py", "x = 1\n");
        let b = Showing {
            span: Some("y".to_owned()),
            ..read("c2", "b.py", "y = 1\n")
        };
        let c = Showing {
            cleared: true,
            ..read("c3", "c.py", "z = 1\n")
        };
        let sent = [
            Message::new(Role::System, "head"),
            result("c1", "x = 1\n"),
            result("c2", "y"),
            result("c3", "z = 1\n"),
            result("c9", "x = 1\n"),
        ];
        assert_eq!(
            files(&sent, &[a.clone(), b, c]),
            [("a.py".to_owned(), 1), ("b.py".to_owned(), 2)]
        );
        let section = format!(
            "<summary>\n{}{}{}\n</summary>",
            outputs::HEADER,
            "- turn 1: read args={\"path\":\"a.py\"}\n",
            format_args!("{}x = 1\n", outputs::RESULT)
        );
        assert_eq!(
            files(
                &[Message::new(Role::User, &section)],
                std::slice::from_ref(&a)
            ),
            [("a.py".to_owned(), 0)]
        );
        assert_eq!(files(&[result("c1", "[cleared]")], &[a]), []);
    }
}
