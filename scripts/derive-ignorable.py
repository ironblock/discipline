#!/usr/bin/env python3
"""Harvest the Unicode `Default_Ignorable_Code_Point` set into a table.

`scripts/default-ignorable.tsv` is what the scanner reads. THIS SCRIPT IS
WHERE IT CAME FROM, and the two are meant to be checked against each other
rather than trusted apart -- the same rule the seeded-fault scopes follow: a
list nobody can re-derive is a list that was right when it was written.

Why a table at all, rather than `unicodedata.category(c) in {...}`:

    RULED 2026-09-12. Enumerating categories was the author-imagined case and
    the sibling one step sideways is a HANGUL FILLER. `U+3164` is category
    `Lo`, `str.isalnum()` is true for it, and it renders as nothing -- so it
    welds INTO a token instead of splitting it, and no set of categories can
    reach it. Unicode already defines the set the rule wants:
    `Default_Ignorable_Code_Point` is exactly "renders as nothing by
    specification", and it carries the four fillers (`U+115F`, `U+1160`,
    `U+3164`, `U+FFA0`) that `Cf` and `Mn` between them do not.

The stdlib does not expose the property -- `unicodedata` has categories and
combining classes and nothing else -- so it is derived from the UCD and
committed.

    # once, from a checkout of the UCD or a download:
    curl -sSO https://www.unicode.org/Public/UCD/latest/ucd/DerivedCoreProperties.txt
    python3 scripts/derive-ignorable.py --from DerivedCoreProperties.txt --emit

    # and to check the committed table against a fresh copy:
    python3 scripts/derive-ignorable.py --from DerivedCoreProperties.txt --check

`--check` is NOT a CI gate, deliberately: it needs a 1.1 MB file this
repository does not carry and a network fetch CI has no business making. It
is a re-runnable harvest, like `--derive-scopes`, and the table it checks is
committed so the scanner never depends on either.

Exit 0 if the table matches (or was written), 1 if it differs, 2 if the
harvest could not run.
"""

import argparse
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
TABLE = HERE / "default-ignorable.tsv"
PROPERTY = "Default_Ignorable_Code_Point"


def harvest(source: pathlib.Path) -> tuple[str, list[tuple[int, int]]]:
    """Every range the UCD marks with the property, and the version it says."""
    version = ""
    found: list[tuple[int, int]] = []
    for line in source.read_text(encoding="utf-8").splitlines():
        if not version and line.startswith("# DerivedCoreProperties-"):
            version = line.removeprefix("# DerivedCoreProperties-").removesuffix(".txt")
        body = line.split("#", 1)[0].strip()
        if not body or ";" not in body:
            continue
        parts = [piece.strip() for piece in body.split(";")]
        if len(parts) < 2 or parts[1] != PROPERTY:
            continue
        field = parts[0]
        low, _, high = field.partition("..")
        found.append((int(low, 16), int(high or low, 16)))
    if not found:
        # A file that yielded nothing is a file that was not the UCD, or a
        # property that was renamed. Either way there is no table to write,
        # and writing an empty one would disable the whole strip in silence.
        #
        # EXIT 2, NOT 1 -- this is "could not run", and it said 1 until
        # 2026-09-12 because `SystemExit(str)` exits 1 whatever the string
        # says. The docstring above has promised three codes since it was
        # written; this path contradicted it, in the same script whose whole
        # subject is a table that must not be silently empty. Found by a fresh
        # instance reading the contract against the code.
        print(
            f"derive-ignorable: no `{PROPERTY}` rows in {source}; that is not "
            f"the file this harvest reads, or the property has moved",
            file=sys.stderr,
        )
        raise SystemExit(2)
    found.sort()
    return version, found


def rendered(version: str, ranges: list[tuple[int, int]]) -> str:
    out = [
        "# Unicode Default_Ignorable_Code_Point, one range per line, hex, inclusive.",
        "#",
        "# HARVESTED, NOT HAND-WRITTEN. Regenerate with:",
        "#   python3 scripts/derive-ignorable.py --from DerivedCoreProperties.txt --emit",
        "# and check a committed copy against a fresh download with `--check`.",
        "#",
        "# These are the code points that render as NOTHING by specification.",
        "# `scripts/decoding.py` drops them before tokenising, on the rule that a",
        "# character with no visual width is not a separator. Four of them are",
        "# alphanumeric to Python -- the Hangul fillers U+115F, U+1160, U+3164 and",
        "# U+FFA0 -- which is why a category test could not do this job.",
        "#",
        f"# DerivedCoreProperties {version}",
    ]
    for low, high in ranges:
        out.append(f"{low:04X}\t{high:04X}")
    return "\n".join(out) + "\n"


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--from", dest="source", required=True,
                        help="a copy of DerivedCoreProperties.txt")
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument("--emit", action="store_true", help="write the table")
    action.add_argument("--check", action="store_true",
                        help="compare the committed table against a fresh harvest")
    args = parser.parse_args(argv)

    source = pathlib.Path(args.source)
    if not source.is_file():
        print(f"derive-ignorable: {source} is not a file; nothing to harvest "
              f"from, so there is no verdict to give", file=sys.stderr)
        return 2
    version, ranges = harvest(source)
    text = rendered(version, ranges)

    if args.emit:
        TABLE.write_text(text, encoding="utf-8")
        total = sum(high - low + 1 for low, high in ranges)
        print(f"derive-ignorable: {len(ranges)} range(s), {total} code point(s), "
              f"from DerivedCoreProperties {version}")
        return 0

    if not TABLE.is_file():
        print(f"derive-ignorable: {TABLE} is missing; the scanner reads it, so "
              f"this is a broken tree rather than a difference", file=sys.stderr)
        return 2
    have = TABLE.read_text(encoding="utf-8")
    if have == text:
        print(f"derive-ignorable: the committed table matches "
              f"DerivedCoreProperties {version}")
        return 0
    import difflib

    sys.stderr.writelines(
        difflib.unified_diff(
            have.splitlines(True), text.splitlines(True),
            fromfile="committed", tofile=f"harvested from {version}",
        )
    )
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
