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
import pathlib
import unicodedata

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


# How many times to decode. Content reaches this repository wrapped, and
# sometimes wrapped twice: a tool that logs another tool's stdout stores an
# already-JSON string inside its own JSON. One pass leaves that inner layer
# still escaped, which is the exact state -- a token with an `n` welded to its
# front by an escaped newline -- that this decoder exists to undo. Four is
# past anything seen here and small enough that a file of deeply nested
# strings cannot make the scan expensive.
MAX_PASSES = 4


def decoded_text(text: str) -> str:
    """The strings `text` carries, decoded, one per line, at every depth.

    Empty when there is nothing encoded to see -- a plain-prose file decodes
    to nothing, and a caller writing a mirror should skip it rather than write
    an empty file that a scan would then have to call clean.

    EVERY LAYER IS KEPT, not just the innermost. A literal can be visible one
    level down and gone the next, because each pass keeps only what parsed;
    returning the last layer alone would lose whatever the previous one was
    the only view of.
    """
    seen = {text}
    layers: list[str] = []
    layer = text
    for _ in range(MAX_PASSES):
        peeled = _decoded_once(layer)
        # A layer that decodes to nothing new is the bottom. `seen` is what
        # stops a self-encoding document -- a string whose decoding is itself
        # -- from spinning until the cap.
        if not peeled or peeled in seen:
            break
        seen.add(peeled)
        layers.append(peeled)
        # AND the same layer with its escapes spent. Parsing JSON undoes ONE
        # level of escaping, so text that was escaped twice arrives here with
        # `\` and `n` still sitting as two characters in front of a token --
        # which is the welding this whole decoder exists to undo, one level
        # further down than the version that only parsed.
        plain = _unescaped(peeled)
        if plain != peeled and plain not in seen:
            seen.add(plain)
            layers.append(plain)
        # The next pass reads the PARSED layer, not the unescaped one: spending
        # the escapes breaks any JSON still nested inside, so the two views have
        # to travel separately rather than one replacing the other.
        layer = peeled
    return "".join(layers)


def _unescaped(text: str) -> str:
    """`text` with the escape sequences that weld tokens spent as whitespace.

    Only the three that join a token to what precedes it. This can split a
    token that was never escaped -- a Windows path reads `\name` the same way
    -- and that is the side to be wrong on: a split token can only ever produce
    a hit a person then dismisses, where a welded one produces silence.
    """
    for escape, character in (("\\n", "\n"), ("\\t", "\t"), ("\\r", "\r")):
        text = text.replace(escape, character)
    return text


