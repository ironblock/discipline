#!/usr/bin/env python3
"""Derive the selftest's shard split from measured per-fault cost, and check
the checked-in one against the manifest.

`verify.sh --selftest --shard K/N` divides the seeded faults between N CI
jobs. It used to divide them by ARITHMETIC -- every Nth fault, starting at the Kth --
which splits the count evenly and the cost not at all. A `test` fault rebuilds
the crate in a sandbox and costs seconds; a pattern class costs milliseconds;
and which of the two a round-robin hands a shard is decided by where in
verify.sh the declaration happens to sit. Measured on run 35415319419, the
slowest of eight shards took 414s and the fastest 277s, and the run came in at
7m27s against the five-minute budget .github/gate-budget.tsv declares -- the
shortest prompt-cache TTL any seat on this repository runs on. Every minute
past it is a cold prefill somebody pays for.

So the split is a HARVEST, in the same shape `derive-scopes.py` established:

    ./verify.sh --selftest --derive-shards DIR    # writes DIR/derive-shards.tsv
    python3 scripts/derive-shards.py --index DIR/derive-shards.tsv
    python3 scripts/derive-shards.py --index ... --emit > tools/gate/shards.tsv
    python3 scripts/derive-shards.py --check      # what CI runs, every PR

THE SHARD COUNT IS AN OUTPUT, NOT A CONSTANT. It is the smallest N whose
slowest shard fits in the budget once the overhead every shard pays is taken
off -- the runner's own (checkout, toolchain, apt), declared and measured in
.github/gate-budget.tsv, and the selftest's own (the sandbox, and the
mechanics assertions, which run unsharded in every shard), measured by the
harvest as whatever of the run is not a fault. It is bounded above by the
account's concurrency, because past that a shard queues instead of running and
the wall clock stops falling.

THE PACKING IS LONGEST-PROCESSING-TIME-FIRST: sort the faults by measured cost
descending, and give each to whichever shard is lightest so far. It is greedy
and it is not optimal -- no more than 4/3 of optimal, and in practice far
closer -- and optimal here is a bin-packing search over hundreds of items, for
a number that changes every time a fault is added. The approximation is measured against
the budget rather than against optimal, which is the only comparison that
decides anything, and `--index` prints the spread it achieved so the choice is
checkable rather than asserted.

`--check` grades what is checked in, on every run of the `ci` check, and asks
three things of it:

  * COVERAGE. Every fault the manifest declares has exactly one assignment, and
    no assignment names a fault that no longer exists. A fault nobody assigned
    is run by every shard or by none, and both are a census that adds up to the
    wrong list -- so a PR that adds a fault must re-derive before it is green,
    which is the drift #87 was filed against and could not otherwise recur.
  * BALANCE. No shard carries three times the median shard's measured cost. An
    assignment can be complete and still be a hand edit that moved forty faults
    into one job, and the run that finds that out is the run that overran.
  * SUBSTRATE. A harvest is a claim about the machine it was measured on, and
    only `harvest-shards.yml`'s own runner is the one this plan is checked
    against. A substrate that is PRESENT and wrong is refused, by name, same as
    a coverage or balance defect. A substrate that is ABSENT is a different
    claim -- a PLACEHOLDER nobody has re-harvested since this field existed --
    and `--check` prints that loudly rather than either refusing it (the
    chicken-and-egg `harvest-shards.yml` can only ever resolve by first landing
    through a PR this check would have blocked) or passing it silently (which
    would read as the same clean state a real harvest produces).

Exit 0 when the assignment covers the manifest, no shard is an outlier, and the
substrate is either the runner or a declared placeholder; 1 when it does not;
2 when the question cannot be answered at all.

THE READER IS EXERCISED BEFORE IT IS TRUSTED. `--selftest` runs the fixture
suite at the foot of this file and `verify.sh --only derive` is that suite,
for the reason `derive-scopes.py` gives in its own: the input this file exists
to read costs forty minutes to produce, so nothing will ever run it against a
case it has not already seen. Its sibling's first defect survived being
written, reviewed and read, and was found by a harvest and nothing else.
"""

from __future__ import annotations

import argparse
import contextlib
import io
import pathlib
import statistics
import subprocess
import sys
import tempfile

EXIT_BAD = 1
EXIT_BROKEN = 2

ROOT = pathlib.Path(__file__).resolve().parent.parent
MANIFEST_READER = ROOT / "scripts" / "check-fault-manifest.py"
BUDGET = ROOT / ".github" / "gate-budget.tsv"
PLAN = ROOT / "tools" / "gate" / "shards.tsv"

# The budget file's constants, and what each is for. Named here so that a file
# missing one is refused by name rather than read as a zero -- a zero budget
# derives the maximum shard count and a zero overhead derives too few, and
# neither announces itself.
BUDGET_KEYS = {
    "wall_clock_seconds": "how long a whole CI run may take",
    "runner_overhead_seconds": "what a shard pays around the selftest step",
    "max_shards": "the concurrency the account can actually run at once",
}

# A shard carrying this many times the median shard is not a rounding error in
# the packing; it is an assignment somebody edited, or a harvest taken against
# a different fault list. Three rather than two: the packer's own worst case
# over a list whose largest single fault is a sizeable fraction of a shard can
# legitimately leave one shard noticeably heavier, and a threshold that fires
# on the packer's own output is a threshold nobody can keep green.
OUTLIER_FACTOR = 3

# What GitHub Actions' `RUNNER_ENVIRONMENT` reports on a `runs-on: ubuntu-latest`
# hosted runner, as opposed to `"self-hosted"`. A harvest's plan is a claim
# about the runner's own timing; a harvest taken anywhere else is a claim about
# a different substrate wearing the runner's label, and `grade_substrate`
# refuses it by comparing against this constant rather than a hand-typed one.
RUNNER_SUBSTRATE = "github-hosted"


class Unusable(Exception):
    """The derivation cannot be run at all."""


# --------------------------------------------------------------------------
# reading
# --------------------------------------------------------------------------


def rows(path: pathlib.Path, what: str) -> list[tuple[int, list[str]]]:
    """The tab-separated, comment-stripped rows of `path`, with line numbers."""
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as err:
        raise Unusable(f"cannot read {what} {path}: {err}") from err
    found = []
    for number, line in enumerate(text.splitlines(), start=1):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        found.append((number, line.split("\t")))
    return found


def whole(value: str, where: str, what: str) -> int:
    """`value` as a non-negative integer, or a refusal naming where it was."""
    try:
        number = int(value)
    except ValueError:
        raise Unusable(f"{where}: {what} is {value!r}, which is not a number") from None
    if number < 0:
        raise Unusable(f"{where}: {what} is {number}, which is not a duration")
    return number


def read_budget(path: pathlib.Path) -> dict[str, int]:
    """The declared constants, every one of them required.

    A missing constant is refused rather than defaulted. The whole point of
    the file is that these numbers are DECLARED with their reasons; a default
    living in this script would be an undeclared budget that nothing states
    and nobody reviews.
    """
    found: dict[str, int] = {}
    for number, row in rows(path, "the budget"):
        key, value = row[0], (row[1] if len(row) > 1 else "")
        where = f"{path.name}:{number}"
        if key not in BUDGET_KEYS:
            raise Unusable(f"{where}: unknown constant {key!r}")
        if key in found:
            raise Unusable(f"{where}: {key!r} is declared twice")
        found[key] = whole(value, where, key)
    for key, why in BUDGET_KEYS.items():
        if key not in found:
            raise Unusable(
                f"{path.name} declares no {key} -- {why}. It is not defaulted "
                f"here: a budget nothing states is a budget nobody reviewed"
            )
    if found["wall_clock_seconds"] < 1 or found["max_shards"] < 1:
        raise Unusable(
            f"{path.name}: a budget of {found['wall_clock_seconds']}s across "
            f"{found['max_shards']} shard(s) permits no run at all"
        )
    return found


