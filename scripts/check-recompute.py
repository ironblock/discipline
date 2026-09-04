#!/usr/bin/env python3
"""Gate 0: every results directory's recorded numbers re-derive, or says why not.

A results directory is a claim with its evidence attached, and the linter
already requires the report to agree with the record. That is agreement
between two things the same run wrote: a summary is a CLAIM about a run, not a
reading of it, so a summary stating three turns over a record holding two
passes the linter and is wrong.

Re-deriving is the answer, and it is the directory's own job -- what re-derives
a token count is not what re-derives a bootstrap interval. So each directory
carries a ``recompute.sh`` implementing one contract: run it, and it exits 0 if
and only if the numbers the report states come back out of the artefacts
committed beside it. This script runs them and counts.

Not everything can be re-derived. A run against a model that no longer exists,
an observation of a system that has since changed -- those are real results and
they are not reproducible by configuration. A directory says which it is, in
its front-matter, and the census prints all three numbers:

    N recomputed, M declared historical, 0 undeclared

The zero is the point. A directory that declares nothing is neither checked nor
counted as skipped, which is how a gate comes to run over nothing while
reporting success -- so an undeclared directory is a failure, and finding no
recomputable directory at all is exit 2 rather than a pass.

Stdlib only. Exit 0 if every recomputable directory re-derives, 1 if one does
not or a directory is undeclared, 2 if there is nothing to recompute.
"""

from __future__ import annotations

import argparse
import pathlib
import subprocess
import sys
import tomllib

FENCE = "+++"
RECOMPUTE = "recompute.sh"

# The two kinds a directory may declare. `reproducible-by-config` is run;
# `historical-observation` is skipped BY DECLARATION and counted, which is a
# different thing from being skipped because nobody said anything.
REPRODUCIBLE = "reproducible-by-config"
HISTORICAL = "historical-observation"
KINDS = (REPRODUCIBLE, HISTORICAL)

EXIT_FAIL = 1
EXIT_NOTHING = 2


def front_matter(directory: pathlib.Path) -> tuple[dict | None, str | None]:
    """A directory's README front-matter, or the reason it could not be read."""
    report = directory / "README.md"
    if not report.is_file():
        return None, "has no README.md"
    try:
        text = report.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as err:
        return None, f"README.md cannot be read: {err}"
    if not text.startswith(FENCE + "\n"):
        return None, "README.md does not open with `+++` front-matter"
    parts = text.split(FENCE + "\n", 2)
    if len(parts) < 3:
        return None, "README.md front-matter is not closed"
    try:
        return tomllib.loads(parts[1]), None
    except ValueError as err:
        return None, f"README.md front-matter is not TOML: {err}"


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default="results", help="the results tree to walk")
    args = parser.parse_args(argv)

    root = pathlib.Path(args.root)
    if not root.is_dir():
        print(f"check-recompute: {root} is not a directory", file=sys.stderr)
        return EXIT_NOTHING

    failures: list[str] = []
    recomputed = 0
    historical = 0
    undeclared = 0

    for directory in sorted(p for p in root.iterdir() if p.is_dir()):
        front, err = front_matter(directory)
        if front is None:
            undeclared += 1
            failures.append(f"{directory}: {err}")
            continue
        kind = front.get("kind")
        if kind not in KINDS:
            undeclared += 1
            failures.append(
                f"{directory}: front-matter `kind` is {kind!r}; it must be one of "
                f"{' or '.join(KINDS)}, because a directory that declares nothing is "
                f"neither checked nor knowingly skipped"
            )
            continue
        if kind == HISTORICAL:
            historical += 1
            continue

        script = directory / RECOMPUTE
        if not script.is_file():
            failures.append(
                f"{directory}: declares `{REPRODUCIBLE}` and carries no {RECOMPUTE}"
            )
            continue
        run = subprocess.run(
            ["bash", str(script.resolve())],
            capture_output=True,
            text=True,
            cwd=directory,
        )
        if run.returncode != 0:
            detail = (run.stderr or run.stdout or "").strip().splitlines()
            failures.append(
                f"{directory}: {RECOMPUTE} exited {run.returncode}"
                + ("\n  " + "\n  ".join(detail) if detail else "")
            )
            continue
        recomputed += 1

    for message in failures:
        print(message, file=sys.stderr)

    census = (
        f"check-recompute: {recomputed} recomputed, {historical} declared "
        f"historical, {undeclared} undeclared"
    )
    if failures:
        print(census, file=sys.stderr)
        return EXIT_FAIL
    if recomputed == 0:
        print(
            f"{census}\ncheck-recompute: nothing was recomputed; a check of nothing "
            f"is not a pass",
            file=sys.stderr,
        )
        return EXIT_NOTHING
    print(census)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
