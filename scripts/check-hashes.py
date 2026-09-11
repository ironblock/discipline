#!/usr/bin/env python3
"""Scan for literals that have no shape, by salted digest rather than pattern.

A hygiene pattern says what a forbidden string LOOKS like. An account name and
a person's address look like every other word, so no pattern reaches them --
and writing them into a public pattern table to be matched would publish
exactly what the table exists to keep out. They are salted digests instead:
`scripts/hygiene-hashes.txt` carries `<sha256-hex>  <label>`, and a hit names
the LABEL.

The interesting half is not the hashing. It is the TOKENISER, where the
obvious choice misses the occurrences that motivated the check:

  * A token class that keeps `.` and `-` reads `owner.example.net` as ONE
    token, so a digest of the bare name never matches it. Split on every
    non-alphanumeric instead. Measured, not assumed.
  * Content reaches this repository JSON-escaped. A token immediately after an
    escaped newline has a literal `n` welded to its front and hashes to
    something else entirely. A regex can be given a looser boundary; a digest
    cannot. So every file is tokenised twice: as stored, and as
    `decoding.decoded_text` would read it.

The live case this closes: 268 occurrences of an account name in committed
replay logs, as the owner column of `ls -l` output, with the pattern scan
green over all of them. Nothing in the pattern table would have caught it.

Reads NUL-separated paths on stdin, or walks `--tree DIR`.

Stdlib only. Exit 0 if nothing matched, 1 if anything did, 2 if the scan
itself could not run. A scan that finds no files is an error, not a pass.
"""

from __future__ import annotations

import argparse
import hashlib
import pathlib
import re
import sys

EXIT_DIRTY = 1
EXIT_BROKEN = 2

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
try:
    import decoding
except ModuleNotFoundError:  # pragma: no cover -- an operator error, not a bug
    # EXIT_BROKEN, not `sys.exit(message)`. That form exits ONE, which is this
    # script's code for "the scan ran and found something" -- so a missing
    # decoder would have been reported as a forbidden literal. The reason is
    # spelled out under `Unusable` below, and this guard was the one place that
    # did not follow it.
    print("check-hashes: decoding.py is not beside this script", file=sys.stderr)
    sys.exit(EXIT_BROKEN)

HERE = pathlib.Path(__file__).resolve().parent
DEFAULT_TABLE = HERE / "hygiene-hashes.txt"


class Unusable(Exception):
    """A table this cannot be run against.

    Its own exception rather than `raise SystemExit(message)`: that form exits
    ONE, which is this script's code for "the scan ran and found something".
    A table nobody can compute against is a scan that did not run, and the two
    have to be different numbers or a caller cannot tell "clean" from "broken"
    apart from "dirty".
    """

# The salt is read from the table it salts, so the two cannot drift: a table
# copied without its salt line is a table nothing can be checked against, and
# that is an error rather than a scan of nothing.
SALT_LINE = re.compile(r"^#\s*Salt:\s*(\S+)\s*$", re.M)
ROW = re.compile(r"^([0-9a-f]{64})\s+(\S+)\s*$")


def digest(salt: str, token: str) -> str:
    """One token's row value. Lowercased first, so case never hides a hit."""
    return hashlib.sha256((salt + token.lower()).encode("utf-8")).hexdigest()


def table(path: pathlib.Path) -> tuple[str, dict[str, str]]:
    """The salt and the digest-to-label map, or a reason it cannot be read."""
    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as err:
        raise Unusable(f"cannot read {path}: {err}") from err
    found = SALT_LINE.search(text)
    if not found:
        raise Unusable(
            f"{path} names no salt; a digest table without its salt cannot be "
            f"computed against, and scanning anyway would be a scan of nothing"
        )
    rows: dict[str, str] = {}
    for number, line in enumerate(text.splitlines(), start=1):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        match = ROW.match(line)
        if not match:
            raise Unusable(
                f"{path}:{number} is neither blank, a comment, nor "
                f"`<sha256-hex>  <label>`"
            )
        rows[match.group(1)] = match.group(2)
    return found.group(1), rows