def read_index(path: pathlib.Path) -> tuple[dict[str, int], dict[str, str], int, str]:
    """The harvest: each fault's cost in ms, its label, the run's ms, and where.

    `fault<TAB>id<TAB>ms<TAB>label`, one `run<TAB>whole<TAB>ms<TAB>note`, and
    one `substrate<TAB>value<TAB>note`, which verify.sh writes in that order.
    """
    cost: dict[str, int] = {}
    label: dict[str, str] = {}
    run_ms: int | None = None
    substrate: str | None = None
    for number, row in rows(path, "the index"):
        kind = row[0]
        where = f"{path.name}:{number}"
        if kind == "fault":
            if len(row) < 3:
                raise Unusable(f"{where}: want fault, id, ms and label")
            ident = row[1]
            if ident in cost:
                raise Unusable(
                    f"{where}: {ident!r} was harvested twice, so nothing here "
                    f"says what it costs"
                )
            cost[ident] = whole(row[2], where, "the cost")
            label[ident] = row[3] if len(row) > 3 else ""
        elif kind == "run":
            if len(row) < 3:
                raise Unusable(f"{where}: want run, whole and ms")
            run_ms = whole(row[2], where, "the run's length")
        elif kind == "substrate":
            if len(row) < 2:
                raise Unusable(f"{where}: want substrate and value")
            substrate = row[1]
        else:
            raise Unusable(f"{where}: unknown row kind {kind!r}")
    if not cost:
        raise Unusable(
            f"{path} names no fault. A packing over nothing is not a packing, "
            f"and an empty index is what a `--derive-shards` run that never ran "
            f"looks like"
        )
    zero = sum(1 for measured in cost.values() if measured == 0)
    if zero * 2 > len(cost):
        raise Unusable(
            f"{path} measures zero cost for {zero} of {len(cost)} fault(s). "
            f"That is what a harvest taken with no clock looks like: below bash "
            f"5.0 there is no EPOCHREALTIME, `now_ms` falls back to whole "
            f"SECONDS, and any fault quicker than one second reads as free. A "
            f"packing over mostly-free faults is not packed against their real "
            f"cost. Re-harvest on bash 5 or later"
        )
    if run_ms is None:
        raise Unusable(
            f"{path} carries no `run whole` row, so nothing in it says what the "
            f"run cost beyond its faults -- which is the per-shard overhead the "
            f"shard count is derived from"
        )
    if substrate is None:
        raise Unusable(
            f"{path} carries no substrate row, so nothing says where it was "
            f"measured; re-harvest with an up-to-date verify.sh"
        )
    return cost, label, run_ms, substrate


def read_plan(path: pathlib.Path) -> tuple[int, dict[str, int], dict[str, int], int, str]:
    """The checked-in assignment: N, id -> shard, id -> harvested ms, overhead, substrate.

    The same file verify.sh reads, parsed the same way -- `key<TAB>value` for
    the scalars, `fault<TAB>id<TAB>shard<TAB>ms` for the assignments.

    `substrate` DEFAULTS TO EMPTY rather than being required, unlike
    `read_index`'s: this is the checked-in file, which predates the field, and
    a plan with no substrate row must still PARSE so `--check` can grade it and
    name the problem by `grade_substrate` -- refusing to read it at all would
    turn a graded, disclosed defect back into an unreadable file.
    """
    shards: int | None = None
    overhead_ms = 0
    substrate = ""
    assign: dict[str, int] = {}
    cost: dict[str, int] = {}
    for number, row in rows(path, "the assignment"):
        key = row[0]
        where = f"{path.name}:{number}"
        value = row[1] if len(row) > 1 else ""
        if key == "fault":
            if len(row) < 4:
                raise Unusable(f"{where}: want fault, id, shard and ms")
            ident = row[1]
            if ident in assign:
                raise Unusable(f"{where}: {ident!r} is assigned twice")
            assign[ident] = whole(row[2], where, "the shard")
            cost[ident] = whole(row[3], where, "the harvested cost")
        elif key == "shards":
            shards = whole(value, where, "the shard count")
        elif key == "overhead_ms":
            overhead_ms = whole(value, where, "the overhead")
        elif key == "harvest_ms":
            pass  # provenance; the overhead is what the derivation uses
        elif key == "substrate":
            substrate = value
        else:
            raise Unusable(f"{where}: unknown key {key!r}")
    if shards is None:
        raise Unusable(
            f"{path.name} declares no shard count, so nothing in it says how "
            f"many shards it was packed for"
        )
    if not assign:
        raise Unusable(f"{path.name} assigns no fault to any shard")
    return shards, assign, cost, overhead_ms, substrate


def manifest_ids() -> dict[str, str]:
    """Every fault `--selftest` runs, asked of the manifest's own reader.

    Asked, not recomputed. `check-fault-manifest.py` reads verify.sh's source,
    the lane applier's output and the fixture directories to decide what the
    selftest runs; a second reader of those three here would be free to
    disagree, and the disagreement would read as drift in the assignment.
    """
    try:
        done = subprocess.run(
            [sys.executable, str(MANIFEST_READER), "--list-selftest-red"],
            capture_output=True,
            text=True,
            check=True,
        )
    except (OSError, subprocess.CalledProcessError) as err:
        raise Unusable(f"cannot ask {MANIFEST_READER.name} which faults run: {err}") from err
    found: dict[str, str] = {}
    for line in done.stdout.splitlines():
        if not line.strip():
            continue
        ident, _, kind = line.partition("\t")
        found[ident] = kind
    if not found:
        raise Unusable(f"{MANIFEST_READER.name} named no fault at all")
    return found


# --------------------------------------------------------------------------
# packing
# --------------------------------------------------------------------------


def pack(cost: dict[str, int], shards: int) -> dict[str, int]:
    """Longest-processing-time-first: the heaviest fault to the lightest shard.

    Deterministic, which matters more here than it looks: the output is a
    checked-in file, and a packer that broke ties by dictionary order would
    produce a different file from the same harvest on a different run and make
    every diff unreadable. Ties break by id.
    """
    if shards < 1:
        raise Unusable(f"cannot pack into {shards} shard(s)")
    load = [0] * shards
    assign: dict[str, int] = {}
    for ident in sorted(cost, key=lambda name: (-cost[name], name)):
        lightest = min(range(shards), key=lambda index: (load[index], index))
        assign[ident] = lightest + 1
        load[lightest] += cost[ident]
    return assign


def totals(assign: dict[str, int], cost: dict[str, int], shards: int) -> list[int]:
    """What each shard carries, 1..shards, as a list indexed from zero."""
    summed = [0] * shards
    for ident, shard in assign.items():
        if 1 <= shard <= shards:
            summed[shard - 1] += cost.get(ident, 0)
    return summed


