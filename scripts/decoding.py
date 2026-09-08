"""The one decoder: content as a reader would see it, not as it is stored.

A hygiene pattern guards prose. Logs are not prose: a captured command's
output reaches this repository inside a JSON string, where a newline is the
two characters `\\` and `n` and every quote is escaped. A token immediately
after such a newline has a literal `n` welded to its left, so a pattern
anchored on a word boundary walks straight past it -- and a salted digest,
which cannot be given a looser boundary at all, misses it outright.

So every consumer of the pattern table and the digest table scans a DECODED
VIEW beside the bytes on disk. This module is the only thing that produces
one.

It sniffs content rather than trusting a filename. A `.jsonl` fixture, a
pretty-printed `.json` file and the patch text of either -- where each line
carries a `+`, `-` or space in front -- all reach the same decoder, because
the third is the one `check-history.py` needs and an extension would not
have found it.
"""

from __future__ import annotations

import json

# A diff line's first column. Stripped before a line is offered to the JSON
# reader, so that the patch text of a record file decodes as the record does.
DIFF_MARKERS = "+- "


def _strings(value: object, into: list[str]) -> None:
    """Every string inside `value`, keys included, in document order."""
    if isinstance(value, str):
        into.append(value)
    elif isinstance(value, dict):
        for key, member in value.items():
            into.append(key)
            _strings(member, into)
    elif isinstance(value, list):
        for member in value:
            _strings(member, into)


def _parsed(text: str) -> object | None:
    """`text` as JSON, or None if it is not JSON.

    A bare number or `true` parses and carries no strings, so it is rejected
    here rather than walked: `1` is not a document, and treating every line of
    digits as one would put the decoder in the hot path for nothing.
    """
    text = text.strip()
    if not text or text[0] not in "{[\"":
        return None
    try:
        return json.loads(text)
    except (json.JSONDecodeError, RecursionError):
        return None


def decoded_text(text: str) -> str:
    """The strings `text` carries, decoded, one per line.

    Empty when there is nothing encoded to see -- a plain-prose file decodes
    to nothing, and a caller writing a mirror should skip it rather than write
    an empty file that a scan would then have to call clean.

    Tries the whole text first, so a pretty-printed JSON document is read as
    the document it is, and falls back to line by line, which is what a JSONL
    file and a patch of one need.
    """
    whole = _parsed(text)
    if whole is not None:
        found: list[str] = []
        _strings(whole, found)
        return "".join(f"{line}\n" for line in found)

    found = []
    for line in text.splitlines():
        candidates = [line]
        if line[:1] in DIFF_MARKERS:
            candidates.append(line[1:])
        for candidate in candidates:
            value = _parsed(candidate)
            if value is not None:
                _strings(value, found)
                break
    return "".join(f"{line}\n" for line in found)


def tokens(text: str) -> list[str]:
    """`text` split into the tokens a digest row is compared against.

    SPLIT ON EVERY NON-ALPHANUMERIC, not on a class that keeps `.` and `-`.
    The obvious class keeps `owner.example.net` as one token, so a digest of
    the bare name never matches it -- measured, and the reason this function
    exists rather than a regex at each call site.
    """
    out: list[str] = []
    current: list[str] = []
    for char in text:
        if char.isalnum():
            current.append(char)
        elif current:
            out.append("".join(current))
            current = []
    if current:
        out.append("".join(current))
    return out
