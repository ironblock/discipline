#!/usr/bin/env python3
"""Campaign one of the prompt-language factorial: re-derive the three headline figures the claim cites.

WHY THIS EXISTS. `campaign-one.report.txt` is the instrument's own output and is
the primary artifact, but it does NOT contain the numbers the ledger quotes.
Those were computed in prose in `RESULTS.md` — 244/336, the 24-cell floor, and
the harness-meta step — so the row was recomputable in principle and had no
committed derivation. This is that derivation, which is what makes the row a
gate-0 target rather than a claim someone would have to re-derive by hand and
hope they matched (the recompute gate: a claim re-derives its number from its committed artifacts).

TWO COLLAPSE CONVENTIONS, and this is the whole reason a hand re-derivation is
untrustworthy. The row uses BOTH, correctly, and the report names one of them
in passing as "any-of collapse":

  aggregate      PER CALL. 7 primary-null steps x 24 cells x k=2 = 336 calls.
  per-cell floor PER STEP, k collapsed with ANY. 7 steps per cell, a step
                 counting as confabulating if EITHER draw did.

Measured while writing this: computing the per-cell floor per-CALL instead
yields a minimum of 0.357, not 0.571 — it looks like the row is wrong, and the
row is right. A reader who re-derived this by hand and picked the other
convention would have "found" a defect that is not there. Pinning the convention
in code is the point.

THE HARNESS-META STEP (67, the `/compact` turn) needs its own note: the grader
left `is_confabulation` as null for all 48 of its records, so the 41/48 cannot
come from that field. Two independently-stated criteria recover it exactly, and
they agree on this data — a parsed non-NONE answer, and an answer carrying at
least one GOTCHA. Both are asserted below; if they ever disagree the assertion
fails rather than silently picking one.
"""
from __future__ import annotations

import collections
import json
import sys
from pathlib import Path

GRADED = Path(__file__).resolve().parent / "campaign-one.graded.jsonl"
HARNESS_META_STEP = 67


def load() -> list[dict]:
    return [json.loads(line) for line in GRADED.read_text().splitlines() if line.strip()]


def derive(rows: list[dict]) -> dict:
    # The primary-null population is exactly the records the grader scored.
    # Not a hand-listed step set — the grader's own verdict defines membership,
    # so this cannot drift away from what was graded.
    scored = [r for r in rows if r.get("is_confabulation") is not None]

    per_call_confab = sum(1 for r in scored if r["is_confabulation"])
    steps = sorted({r["step"] for r in scored})

    by_cell: dict[tuple, dict[int, list[bool]]] = collections.defaultdict(
        lambda: collections.defaultdict(list))
    for r in scored:
        c = r["cell"]
        by_cell[(c["E"], c["A"], c["B"], c["C"])][r["step"]].append(r["is_confabulation"])
    cell_rates = {
        "·".join(cell): sum(any(v) for v in per_step.values()) / len(per_step)
        for cell, per_step in by_cell.items()
    }

    meta = [r for r in rows if r["step"] == HARNESS_META_STEP]
    by_parse = sum(1 for r in meta if not r.get("is_none") and r.get("parse_ok"))
    by_gotcha = sum(1 for r in meta if r.get("gotchas"))
    assert by_parse == by_gotcha, (
        f"the two criteria for the harness-meta step disagree "
        f"({by_parse} vs {by_gotcha}); the docstring's claim that they are "
        f"equivalent on this data no longer holds and the figure needs a ruling")
    assert all(r.get("is_confabulation") is None for r in meta), (
        "the grader now scores the harness-meta step — use is_confabulation "
        "directly and delete the two-criteria workaround")

    return {
        "aggregate_per_call": {
            "confabulated": per_call_confab,
            "calls": len(scored),
            "rate": per_call_confab / len(scored),
            "primary_null_steps": steps,
        },
        "per_cell_any_of_collapse": {
            "cells": len(cell_rates),
            "min": min(cell_rates.values()),
            "max": max(cell_rates.values()),
            "at_1_000": sum(1 for v in cell_rates.values() if v == 1.0),
            "rates": dict(sorted(cell_rates.items())),
        },
        "harness_meta_step": {
            "step": HARNESS_META_STEP,
            "confabulated": by_parse,
            "calls": len(meta),
            "rate": by_parse / len(meta),
            "criterion": "not is_none AND parse_ok (== has >=1 gotcha, asserted)",
        },
    }


def main() -> int:
    result = derive(load())
    out = json.dumps(result, indent=2) + "\n"
    if "--out" in sys.argv:
        # `--out` is a DIRECTORY and the file inside it is `headline.json`,
        # matching die45_grade.py's convention so the recompute harness can
        # drive both instruments the same way.
        d = Path(sys.argv[sys.argv.index("--out") + 1])
        d.mkdir(parents=True, exist_ok=True)
        (d / "headline.json").write_text(out)
    else:
        sys.stdout.write(out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
