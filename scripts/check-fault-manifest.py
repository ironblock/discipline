#!/usr/bin/env python3
"""Keep tools/gate/faults.toml in step with what verify.sh actually proves.

The manifest defines what parity means: the replacement gate retires the old
one only when it proves every entry. A manifest that has drifted from the gate
defines the wrong parity, and would let the new gate retire the old one while
covering less -- so the manifest is itself gated.

THIS GATE PROVES ONE HALF. It compares what verify.sh DECLARES against what
the manifest claims -- reading declaration tables and fixture directories out
of the source. That the declared faults actually go RED is proved elsewhere:
by `--selftest` for most kinds, and by the per-run `regimen` check for the
subset fixtures. Composed, the two give declared-to-manifest and
declared-to-executed; neither alone gives both, and a reader should not credit
this script with the second.

Checks:
  * every seeded case, mechanics assertion, results fixture and pinned pattern
    class verify.sh proves has an entry here, and vice versa;
  * the counts in [meta] match reality;
  * every entry names a failure_class, and no entry has been marked migrated
    while still carrying a legacy_signature as its only evidence.

Stdlib only. Exit 0 if the manifest matches, 1 otherwise.
"""

from __future__ import annotations

import pathlib
import re
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
VERIFY = ROOT / "verify.sh"
MANIFEST = ROOT / "tools" / "gate" / "faults.toml"
FIXTURES = ROOT / "tests" / "fixtures" / "results-bad"
REGIMEN_INVALID = ROOT / "diet" / "formats" / "regimen" / "fixtures" / "invalid"

# A fixture whose .reason opens with this pins the TOML-subset agreement: both
# readers must reject it. Those are the entries that relocate when the second
# reader goes away, so each must say where its assertion lands.
NOT_TOML = "NOT-TOML:"
RELOCATING = {"subset-fixture"}

CASE = re.compile(r'seeded_case\s+"([^"]+)"\s+(\w+)\s+(\w+)\s*\\\s*\n\s*\'([^\']*)\'')
MECH = re.compile(r'expect_exit\s+"([^"]+)"\s+(\d+)')
REQ = re.compile(r"REQUIRED_(HYGIENE|PAGES)_CLASSES=\(([^)]*)\)", re.DOTALL)


# What each seeded case says about itself, keyed by the id derived from it.
# Recorded so the manifest's own prose can be checked rather than trusted: an
# entry whose `label` and `legacy_signature` are fiction describes a fault
# nobody proves, and the id alone cannot tell you that.
DETAILS: dict[str, dict[str, str]] = {}


def observed() -> dict[str, set[str]]:
    """What verify.sh proves, read out of verify.sh rather than assumed."""
    s = VERIFY.read_text(encoding="utf-8")
    seen: dict[str, set[str]] = {k: set() for k in
                                 ("seeded-gate", "mechanics", "results-fixture",
                                  "pattern-class", "subset-fixture")}
    for label, check, inject, sig in CASE.findall(s):
        ident = f"{check}.{inject.removeprefix('inject_')}"
        seen["seeded-gate"].add(ident)
        DETAILS[ident] = {"label": label, "legacy_signature": sig}
    for label, _want in MECH.findall(s):
        seen["mechanics"].add("mech." + re.sub(r"[^a-z0-9]+", "-", label.lower()).strip("-"))
    for kind, body in REQ.findall(s):
        prefix = kind.lower()
        for cls in body.split():
            seen["pattern-class"].add(f"{prefix}.{cls}")
    if FIXTURES.is_dir():
        for d in FIXTURES.iterdir():
            if d.is_dir():
                seen["results-fixture"].add(f"results.{d.name}")
    if REGIMEN_INVALID.is_dir():
        for reason in REGIMEN_INVALID.glob("*.reason"):
            if reason.read_text(encoding="utf-8").lstrip().startswith(NOT_TOML):
                seen["subset-fixture"].add(f"subset.{reason.stem}")
    return seen


