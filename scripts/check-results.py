#!/usr/bin/env python3
"""Lint discipline results directories.

A results directory is a claim with its evidence attached. This script is the
gate that keeps the two in agreement: it checks that the required files are
present, that the report's front-matter carries the required keys at the
required types, that the report's sections appear in the required order, and
-- the point of the whole exercise -- that what the report *states* is backed
by what the run *recorded*.

Whether ``run.jsonl`` IS a session record is not this script's question.
``diet check-record`` is the sole authority on that -- the regime trio, the
retry lineage, the recompute-sufficiency fields, the value space -- and this
script dispatches to it through ``scripts/resolve-diet.py``, prints which
binary answered and its SHA-256, and relays the verdict. It does not read the
file with ``json`` and reach its own. When diet refuses the record, every
check below that would have read the record is not reached: a linter that
judged the format itself would be a second reader, and two readers of one
file is how they come to disagree about it.

What stays here is what is not a format question. Three cross-checks:

  1. Every number in the front-matter, outside ``[regime]``, must be bound at
     the SAME path in the record's summary row, and must be equal to it.
     Matching by value alone was not enough: a report could state
     ``turns = 2048`` against a run that recorded two turns, merely because
     2048 appeared elsewhere in the record. If you state a number, the run has
     to have recorded that number under that name. The rows are read from the
     canonical rendering diet handed back, never from the file.
  2. Every key under ``[regime]`` -- not only the three required ones -- must
     be bound by ``regimen.toml`` and must equal it; and the required three
     must equal what the record's ``start`` row carries.
  3. ``product_sha256`` must equal the summary row's ``product_sha256``.
  4. Every ``consumes`` entry on every claim row -- and every ``comparison``
     row, which names its evidence the same way (#142) -- must name a file in the
     directory whose SHA-256 is the digest recorded beside it. The record
     carried those digests and nothing compared them to the bytes, so a claim
     could cite evidence it had never read -- provenance for the wrong
     artefact, which reads exactly like provenance for the right one.
     ``diet check-record`` stays shape-only: a format check is pure, and this
     linter is the reader that has the filesystem.

Stdlib only, by design: this runs in ``verify.sh`` and must not need an
install step to tell the truth. It needs a built ``diet``, and refuses --
exit 2, not a pass -- when the resolver cannot name one.

Usage:
    check-results.py --root results          # lint every run directory under a root
    check-results.py DIR [DIR ...]           # lint the named run directories
    check-results.py --root results --ledger PATH
                                             # and, if every directory passes, write the ledger

THE LEDGER (#32 I2) is what the results page is rendered from, and only from:
per directory that passed, its front-matter word and product digest, its
hypothesis, and each claim row as diet's canonical rendering gave it -- its
id, its word, the digests it consumed -- with the `decision-rule*.toml` among
those named as the rule. Written only when every linted directory passes: a
page drawn from a ledger with a failing row in it would be drawn from what
this gate refused. `exercise/scripts/render-ledger.py` draws it.

Exit code is 0 if every linted directory passes, 1 otherwise, and 2 when no
``diet`` binary could be resolved to ask. A sweep that finds no run
directories is an error, not a pass.
"""

from __future__ import annotations

from collections.abc import Callable

import argparse
import datetime
import decimal
import hashlib
import json
import pathlib
import re
import subprocess
import sys
import tomllib

FENCE = "+++"

REQUIRED_KEYS: dict[str, type | tuple[type, ...]] = {
    "hypothesis": str,
    "result": str,
    # Which of the two gate-0 kinds this directory is. Required here rather
    # than only in check-recompute.py so that a directory cannot be added
    # without declaring it: an undeclared directory is neither recomputed nor
    # knowingly skipped, which is how a gate comes to run over nothing.
    "kind": str,
    "regime": dict,
    "product_sha256": str,
    "controls_run": list,
    "known_defects": list,
}

REQUIRED_REGIME_KEYS: dict[str, type | tuple[type, ...]] = {
    "arm": str,
    "substrates": list,
    "dogma_version": int,
}

# `[derivation]`'s four keys, ruled 2026-09-19 on #84's second ruling: the
# arithmetic's environment is a typed block, not README prose. Optional at
# the top level (most directories derive from nothing); required whole once
# declared, the way `[regime]` is.
REQUIRED_DERIVATION_KEYS: dict[str, type | tuple[type, ...]] = {
    "applier_sha256": str,
    "runtime": str,
    "substrate_id": str,
    "derived_from": str,
}

SECTIONS = ["Observation", "Hypothesis", "Test", "Results", "Conclusion"]

# The two kinds a directory may declare. Spelled here as well as in
# check-recompute.py because this linter refuses a third spelling and that one
# decides what to run; a directory whose kind neither reader knows would
# otherwise be caught by neither.
KINDS = ("reproducible-by-config", "historical-observation")
REPRODUCIBLE = KINDS[0]

REQUIRED_FILES = ["run.jsonl", "regimen.toml", "README.md"]

# `fullmatch` throughout: `re.match` with a trailing `$` also accepts a final
# newline, so a 65-character product_sha256 would have passed the "exactly 64
# hex characters" check.
DIR_NAME = re.compile(r"(\d{4}-\d{2}-\d{2})-([a-z0-9]+(?:-[a-z0-9]+)*)")
SHA256 = re.compile(r"[0-9a-f]{64}")
HEADING = re.compile(r"^##\s+(.*?)\s*$")
# CommonMark: a fence is three or more backticks or tildes, indented at most
# three spaces. It is closed only by at least as many of the SAME character
# with nothing but whitespace after.
CODE_FENCE = re.compile(r"^ {0,3}(`{3,}|~{3,})")

HTML_COMMENT = re.compile(r"<!--.*?-->", re.DOTALL)

# The one directory under results/ that is an example rather than a run, and so
# the one exempt from the YYYY-MM-DD-<slug> rule.
TEMPLATE_DIR = "_template"


# FIGURES ARE REFERENCES, NOT DIGITS (#63, ruled 2026-10-02). Every directory
# declares `figures`: `referenced` -- its body's numbers are `{{...}}`
# references, resolved from the product, the front-matter and the record's
# summary row at check time, and a typed figure is refused -- or `typed`, not
# linted, its `known_defects` the disclosure. A directory dated after
# FIGURES_LANDED must say `referenced`. On or before it, a missing key reads as
# `typed` until the data seat's pass writes the key on every directory; that
# pass makes a missing key a refusal by deleting this allowance.
FIGURES = ("referenced", "typed")
FIGURES_LANDED = "2026-10-02"

# The sections that may carry no typed figure at all, and the ones that may
# carry one only inside an `[uncited: <reason>]` marker, which declares it.
FIGURES_NEVER_TYPED = ("Results", "Conclusion")
# One line, and a reason with a word in it: an unterminated marker must not
# reach across paragraphs to the next `]`, and `[uncited: 42]` declares
# nothing (#265's review).
#
# No `<` or `>` in it (#265's third review): `<!--[uncited: x-->0.241<!--]-->`
# was a marker to this reader and, its brackets stripped with the comments, a
# bare figure to every other.
UNCITED = re.compile(r"\[uncited:[^\]\n<>]*[A-Za-z][^\]\n<>]*\]")
REFERENCE = re.compile(r"\{\{(.*?)\}\}", re.DOTALL)
# What a typed figure is: an ISO date, as ONE token (#63's measurement: a
# date tokenised as three numbers made every Conclusion unmigratable), or a
# number standing on its own -- not a digit inside a word (`v2`, `sha256`)
# and not an issue or ordinal reference (`#63`).
#
# A dot before the digits exempts them only after a word or a number -- the
# `2` of `v1.2` -- never a leading-dot decimal: `.241` and `p < .05` are
# figures (#265's third review).
#
# A `#` before the digits exempts them only when they are a bare integer,
# an issue's number (`#63`); `#0.241` and `#14.2%` are figures (#265's fifth
# review). See `typed_figures`.
TYPED_FIGURE = re.compile(
    r"(?<![A-Za-z0-9])(?<![A-Za-z0-9]\.)(?:\d{4}-\d{2}-\d{2}|\d+(?:[.,]\d+)*%?)(?![A-Za-z0-9])"
)


