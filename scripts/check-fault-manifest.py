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

import gatelib
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

# The kinds `verify.sh --selftest` proves red itself, and so the kinds a shard
# can be assigned. Named once: the census script and the report at the bottom
# both derive from this, and they used to be two lists that agreed by hand.
SELFTEST_KINDS = ("seeded-gate", "results-fixture", "pattern-class")

# A line git writes into a file it could not merge. `=======` alone is not
# one: it is a plausible separator in ordinary prose, and a checker that
# refused it would refuse files nobody is merging. The two arrow markers are
# not plausible as anything else.
CONFLICTED = re.compile(r"^(<{7} |>{7} )", re.M)


def refuse_conflicted(path: pathlib.Path, text: str) -> None:
    """Exit 2 if a file still carries conflict markers.

    Both of this gate's two inputs go through here, for one reason: what a
    checker reads out of a half-merged file is the union of both sides, or
    neither side, depending on where the markers fell, and either way it
    looks like an answer.

    THE COUNT IS DERIVED FROM verify.sh. So a `--count-red` taken while
    verify.sh is still half-merged hands back a number about a file nobody
    wrote. The tool that asked is mid-merge and about to write that number
    into the manifest, which is the one moment nothing is watching.

    AND THE MANIFEST IS THE FILE BEING MERGED. `faults.toml` conflicts on
    essentially every merge in a stack -- `merge-gate.py --union` exists for
    exactly that -- so a half-merged manifest is the ordinary state of this
    file during ordinary work, not a corruption. It reached `tomllib` and
    came back "not TOML", exit 1, which reads as a finding about the
    manifest when the truth is that the caller is mid-merge.

    Exit 2 rather than 1: this is "I was asked something I cannot answer",
    the code every other refusal in this gate uses for that.
    """
    if CONFLICTED.search(text):
        found = CONFLICTED.findall(text)
        print(
            f"{path}: still carries {len(found)} conflict marker(s). Nothing is "
            f"read from a half-merged file -- what is in one is the union of both "
            f"sides, or neither side. Resolve it, then ask again.",
            file=sys.stderr,
        )
        raise SystemExit(2)


MECH = re.compile(r'expect_exit\s+"([^"]+)"\s+(\d+)')
# The line check_test prints before it runs anything, taken from verify.sh so
# that this file holds no second copy of it. Captured with its `%s` and `%d`
# still in place; the guard below substitutes the scope and every count.
ANNOUNCE = re.compile(r"printf '(test scope: %s \(%d test\(s\) selected\))")
REQ = re.compile(r"REQUIRED_(HYGIENE|PAGES)_CLASSES=\(([^)]*)\)", re.DOTALL)


# What each seeded case says about itself, keyed by the id derived from it.
# Recorded so the manifest's own prose can be checked rather than trusted: an
# entry whose `label` and `legacy_signature` are fiction describes a fault
# nobody proves, and the id alone cannot tell you that.
DETAILS: dict[str, dict[str, str]] = {}