def derive_count(cost: dict[str, int], overhead_ms: int, budget: dict[str, int]) -> int:
    """The smallest shard count whose slowest shard fits in the budget.

    Returns the ceiling when nothing fits, rather than refusing: "the budget
    cannot be met at any count this account can run" is a fact to report, not
    a reason to refuse to produce a split. It is reported by the caller.
    """
    budget_ms = budget["wall_clock_seconds"] * 1000
    fixed = overhead_ms + budget["runner_overhead_seconds"] * 1000
    for shards in range(1, budget["max_shards"] + 1):
        if fixed + max(totals(pack(cost, shards), cost, shards)) <= budget_ms:
            return shards
    return budget["max_shards"]


def slowest(assign: dict[str, int], cost: dict[str, int], shards: int, overhead_ms: int,
            budget: dict[str, int]) -> tuple[int, int]:
    """The slowest shard's whole cost in ms, and the budget in ms."""
    fixed = overhead_ms + budget["runner_overhead_seconds"] * 1000
    return fixed + max(totals(assign, cost, shards)), budget["wall_clock_seconds"] * 1000


# --------------------------------------------------------------------------
# grading
# --------------------------------------------------------------------------


def grade_coverage(declared: dict[str, str], assign: dict[str, int]) -> list[str]:
    """Every declared fault assigned once, and nothing assigned that is gone."""
    failures = []
    missing = sorted(set(declared) - set(assign))
    if missing:
        failures.append(
            f"{len(missing)} fault(s) the manifest declares have no shard: "
            + ", ".join(missing[:10])
            + (" ..." if len(missing) > 10 else "")
            + ". A fault no shard is assigned is run by every shard or by none. "
            + "Re-harvest: ./verify.sh --selftest --derive-shards DIR"
        )
    stray = sorted(set(assign) - set(declared))
    if stray:
        failures.append(
            f"{len(stray)} assignment(s) name a fault the manifest does not "
            f"declare: "
            + ", ".join(stray[:10])
            + (" ..." if len(stray) > 10 else "")
            + ". The assignment is about a fault list that no longer exists"
        )
    return failures


def grade_harvest(declared: dict[str, str], cost: dict[str, int]) -> list[str]:
    """The harvest measured this tree's fault list, and only it.

    Separate from `grade_coverage` because the two refusals mean different
    things to whoever reads them. An assignment missing a fault is drift to
    re-derive; a HARVEST missing one is a run that did not measure everything
    it ran, and the answer is to run it again rather than to edit anything.
    """
    failures = []
    missing = sorted(set(declared) - set(cost))
    if missing:
        failures.append(
            f"{len(missing)} fault(s) the manifest declares were not measured: "
            + ", ".join(missing[:10])
            + (" ..." if len(missing) > 10 else "")
            + ". A cost the packer does not have is a cost it packs as zero"
        )
    stray = sorted(set(cost) - set(declared))
    if stray:
        failures.append(
            f"{len(stray)} measured fault(s) are not in the manifest: "
            + ", ".join(stray[:10])
            + (" ..." if len(stray) > 10 else "")
            + ". The harvest is of a tree this one is not"
        )
    return failures


def grade_shape(shards: int, assign: dict[str, int], budget: dict[str, int]) -> list[str]:
    """The assignment is a partition into exactly the shards it claims."""
    failures = []
    if shards > budget["max_shards"]:
        failures.append(
            f"the assignment is packed for {shards} shards, past the "
            f"{budget['max_shards']} the account can run at once; the shards "
            f"past that queue rather than run, so the wall clock stops falling"
        )
    outside = sorted(i for i in set(assign.values()) if i < 1 or i > shards)
    if outside:
        failures.append(
            f"shard number(s) {outside} appear in an assignment packed for "
            f"{shards}; the faults in them are run by no job"
        )
    empty = [i for i in range(1, shards + 1) if i not in set(assign.values())]
    if empty:
        failures.append(
            f"shard(s) {empty} of {shards} are assigned no fault. A job that "
            f"runs no seeded case exits 2 rather than reporting a pass, so this "
            f"is a red build and not a spare runner"
        )
    return failures


def grade_substrate(substrate: str) -> list[str]:
    """A WRONG substrate is refused; NO substrate is a declared placeholder.

    Ruled on #87 (2026-09-24), resolving the chicken-and-egg `--check` itself
    created: `harvest-shards.yml` can only ever reach the default branch --
    where `workflow_dispatch` lists it at all -- by landing through a PR, and
    the `ci` check would have refused that very PR for the one thing it exists
    to fix. So an EMPTY substrate (no harvest has ever run against this field)
    is not graded here at all: it is the state of a plan nobody has re-derived
    since this field was added, no different in kind from gate 0's census
    before the first results directory, and `main()`'s `--check` branch prints
    it as a named placeholder rather than folding it into this function's
    failures.

    A substrate that is PRESENT and WRONG is a different claim entirely: a
    shard split derived from timings measured on a laptop, a self-hosted box,
    or anywhere but the CI runner is a claim about the runner made on a
    different machine -- the label-versus-value class this check exists to
    catch, the same as a fault's cost claiming to belong to a manifest it does
    not -- and that is refused, by name, same as ever.
    """
    if not substrate or substrate == RUNNER_SUBSTRATE:
        return []
    return [
        f"the plan was harvested on {substrate!r}, not {RUNNER_SUBSTRATE!r} -- "
        f"the CI runner. Harvest it there instead: dispatch the harvest-shards "
        f"workflow"
    ]


def grade_balance(shards: int, assign: dict[str, int], cost: dict[str, int]) -> list[str]:
    """No shard carries OUTLIER_FACTOR times the median shard's cost.

    Against the MEDIAN rather than the mean: the mean is dragged up by the
    outlier itself, so a single shard carrying half the list makes the mean
    large enough that the shard no longer looks unusual against it. That is
    the one case this exists to catch.
    """
    summed = totals(assign, cost, shards)
    middle = statistics.median(summed)
    if middle <= 0:
        # Every shard measured zero. That is a harvest with no costs in it, not
        # an imbalance, and it is the index reader's to refuse; calling it an
        # outlier here would divide a judgment by nothing.
        return []
    failures = []
    for index, carried in enumerate(summed, start=1):
        if carried > OUTLIER_FACTOR * middle:
            failures.append(
                f"shard {index} of {shards} carries {carried / 1000:.1f}s of "
                f"harvested cost, {carried / middle:.1f}x the median shard's "
                f"{middle / 1000:.1f}s. A split that uneven is not what the "
                f"packer produces; it is an assignment that was edited, or one "
                f"harvested against a different fault list"
            )
    return failures


# --------------------------------------------------------------------------
# emitting
# --------------------------------------------------------------------------


