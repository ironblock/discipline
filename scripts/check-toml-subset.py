#!/usr/bin/env python3
"""Check that the `regimen` format really is the TOML subset it claims to be.

`diet/formats/regimen/grammar.pest` states that a regimen document is a subset
of TOML, and the claim is load-bearing: `regimen.toml` files under `results/`
are read by that grammar (via the `exercise` binary) and by Python's `tomllib`
(via `check-results.py`). If the two ever disagree about the same bytes, one of
the gates is lying about the file in front of it.

So: every fixture the conformance harness accepts as a valid regimen must also
parse as TOML. The converse is deliberately NOT checked -- TOML accepts tables,
arrays and floats that regimen v0 does not, which is what "subset" means.

Stdlib only. Exit 0 if the subset property holds, 1 otherwise.
"""

from __future__ import annotations

import pathlib
import sys
import tomllib

VALID = pathlib.Path("diet/formats/regimen/fixtures/valid")


def main() -> int:
    failures: list[str] = []

    if not VALID.is_dir():
        print(f"{VALID}: missing", file=sys.stderr)
        return 1

    cases = sorted(VALID.glob("*.toml"))
    if not cases:
        print(f"{VALID}: holds no fixtures; a check of nothing is not a pass", file=sys.stderr)
        return 1

    for case in cases:
        raw = case.read_bytes()
        try:
            tomllib.loads(raw.decode("utf-8"))
        except (tomllib.TOMLDecodeError, UnicodeDecodeError) as err:
            failures.append(
                f"{case}: accepted as a regimen but rejected by tomllib: {err}"
            )

    for message in failures:
        print(message, file=sys.stderr)
    if failures:
        print(
            f"check-toml-subset: {len(failures)} of {len(cases)} valid fixture(s) "
            f"are not TOML",
            file=sys.stderr,
        )
        return 1
    print(f"check-toml-subset: {len(cases)} valid regimen fixture(s) are also TOML")
    return 0


if __name__ == "__main__":
    sys.exit(main())