def observed() -> dict[str, set[str]]:
    """What verify.sh proves, read out of verify.sh rather than assumed."""
    s = VERIFY.read_text(encoding="utf-8")
    refuse_conflicted(VERIFY, s)
    seen: dict[str, set[str]] = {k: set() for k in
                                 ("seeded-gate", "mechanics", "results-fixture",
                                  "pattern-class", "subset-fixture")}
    for label, check, inject, sig, scope in gatelib.seeded_cases(s):
        ident = f"{check}.{inject.removeprefix('inject_')}"
        seen["seeded-gate"].add(ident)
        DETAILS[ident] = {"label": label, "legacy_signature": sig}
        # Only the `test` check takes a scope, so only its cases carry a
        # target here. Recording an empty one for the rest would make three
        # hundred manifest entries restate that a Python check has no cargo
        # test target.
        if check == "test":
            DETAILS[ident]["target"] = scope
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
    # `--count-red` prints the number `red_faults` must carry and nothing
    # else, so a tool that has just assembled the fault list can ask THIS
    # reader for the count rather than reimplementing the arithmetic or
    # scraping it out of a failure message. One reader, structured answer.
    counting = "--count-red" in sys.argv
    counting_mechanics = "--count-mechanics" in sys.argv
    listing_fixtures = "--fixture-classes" in sys.argv
    # The same question narrowed to the faults `--selftest` itself proves --
    # the subset fixtures go red on every run through check_regimen and are
    # never in a shard. check-selftest-census.py asks for this rather than
    # subtracting one number from another, because a subtraction is a second
    # opinion about which kinds the selftest runs.
    counting_selftest = "--count-selftest-red" in sys.argv

    # Asked for the count, answer the count -- before the manifest is read at
    # all. The count derives from `verify.sh` and the fixture directories and
    # never from the manifest, and the caller is a tool that has just
    # assembled a fault list and is holding a manifest that does not yet
    # agree with it. Refusing to answer because the file it is about to
    # rewrite is missing, empty or not yet valid TOML is refusing exactly
    # when asked. `observed()` also fills DETAILS, which the loop below reads.
    seen = observed()
    if counting:
        print(sum(len(seen[k]) for k in seen if k != "mechanics"))
        return 0
    # The same question for the other total the manifest carries. A branch
    # that adds a mechanics assertion changes this line, and a resolver that
    # cannot recount it refuses a merge over a number it could have derived.
    if counting_mechanics:
        print(len(seen["mechanics"]))
        return 0
    if counting_selftest:
        print(sum(len(seen[k]) for k in SELFTEST_KINDS))
        return 0
    # The selftest greps each results fixture's output for the class declared
    # here. It is emitted from THIS script because the manifest has one reader
    # and a second one in the shell would be free to disagree with it.
    if listing_fixtures:
        try:
            text = MANIFEST.read_text(encoding="utf-8")
        except OSError as err:
            print(f"cannot read the manifest: {err}", file=sys.stderr)
            return 1
        refuse_conflicted(MANIFEST, text)
        try:
            entries = tomllib.loads(text)["fault"]
        except (ValueError, KeyError) as err:
            print(f"cannot read the manifest: {err}", file=sys.stderr)
            return 1
        for entry in entries:
            if entry.get("kind") == "results-fixture":
                print(f"{entry.get('label')}\t{entry.get('failure_class')}")
        return 0

    failures: list[str] = []

    if not MANIFEST.is_file():
        print(f"{MANIFEST}: missing", file=sys.stderr)
        return 1
    # THE MANIFEST IS REFUSED MID-MERGE, and not merely reported as bad TOML.
    # `faults.toml` conflicts on essentially every merge in a stack -- it is
    # what `merge-gate.py --union` exists for -- so a half-merged one is the
    # ordinary state of this file during the ordinary operation, not a
    # corruption. `tomllib` fails on the markers and the old message said
    # "not TOML", exit 1: a finding about the manifest, when the truth was
    # that the caller is mid-merge and this gate could not answer. The
    # reasoning `refuse_conflicted` already carries for `verify.sh` is the
    # same reasoning here, so it is the same call.
    #
    # A TOML error that is NOT a conflict marker stays 1. The manifest is a
    # checked artefact of this repository and a malformed one is a finding
    # about it; only the mid-merge case is "I was asked something I cannot
    # answer". The two are told apart by the markers and by nothing else.
    #
    # And this is HERE, not up with the counting modes, on purpose. The
    # counting modes answer before the manifest is read at all, precisely so
    # that `merge-gate.py` can ask for a count while holding a conflicted
    # manifest it is about to rewrite. Refusing up there would break the one
    # caller this refusal is about.
    try:
        text = MANIFEST.read_bytes().decode("utf-8")
    except UnicodeDecodeError as err:
        print(f"{MANIFEST}: not TOML: {err}", file=sys.stderr)
        return 1
    refuse_conflicted(MANIFEST, text)
    try:
        doc = tomllib.loads(text)
    except ValueError as err:
        print(f"{MANIFEST}: not TOML: {err}", file=sys.stderr)
        return 1

    entries = doc.get("fault") or []
    if not entries:
        print(f"{MANIFEST}: declares no faults, so it defines no parity", file=sys.stderr)
        return 1

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
        # A `test` fault with no target runs the whole workspace: every test
        # binary linked and every test run, to ask whether one gate fired.
        # That was the selftest's bill. Refused rather than defaulted, because
        # a default here is a cost nobody sees and nobody files.
        if kind == "seeded-gate" and entry.get("check") == "test":
            target = entry.get("target")
            if not isinstance(target, str) or not target.strip():
                failures.append(
                    f"{where}: a `test` fault must name the target its scope runs -- "
                    f"`lib`, `bins`, `all` or `test:NAME`, each optionally /FILTER"
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

    # A scope must not be able to satisfy the signature that grades it.
    #
    # verify.sh prints one line naming the scope before it runs the tests, and
    # the selftest greps the whole log for the case's signature. A signature
    # the scope line itself matches cannot tell "red for its own fault" from
    # "red for something else" -- the evidence would be in the log whatever
    # happened. It cannot manufacture a false RED, because the exit code is
    # still the verdict; it can hide a WRONG, which is the verdict this gate
    # was given a signature to be able to reach.
    #
    # THE FIRST VERSION OF THIS GUARD COULD NOT FIRE, and it is worth saying
    # exactly how, because the mistake is the one this repository names most
    # often. It reconstructed the line by hand as `... (1 test(s) selected)`.
    # No scope in the tree selects exactly one test -- the counts run 0, 2, 4,
    # 5, 9, 12 ... 235, 420 -- so for all 181 scoped faults it compared the
    # signature against a string that appears in no log the selftest has ever
    # produced. It passed for every input, which reads exactly like a guard
    # with nothing to complain about.
    #
    # Two things follow, and both are here. The template is READ OUT OF
    # verify.sh rather than copied, so there is one text; and the count is
    # tried across every value a run could print, because the count is the
    # part that varies and the part the hand-written copy froze.
    announce = ANNOUNCE.search(VERIFY.read_text(encoding="utf-8"))
    if not announce:
        failures.append(
            f"{VERIFY.name}: no `test scope:` line to grade signatures against. "
            f"Either check_test stopped announcing its scope -- in which case "
            f"this guard has nothing to do and should go -- or the wording moved "
            f"and this reader is now guarding a line nobody prints"
        )
    for ident, detail in sorted(DETAILS.items()):
        signature, scope = detail.get("legacy_signature"), detail.get("target")
        if not signature or not scope or not announce:
            continue
        try:
            pattern = re.compile(signature)
        except re.error as err:
            failures.append(f"`{ident}`: its signature is not a regex: {err}")
            continue
        # Every count a scope could announce. The largest in the tree today is
        # 420 -- the whole library -- so this range covers it with room, and a
        # signature that matches at ANY count is one the scope line can satisfy.
        hit = next(
            (n for n in range(0, 1000)
             if pattern.search(announce.group(1).replace("%s", scope).replace("%d", str(n)))),
            None,
        )
        if hit is not None:
            failures.append(
                f"`{ident}`: its signature /{signature}/ matches the line naming "
                f"its own scope when {hit} test(s) are selected, so the log carries "
                f"the signature whether or not the gate fired"
            )

    meta = doc.get("meta") or {}
    red = sum(len(seen[k]) for k in seen if k != "mechanics")
    if meta.get("red_faults") != red:
        failures.append(f"[meta] red_faults is {meta.get('red_faults')}, observed {red}")
    if meta.get("mechanics_assertions") != len(seen["mechanics"]):
        failures.append(
            f"[meta] mechanics_assertions is {meta.get('mechanics_assertions')}, "
            f"observed {len(seen['mechanics'])}"
        )

    # Asked for the count, answer the count. Deliberately before the failure
    # report: the caller is a tool that has just assembled the fault list and
    # is asking what `red_faults` must say, so the one failure it is about to
    # fix must not silence the answer.
    if counting:
        print(sum(len(seen[k]) for k in seen if k != "mechanics"))
        return 0

    for message in failures:
        print(message, file=sys.stderr)
    if failures:
        print(f"check-fault-manifest: {len(failures)} failure(s)", file=sys.stderr)
        return 1

    # Report the split by WHERE each kind is proven red, so the arithmetic
    # explains itself. `--selftest` reports 67; the manifest says 75 red; the
    # difference is the 8 subset fixtures, which go red on every run through
    # check_regimen rather than in the selftest.
    by_selftest = sum(len(seen[k]) for k in SELFTEST_KINDS)
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