def emit(cost: dict[str, int], run_ms: int, budget: dict[str, int], substrate: str) -> str:
    """The whole of tools/gate/shards.tsv, from one harvest.

    A pure function of the harvest and the budget, deliberately: re-emitting
    from the same index gives the same bytes, so a diff on this file is a diff
    in what was measured and never in when it was measured.
    """
    overhead_ms = max(0, run_ms - sum(cost.values()))
    shards = derive_count(cost, overhead_ms, budget)
    assign = pack(cost, shards)
    summed = totals(assign, cost, shards)
    fixed = overhead_ms + budget["runner_overhead_seconds"] * 1000
    worst = max(summed) + fixed
    budget_ms = budget["wall_clock_seconds"] * 1000
    fits = worst <= budget_ms
    out = [
        "# Which shard runs which fault. HARVESTED, NOT WRITTEN.",
        "#",
        "# Generated by:",
        "#   ./verify.sh --selftest --derive-shards DIR",
        "#   python3 scripts/derive-shards.py --index DIR/derive-shards.tsv --emit \\",
        "#     > tools/gate/shards.tsv",
        "#",
        "# Readers: verify.sh's `in_shard`, which runs a fault when this file",
        "# gives it this job's number, and scripts/derive-shards.py --check,",
        "# which refuses an assignment that has drifted from the manifest.",
        "#",
        "# Its own file rather than a field in faults.toml: an entry there is",
        "# author-facing and stable -- a label, a signature, a failure class",
        "# somebody wrote -- while this is a machine-rebalanced artefact that",
        "# changes on every manifest edit. Folding it in would make faults.toml",
        "# diff on every entry it holds each time one fault was added, and the",
        "# one hand-written line in that diff would be the one nobody could find.",
        "#",
        f"# Packed longest-processing-time-first into {shards} shard(s) -- "
        + (
            f"the smallest count whose"
            if fits
            else f"THE CEILING (`max_shards`); no count up to it"
        ),
        f"# slowest shard fits {budget['wall_clock_seconds']}s once the "
        f"{fixed / 1000:.0f}s",
        f"# every shard pays regardless is taken off -- "
        f"{overhead_ms / 1000:.0f}s of unsharded",
        f"# selftest measured by the harvest, plus "
        f"{budget['runner_overhead_seconds']}s of runner declared",
        "# in .github/gate-budget.tsv."
        + (
            ""
            if fits
            else f" At {shards} shards the slowest is still {worst / 1000:.0f}s"
            f" against the {budget['wall_clock_seconds']}s budget on these"
            f" numbers -- OVER by {(worst - budget_ms) / 1000:.0f}s. This is"
            f" not a derivation; it is the ceiling reported because nothing"
            f" under it fits. A harvest on the runner is what settles"
            f" whether {shards} is enough, and it may need to be higher"
            f" than `max_shards` currently allows."
        ),
        "#",
        "# A HARVEST IS MACHINE-RELATIVE, and the two halves of it travel",
        "# differently. The BALANCE rests on the costs relative to each other,",
        "# and those hold wherever it was measured: a fault that rebuilds the",
        "# crate is dear on every machine. The COUNT rests on the absolute",
        "# seconds against a budget in absolute seconds, and those do not: a",
        "# harvest taken somewhere slower than the runner derives more shards",
        "# than the runner needs. That is the safe direction -- runner-minutes",
        "# are unbilled here and a shard too many costs nothing but admission",
        "# skew -- but a count derived from a run on the runner is the right",
        "# one, and re-harvesting there is what produces it.",
        "#",
        f"# Harvested: {len(cost)} fault(s), {run_ms / 1000:.0f}s end to end.",
        f"# Harvested on `{substrate}`.",
        f"# Slowest shard {max(summed) / 1000:.0f}s of fault cost, fastest "
        f"{min(summed) / 1000:.0f}s -- a spread of "
        f"{(max(summed) - min(summed)) / max(1, min(summed)) * 100:.1f}%.",
        f"# With overhead the slowest is {(max(summed) + fixed) / 1000:.0f}s "
        f"against the {budget['wall_clock_seconds']}s budget, on these numbers.",
        "#",
        "# Per shard (fault cost only):",
    ]
    for index, carried in enumerate(summed, start=1):
        held = sum(1 for shard in assign.values() if shard == index)
        out.append(f"#   {index:>3}  {carried / 1000:7.1f}s  {held:>4} fault(s)")
    out += [
        "",
        f"shards\t{shards}",
        f"overhead_ms\t{overhead_ms}",
        f"harvest_ms\t{run_ms}",
        f"substrate\t{substrate}",
        "",
        "# fault<TAB>id<TAB>shard<TAB>harvested milliseconds",
        "#",
        "# Sorted by id, not by shard or by cost: every row's cost is",
        "# re-measured on every re-harvest, so the diff touches every line",
        "# regardless -- a stable order at least keeps each fault on the SAME",
        "# line across harvests, so the diff is by fault rather than a reshuffle",
        "# of the whole file, and the shard column is where a real move reads.",
    ]
    for ident in sorted(cost):
        out.append(f"fault\t{ident}\t{assign[ident]}\t{cost[ident]}")
    return "\n".join(out) + "\n"


# --------------------------------------------------------------------------
# the fixture suite
# --------------------------------------------------------------------------
#
# What each fixture holds is a BEHAVIOUR of the reader, named for the thing
# that goes wrong when the behaviour is missing. The input this file reads is
# forty minutes away; a fixture is not.

FIXTURES: list[tuple[str, object]] = []


def fixture(name: str):
    def take(fn):
        FIXTURES.append((name, fn))
        return fn

    return take


BUDGET_FIXTURE = {
    "wall_clock_seconds": 300,
    "runner_overhead_seconds": 60,
    "max_shards": 16,
}


@fixture("the packer balances cost, not count")
def _packs_by_cost():
    # THE WHOLE POINT. Round-robin gives each shard the same NUMBER of faults;
    # what CI waits on is the same COST. One expensive fault beside many cheap
    # ones is the shape the fault list actually has -- a `test` case rebuilds
    # the crate, a pattern class runs a grep -- and a packer that got this
    # wrong would reproduce the 58% spread it exists to remove.
    cost = {"big": 100, **{f"small{n}": 10 for n in range(10)}}
    assign = pack(cost, 2)
    summed = totals(assign, cost, 2)
    if sorted(summed) != [100, 100]:
        return f"packed {summed}, not two equal halves"
    # And the count is deliberately NOT equal: eleven faults, and the shard
    # holding the expensive one holds only it.
    held = sorted(sum(1 for s in assign.values() if s == k) for k in (1, 2))
    if held != [1, 10]:
        return f"split the count {held}, which means it balanced the wrong thing"
    return None


@fixture("the same harvest packs the same way twice")
def _packing_is_deterministic():
    # The output is a CHECKED-IN FILE. A packer that broke ties by whatever
    # order a dict happened to iterate in would produce different bytes from
    # the same measurements, and every re-derive would diff on every row
    # with nothing having changed.
    cost = {f"f{n}": 10 for n in range(20)}
    if pack(cost, 4) != pack(dict(reversed(list(cost.items()))), 4):
        return "two orderings of one harvest packed differently"
    return None


@fixture("the shard count is derived from the budget, not chosen")
def _count_is_derived():
    # A constant here is the defect #87 was filed against: eight was right when
    # it was written and wrong two hundred faults later, and nothing said so.
    budget = dict(BUDGET_FIXTURE)          # 300s, 60s of runner overhead
    cheap = {f"f{n}": 1000 for n in range(10)}          # 10s of faults
    if derive_count(cheap, 0, budget) != 1:
        return f"10s of faults derived {derive_count(cheap, 0, budget)} shards"
    dear = {f"f{n}": 1000 for n in range(1000)}         # 1000s of faults
    got = derive_count(dear, 0, budget)
    if got != 5:                                        # 240s usable, 200s each
        return f"1000s of faults derived {got} shards, not 5"
    # The overhead comes off the top and is per shard, so raising it raises the
    # count. An overhead subtracted once for the whole run would not.
    if derive_count(dear, 60_000, budget) <= got:
        return "a minute of per-shard overhead did not cost a single shard"
    return None