def hits(text: str, salt: str, rows: dict[str, str]) -> list[tuple[int, str, str]]:
    """Every (line number, label, view) a digest row matches in `text`.

    Both views, because the whole point is that one of them is escaped. The
    decoded view's line numbers are its own, so it says which view it is
    reporting rather than pointing at a line the reader would not find.
    """
    found: list[tuple[int, str, str]] = []
    for view, body in (("as stored", text), ("decoded", decoding.decoded_text(text))):
        if not body:
            continue
        for number, line in enumerate(body.splitlines(), start=1):
            for token in decoding.tokens(line):
                label = rows.get(digest(salt, token))
                if label is not None:
                    found.append((number, label, view))
    return found


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--table", default=str(DEFAULT_TABLE))
    parser.add_argument("--tree", help="scan every file under this directory")
    parser.add_argument(
        "--emit",
        nargs=2,
        metavar=("LITERAL", "LABEL"),
        help="print the row for one literal, so a row is never typed by hand",
    )
    args = parser.parse_args(argv)

    try:
        salt, rows = table(pathlib.Path(args.table))
    except Unusable as err:
        print(f"check-hashes: {err}", file=sys.stderr)
        return EXIT_BROKEN

    if args.emit:
        literal, label = args.emit
        tokenised = decoding.tokens(literal)
        if len(tokenised) != 1:
            print(
                f"check-hashes: {literal!r} is {len(tokenised)} tokens, and a "
                f"row matches one; emit each of {tokenised} separately",
                file=sys.stderr,
            )
            return EXIT_BROKEN
        print(f"{digest(salt, tokenised[0])}  {label}")
        return 0

    if not rows:
        print(
            "check-hashes: the table defines no digests; that is not a pass",
            file=sys.stderr,
        )
        return EXIT_BROKEN

    if args.tree:
        root = pathlib.Path(args.tree)
        if not root.is_dir():
            print(f"check-hashes: {root} is not a directory", file=sys.stderr)
            return EXIT_BROKEN
        paths = [p for p in sorted(root.rglob("*")) if p.is_file()]
    else:
        paths = [
            pathlib.Path(name.decode("utf-8", "surrogateescape"))
            for name in sys.stdin.buffer.read().split(b"\0")
            if name
        ]

    # The table itself is the one file the scan skips, exactly as the pattern
    # table is: it is where these values are meant to be written down.
    table_path = pathlib.Path(args.table).resolve()
    paths = [p for p in paths if p.resolve() != table_path]

    if not paths:
        print(
            "check-hashes: nothing to scan; a scan of no files is not a pass",
            file=sys.stderr,
        )
        return EXIT_BROKEN

    dirty = 0
    for path in paths:
        try:
            # `errors="replace"`, and NOT a skip on UnicodeDecodeError. The
            # skip dropped the WHOLE FILE over one byte, and the reason given
            # for it -- that the pattern scan covers the file instead -- is the
            # one thing that cannot be true here: patterns reach strings that
            # have a shape, and this half of the gate exists for the strings
            # that do not. A committed replay log is exactly the artefact this
            # check was built for (268 occurrences of an account name in one),
            # and exactly the kind of file that picks up a stray byte from a
            # terminal, an editor or a locale.
            #
            # Undecodable bytes become U+FFFD, which is not alphanumeric, so
            # the tokeniser treats it as a separator like any other. That can
            # split a token that spanned the bad byte -- the run either side is
            # still scanned -- and it cannot invent a token that was not there,
            # so it adds no false positives. Bytes that are not text at all
            # yield tokens that match nothing, which costs time and no
            # correctness.
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError as err:
            print(f"check-hashes: cannot read {path}: {err}", file=sys.stderr)
            return EXIT_BROKEN
        for number, label, view in hits(text, salt, rows):
            print(f"check-hashes: {label}: {path}:{number} ({view})", file=sys.stderr)
            dirty += 1

    if dirty:
        print(
            f"check-hashes: {dirty} forbidden literal(s) across {len(paths)} "
            f"file(s)",
            file=sys.stderr,
        )
        return EXIT_DIRTY

    print(f"check-hashes: {len(paths)} file(s) clean against {len(rows)} digest(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