def typed_figures(text: str) -> list[str]:
    """The typed figures in `text`, an issue number (`#` then a bare
    integer) excepted."""
    return [
        m.group(0)
        for m in TYPED_FIGURE.finditer(text)
        if not (m.start() > 0 and text[m.start() - 1] == "#" and m.group(0).isdigit())
    ]
# HEADINGS ARE A WHITELIST (#265's third and fourth reviews). Two reviews
# found headings a renderer shows that this linter did not read -- indented,
# setext, `<h2>`, in a blockquote, in a list item, `## Results ##` -- each
# putting text a reader sees under "Results" in a laxer section. So in a
# referenced body a heading is one of exactly two lines: `## <section>` at
# column 0, the section one of the five, or the first `# ` line before them
# (the title). Any other line that renders as a heading -- after stripping the
# blockquote and list prefixes a heading can sit behind -- is refused.
SECTION_LINE = re.compile(r"## (" + "|".join(SECTIONS) + r")")
# A tab after `>` or a list marker is a container's separator too (#265's
# fifth review: `>` TAB `## Results` rendered as a heading), and a leading tab
# is an indent like spaces (its sixth: a tab-indented `## Results` in a list
# item, and `>` space TAB `## Results`, rendered as headings).
CONTAINER_PREFIX = re.compile(r"^(?: {0,3}>[ \t]?| {0,3}(?:[-*+]|\d{1,9}[.)])(?:[ \t]|$)| {1,3}|\t)")
ATX = re.compile(r"#{1,6}(?:[ \t]|$)")
SETEXT_UNDERLINE = re.compile(r"(?:=+|-+)[ \t]*$")
HTML_HEADING = re.compile(r"<h[1-6](?:[\s>/]|$)", re.IGNORECASE)
# A backslash before ASCII punctuation, which CommonMark renders as the
# character alone: `\{\{` shows `{{` (#265's fourth review).
BACKSLASH_ESCAPE = re.compile(r"\\([!-/:-@\[-`{-~])")


def visible(text: str) -> str:
    """Text as a reader sees it, line for line: backslash escapes dropped,
    HTML entities decoded, format characters (zero-width and the like)
    removed. What the figure lint and the brace check read (#265's fourth
    review: backslash-escaped braces, `&#123;&#123;` and a zero-width space
    between braces all showed `{{`)."""
    import html
    import unicodedata

    text = html.unescape(BACKSLASH_ESCAPE.sub(r"\1", text))
    return "".join(ch for ch in text if unicodedata.category(ch) != "Cf")


def code_spans(line: str) -> list[tuple[int, int]]:
    """The stretches of `line` inside a backtick code span: from a run of
    backticks to the next run of the same length, or to the line's end when
    none closes it -- fail closed, since a span may continue onto the next
    line."""
    runs = [(m.start(), m.end()) for m in re.finditer(r"`+", line)]
    spans: list[tuple[int, int]] = []
    k = 0
    while k < len(runs):
        start, end = runs[k]
        close = next((j for j in range(k + 1, len(runs)) if runs[j][1] - runs[j][0] == end - start), None)
        if close is None:
            spans.append((end, len(line)))
            break
        spans.append((end, runs[close][0]))
        k = close + 1
    return spans


def misread_structure(line: str) -> str | None:
    """Why `line` would make this linter's reading of comments and fences
    differ from a renderer's, or None (#265's sixth review). `rendered_headings`
    blanks comments before it looks for code, so a `<!--` inside a code span
    opened a comment no renderer shows, hiding a heading the reader sees; and
    a backtick fence whose info string holds a backtick is no fence to a
    renderer, which then shows the headings this linter read as code."""
    for marker in re.finditer(r"<!--|-->", line):
        if any(start <= marker.start() < end for start, end in code_spans(line)):
            return f"a comment marker `{marker.group(0)}` inside a code span"
    opener = CODE_FENCE.match(line)
    if opener and opener.group(1)[0] == "`" and "`" in line[opener.end():]:
        return "a backtick fence whose info string holds a backtick, which renders as no fence"
    return None


def unread_heading(lines: list[str], at: int, first_section: int, title_at: int | None) -> bool:
    """Whether line `at` renders as a heading this linter does not read."""
    line = lines[at]
    if SECTION_LINE.fullmatch(line) or at == title_at:
        return False
    if HTML_HEADING.search(line):
        return True
    rest = line
    while True:
        stripped = CONTAINER_PREFIX.sub("", rest, count=1)
        if stripped == rest:
            break
        rest = stripped
    if ATX.match(rest):
        return True
    previous = lines[at - 1] if at > 0 else ""
    return bool(SETEXT_UNDERLINE.match(rest.strip()) and rest.strip() and previous.strip()
                and not CONTAINER_PREFIX.sub("", previous).strip() == "")
# `ns.key.key[0]`, or one of three functions over one: the whole grammar.
REF_PATH = re.compile(r"(product|front|summary)((?:\.[A-Za-z_][A-Za-z0-9_-]*|\[\d+\])+)")
REF_CALL = re.compile(r"(count|round|pct)\(\s*([^,()]+?)\s*(?:,\s*(\d+)\s*)?\)")
# The front-matter digests a reference may name: each is checked against a
# file here, so a digest a section needs is written as one of these
# (#63, ruled 2026-10-02).
FRONT_DIGESTS = (("product_sha256",), ("pre_registration_sha256",))
REF_STEP = re.compile(r"\.([A-Za-z_][A-Za-z0-9_-]*)|\[(\d+)\]")


class Written(str):
    """A JSON number as it was written, so a rendered figure is the product's
    own digits and not a float's re-spelling of them."""


class Unreadable(Exception):
    """A file that cannot be decoded. Reported, never raised out of a lint."""


REPO = pathlib.Path(__file__).resolve().parent.parent

# The binary every record verdict comes from, resolved once per run and named
# in the output, so that a verdict can be traced to the build that gave it.
DIET: pathlib.Path | None = None