@fixture("the count stops at the concurrency the account has")
def _count_is_bounded():
    # Past the concurrency limit a shard queues rather than runs, so the wall
    # clock stops falling and the packer is splitting for nothing. Returning
    # the ceiling rather than refusing: "this cannot fit" is a fact to report.
    budget = dict(BUDGET_FIXTURE, max_shards=4)
    huge = {f"f{n}": 10_000 for n in range(100)}        # 1000s, can never fit
    if derive_count(huge, 0, budget) != 4:
        return f"derived {derive_count(huge, 0, budget)} shards past a ceiling of 4"
    return None


@fixture("a fault the manifest declares and nothing assigns is refused")
def _missing_fault_is_refused():
    # THE DRIFT THIS EXISTS FOR. A PR adds a fault; the assignment predates it;
    # the fault is then in no shard's list and in every shard's census total.
    # Silently rebalancing it would be worse than refusing -- the rebalance
    # would be against a cost nobody measured.
    declared = {"a": "seeded-gate", "b": "seeded-gate", "c": "seeded-gate"}
    found = grade_coverage(declared, {"a": 1, "b": 2})
    if not found:
        return "an unassigned fault was accepted"
    if "c" not in found[0]:
        return f"the refusal does not name the fault: {found[0]!r}"
    return None


@fixture("an assignment naming a retired fault is refused")
def _stray_fault_is_refused():
    # The other direction, and not cosmetic: the row carries a COST, and the
    # packing that produced the file counted it. An assignment holding costs
    # for faults that no longer run is balanced for a list nobody runs.
    found = grade_coverage({"a": "seeded-gate"}, {"a": 1, "gone": 2})
    if not found:
        return "an assignment for a fault the manifest does not declare was accepted"
    if "gone" not in found[0]:
        return f"the refusal does not name the stray assignment: {found[0]!r}"
    return None


@fixture("a shard carrying three times the median is refused, by name")
def _outlier_is_refused():
    # An assignment can cover the manifest exactly and still be a hand edit
    # that put half the list in one job. Coverage cannot see that; the run that
    # finds it out is the run that overran.
    cost = {f"f{n}": 100 for n in range(30)}
    assign = {f"f{n}": (1 if n < 24 else 2 + (n % 3)) for n in range(30)}
    found = grade_balance(4, assign, cost)
    if not found:
        return "a shard with twenty-four of thirty faults was accepted"
    if "shard 1 of 4" not in found[0]:
        return f"the refusal does not name the shard: {found[0]!r}"
    return None


@fixture("the outlier is measured against the median and not the mean")
def _median_not_mean():
    # The mean is dragged up by the outlier itself. Four shards at 10 and one
    # at 400 have a mean of 88, so the outlier is 4.5x the mean -- but add a
    # second outlier and the mean rises until neither looks unusual. The median
    # does not move.
    cost = {"a": 400, "b": 400, "c": 10, "d": 10, "e": 10}
    assign = {"a": 1, "b": 2, "c": 3, "d": 4, "e": 5}
    found = grade_balance(5, assign, cost)
    if len(found) != 2:
        return f"two outliers beside three small shards produced {len(found)} finding(s)"
    return None


@fixture("an ordinary packing is not called an outlier")
def _packer_output_passes():
    # A threshold that fires on what the packer itself produces is a threshold
    # nobody can keep green, and a check nobody can keep green is one that gets
    # deleted. Driven with the real packer rather than a hand-built assignment.
    cost = {f"f{n}": 1 + (n * 7) % 40 for n in range(200)}
    assign = pack(cost, 6)
    found = grade_balance(6, assign, cost)
    if found:
        return f"the packer's own output was refused: {found[0]!r}"
    return None


@fixture("a harvest of nothing but zeroes is not read as an imbalance")
def _zero_costs_do_not_divide():
    # Every shard measures zero when a harvest was taken with no clock. That is
    # a broken harvest -- the index reader's to refuse -- and dividing by the
    # median here would raise a ZeroDivisionError out of a check, which reads
    # as the checker being broken rather than the input.
    cost = {f"f{n}": 0 for n in range(10)}
    if grade_balance(2, {f"f{n}": 1 + n % 2 for n in range(10)}, cost):
        return "an all-zero harvest was reported as an imbalance"
    return None


@fixture("an empty shard is refused")
def _empty_shard_is_refused():
    # `--selftest --shard K/N` over an empty shard exits 2 rather than
    # reporting a pass, so an assignment that leaves one empty is a red build.
    # Caught here, where the message can say which shard, rather than in CI.
    found = grade_shape(4, {"a": 1, "b": 2, "c": 2}, BUDGET_FIXTURE)
    if not found or "[3, 4]" not in found[0]:
        return f"two empty shards were reported as {found!r}"
    # And a shard number the plan was not packed for.
    outside = grade_shape(2, {"a": 1, "b": 2, "c": 5}, BUDGET_FIXTURE)
    if not any("5" in message for message in outside):
        return f"a fault assigned to shard 5 of 2 was reported as {outside!r}"
    return None


@fixture("a plan not harvested on the runner is refused, by name")
def _wrong_substrate_is_refused_by_name():
    # Defect 1's ruling on #87 (2026-09-23): re-harvest on the runner, and make
    # that the only place a harvest is valid. A shard plan derived from timings
    # taken anywhere else is a claim about the runner made on a different
    # substrate -- the label-versus-value class this whole file exists to
    # catch -- so it must be refused BY NAME, the same as a stray or missing
    # fault is, rather than trusted because the file otherwise parses.
    found = grade_substrate("self-hosted")
    if not found or "self-hosted" not in found[0]:
        return f"a plan harvested on 'self-hosted' was reported as {found!r}"
    if grade_substrate(RUNNER_SUBSTRATE):
        return "a plan harvested on the runner was refused"
    return None


@fixture("an undeclared substrate is a placeholder, not a refusal")
def _no_substrate_is_a_placeholder_not_a_refusal():
    # The chicken-and-egg ruling on #87 (2026-09-24): refusing an EMPTY
    # substrate here would have kept this PR's own `ci` check red forever,
    # since `harvest-shards.yml` can only become dispatchable by first
    # landing through a PR this check would have blocked. A substrate that
    # is PRESENT and wrong is still refused above -- only "never declared"
    # reads as the placeholder state, not "declared wrong".
    found = grade_substrate("")
    if found:
        return f"an undeclared substrate was graded a failure: {found!r}"
    return None


@fixture("the overhead is what the run cost beyond its faults")
def _overhead_is_measured():
    # Per shard, and measured rather than guessed: it is the sandbox, and the
    # mechanics assertions, which run unsharded in EVERY shard. Guessing it
    # low derives too few shards and the budget is missed by exactly the
    # amount of the guess.
    index = (
        "fault\ta\t1000\tone\nfault\tb\t2000\ttwo\nrun\twhole\t9000\tthe lot\n"
        "substrate\tgithub-hosted\ttest\n"
    )
    with tempfile.TemporaryDirectory() as box:
        path = pathlib.Path(box) / "harvest.tsv"
        path.write_text(index, encoding="utf-8")
        cost, _label, run_ms, substrate = read_index(path)
        if run_ms - sum(cost.values()) != 6000:
            return f"read an overhead of {run_ms - sum(cost.values())}ms, not 6000"
        text = emit(cost, run_ms, BUDGET_FIXTURE, substrate)
        if "overhead_ms\t6000" not in text:
            return "the emitted plan does not carry the measured overhead"
    return None