def main() -> int:
    failures: list[str] = []

    if not MANIFEST.is_file():
        print(f"{MANIFEST}: missing", file=sys.stderr)
        return 1
    try:
        doc = tomllib.loads(MANIFEST.read_bytes().decode("utf-8"))
    except (ValueError, UnicodeDecodeError) as err:
        print(f"{MANIFEST}: not TOML: {err}", file=sys.stderr)
        return 1

    entries = doc.get("fault") or []
    if not entries:
        print(f"{MANIFEST}: declares no faults, so it defines no parity", file=sys.stderr)
        return 1

    # Before the loop: `observed()` is what fills DETAILS, which the loop reads.
    seen = observed()

    declared: dict[str, set[str]] = {}
    for index, entry in enumerate(entries):
        where = f"{MANIFEST.name}[{index}]"
        for field in ("id", "kind", "check", "label", "failure_class"):
            if not isinstance(entry.get(field), str) or not entry[field].strip():
                failures.append(f"{where}: `{field}` is missing or empty")
        kind, ident = entry.get("kind"), entry.get("id")
        if isinstance(kind, str) and isinstance(ident, str):
            if ident in declared.setdefault(kind, set()):
                failures.append(f"{where}: duplicate id `{ident}`")
            declared[kind].add(ident)
        # The id says the case exists; these say what it proves. Both were
        # unchecked prose until an adversarial review put fiction in them and
        # watched this script pass.
        if kind == "seeded-gate" and isinstance(ident, str) and ident in DETAILS:
            for field, actual in DETAILS[ident].items():
                stated = entry.get(field)
                if stated != actual:
                    failures.append(
                        f"{where}: {field} is {stated!r}, but verify.sh's "
                        f"case says {actual!r}"
                    )
        if entry.get("migrated") is True and "failure_class" not in entry:
            failures.append(f"{where}: marked migrated with no failure_class")
        # An entry whose assertion moves must name where it lands, or the
        # count drops at retirement and reads as coverage loss.
        if kind in RELOCATING and not isinstance(entry.get("migrated_to"), str):
            failures.append(f"{where}: kind `{kind}` must carry a `migrated_to` pointer")

    for kind in sorted(set(seen) | set(declared)):
        have, want = declared.get(kind, set()), seen.get(kind, set())
        for ident in sorted(want - have):
            failures.append(f"verify.sh proves `{ident}` ({kind}), which the manifest omits")
        for ident in sorted(have - want):
            failures.append(f"the manifest claims `{ident}` ({kind}), which verify.sh does not prove")

    meta = doc.get("meta") or {}
    red = sum(len(seen[k]) for k in seen if k != "mechanics")
    if meta.get("red_faults") != red:
        failures.append(f"[meta] red_faults is {meta.get('red_faults')}, observed {red}")
    if meta.get("mechanics_assertions") != len(seen["mechanics"]):
        failures.append(
            f"[meta] mechanics_assertions is {meta.get('mechanics_assertions')}, "
            f"observed {len(seen['mechanics'])}"
        )

    for message in failures:
        print(message, file=sys.stderr)
    if failures:
        print(f"check-fault-manifest: {len(failures)} failure(s)", file=sys.stderr)
        return 1

    # Report the split by WHERE each kind is proven red, so the arithmetic
    # explains itself. `--selftest` reports 67; the manifest says 75 red; the
    # difference is the 8 subset fixtures, which go red on every run through
    # check_regimen rather than in the selftest.
    by_selftest = sum(len(seen[k]) for k in ("seeded-gate", "results-fixture", "pattern-class"))
    by_per_run = len(seen["subset-fixture"])
    migrated = sum(1 for e in entries if e.get("migrated") is True)
    print(
        f"check-fault-manifest: {len(entries)} fault(s) define parity = "
        f"{by_selftest} red in --selftest + {by_per_run} red per run (regimen) + "
        f"{len(seen['mechanics'])} mechanics assertions; {migrated} migrated"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
