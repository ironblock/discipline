#!/usr/bin/env python3
"""Check that the `regimen` format really is the TOML subset it claims to be.

`diet/formats/regimen/grammar.pest` states that a regimen document is a subset
of TOML, and the claim is load-bearing: `regimen.toml` files under `results/`
are read by that grammar (via the `diet` binary) and by Python's `tomllib`
(via `check-results.py`). If the two ever disagree about the same bytes, one of
the gates is lying about the file in front of it.

Two halves, because one alone cannot see the grammar widen:

  * Every fixture the conformance harness accepts as a valid regimen must also
    parse as TOML. The converse is deliberately not checked -- TOML accepts
    tables, arrays and floats that regimen v0 does not, which is what "subset"
    means.
  * Every invalid fixture whose ``.reason`` begins ``NOT-TOML:`` must be
    rejected by tomllib as well. Those are the near misses: a DEL inside a
    string, a bare CR, a non-ASCII bare key, ``True``. Without them, relaxing
    a single grammar rule would widen regimen past TOML while the ten valid
    fixtures -- all plain ASCII -- kept parsing and the whole ratchet stayed
    green.

Stdlib only. Exit 0 if the subset property holds, 1 otherwise.
"""

from __future__ import annotations

import pathlib
import sys
import tomllib

FIXTURES = pathlib.Path("diet/formats/regimen/fixtures")
VALID = FIXTURES / "valid"
INVALID = FIXTURES / "invalid"

# A `.reason` starting with this marks a fixture that is not TOML either, and
# so must be rejected by both readers.
NOT_TOML = "NOT-TOML:"


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
        try:
            tomllib.loads(case.read_bytes().decode("utf-8"))
        except (ValueError, UnicodeDecodeError) as err:
            failures.append(
                f"{case}: accepted as a regimen but rejected by tomllib: {err}"
            )

    # The adversarial half.
    near_misses = []
    for case in sorted(INVALID.glob("*.toml")):
        reason = case.with_suffix(".reason")
        if not reason.is_file():
            continue  # the conformance harness reports the missing pair
        if not reason.read_text(encoding="utf-8").lstrip().startswith(NOT_TOML):
            continue
        near_misses.append(case)
        try:
            tomllib.loads(case.read_bytes().decode("utf-8"))
        except (ValueError, UnicodeDecodeError):
            continue
        failures.append(
            f"{case}: marked {NOT_TOML} but tomllib accepts it, so it cannot "
            f"pin the subset claim"
        )

    if not near_misses:
        print(
            f"{INVALID}: no fixture is marked {NOT_TOML}, so nothing would "
            f"notice the grammar widening past TOML",
            file=sys.stderr,
        )
        return 1

    for message in failures:
        print(message, file=sys.stderr)
    if failures:
        print(f"check-toml-subset: {len(failures)} failure(s)", file=sys.stderr)
        return 1
    print(
        f"check-toml-subset: {len(cases)} valid regimen fixture(s) are also TOML; "
        f"{len(near_misses)} near miss(es) are rejected by both readers"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