@fixture("a harvest with no run total is refused")
def _harvest_needs_its_total():
    # Without it the overhead is unknown, and an unknown overhead defaults to
    # zero, which derives a shard count that fits a budget nobody is paying.
    with tempfile.TemporaryDirectory() as box:
        path = pathlib.Path(box) / "harvest.tsv"
        path.write_text("fault\ta\t1000\tone\n", encoding="utf-8")
        try:
            read_index(path)
        except Unusable:
            pass
        else:
            return "an index with no `run whole` row was read as a harvest"
        path.write_text("run\twhole\t10\tnothing ran\n", encoding="utf-8")
        try:
            read_index(path)
        except Unusable:
            return None
        return "an index naming no fault was read as a harvest"


@fixture("a harvest of another tree's fault list is refused")
def _harvest_is_of_this_tree():
    # A harvest and an assignment can each be incomplete, and the two refusals
    # mean different things: an assignment missing a fault is drift to
    # re-derive, a harvest missing one is a run to take again. Separate
    # messages, so the operator is not told to edit a file when the answer is
    # to measure again.
    declared = {"a": "seeded-gate", "b": "seeded-gate"}
    missing = grade_harvest(declared, {"a": 10})
    if not missing or "b" not in missing[0] or "not measured" not in missing[0]:
        return f"an unmeasured fault was reported as {missing!r}"
    stray = grade_harvest(declared, {"a": 10, "b": 10, "old": 10})
    if not stray or "old" not in stray[0]:
        return f"a measurement of a retired fault was reported as {stray!r}"
    if grade_harvest(declared, {"a": 10, "b": 10}):
        return "a complete harvest was refused"
    return None


@fixture("a fault harvested twice is refused")
def _double_harvest_is_refused():
    # Two rows for one id is an index appended to across two runs, and the two
    # costs are about two different trees. Taking either silently is a packing
    # against a measurement nobody can point at.
    with tempfile.TemporaryDirectory() as box:
        path = pathlib.Path(box) / "harvest.tsv"
        path.write_text(
            "fault\ta\t1000\tone\nfault\ta\t9000\tone again\nrun\twhole\t10000\tx\n"
            "substrate\tgithub-hosted\ttest\n",
            encoding="utf-8",
        )
        try:
            read_index(path)
        except Unusable:
            return None
        return "an id harvested twice was read as one fault"


@fixture("a harvest taken with no clock is refused, not packed")
def _clockless_harvest_is_refused():
    # Below bash 5.0 there is no EPOCHREALTIME and `now_ms` falls back to whole
    # SECONDS, so any fault quicker than one second measures zero. A harvest
    # mostly of zeroes is not a balanced packing away from what it looks like --
    # it is a broken harvest, and it is this reader's to refuse rather than the
    # packer's to divide by.
    with tempfile.TemporaryDirectory() as box:
        path = pathlib.Path(box) / "harvest.tsv"
        rows = [f"fault\tf{n}\t{0 if n < 6 else 1000}\tf{n}\n" for n in range(10)]
        path.write_text(
            "".join(rows) + "run\twhole\t9000\tx\nsubstrate\tgithub-hosted\ttest\n",
            encoding="utf-8",
        )
        try:
            read_index(path)
        except Unusable as err:
            if "zero cost" not in str(err):
                return f"refused for the wrong reason: {err}"
            return None
        return "a harvest measuring zero for 6 of 10 faults was read as sound"


@fixture("a harvest with only a minority of zeroes is not refused")
def _a_few_real_zeroes_are_not_a_clockless_harvest():
    # A handful of faults can legitimately cost nothing even with a clock --
    # this must not fire on ordinary variance, only on the majority-zero shape
    # a clockless harvest actually produces.
    with tempfile.TemporaryDirectory() as box:
        path = pathlib.Path(box) / "harvest.tsv"
        rows = [f"fault\tf{n}\t{0 if n < 2 else 1000}\tf{n}\n" for n in range(10)]
        path.write_text(
            "".join(rows) + "run\twhole\t9000\tx\nsubstrate\tgithub-hosted\ttest\n",
            encoding="utf-8",
        )
        read_index(path)
    return None


@fixture("a harvest with no declared substrate is refused")
def _harvest_needs_a_substrate():
    # THE PROVENANCE IS PART OF THE HARVEST, not asserted after the fact. A
    # cost index that names its faults and their timing but not where it was
    # taken is exactly what a harvest from before this field existed looks
    # like -- and, unrefused, it is indistinguishable from a runner harvest to
    # everything downstream of this function.
    with tempfile.TemporaryDirectory() as box:
        path = pathlib.Path(box) / "harvest.tsv"
        path.write_text(
            "fault\ta\t1000\tone\nrun\twhole\t2000\tthe lot\n", encoding="utf-8",
        )
        try:
            read_index(path)
        except Unusable as err:
            if "substrate" not in str(err):
                return f"refused for the wrong reason: {err}"
            return None
        return "an index with no substrate row was read as a harvest"


@fixture("a budget declaring nothing is refused rather than defaulted")
def _budget_is_required():
    # A default in this file would be an undeclared budget: a number nothing
    # states, nobody reviewed, and no comment explains. The five minutes is a
    # cache TTL somebody measured, and it belongs beside the reason.
    with tempfile.TemporaryDirectory() as box:
        path = pathlib.Path(box) / "budget.tsv"
        path.write_text("# only a comment\n", encoding="utf-8")
        try:
            read_budget(path)
        except Unusable as err:
            if "wall_clock_seconds" not in str(err):
                return f"the refusal does not name what is missing: {err}"
        else:
            return "a budget file declaring nothing was read as a budget"
        path.write_text(
            "wall_clock_seconds\t300\nrunner_overhead_seconds\t60\n"
            "max_shards\t16\nwhat_even\t1\n",
            encoding="utf-8",
        )
        try:
            read_budget(path)
        except Unusable:
            pass
        else:
            return "an unknown constant was read past rather than refused"
        path.write_text(
            "wall_clock_seconds\tsoon\nrunner_overhead_seconds\t60\nmax_shards\t16\n",
            encoding="utf-8",
        )
        try:
            read_budget(path)
        except Unusable:
            return None
        return "a budget of 'soon' was read as a number"


@fixture("what --emit produces is what --check accepts")
def _emit_round_trips():
    # The two halves are written together and can drift apart together. A
    # packer whose output its own checker refuses is a repository that cannot
    # be made green, and the harvest that would find that out is forty minutes
    # long.
    cost = {f"f{n}": 1 + (n * 13) % 90 for n in range(120)}
    run_ms = sum(cost.values()) + 5000
    with tempfile.TemporaryDirectory() as box:
        path = pathlib.Path(box) / "shards.tsv"
        path.write_text(emit(cost, run_ms, BUDGET_FIXTURE, "github-hosted"), encoding="utf-8")
        shards, assign, back, overhead, substrate = read_plan(path)
        if back != cost:
            return "the costs did not survive the round trip"
        if overhead != 5000:
            return f"the overhead came back as {overhead}, not 5000"
        found = (grade_coverage({i: "seeded-gate" for i in cost}, assign)
                 + grade_shape(shards, assign, BUDGET_FIXTURE)
                 + grade_balance(shards, assign, back)
                 + grade_substrate(substrate))
        if found:
            return f"--check refused what --emit produced: {found[0]!r}"
        # Byte-for-byte on a second emit, because the file is checked in.
        if emit(cost, run_ms, BUDGET_FIXTURE, "github-hosted") != path.read_text(encoding="utf-8"):
            return "emitting the same harvest twice produced two files"
    return None