def resolve_diet() -> tuple[pathlib.Path, str] | None:
    """Ask the resolver which `diet` to run, or None if it refuses.

    Through the resolver and never a path composed here: a hand-built path is
    how a gate ends up running a binary cargo did not produce. The resolver
    refuses to guess -- two builds, a stale build, no build -- and each of
    those is a reason this script cannot answer either.
    """
    result = subprocess.run(
        [sys.executable, str(REPO / "scripts" / "resolve-diet.py")],
        cwd=REPO,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        sys.stderr.write(result.stderr)
        return None
    resolved = json.loads(result.stdout)
    return REPO / resolved["path"], resolved["sha256"]


class Verdict:
    """What `diet check-record` said about one file."""

    def __init__(self, ok: bool, value: dict | None, error: str | None) -> None:
        self.ok = ok
        self.value = value
        self.error = error


def record_verdict(path: pathlib.Path) -> Verdict:
    """Run `diet check-record` over `path` and relay its envelope.

    Exit 0 and 1 are verdicts. Anything else -- a usage error, a binary that
    would not run, stdout that is not the envelope -- is not a verdict, and is
    raised rather than read as one.
    """
    assert DIET is not None
    result = subprocess.run(
        [str(DIET), "check-record", str(path)],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode not in (0, 1):
        raise Unreadable(
            f"diet check-record exited {result.returncode} on {path.name}: "
            f"{result.stderr.strip()}"
        )
    try:
        envelope = json.loads(result.stdout)
    except json.JSONDecodeError as err:
        raise Unreadable(f"diet check-record did not answer with JSON: {err}") from err
    if envelope.get("ok") is True:
        return Verdict(True, envelope.get("value"), None)
    error = str(envelope.get("error", "refused without a reason"))
    # diet says "not a session record: <why>"; this script says the first
    # half itself when it relays, so keep only the why.
    return Verdict(False, None, error.removeprefix("not a session record: "))


def read_text(path: pathlib.Path) -> str:
    """Read a file as UTF-8, turning a decode error into a lint failure.

    An uncaught UnicodeDecodeError would abort a whole ``--root`` sweep on one
    bad file, leaving every later directory unlinted and looking clean.
    """
    try:
        return path.read_text(encoding="utf-8")
    except (UnicodeDecodeError, OSError) as err:
        raise Unreadable(f"{path.name} is not readable as UTF-8: {err}") from err


def type_name(expected: type | tuple[type, ...]) -> str:
    if isinstance(expected, tuple):
        return " or ".join(t.__name__ for t in expected)
    return expected.__name__


def is_number(value: object) -> bool:
    """True for a TOML integer or float. ``bool`` is not a number here."""
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def same_value(left: object, right: object) -> bool:
    """Equality that does not conflate a TOML boolean with an integer.

    Python says ``True == 1``. TOML does not, and a regime binding
    ``dogma_version = true`` is not a regime binding ``dogma_version = 1``.
    """
    if isinstance(left, bool) != isinstance(right, bool):
        return False
    return left == right


def has_type(value: object, expected: type | tuple[type, ...]) -> bool:
    """``isinstance`` with ``bool`` excluded from ``int``.

    TOML distinguishes ``true`` from ``1``; Python's type system does not, and
    a lint that accepts ``dogma_version = true`` is not a lint.
    """
    if expected is int and isinstance(value, bool):
        return False
    return isinstance(value, expected)


def walk_numbers(
    value: object, path: tuple[object, ...] = ()
) -> list[tuple[tuple[object, ...], float]]:
    """Every number in a nested structure, with its path as a tuple of steps.

    A tuple, not a dotted string: a top-level key literally named
    ``regime.something`` would otherwise be indistinguishable from the
    ``regime`` table's ``something`` field, and so would inherit that table's
    exemption and be checked by nothing at all.
    """
    found: list[tuple[tuple[object, ...], float]] = []
    if isinstance(value, dict):
        for key, child in value.items():
            found += walk_numbers(child, (*path, key))
    elif isinstance(value, list):
        for index, child in enumerate(value):
            found += walk_numbers(child, (*path, index))
    elif is_number(value):
        found.append((path, value))
    return found


def show_path(path: tuple[object, ...]) -> str:
    """A path tuple rendered for a human."""
    out = ""
    for step in path:
        out += f"[{step}]" if isinstance(step, int) else (f".{step}" if out else str(step))
    return out


def resolve(value: object, path: tuple[object, ...]) -> tuple[bool, object]:
    """Follow a path tuple into a nested structure. Returns (found, value)."""
    current = value
    for step in path:
        if isinstance(step, int):
            if not isinstance(current, list) or step >= len(current):
                return False, None
            current = current[step]
        else:
            if not isinstance(current, dict) or step not in current:
                return False, None
            current = current[step]
    return True, current


def split_front_matter(text: str) -> tuple[str | None, str, str | None, str | None]:
    """Split a report into (front-matter source, body, error, failure class).

    Exactly one of the front-matter source or the error is not ``None``. The
    two ways the fence can be wrong are different failures and are declared
    separately in the manifest, so the split says which one it met rather than
    leaving the caller to re-derive it from the message text.
    """
    lines = text.split("\n")
    if not lines or lines[0].strip() != FENCE:
        return None, "", f"does not open with a {FENCE!r} front-matter fence", \
            "results.front-matter-absent"
    for index in range(1, len(lines)):
        if lines[index].strip() == FENCE:
            return "\n".join(lines[1:index]), "\n".join(lines[index + 1 :]), None, None
    return None, "", f"front-matter fence {FENCE!r} is never closed", \
        "results.front-matter-unterminated"


def body_sections(body: str) -> list[str]:
    """Level-2 headings in document order, ignoring fenced code blocks."""
    return list(rendered_headings(body).values())


def rendered_headings(body: str) -> dict[int, str]:
    """The level-2 headings a renderer shows, by line index: none inside a
    fenced code block or an HTML comment."""
    # A heading inside an HTML comment is not a heading: it renders as nothing.
    # The comment is blanked to its newlines, so line indices still count.
    body = HTML_COMMENT.sub(lambda m: "\n" * m.group(0).count("\n"), body)
    headings: dict[int, str] = {}
    fence: str | None = None
    for index, line in enumerate(body.split("\n")):
        opener = CODE_FENCE.match(line)
        if fence is None:
            if opener:
                fence = opener.group(1)
            else:
                match = HEADING.match(line)
                if match:
                    headings[index] = match.group(1)
            continue
        # Inside a fence: only a run of the same character, at least as long as
        # the opener, with nothing but whitespace after it, closes the block.
        if opener and opener.group(1)[0] == fence[0] and len(opener.group(1)) >= len(fence):
            if line.strip().strip(fence[0]) == "":
                fence = None
    return headings


def check_run(directory: pathlib.Path) -> list[str]:
    """Lint one run directory. Returns a list of failure messages."""
    failures: list[str] = []

    def fail(failure_class: str, message: str) -> None:
        # The class is EMITTED, not merely declared. The manifest names a
        # `failure_class` for every one of these fixtures, and for as long as
        # nothing printed it the selftest could only grade on "exited 1" --
        # so a fixture going red for a reason nobody checked graded the same
        # as one going red for its own. Red for the wrong reason is the WRONG
        # verdict wearing the right exit code.
        failures.append(f"{directory}: {message}  [{failure_class}]")

    name = directory.name
    if name != TEMPLATE_DIR:
        match = DIR_NAME.fullmatch(name)
        if not match:
            fail("results.name-malformed", "name is not `YYYY-MM-DD-<slug>` with a lowercase hyphenated slug")
        else:
            try:
                datetime.date.fromisoformat(match.group(1))
            except ValueError:
                fail("results.name-malformed", f"name carries an impossible date `{match.group(1)}`")

    missing = [f for f in REQUIRED_FILES if not (directory / f).is_file()]
    for f in missing:
        fail("results.required-file-missing", f"missing required file `{f}`")
    if missing:
        return failures

    # --- run.jsonl: diet's verdict, relayed ------------------------------
    verdict = record_verdict(directory / "run.jsonl")
    summary: dict | None = None
    written_summary: dict | None = None
    recorded_regime: dict | None = None
    claims: list[dict] = []
    recorded_source: dict | None = None
    if not verdict.ok:
        fail(
            "results.record-refused",
            f"run.jsonl is not a session record, says diet check-record: {verdict.error}",
        )
        # Nothing below reads the record. The number and digest checks need a
        # summary row, and a summary row is only known to exist once diet has
        # said the file is a record; reaching for one here would be this
        # script judging the format after all.
    else:
        value = verdict.value or {}
        rows = [json.loads(line) for line in value.get("canonical", "").splitlines() if line.strip()]
        summaries = [r for r in rows if r.get("record") == "summary"]
        if len(summaries) != 1:
            fail("results.summary-record-count", f"run.jsonl holds {len(summaries)} summary rows, expected exactly 1")
        else:
            summary = summaries[0]
            written_summary = next(
                written for line in value.get("canonical", "").splitlines()
                if line.strip() and (written := as_written(line)).get("record") == "summary"
            )
        recorded_regime = value.get("regime")
        claims = [r for r in rows if r.get("record") == "claim"]
        recorded_source = value.get("source")
        check_consumed(directory, rows, fail)

    # --- regimen.toml ----------------------------------------------------
    regimen: dict | None = None
    try:
        # read_bytes, not read_text: universal-newline translation would turn a
        # bare CR into a newline, so this reader would accept a file the other
        # reader of the same bytes -- the regimen grammar -- must reject. The
        # two must never disagree about the same file.
        regimen = tomllib.loads((directory / "regimen.toml").read_bytes().decode("utf-8"))
    # tomllib raises bare ValueError for an integer too large to convert, and
    # TOMLDecodeError is itself a ValueError, so one clause covers both.
    except (ValueError, UnicodeDecodeError, OSError) as err:
        fail("results.regimen-not-toml", f"regimen.toml is not TOML: {err}")

    # --- README.md front-matter -----------------------------------------
    try:
        text = read_text(directory / "README.md")
    except Unreadable as err:
        fail("results.unreadable-encoding", str(err))
        return failures
    source, body, err, err_class = split_front_matter(text)
    if err is not None or source is None:
        fail(err_class or "results.front-matter-absent", f"README.md {err}")
        return failures
    try:
        front = tomllib.loads(source)
    except ValueError as exc:
        fail("results.front-matter-not-toml", f"README.md front-matter is not TOML: {exc}")
        return failures

    for key, expected in REQUIRED_KEYS.items():
        if key not in front:
            fail("results.required-key-missing", f"front-matter is missing required key `{key}`")
        elif not has_type(front[key], expected):
            fail(
                "results.key-mistyped",
                f"front-matter key `{key}` is {type(front[key]).__name__}, "
                f"expected {type_name(expected)}"
            )

    for key in ("controls_run", "known_defects"):
        value = front.get(key)
        if isinstance(value, list) and not all(isinstance(item, str) for item in value):
            fail("results.key-mistyped", f"front-matter key `{key}` must be a list of strings")

    regime = front.get("regime")
    if isinstance(regime, dict):
        for key, expected in REQUIRED_REGIME_KEYS.items():
            if key not in regime:
                fail("results.required-key-missing", f"front-matter is missing required key `regime.{key}`")
            elif not has_type(regime[key], expected):
                fail(
                    "results.key-mistyped",
                    f"front-matter key `regime.{key}` is {type(regime[key]).__name__}, "
                    f"expected {type_name(expected)}"
                )
        if recorded_regime is not None:
            # The report's trio against the record's own start row. The regimen
            # says what was intended; the record says what ran.
            for key in REQUIRED_REGIME_KEYS:
                if key in regime and key in recorded_regime and not same_value(
                    recorded_regime[key], regime[key]
                ):
                    fail(
                        "results.regime-disagrees-with-regimen",
                        f"front-matter `regime.{key}` is {regime[key]!r} but the "
                        f"record's start row carries {recorded_regime[key]!r}"
                    )
        if regimen is not None:
            # Every key under [regime], not only the required three: a regime
            # field the regimen does not bind is a claim about the run that
            # nothing backs.
            for key in regime:
                if key not in regimen:
                    fail("results.regime-unbacked", f"regimen.toml does not bind `{key}`, so `regime.{key}` is unbacked")
                elif not same_value(regimen[key], regime[key]):
                    fail(
                        "results.regime-disagrees-with-regimen",
                        f"front-matter `regime.{key}` is {regime[key]!r} but "
                        f"regimen.toml binds {regimen[key]!r}"
                    )

    kind = front.get("kind")
    if isinstance(kind, str) and kind not in KINDS:
        fail("results.kind-undeclared", f"front-matter `kind` is {kind!r}, which is neither {' nor '.join(KINDS)}")

    # A hosted substrate is one whose weights can change under you: the
    # provider re-points a tag and the same config serves different weights.
    # Gate 0 does not care -- re-deriving numbers from committed artefacts is
    # indifferent to what produced them -- but a directory declaring
    # `reproducible-by-config` is promising a RE-FIRING, and that is the
    # promise nobody can keep here. Such a run is a real result and its kind is
    # `historical-observation`. Ruled 2026-09-08.
    #
    # The list comes from diet, which is the only reader of the record; this
    # script asking `canonical` which weights each substrate carries would be
    # a second opinion about the format.
    if kind == REPRODUCIBLE and recorded_regime is not None:
        hosted = recorded_regime.get("hosted_substrates") or []
        if hosted:
            fail(
                "results.hosted-cannot-be-reproducible",
                f"front-matter `kind` is {REPRODUCIBLE!r} but the record is "
                f"served by hosted weights ({', '.join(sorted(hosted))}), which "
                f"can change under a re-firing; this is {KINDS[1]!r}",
            )

    # THE FRONT-MATTER'S VERDICT AND THE RECORD'S ARE ONE STATEMENT WRITTEN
    # TWICE. Ruled 2026-09-14, after `diet bakeoff` wrote `unadjudicated` in
    # the README and `inconclusive` on the claim row of the same directory
    # for a day, with nothing in this script comparing them: `result` is a
    # free string in the front-matter schema and `Verdict` is a closed
    # vocabulary in the record, so neither reader could see the other's copy.
    #
    # They are not two measurements taken differently. `inconclusive` is a
    # verdict -- the evidence was held against a rule and did not decide --
    # and `unadjudicated` is the absence of one. A directory that says both
    # is a directory whose reader has to guess which half to believe.
    #
    # A directory with no claim row is not this check's business: the record
    # format decides whether one is required, and a results directory with no
    # claim is already refused upstream by the rule that a claim must name the
    # artefacts it consumed. More than one claim row and every one must agree
    # with the front-matter, because the front-matter has one `result` field
    # and cannot mean different things to different rows.
    stated = front.get("result")
    if isinstance(stated, str):
        for claim in claims:
            recorded = claim.get("result")
            if recorded is not None and recorded != stated:
                fail(
                    "results.verdict-disagrees-with-record",
                    f"front-matter `result` is {stated!r} but claim "
                    f"{claim.get('id')!r} carries {recorded!r}; the report and the "
                    f"record are one statement written twice and must agree"
                )

    # THE PRE-REGISTRATION IS PINNED BY THE DIGEST OF A FILE, not by a hash of
    # a block inside `report.json`. Ruled 2026-09-14.
    #
    # The block version would have made this script a SECOND canonicaliser of
    # record data: to hash a sub-object it would have to serialise it, and
    # serialising means deciding key order, separators and -- since the
    # pre-registration carries the attainable p floor and the budget ladder --
    # decimal formatting. A second canonicaliser that disagrees with `diet` by
    # one digit reports a mismatch that looks exactly like tampering. This
    # script hashes whole files and delegates every structural question, and
    # that is the property being kept.
    #
    # BICONDITIONAL, so neither half can be dropped to escape it: a directory
    # with the file must declare the digest, and a directory declaring the
    # digest must have the file. An optional pin is a pin nobody has to carry.
    #
    # Whether a results directory must HAVE a pre-registration is a different
    # question and is not decided here -- a recompute-confirmed row that never
    # pre-registered anything is a real shape, and this check is about the two
    # halves agreeing, not about requiring one.
    declared_pre = front.get("pre_registration_sha256")
    pre_file = directory / "pre-registration.json"
    if declared_pre is not None and not isinstance(declared_pre, str):
        fail("results.key-mistyped", "front-matter `pre_registration_sha256` is not a string")
    elif isinstance(declared_pre, str) and not SHA256.fullmatch(declared_pre):
        fail(
            "results.sha-malformed",
            "front-matter `pre_registration_sha256` is not 64 lowercase hex characters",
        )
    elif isinstance(declared_pre, str) and not pre_file.is_file():
        fail(
            "results.pre-registration-absent",
            "front-matter declares `pre_registration_sha256` and there is no "
            "`pre-registration.json` here; a digest of a file that is not in the "
            "directory pins nothing",
        )
    elif isinstance(declared_pre, str):
        found_pre = digest_of(pre_file)
        if found_pre != declared_pre:
            fail(
                "results.pre-registration-digest",
                f"`pre-registration.json` hashes to {found_pre} and the front-matter "
                f"declares {declared_pre}; the endpoints a run was scored against are "
                f"not the endpoints committed beside it",
            )
    elif pre_file.is_file():
        fail(
            "results.pre-registration-unpinned",
            "`pre-registration.json` is here and the front-matter declares no "
            "`pre_registration_sha256`; endpoints nothing pins can be edited after "
            "the numbers, which is what pre-registering them is against",
        )

    # A record ADAPTED from a log nobody else can read cannot promise a
    # re-firing either, and for a nearer reason than hosted weights: the input
    # itself is gone. `pinned_only` says the digest names a file that is not in
    # this repository and cannot be -- an operator's own session transcript,
    # unscrubbed. Pinning WHICH file was read is not the same as making it
    # readable, and `reproducible-by-config` promises the second. `committed`
    # is fine: the source is right there beside the record.
    #
    # Read from diet's projection rather than from `canonical`, for the reason
    # the hosted check above gives: this script must not become a second
    # opinion about the record format.
    if (
        kind == REPRODUCIBLE
        and recorded_source is not None
        and recorded_source.get("source_available") == "pinned_only"
    ):
        fail(
            "results.pinned-only-cannot-be-reproducible",
            f"front-matter `kind` is {REPRODUCIBLE!r} but the record is adapted "
            f"from a source pinned by digest and not reachable "
            f"({recorded_source.get('adapter')!r}), so nothing can re-fire it; "
            f"this is {KINDS[1]!r}",
        )

    # THE ARITHMETIC'S ENVIRONMENT IS A TYPED BLOCK, NOT README PROSE. Ruled
    # 2026-09-19 on #84's second ruling, after a derived directory's README
    # stated "the arithmetic ran under Python 3.14.6 ... the registry's
    # mac-pro-2019 instance" in prose that nothing here checked -- the
    # prose-claims class #77 names: true when written, checked by nothing.
    #
    # OPTIONAL, the way `pre_registration_sha256` is: most directories derive
    # from nothing, and this check is about a declared derivation being real,
    # not about requiring every directory to be one. Required whole once
    # declared, the way `[regime]` is required whole.
    #
    # Two of its four keys are digests and are verified as such, the same way
    # `pre_registration_sha256` is: `applier_sha256` against `recompute.sh`
    # itself -- "the applier is the Python inside recompute.sh; there is no
    # other copy" -- and `derived_from` against the claim row's own
    # `consumes`, so citing an original by digest is a checked fact and not
    # an assertion nobody reads back. `runtime` and `substrate_id` are typed
    # and required but not independently verifiable from this directory
    # alone -- there is no second reading of which interpreter or which
    # machine ran, only the one this directory declares.
    derivation = front.get("derivation")
    if derivation is not None and not isinstance(derivation, dict):
        fail("results.key-mistyped", "front-matter `derivation` is not a table")
    elif isinstance(derivation, dict):
        for key, expected in REQUIRED_DERIVATION_KEYS.items():
            if key not in derivation:
                fail(
                    "results.required-key-missing",
                    f"front-matter `derivation` is missing required key `{key}`",
                )
            elif not has_type(derivation[key], expected):
                fail(
                    "results.key-mistyped",
                    f"front-matter `derivation.{key}` is {type(derivation[key]).__name__}, "
                    f"expected {type_name(expected)}",
                )

        applier_sha256 = derivation.get("applier_sha256")
        applier_file = directory / "recompute.sh"
        if isinstance(applier_sha256, str):
            if not SHA256.fullmatch(applier_sha256):
                fail(
                    "results.sha-malformed",
                    "front-matter `derivation.applier_sha256` is not 64 lowercase hex characters",
                )
            elif not applier_file.is_file():
                fail(
                    "results.derivation-applier-absent",
                    "front-matter declares `derivation.applier_sha256` and there is no "
                    "`recompute.sh` here to be the applier",
                )
            else:
                found_applier = digest_of(applier_file)
                if found_applier != applier_sha256:
                    fail(
                        "results.derivation-applier-digest",
                        f"`recompute.sh` hashes to {found_applier} and the front-matter "
                        f"declares {applier_sha256}; the applier that ran is not the one "
                        f"committed beside it",
                    )

        derived_from = derivation.get("derived_from")
        if isinstance(derived_from, str):
            if not SHA256.fullmatch(derived_from):
                fail(
                    "results.sha-malformed",
                    "front-matter `derivation.derived_from` is not 64 lowercase hex characters",
                )
            elif not any(
                isinstance(artifact, dict) and artifact.get("sha256") == derived_from
                for claim in claims
                for artifact in claim.get("consumes", [])
            ):
                fail(
                    "results.derivation-unconsumed",
                    f"front-matter `derivation.derived_from` is {derived_from!r}, which no "
                    f"claim row's `consumes` names; citing an original by digest means "
                    f"consuming it",
                )

    sha = front.get("product_sha256")
    if isinstance(sha, str):
        if not SHA256.fullmatch(sha):
            fail("results.sha-malformed", "front-matter `product_sha256` is not 64 lowercase hex characters")
        elif summary is not None:
            recorded = summary.get("product_sha256")
            if recorded is None:
                fail("results.sha-disagrees-with-summary", "the record's summary row does not carry `product_sha256`")
            elif recorded != sha:
                fail(
                    "results.sha-disagrees-with-summary",
                    f"front-matter `product_sha256` is {sha} but the summary "
                    f"record carries {recorded!r}"
                )

    # --- prose against data ----------------------------------------------
    if summary is not None:
        for path, value in walk_numbers(front):
            if path and path[0] == "regime":
                continue  # checked against regimen.toml above
            shown = show_path(path)
            found, at_path = resolve(summary, path)
            if not found:
                fail(
                    "results.number-unbound-in-summary",
                    f"front-matter `{shown}` states {value!r}, but the summary "
                    f"record binds no `{shown}`"
                )
            elif not (is_number(at_path) and same_value(at_path, value)):
                fail(
                    "results.number-contradicts-summary",
                    f"front-matter `{shown}` states {value!r} but the summary "
                    f"record binds `{shown}` to {at_path!r}"
                )

    # --- sections ---------------------------------------------------------
    headings = body_sections(body)
    if headings != SECTIONS:
        fail(
            "results.sections-wrong",
            f"README.md sections are {headings!r}, expected exactly {SECTIONS!r} in order"
        )

    # --- figures (#63) ---------------------------------------------------
    figures = figures_declared(name, front, fail)
    rendered = None
    if figures == "referenced":
        rendered = lint_figures(directory, front, body, written_summary, fail)
    if RENDERED is not None and rendered is not None:
        RENDERED[directory.resolve()] = rendered
    if FIGURES_SEEN is not None and figures in FIGURES and name != TEMPLATE_DIR:
        FIGURES_SEEN[figures] += 1

    if LEDGER is not None and not failures and name != TEMPLATE_DIR:
        row = ledger_row(directory, front, claims)
        row["figures"] = figures
        if rendered is not None:
            row["report"] = rendered
        LEDGER.append(row)
    return failures


def as_written(text: str) -> dict:
    """JSON with every number kept as the digits it was written in."""
    return json.loads(text, parse_float=Written, parse_int=Written)


def figures_declared(name: str, front: dict, fail: Callable[[str, str], None]) -> str | None:
    """The directory's `figures` word, or None when it declares none it may."""
    dated = DIR_NAME.fullmatch(name)
    after = bool(dated) and dated.group(1) > FIGURES_LANDED
    word = front.get("figures")
    if word is None:
        if after:
            fail(
                "results.figures-undeclared",
                f"front-matter declares no `figures`, and a directory dated after "
                f"{FIGURES_LANDED} must declare `figures = \"referenced\"` (#63)",
            )
            return None
        return "typed"
    if word not in FIGURES:
        fail("results.figures-undeclared", f"front-matter `figures` is {word!r}, which is neither {' nor '.join(FIGURES)}")
        return None
    if word == "typed" and after:
        fail(
            "results.figures-undeclared",
            f"front-matter `figures` is \"typed\", and a directory dated after "
            f"{FIGURES_LANDED} must be `referenced` (#63)",
        )
        return None
    return word


# What precedes the first level-2 heading -- the title and any introduction
# -- is linted as a section of its own, like Observation (#265's review: six
# of the fifteen directories carry figures there).
PREAMBLE = "preamble"


def sections_of(body: str) -> dict[str, str]:
    """Each level-2 section's text, and the preamble before the first, read
    as written: nothing in the body is exempt (#63, ruled 2026-10-02).

    Where a section ends is read twice -- every `## ` line a heading, and only
    the headings a renderer shows (`rendered_headings`) -- and each line is
    linted under the stricter of the two. A `## Notes` inside a fenced block
    in Conclusion therefore cannot carry what follows out of Conclusion, and a
    misread fence cannot carry Conclusion's text into a laxer section."""
    shown = rendered_headings(body)
    found: dict[str, list[str]] = {PREAMBLE: []}
    blind = aware = PREAMBLE
    for index, line in enumerate(body.split("\n")):
        heading = HEADING.match(line)
        if heading:
            blind = heading.group(1)
        if index in shown:
            aware = shown[index]
        # The heading line is linted too: under the other reading it may be
        # text a renderer shows inside the section it seems to open.
        current = aware if aware in FIGURES_NEVER_TYPED else blind
        found.setdefault(current, []).append(line)
    return {name: "\n".join(lines) for name, lines in found.items()}


def the_product(directory: pathlib.Path, front: dict) -> tuple[dict | None, str | None]:
    """The one file here whose SHA-256 is `product_sha256`, read as JSON with
    its numbers as written -- or why there is none."""
    sha = front.get("product_sha256")
    if not isinstance(sha, str):
        return None, "front-matter carries no `product_sha256`"
    matches = sorted(p for p in directory.iterdir() if p.is_file() and digest_of(p) == sha)
    if len(matches) != 1:
        return None, (
            f"{len(matches)} file(s) here hash to `product_sha256` {sha}; a reference "
            f"resolves against exactly one product"
        )
    try:
        product = as_written(matches[0].read_text(encoding="utf-8"))
    except (ValueError, OSError) as err:
        return None, f"the product, `{matches[0].name}`, is not JSON: {err}"
    return (product, None) if isinstance(product, dict) else (None, f"the product, `{matches[0].name}`, is not a JSON object")


def resolve_reference(text: str, scopes: dict[str, object]) -> tuple[str | None, str | None]:
    """One `{{...}}` reference rendered, or why it cannot be."""
    text = text.strip()
    call = REF_CALL.fullmatch(text)
    target, function, places = (call.group(2), call.group(1), call.group(3)) if call else (text, None, None)
    path = REF_PATH.fullmatch(target)
    if not path:
        return None, f"`{{{{{text}}}}}` is not a reference: want product., front. or summary. and a path, or count(), round() or pct() of one"
    scope = scopes.get(path.group(1))
    if isinstance(scope, str):
        return None, f"`{{{{{text}}}}}`: {scope}"
    steps: tuple[object, ...] = tuple(
        int(index) if index else key for key, index in REF_STEP.findall(path.group(2))
    )
    found, value = resolve(scope, steps)
    if not found:
        return None, f"`{{{{{text}}}}}` names `{target}`, which {path.group(1)} does not carry"
    # `front` is a date, or one of the three `[regime]` keys checked against
    # the record's start row -- nothing else, counted or not (#265's reviews:
    # a front string, and then any other `[regime]` key, which only the
    # author's own regimen.toml backs, carried a figure past the lint).
    # A date only: `datetime.datetime` is a `datetime.date` too, and its time
    # part is digits an author chose (#265's third review).
    if path.group(1) == "front" and type(value) is not datetime.date \
            and not (len(steps) >= 2 and steps[0] == "regime" and steps[1] in REQUIRED_REGIME_KEYS) \
            and steps not in FRONT_DIGESTS:
        return None, (
            f"`{{{{{text}}}}}` names `{target}`; a front-matter reference is a date, "
            f"`regime.arm`, `regime.substrates` or `regime.dogma_version`, or a digest this "
            f"linter checks (`product_sha256`, `pre_registration_sha256`) -- the values "
            f"something backs; reference the summary row or the product"
        )
    if function == "count":
        if isinstance(value, (list, dict)):
            return str(len(value)), None
        return None, f"`{{{{{text}}}}}` counts `{target}`, which is not a list or a table"
    # `front` is a date or a `[regime]` value, nothing else (#265's review): a
    # front-matter string is checked against nothing, so a figure in one is a
    # typed figure in a costume, and a front-matter number re-spells through
    # TOML -- `summary` holds the same number as written.
    if path.group(1) == "front" and type(value) is datetime.date:
        return value.isoformat(), None
    if isinstance(value, bool):
        shown = "true" if value else "false"
    elif isinstance(value, (Written, str, int, float, datetime.date)):
        shown = str(value)
    else:
        return None, f"`{{{{{text}}}}}` names `{target}`, which is not one value"
    if function in ("round", "pct"):
        if not isinstance(value, Written) or int(places or 0) > 12:
            return None, f"`{{{{{text}}}}}` rounds `{target}`, which is not a number the product or summary wrote, or asks for more than 12 places"
        try:
            number = decimal.Decimal(shown)
            if function == "pct":
                number *= 100
            number = number.quantize(
                decimal.Decimal(1).scaleb(-int(places or 0)),
                rounding=decimal.ROUND_HALF_EVEN,
                context=decimal.Context(prec=60),
            )
        except decimal.InvalidOperation:
            return None, f"`{{{{{text}}}}}` rounds `{target}`, which does not round to that many places"
        shown = f"{number}%" if function == "pct" else str(number)
    return shown, None


def lint_figures(
    directory: pathlib.Path, front: dict, body: str, summary: dict | None, fail: Callable[[str, str], None]
) -> str | None:
    """A `referenced` directory's body: no typed figure where none may stand,
    every reference resolved. Returns the rendered body, or None if it fails."""
    # NOTHING IS EXEMPT (#63, ruled 2026-10-02, withdrawing the code
    # exemption on #265's measurement). A stdlib reader is not a CommonMark
    # parser, and three reviews found a finder that saw code, a link target or
    # a comment where a renderer shows prose -- each a figure the lint could
    # not see and a reference the resolver skipped. An exemption that cannot
    # fail closed is a hole with a name, so the body is read as written: a
    # figure in a code sample, a URL or a comment in Results or Conclusion is
    # refused like any other, and every `{{...}}` is resolved.
    clean = True
    lines = body.split("\n")
    first_section = next((i for i, line in enumerate(lines) if HEADING.match(line)), len(lines))
    title_at = next((i for i, line in enumerate(lines[:first_section]) if re.match(r"# \S", line)), None)
    for at, line in enumerate(lines):
        if unread_heading(lines, at, first_section, title_at):
            clean = False
            fail(
                "results.heading-unread",
                f"line {at + 1} of the body, {line[:60]!r}, is a heading a renderer shows "
                f"and this linter does not read as a section; a referenced body's headings are "
                f"`## <section>` at column 0, one of {', '.join(SECTIONS)}, and its title the "
                f"first `# ` line before them (#63)",
            )
    for at, line in enumerate(lines):
        why = misread_structure(line)
        if why:
            clean = False
            fail(
                "results.heading-unread",
                f"line {at + 1} of the body, {line[:60]!r}, carries {why}; this linter cannot read "
                f"which headings a renderer shows past it, so a referenced body may not (#63)",
            )
    # SECTIONS FROM THE RAW LINES (#265's fifth review): where a section
    # starts is read from the lines the whitelist and the renderer read, never
    # from decoded text -- an escaped or comment-hidden `## Observation` in
    # Results is no heading to a reader, so it must not move what follows into
    # a laxer section. `visible()` is applied to each section's text, for the
    # figure lint only.
    for section, raw_text in sections_of(body).items():
        # References come out at the raw positions the resolver reads, THEN
        # the text is decoded (#265's sixth review): decoding first turned
        # `\{\{` or `&#123;&#123;` in a comment into `{{`, and the figure
        # between two such spans was stripped as a reference the resolver
        # never saw.
        text = visible(REFERENCE.sub(" ", raw_text))
        uncited = UNCITED.findall(text)
        if uncited and section in FIGURES_NEVER_TYPED:
            clean = False
            fail(
                "results.figure-typed",
                f"the {section} section declares a figure `{uncited[0]}`; {section} carries "
                f"no typed figure, cited or not -- reference the field instead (#63)",
            )
        bare = UNCITED.sub(" ", text)
        typed = typed_figures(bare)
        if typed:
            clean = False
            where = "may carry one only inside `[uncited: <reason>]`" if section not in FIGURES_NEVER_TYPED else "carries none"
            fail(
                "results.figure-typed",
                f"the {section} section types the figure(s) {', '.join(repr(t) for t in typed[:5])}; "
                f"{section} {where} -- reference the product, front-matter or summary field (#63)",
            )

    references = list(REFERENCE.finditer(body))
    needs_product = any("product." in m.group(1) for m in references)
    product, why = the_product(directory, front) if needs_product else (None, "no reference names the product")
    scopes: dict[str, object] = {
        "product": product if product is not None else why,
        "front": front,
        "summary": summary if summary is not None else "the record's summary row could not be read",
    }
    rendered: list[str] = []
    at = 0
    unreadable: set[str] = set()
    for match in references:
        shown, problem = resolve_reference(match.group(1), scopes)
        if problem:
            clean = False
            # A scope that cannot be read is said once, not once per figure.
            scope = next((n for n, v in scopes.items() if isinstance(v, str) and problem.endswith(v)), None)
            if scope is None or scope not in unreadable:
                fail("results.reference-unresolved", problem)
            if scope is not None:
                unreadable.add(scope)
            continue
        rendered.append(body[at : match.start()])
        rendered.append(shown or "")
        at = match.end()
    rendered.append(body[at:])
    # EVERY `{{` IS A REFERENCE OR AN ERROR (#265's third review): a brace
    # pair this grammar did not match -- `{{product.x}` -- was neither
    # resolved nor refused, and rendered as written.
    # Braces as a reader sees them: escapes, entities and format characters
    # dropped (visible), and comments, tags and emphasis markers removed, which
    # GitHub never shows -- `{<!-- -->{x}<!-- -->}` and `{*{*x*}*}` read `{{x}}`
    # (#265's fifth review). Strikethrough's `~` too, and a link down to its
    # text: `{~{x}~}` and `{[{](#a)x}[}](#a)}` read `{{x}}` (its sixth).
    seen = visible("".join(rendered) if clean else REFERENCE.sub(" ", body))
    seen = re.sub(r"!?\[([^\]\n]*)\]\([^)\n]*\)", r"\1", seen)
    left = re.sub(r"<!--.*?-->|<[^>\n]*>|[*_`~]", "", seen, flags=re.DOTALL)
    for stray in re.finditer(r"\{\{|\}\}", left):
        clean = False
        fail(
            "results.reference-unresolved",
            f"`{left[max(0, stray.start() - 20) : stray.end() + 20].strip()}` carries a "
            f"`{stray.group(0)}` that is no reference; every `{{{{...}}}}` resolves, and nothing else "
            f"in a referenced body is written with double braces",
        )
        break
    return "".join(rendered) if clean else None


# Each directory this run passed, as the results page draws it (--ledger), or
# None when no ledger was asked for.
LEDGER: list[dict] | None = None

# A referenced directory's rendered body, for `--render` (#63).
RENDERED: dict[pathlib.Path, str] | None = None

# How many directories declared each `figures` word, printed so a reader of
# the gate sees how many are still typed (#63).
FIGURES_SEEN: dict[str, int] | None = None

# The rule a claim was decided by: a pre-registered decision rule it consumed,
# by any name the directories have used (`decision-rule.toml`,
# `decision-rule-v2.toml`). Matched by name, since the claim row names its
# evidence and nothing else says which is the rule.
RULE_FILE = re.compile(r"decision-rule[^/]*\.toml")


def ledger_row(directory: pathlib.Path, front: dict, claims: list[dict]) -> dict:
    """One passing directory for the ledger, from what was just checked."""
    consumed = [c for claim in claims for c in claim.get("consumes", []) if isinstance(c, dict)]
    rules = sorted({(c.get("path"), c.get("sha256")) for c in consumed if RULE_FILE.fullmatch(str(c.get("path", "")))})
    return {
        "directory": directory.name,
        "result": front.get("result"),
        "product_sha256": front.get("product_sha256"),
        "hypothesis": front.get("hypothesis"),
        "claims": [
            {"id": claim.get("id"), "result": claim.get("result"), "consumes": claim.get("consumes", [])}
            for claim in claims
        ],
        "rules": [{"path": path, "sha256": sha} for path, sha in rules],
    }


def digest_of(path: pathlib.Path) -> str:
    """The SHA-256 of a file, read in chunks so a large artefact is not held."""
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def check_consumed(
    directory: pathlib.Path, rows: list[dict], fail: Callable[[str, str], None]
) -> None:
    """Every claim's consumed evidence, hashed against the committed file.

    The record states which artefacts a claim was derived from and the digest
    each one had. Until this existed, both halves were shape-checked and
    neither was compared to anything: a claim could name a file that had since
    changed, or a file that was never there, and the whole gate stayed green.
    A digest that is never checked is a decoration on a claim.
    """
    for row in rows:
        # A comparison row cites its evidence the same way a claim does (#142),
        # and a digest nobody compares is a decoration on either.
        if row.get("record") not in ("claim", "comparison"):
            continue
        claim = row.get("id", "<unnamed>")
        kind = row.get("record")
        entries = row.get("consumes")
        if not isinstance(entries, list):
            continue  # shape is diet's question, and it has already answered
        for entry in entries:
            if not isinstance(entry, dict):
                continue
            stated, recorded = entry.get("path"), entry.get("sha256")
            if not isinstance(stated, str) or not isinstance(recorded, str):
                continue
            # A results directory is self-contained: its evidence is committed
            # beside it. A path that leaves the directory names something this
            # repository does not carry, and a digest of it would be a digest
            # of whatever happened to be on the machine that ran the linter.
            parts = pathlib.PurePosixPath(stated).parts
            if stated.startswith("/") or ".." in parts:
                fail(
                    "results.provenance-unchecked",
                    f"{kind} `{claim}` consumes `{stated}`, which is outside the "
                    f"run directory; evidence is committed beside the row"
                )
                continue
            if stated == "run.jsonl":
                fail(
                    "results.provenance-unchecked",
                    f"{kind} `{claim}` consumes `run.jsonl`, whose digest it is "
                    f"itself part of; a record cannot state its own hash"
                )
                continue
            artefact = directory / stated
            # `..` and a leading `/` are refused above, but a SYMLINK carries
            # neither and `is_file()` follows it: a committed link can name a
            # path outside the directory, outside the repository, and the
            # digest recorded would be a digest of whatever happened to sit
            # there on the machine that ran the linter -- the machine-local
            # bytes this check exists to rule out. A directory symlink does it
            # with no `..` in the path at all. So containment is checked on
            # the RESOLVED path, which is the only form that can answer it.
            try:
                resolved = artefact.resolve(strict=True)
            except (OSError, RuntimeError):
                # Same sentence as the not-a-file branch below, deliberately:
                # "there is no such file" and "there is something there that
                # is not a file" are one fault to the person reading it, and
                # one guard with two spellings is one the seeded case can only
                # half cover.
                fail(
                    "results.provenance-unchecked",
                    f"{kind} `{claim}` consumes `{stated}`, which is not a file here",
                )
                continue
            here = directory.resolve()
            if not resolved.is_relative_to(here):
                fail(
                    "results.provenance-escapes-the-directory",
                    f"{kind} `{claim}` consumes `{stated}`, which resolves to "
                    f"`{resolved}`, outside the run directory; evidence is "
                    f"committed beside the row, and a link is not evidence",
                )
                continue
            if not resolved.is_file():
                fail("results.provenance-unchecked", f"{kind} `{claim}` consumes `{stated}`, which is not a file here")
                continue
            found = digest_of(artefact)
            if found != recorded:
                fail(
                    "results.provenance-unchecked",
                    f"{kind} `{claim}` consumes `{stated}` at sha256 {recorded}, "
                    f"but the committed file hashes to {found}"
                )


def run_directories(root: pathlib.Path) -> list[pathlib.Path]:
    """Every subdirectory of a root, dot-prefixed ones included.

    Skipping hidden directories would let a results directory the linter would
    reject sit unlinted while other tooling still walks it.
    """
    return sorted(p for p in root.iterdir() if p.is_dir())


def nested_run_directories(root: pathlib.Path) -> list[pathlib.Path]:
    """Run directories sitting INSIDE a run directory.

    Every walker here is one level deep, which is not a bug in the walkers --
    a results directory is a flat, dated, registered thing. What it means is
    that anything a level down is walked by nobody, and two such directories
    were committed with claim records and product digests in them, unlinted
    and unregistered. Skipping them quietly is how they got there. An
    unregistered artefact accumulates authority by sitting still, so this
    refuses rather than ignores: a claim record nothing grades is worse than
    no claim record, because it reads like one that passed.
    """
    nested = []
    for directory in run_directories(root):
        for inner in run_directories(directory):
            if any((inner / f).is_file() for f in REQUIRED_FILES):
                nested.append(inner)
    return sorted(nested)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--root",
        type=pathlib.Path,
        action="append",
        default=[],
        metavar="DIR",
        help="a directory whose subdirectories are run directories",
    )
    parser.add_argument(
        "--ledger",
        type=pathlib.Path,
        metavar="PATH",
        help="write the results ledger here if every directory passes (#32 I2)",
    )
    parser.add_argument(
        "--render",
        type=pathlib.Path,
        metavar="DIR",
        help="lint one directory and print its report with every figure rendered (#63)",
    )
    parser.add_argument(
        "directories",
        nargs="*",
        type=pathlib.Path,
        metavar="DIR",
        help="a run directory to lint directly",
    )
    args = parser.parse_args(argv)

    if args.render is not None:
        if args.root or args.directories or args.ledger is not None:
            parser.error("--render takes one directory and nothing else")
        args.directories = [args.render]
    if not args.root and not args.directories:
        parser.error("nothing to lint: pass --root DIR or one or more run directories")

    global DIET, LEDGER, RENDERED, FIGURES_SEEN
    if args.ledger is not None:
        LEDGER = []
    RENDERED = {}
    FIGURES_SEEN = dict.fromkeys(FIGURES, 0)
    resolved = resolve_diet()
    if resolved is None:
        print(
            "check-results: no diet binary to ask; a record this script cannot "
            "have checked is not a record it can pass",
            file=sys.stderr,
        )
        return 2
    DIET, sha = resolved
    # To stderr under --render, whose stdout is the report and nothing else.
    print(
        f"check-results: record verdicts from {DIET} sha256={sha}",
        file=sys.stderr if args.render is not None else sys.stdout,
    )

    targets: list[pathlib.Path] = []
    failures: list[str] = []

    for root in args.root:
        if not root.is_dir():
            failures.append(f"{root}: root is not a directory")
            continue
        found = run_directories(root)
        if not found:
            failures.append(f"{root}: root holds no run directories")
        for inner in nested_run_directories(root):
            failures.append(
                f"{inner}: a run directory inside a run directory. Every walker "
                f"here is one level deep, so this one is linted by nothing and "
                f"registered nowhere while carrying the files of a real result  "
                f"[results.nested-run-directory]"
            )
        targets += found

    for directory in args.directories:
        if not directory.is_dir():
            failures.append(f"{directory}: not a directory")
            continue
        targets.append(directory)

    for directory in targets:
        # One unreadable directory must not stop the sweep: every later
        # directory would go unlinted and the run would look thinner, not
        # redder.
        try:
            failures += check_run(directory)
        except Unreadable as err:
            failures.append(f"{directory}: {err}")
        except OSError as err:
            failures.append(f"{directory}: cannot be read: {err}")

    for message in failures:
        print(message, file=sys.stderr)

    checked = len(targets)
    if failures:
        print(
            f"check-results: {len(failures)} failure(s) across {checked} directory(ies)",
            file=sys.stderr,
        )
        return 1
    if args.render is not None:
        rendered = RENDERED.get(args.render.resolve())
        if rendered is None:
            print(f"check-results: {args.render} declares `figures = \"typed\"`; there is nothing to render", file=sys.stderr)
            return 1
        sys.stdout.write(rendered)
        return 0
    print(
        f"check-results: {checked} directory(ies) pass; figures referenced in "
        f"{FIGURES_SEEN['referenced']}, still typed in {FIGURES_SEEN['typed']} (#63)"
    )
    if args.ledger is not None:
        args.ledger.write_text(json.dumps({"version": 1, "directories": LEDGER}, indent=1) + "\n", encoding="utf-8")
        print(f"check-results: the ledger, {len(LEDGER)} directory(ies), to {args.ledger}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
