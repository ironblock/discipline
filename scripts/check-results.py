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
  4. Every ``consumes`` entry on every claim row must name a file in the
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

Exit code is 0 if every linted directory passes, 1 otherwise, and 2 when no
``diet`` binary could be resolved to ask. A sweep that finds no run
directories is an error, not a pass.
"""

from __future__ import annotations

from collections.abc import Callable

import argparse
import datetime
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
    "substrate": str,
    "dogma_version": int,
}

SECTIONS = ["Observation", "Hypothesis", "Test", "Results", "Conclusion"]

# The two kinds a directory may declare. Spelled here as well as in
# check-recompute.py because this linter refuses a third spelling and that one
# decides what to run; a directory whose kind neither reader knows would
# otherwise be caught by neither.
KINDS = ("reproducible-by-config", "historical-observation")

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


def split_front_matter(text: str) -> tuple[str | None, str, str | None]:
    """Split a report into (front-matter source, body, error).

    Exactly one of the front-matter source or the error is not ``None``.
    """
    lines = text.split("\n")
    if not lines or lines[0].strip() != FENCE:
        return None, "", f"does not open with a {FENCE!r} front-matter fence"
    for index in range(1, len(lines)):
        if lines[index].strip() == FENCE:
            return "\n".join(lines[1:index]), "\n".join(lines[index + 1 :]), None
    return None, "", f"front-matter fence {FENCE!r} is never closed"


def body_sections(body: str) -> list[str]:
    """Level-2 headings in document order, ignoring fenced code blocks."""
    # A heading inside an HTML comment is not a heading: it renders as nothing.
    body = HTML_COMMENT.sub("", body)
    headings: list[str] = []
    fence: str | None = None
    for line in body.split("\n"):
        opener = CODE_FENCE.match(line)
        if fence is None:
            if opener:
                fence = opener.group(1)
            else:
                match = HEADING.match(line)
                if match:
                    headings.append(match.group(1))
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

    def fail(message: str) -> None:
        failures.append(f"{directory}: {message}")

    name = directory.name
    if name != TEMPLATE_DIR:
        match = DIR_NAME.fullmatch(name)
        if not match:
            fail("name is not `YYYY-MM-DD-<slug>` with a lowercase hyphenated slug")
        else:
            try:
                datetime.date.fromisoformat(match.group(1))
            except ValueError:
                fail(f"name carries an impossible date `{match.group(1)}`")

    missing = [f for f in REQUIRED_FILES if not (directory / f).is_file()]
    for f in missing:
        fail(f"missing required file `{f}`")
    if missing:
        return failures

    # --- run.jsonl: diet's verdict, relayed ------------------------------
    verdict = record_verdict(directory / "run.jsonl")
    summary: dict | None = None
    recorded_regime: dict | None = None
    if not verdict.ok:
        fail(f"run.jsonl is not a session record, says diet check-record: {verdict.error}")
        # Nothing below reads the record. The number and digest checks need a
        # summary row, and a summary row is only known to exist once diet has
        # said the file is a record; reaching for one here would be this
        # script judging the format after all.
    else:
        value = verdict.value or {}
        rows = [json.loads(line) for line in value.get("canonical", "").splitlines() if line.strip()]
        summaries = [r for r in rows if r.get("record") == "summary"]
        if len(summaries) != 1:
            fail(f"run.jsonl holds {len(summaries)} summary rows, expected exactly 1")
        else:
            summary = summaries[0]
        recorded_regime = value.get("regime")
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
        fail(f"regimen.toml is not TOML: {err}")

    # --- README.md front-matter -----------------------------------------
    try:
        text = read_text(directory / "README.md")
    except Unreadable as err:
        fail(str(err))
        return failures
    source, body, err = split_front_matter(text)
    if err is not None or source is None:
        fail(f"README.md {err}")
        return failures
    try:
        front = tomllib.loads(source)
    except ValueError as exc:
        fail(f"README.md front-matter is not TOML: {exc}")
        return failures

    for key, expected in REQUIRED_KEYS.items():
        if key not in front:
            fail(f"front-matter is missing required key `{key}`")
        elif not has_type(front[key], expected):
            fail(
                f"front-matter key `{key}` is {type(front[key]).__name__}, "
                f"expected {type_name(expected)}"
            )

    for key in ("controls_run", "known_defects"):
        value = front.get(key)
        if isinstance(value, list) and not all(isinstance(item, str) for item in value):
            fail(f"front-matter key `{key}` must be a list of strings")

    regime = front.get("regime")
    if isinstance(regime, dict):
        for key, expected in REQUIRED_REGIME_KEYS.items():
            if key not in regime:
                fail(f"front-matter is missing required key `regime.{key}`")
            elif not has_type(regime[key], expected):
                fail(
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
                        f"front-matter `regime.{key}` is {regime[key]!r} but the "
                        f"record's start row carries {recorded_regime[key]!r}"
                    )
        if regimen is not None:
            # Every key under [regime], not only the required three: a regime
            # field the regimen does not bind is a claim about the run that
            # nothing backs.
            for key in regime:
                if key not in regimen:
                    fail(f"regimen.toml does not bind `{key}`, so `regime.{key}` is unbacked")
                elif not same_value(regimen[key], regime[key]):
                    fail(
                        f"front-matter `regime.{key}` is {regime[key]!r} but "
                        f"regimen.toml binds {regimen[key]!r}"
                    )

    kind = front.get("kind")
    if isinstance(kind, str) and kind not in KINDS:
        fail(f"front-matter `kind` is {kind!r}, which is neither {' nor '.join(KINDS)}")

    sha = front.get("product_sha256")
    if isinstance(sha, str):
        if not SHA256.fullmatch(sha):
            fail("front-matter `product_sha256` is not 64 lowercase hex characters")
        elif summary is not None:
            recorded = summary.get("product_sha256")
            if recorded is None:
                fail("the record's summary row does not carry `product_sha256`")
            elif recorded != sha:
                fail(
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
                    f"front-matter `{shown}` states {value!r}, but the summary "
                    f"record binds no `{shown}`"
                )
            elif not (is_number(at_path) and same_value(at_path, value)):
                fail(
                    f"front-matter `{shown}` states {value!r} but the summary "
                    f"record binds `{shown}` to {at_path!r}"
                )

    # --- sections ---------------------------------------------------------
    headings = body_sections(body)
    if headings != SECTIONS:
        fail(
            f"README.md sections are {headings!r}, expected exactly {SECTIONS!r} in order"
        )

    return failures


def digest_of(path: pathlib.Path) -> str:
    """The SHA-256 of a file, read in chunks so a large artefact is not held."""
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def check_consumed(
    directory: pathlib.Path, rows: list[dict], fail: Callable[[str], None]
) -> None:
    """Every claim's consumed evidence, hashed against the committed file.

    The record states which artefacts a claim was derived from and the digest
    each one had. Until this existed, both halves were shape-checked and
    neither was compared to anything: a claim could name a file that had since
    changed, or a file that was never there, and the whole gate stayed green.
    A digest that is never checked is a decoration on a claim.
    """
    for row in rows:
        if row.get("record") != "claim":
            continue
        claim = row.get("id", "<unnamed>")
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
                    f"claim `{claim}` consumes `{stated}`, which is outside the "
                    f"run directory; evidence is committed beside the claim"
                )
                continue
            if stated == "run.jsonl":
                fail(
                    f"claim `{claim}` consumes `run.jsonl`, whose digest it is "
                    f"itself part of; a record cannot state its own hash"
                )
                continue
            artefact = directory / stated
            if not artefact.is_file():
                fail(f"claim `{claim}` consumes `{stated}`, which is not a file here")
                continue
            found = digest_of(artefact)
            if found != recorded:
                fail(
                    f"claim `{claim}` consumes `{stated}` at sha256 {recorded}, "
                    f"but the committed file hashes to {found}"
                )


def run_directories(root: pathlib.Path) -> list[pathlib.Path]:
    """Every subdirectory of a root, dot-prefixed ones included.

    Skipping hidden directories would let a results directory the linter would
    reject sit unlinted while other tooling still walks it.
    """
    return sorted(p for p in root.iterdir() if p.is_dir())


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
        "directories",
        nargs="*",
        type=pathlib.Path,
        metavar="DIR",
        help="a run directory to lint directly",
    )
    args = parser.parse_args(argv)

    if not args.root and not args.directories:
        parser.error("nothing to lint: pass --root DIR or one or more run directories")

    global DIET
    resolved = resolve_diet()
    if resolved is None:
        print(
            "check-results: no diet binary to ask; a record this script cannot "
            "have checked is not a record it can pass",
            file=sys.stderr,
        )
        return 2
    DIET, sha = resolved
    print(f"check-results: record verdicts from {DIET} sha256={sha}")

    targets: list[pathlib.Path] = []
    failures: list[str] = []

    for root in args.root:
        if not root.is_dir():
            failures.append(f"{root}: root is not a directory")
            continue
        found = run_directories(root)
        if not found:
            failures.append(f"{root}: root holds no run directories")
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
    print(f"check-results: {checked} directory(ies) pass")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