@fixture("--matrix prints the plan's shard numbers as JSON")
def _matrix_prints_the_range():
    cost = {f"f{n}": 1 + (n * 13) % 90 for n in range(120)}
    run_ms = sum(cost.values()) + 5000
    with tempfile.TemporaryDirectory() as box:
        path = pathlib.Path(box) / "shards.tsv"
        path.write_text(emit(cost, run_ms, BUDGET_FIXTURE, "github-hosted"), encoding="utf-8")
        shards, _assign, _cost, _overhead, _substrate = read_plan(path)
        text = matrix_of(path)
        if text != "[" + ", ".join(str(n) for n in range(1, shards + 1)) + "]":
            return f"the matrix does not name every shard 1..{shards}: {text!r}"
    return None


@fixture("--matrix refuses a plan of zero shards rather than printing []")
def _matrix_refuses_an_empty_range():
    # `range(1, 1)` is empty, so an ungated print would emit `[]` -- a matrix
    # of no jobs, which GitHub Actions runs as a success with nothing done.
    # That is worse than a job that fails: nothing files a census at all.
    with tempfile.TemporaryDirectory() as box:
        path = pathlib.Path(box) / "shards.tsv"
        path.write_text(
            "shards\t0\nfault\ta\t1\t100\n", encoding="utf-8",
        )
        try:
            matrix_of(path)
        except Unusable as err:
            if "0 shard" not in str(err):
                return f"refused for the wrong reason: {err}"
            return None
        return "a plan of zero shards printed a matrix rather than refusing"


@fixture("the exit codes say which of the three things happened")
def _exit_codes_are_distinct():
    # The entry point, driven end to end. Every leg above can be right while
    # main() returns 0 for all of them -- which is the defect
    # `check-merge-gate.py` records having shipped with.
    # THE IDS ARE THE REAL ONES, with invented costs. `--index` refuses a
    # harvest whose faults are not the manifest's, which is the behaviour that
    # stops an incomplete harvest becoming an assignment -- so a fixture driving
    # it with `f0`..`f39` would prove that main() exits 1, not that it exits 0
    # on a sound harvest, and the sound path is the one nothing else covers.
    declared = sorted(manifest_ids())
    with tempfile.TemporaryDirectory() as box:
        room = pathlib.Path(box)
        index = room / "harvest.tsv"
        index.write_text(
            "".join(
                f"fault\t{ident}\t{100 + (n * 37) % 900}\tcase {n}\n"
                for n, ident in enumerate(declared)
            )
            + "run\twhole\t9000\tthe lot\n"
            + "substrate\tgithub-hosted\ttest\n",
            encoding="utf-8",
        )

        def drive(*argv: str) -> tuple[int, str, str]:
            out, err = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                code = main(list(argv))
            return code, out.getvalue(), err.getvalue()

        code, out, err = drive("--index", str(index))
        if code != 0:
            return f"a sound index did not exit 0: {err!r}"
        if f"{len(declared)} fault(s)" not in out:
            return f"the report does not say what it read: {out!r}"
        code, out, err = drive("--index", str(index), "--emit")
        if code != 0:
            return f"emitting from a sound index did not exit 0: {err!r}"
        if "\nshards\t" not in out:
            return f"--emit printed no shard count: {out[:200]!r}"

        # And the refusal the fixture's own ids exist to leave reachable: one
        # fault dropped from the harvest is not a harvest of this tree.
        short = room / "short.tsv"
        short.write_text(
            "\n".join(index.read_text(encoding="utf-8").splitlines()[1:]) + "\n",
            encoding="utf-8",
        )
        code, _, err = drive("--index", str(short), "--emit")
        if code != EXIT_BAD:
            return f"a harvest missing a declared fault exited {code}, not {EXIT_BAD}"
        if declared[0] not in err:
            return f"the refusal does not name the fault it is missing: {err!r}"

        gone = room / "nowhere.tsv"
        if drive("--index", str(gone))[0] != EXIT_BROKEN:
            return f"an index that does not exist did not exit {EXIT_BROKEN}"
        if drive("--selftest", "--emit")[0] != EXIT_BROKEN:
            return f"--selftest beside --emit did not exit {EXIT_BROKEN}"
        if drive()[0] != EXIT_BROKEN:
            return "no mode at all did not exit 2"
        if drive("--index", str(index), "--check")[0] != EXIT_BROKEN:
            return "--check beside --index did not exit 2; they read different files"
    return None


@fixture("--check reads the repository's own assignment and passes on it")
def _the_committed_plan_is_sound():
    # THE ONE FIXTURE WITH A REAL INPUT. Everything above is synthetic, and a
    # suite of synthetic cases can pass while the file that is actually checked
    # in has drifted. This is the same call CI makes, against the same two
    # files, so a manifest edit with no re-harvest is red here and not only
    # there.
    #
    # `code == 0` even though `tools/gate/shards.tsv` predates the substrate
    # field and correctly carries none. Per the ruling on #87's chicken-and-egg
    # (2026-09-24), an undeclared substrate is a PLACEHOLDER, not a failure --
    # `grade_substrate` returns no failures for it, so this assertion is
    # generic (it would equally pass once a real harvest lands) rather than
    # hardcoded to today's placeholder state. What this fixture must still
    # catch is any OTHER defect, so it grades against the plan's OWN declared
    # substrate rather than assuming "none": a coverage, shape or balance
    # regression -- or a real but WRONG substrate -- still reddens it.
    if not PLAN.is_file():
        return f"{PLAN} is not there, so the sharded selftest cannot run at all"
    code, failures = check()
    _shards, _assign, _cost, _overhead, substrate = read_plan(PLAN)
    expected = grade_substrate(substrate)
    if failures != expected:
        return f"{PLAN}: expected only {expected!r}, got {failures!r}"
    if code != (EXIT_BAD if expected else 0):
        return f"{PLAN}: exit {code} does not match failure list {failures!r}"
    return None


@fixture("--check prints the placeholder state loudly rather than passing silently")
def _placeholder_state_is_named_not_silent():
    # An undeclared substrate is not graded as a failure (the fixture above),
    # but passing with the SAME message a real runner harvest produces would
    # read as the same clean state -- and the two are not the same claim.
    # Driven against the real checked-in plan, which today has no substrate.
    if not PLAN.is_file():
        return f"{PLAN} is not there, so the sharded selftest cannot run at all"
    _shards, _assign, _cost, _overhead, substrate = read_plan(PLAN)
    if substrate:
        # A real harvest has landed since this fixture was written; the
        # placeholder path is exercised by construction above, and this
        # fixture has nothing left to prove against the real file.
        return None
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        code = main(["--check"])
    if code != 0:
        return f"the placeholder state exited {code}, not 0: {out.getvalue()!r}"
    if "placeholder" not in out.getvalue():
        return f"the placeholder state was not named: {out.getvalue()!r}"
    return None