def _decoded_once(text: str) -> str:
    """One pass of `decoded_text`.

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


class NoTable(Exception):
    """The invisible-character table is missing or unreadable.

    Its own exception, and every caller turns it into EXIT 2, for the reason
    the callers already spell out: exit 1 is "the scan ran and found
    something". A tokeniser that does not know which characters are invisible
    is a tokeniser that cannot scan, and calling that clean would be the
    vacuous class -- the whole point of the table is the characters a reader
    cannot see, so a scan without it is exactly as blind as the defect.
    """


TABLE = pathlib.Path(__file__).resolve().parent / "default-ignorable.tsv"

# Nonspacing marks: combining accents. VISIBLE, but not separators, and they
# stay in the strip -- ruled 2026-09-11, and the ruling of 2026-09-12 that
# widened everything else left this one standing. `unicodedata` carries them,
# so unlike the table below they need no harvest.
COMBINING = frozenset({"Mn"})


def _ignorable() -> frozenset[int]:
    """Every code point Unicode marks `Default_Ignorable_Code_Point`.

    FROM A TABLE, NOT FROM A CATEGORY TEST, and that is the 2026-09-12 ruling
    rather than a preference. The first version of this strip enumerated
    categories -- `Cf` and `Mn` -- which is the author-imagined case; the
    sibling one step sideways is `U+3164 HANGUL FILLER`, category `Lo`,
    `isalnum()` TRUE, and invisible. It does not split a token, it WELDS INTO
    one, so no set of categories can reach it and no amount of adding
    categories would have found it. Unicode already names the set the rule
    wants: the code points that render as nothing by specification. Four of
    them are alphanumeric -- U+115F, U+1160, U+3164, U+FFA0.

    The stdlib does not expose the property, so it is harvested into
    `default-ignorable.tsv` by `derive-ignorable.py` beside this file.
    """
    try:
        text = TABLE.read_text(encoding="utf-8")
    except OSError as err:
        raise NoTable(f"cannot read {TABLE}: {err}") from err
    found: set[int] = set()
    for line in text.splitlines():
        body = line.split("#", 1)[0].strip()
        if not body:
            continue
        low, _, high = body.partition("\t")
        try:
            start, stop = int(low, 16), int(high or low, 16)
        except ValueError as err:
            raise NoTable(f"{TABLE}: {line!r} is not a hex range") from err
        found.update(range(start, stop + 1))
    if not found:
        # An empty table would disable the strip and print the same green as
        # a full one. A check of nothing is not a pass.
        raise NoTable(f"{TABLE} carries no ranges, so nothing would be stripped")
    return frozenset(found)


_IGNORABLE: frozenset[int] | None = None


def ignorable() -> frozenset[int]:
    """The table, read once."""
    global _IGNORABLE  # noqa: PLW0603 -- one file, read once, per process
    if _IGNORABLE is None:
        _IGNORABLE = _ignorable()
    return _IGNORABLE


# WHAT IS STRIPPED, AND WHY, in one place:
#
#   * `Default_Ignorable_Code_Point` -- renders as nothing by specification.
#     The zero-width space and its kin, the variation selectors, the tag
#     characters, and the four Hangul fillers that are alphanumeric.
#   * `Mn` -- combining marks. Visible, but not separators.
#
# One rule: A CHARACTER WITH NO VISUAL WIDTH IS NOT A SEPARATOR.
# `zzsub<U+200B>jectzz` and `zzsub<U+3164>jectzz` are the same literal to
# every human reader and to `git diff`, neither of which can see the
# character; a tokeniser that splits on the first and keeps the second welded
# hands back tokens that hash to nothing and calls the file clean.
#
# STILL OUT: visible confusables. A Cyrillic `\u043e` where a Latin `o`
# belongs is a different literal to the scanner and an identical one to the
# eye -- but the line this gate draws is INVISIBLE TO A READER, and a
# lookalike is not invisible. Anyone who can transliterate a hostname has
# commit access and can defeat any pattern; see check-hashes.py's docstring
# for the threat model. Ruled 2026-09-12, and declared here so the omission
# is never read as an oversight.
#
# Nothing wider is folded. NFKC compatibility folding would rewrite
# legitimate content -- ligatures, full-width forms, superscripts -- to guard
# against a case the threat model does not contain.
#
# TWO CONSEQUENCES, named rather than discovered later. Dropping `Mn` strips
# accents from decomposed text, so a decomposed `resume` with an acute on it
# tokenises as `resume` and would hit a row holding that word: a false
# positive bought deliberately, in a table whose rows are names and addresses
# rather than ordinary vocabulary. And this is the DIGEST half's tokeniser
# only -- the pattern half matches by shape and does not come through here, so
# a shaped literal broken by a zero-width space still evades it.


def tokens(text: str) -> list[str]:
    """`text` split into the tokens a digest row is compared against.

    SPLIT ON EVERY NON-ALPHANUMERIC, not on a class that keeps `.` and `-`.
    The obvious class keeps `owner.example.net` as one token, so a digest of
    the bare name never matches it -- measured, and the reason this function
    exists rather than a regex at each call site.

    EXCEPT the characters with no width, which are dropped instead of split
    on: see the note above `ignorable`.

    # Raises

    `NoTable` when the invisible-character table cannot be read. Callers turn
    that into exit 2 rather than letting a blind scan print `clean`.
    """
    invisible = ignorable()
    out: list[str] = []
    current: list[str] = []
    for char in text:
        if ord(char) in invisible or unicodedata.category(char) in COMBINING:
            # Not a token character and not a boundary either: it is not
            # there, as far as anything that reads the file is concerned.
            continue
        if char.isalnum():
            current.append(char)
        elif current:
            out.append("".join(current))
            current = []
    if current:
        out.append("".join(current))
    return out