def selftest() -> int:
    """Run every fixture, reporting each by name."""
    broken = []
    for name, run in FIXTURES:
        try:
            why = run()
        except Exception as err:  # a fixture that explodes is a fixture that failed
            why = f"{type(err).__name__}: {err}"
        print(f"{'ok  ' if why is None else 'FAIL'}  {name}")
        if why is not None:
            broken.append((name, why))
    for name, why in broken:
        print(f"  {name}\n    {why}", file=sys.stderr)
    print(
        f"derive-shards: {len(FIXTURES)} fixture(s), {len(broken)} failed",
        file=sys.stderr if broken else sys.stdout,
    )
    return EXIT_BAD if broken else 0


def check() -> tuple[int, list[str]]:
    """Grade the checked-in assignment. The code, and why, for both callers."""
    budget = read_budget(BUDGET)
    shards, assign, cost, overhead_ms, substrate = read_plan(PLAN)
    declared = manifest_ids()
    failures = (
        grade_coverage(declared, assign)
        + grade_shape(shards, assign, budget)
        + grade_balance(shards, assign, cost)
        + grade_substrate(substrate)
    )
    return (EXIT_BAD if failures else 0), failures


def matrix_of(path: pathlib.Path) -> str:
    """The CI matrix for the assignment at `path`: `[1, 2, .. shards]`.

    Refuses rather than prints `[]` for a plan declaring fewer than one
    shard -- an empty matrix is a workflow of no jobs, and GitHub Actions
    accepts it as a success with nothing run, not as the failure it is.
    """
    shards, _assign, _cost, _overhead, _substrate = read_plan(path)
    if shards < 1:
        raise Unusable(
            f"{path.name} declares {shards} shard(s). A matrix over that is "
            f"`[]` -- a workflow of no jobs, printed as if it were one"
        )
    return "[" + ", ".join(str(n) for n in range(1, shards + 1)) + "]"


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--index", help="the TSV --derive-shards wrote")
    parser.add_argument(
        "--emit",
        action="store_true",
        help="print the assignment a harvest derives, rather than reporting on it",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="grade the checked-in assignment against the manifest and itself",
    )
    parser.add_argument(
        "--matrix",
        action="store_true",
        help="print the checked-in shard numbers as JSON, for the CI matrix",
    )
    parser.add_argument(
        "--selftest",
        action="store_true",
        help="run the fixture suite over this file's reader and exit",
    )
    args = parser.parse_args(argv)

    modes = [bool(args.index), args.check, args.matrix, args.selftest]
    if sum(modes) != 1:
        print(
            "derive-shards: give exactly one of --index (read a harvest), "
            "--check (grade what is checked in), --matrix (print the CI matrix) "
            "or --selftest (exercise the reader). They read different files and "
            "answer different questions",
            file=sys.stderr,
        )
        return EXIT_BROKEN
    if args.emit and not args.index:
        print("derive-shards: --emit derives from a harvest, so it needs --index",
              file=sys.stderr)
        return EXIT_BROKEN

    if args.selftest:
        return selftest()

    try:
        if args.matrix:
            print(matrix_of(PLAN))
            return 0

        if args.check:
            code, failures = check()
            for message in failures:
                print(f"derive-shards: {message}", file=sys.stderr)
            if code:
                print(
                    f"derive-shards: {len(failures)} failure(s) in {PLAN.name}. "
                    f"Re-derive it: ./verify.sh --selftest --derive-shards DIR, "
                    f"then --index DIR/derive-shards.tsv --emit > {PLAN.name}",
                    file=sys.stderr,
                )
                return code
            budget = read_budget(BUDGET)
            shards, assign, cost, overhead_ms, substrate = read_plan(PLAN)
            # THE PLACEHOLDER STATE, NAMED LOUDLY. `grade_substrate` passes an
            # empty substrate through as no failure (the chicken-and-egg
            # ruling on #87, 2026-09-24: harvest-shards.yml can only reach the
            # default branch, where it becomes dispatchable, by landing
            # through a PR this check would otherwise have refused) -- but
            # passing silently would read as the same clean state a real
            # runner harvest produces, and the two are not the same claim.
            if not substrate:
                print(
                    f"derive-shards: placeholder: no runner harvest yet -- "
                    f"{PLAN.name} declares no substrate. {len(assign)} fault(s) "
                    f"across {shards} shard(s), packed elsewhere or by hand. "
                    f"Dispatch the harvest-shards workflow to replace this "
                    f"with a real one"
                )
                return 0
            worst, allowed = slowest(assign, cost, shards, overhead_ms, budget)
            print(
                f"derive-shards: {len(assign)} fault(s) across {shards} shard(s); "
                f"on the harvest's own numbers the slowest carries "
                f"{worst / 1000:.0f}s of the {allowed / 1000:.0f}s budget"
                + ("" if worst <= allowed else
                   " -- OVER on those numbers, which were measured wherever the "
                   "harvest was taken and not necessarily on the runner")
                + f", harvested on {substrate}"
            )
            return 0

        budget = read_budget(BUDGET)
        cost, label, run_ms, substrate = read_index(pathlib.Path(args.index))

        # A HARVEST IS GRADED AGAINST THE MANIFEST BEFORE IT IS PACKED. An
        # index missing a fault would emit an assignment missing that fault,
        # which --check then refuses -- correctly, but one step further from
        # the run that could have said so, and the operator has already thrown
        # the harvest away by then. Exit 1 rather than 2: an incomplete harvest
        # is a finding about the run, not a question this cannot answer.
        declared = manifest_ids()
        drift = grade_harvest(declared, cost)
        if drift:
            for message in drift:
                print(f"derive-shards: {message}", file=sys.stderr)
            print(
                f"derive-shards: the harvest at {args.index} is not about the "
                f"fault list this tree declares, so nothing can be packed from it",
                file=sys.stderr,
            )
            return EXIT_BAD

        if args.emit:
            sys.stdout.write(emit(cost, run_ms, budget, substrate))
            return 0

        overhead_ms = max(0, run_ms - sum(cost.values()))
        shards = derive_count(cost, overhead_ms, budget)
        assign = pack(cost, shards)
        summed = totals(assign, cost, shards)
        worst, allowed = slowest(assign, cost, shards, overhead_ms, budget)
        print(
            f"derive-shards: {len(cost)} fault(s) costing "
            f"{sum(cost.values()) / 1000:.0f}s, beside {overhead_ms / 1000:.0f}s "
            f"every shard pays whatever it draws"
        )
        print(
            f"derive-shards: {shards} shard(s) -- slowest {max(summed) / 1000:.1f}s "
            f"of faults, fastest {min(summed) / 1000:.1f}s, spread "
            f"{(max(summed) - min(summed)) / max(1, min(summed)) * 100:.0f}%"
        )
        dearest = sorted(cost, key=lambda name: (-cost[name], name))[:5]
        print(
            "derive-shards: the five dearest faults are "
            + "; ".join(f"{cost[i] / 1000:.1f}s {label.get(i) or i}" for i in dearest)
        )
        print(
            f"derive-shards: with overhead the slowest shard is "
            f"{worst / 1000:.0f}s against a {allowed / 1000:.0f}s budget"
            + ("" if worst <= allowed else
               f" -- OVER by {(worst - allowed) / 1000:.0f}s at the "
               f"{budget['max_shards']}-shard ceiling. On THESE numbers, which "
               f"are this machine's, splitting alone does not get there; a "
               f"harvest taken on the runner is the one that settles it")
        )
        return 0
    except Unusable as err:
        print(f"derive-shards: {err}", file=sys.stderr)
        return EXIT_BROKEN


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
