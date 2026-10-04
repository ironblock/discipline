"""One reader of `seeded_case`, because two of them disagree.

`check-injections.py` asks which injections the seeded cases name, so it can
refuse a case naming an injection that no longer exists. `check-fault-
manifest.py` asks the same question to count what must go red and to check
each manifest entry's label and signature against the case that proves it.
They had a regex each, and the two were not the same regex: a case spelled
across continuation lines was invisible to one of them, and a commented-out
case was live to the other. A case one reader sees and the other does not is
a fault that runs, goes red, and is counted by nobody.

So the spelling is parsed once, here. It is parsed the way the shell reads it
-- continuations joined, then split on shell quoting -- rather than matched,
because the thing being read IS a shell call, and a regex that approximates
shell quoting is a third dialect nobody declared.
"""

import shlex
from typing import NamedTuple


class Case(NamedTuple):
    """One `seeded_case` call in verify.sh."""

    label: str
    check: str
    injection: str
    signature: str | None
    # The fifth word: which test binaries and tests the case's `test` check
    # runs. Parsed here rather than by whoever wants it, for the same reason
    # the rest of the call is: a scope one reader sees and another does not is
    # a fault that runs somewhere nobody is counting.
    scope: str | None


def logical_lines(text: str):
    """The text's lines with backslash-continuations joined, as the shell
    reads them. A case may be spelled over four lines for width and it is
    still one call."""
    held: list[str] = []
    for raw in text.splitlines():
        line = raw.strip()
        if line.endswith("\\"):
            held.append(line[:-1].strip())
            continue
        held.append(line)
        yield " ".join(part for part in held if part)
        held = []
    if held:
        yield " ".join(part for part in held if part)


def seeded_cases(text: str) -> list[Case]:
    """Every seeded case the shell would run, in order.

    A line whose first character is `#` is not a call: a case parked behind a
    comment while its injection is being written is a legitimate editing
    state, and reporting it as a case naming nothing is a false red.
    """
    found: list[Case] = []
    for line in logical_lines(text):
        if line.startswith("#") or not line.startswith("seeded_case"):
            continue
        try:
            parts = shlex.split(line)
        except ValueError:
            # An unbalanced quote is not this reader's to diagnose; bash will
            # refuse the file long before the gate asks what it contains.
            continue
        if len(parts) < 4 or not parts[3].startswith("inject_"):
            continue
        found.append(
            Case(
                parts[1],
                parts[2],
                parts[3],
                parts[4] if len(parts) > 4 else None,
                parts[5] if len(parts) > 5 else None,
            )
        )
    return found


# The line a check's `--names` mode prints before its members (#268's third
# review): the members are the lines after it, and a check that ignored
# `--names` and ran prints none, so it cannot pass for a listing.
LISTING = "members (listed, none run):"


def in_shard(name: str, index: int, count: int) -> bool:
    """Whether `name` belongs to shard `index` of `count` (#262): a stable hash
    of the name, so a check split across jobs never depends on order or on run,
    and every name lands in exactly one shard."""
    import hashlib

    return int(hashlib.sha256(name.encode("utf-8")).hexdigest(), 16) % count + 1 == index


def shard_arg(spec: str) -> tuple[int, int] | None:
    """`K/N` with 1 <= K <= N, or None."""
    import re

    match = re.fullmatch(r"([1-9]\d*)/([1-9]\d*)", spec)
    if not match or int(match.group(1)) > int(match.group(2)):
        return None
    return int(match.group(1)), int(match.group(2))


# THE RUNTIME CENSUS (#262, ruled on #268). A listing proves what a check
# WOULD run; only the run can say what it ran. So each check that can be
# sharded records every member its run loop finishes, with the outcome the
# member's work returned (never on entry), into the file this variable names, and
# scripts/check-shard-census.py adds the shards' files back up against the
# unsplit listing. The listing path returns before the loop, so it never
# writes here.
MEMBERS_RAN = "VERIFY_MEMBERS_RAN"

# A DRY run records each member and skips its work: what the coverage check
# runs locally to prove the census reads the run loop. Its rows say `dry`,
# and the census on CI refuses a `dry` row, so a job that set this would be
# red rather than a green run of nothing.
CENSUS_DRY = "VERIFY_CENSUS_DRY"


# The outcomes that mean a member RAN, per sharded check (#268's sixth
# review). A row carries the outcome its work returned; the census reads it,
# and an outcome not listed here -- `skipped`, say, written honestly by a
# shard-only shortcut -- is a member that did not run, never one that did.
# `bsd` runs the applier's own loop.
RAN_OUTCOMES = {
    "recompute": frozenset({"recomputed", "template", "historical", "failed", "unprobed"}),
    "injections": frozenset({"applied", "inert", "unrestored"}),
    "bsd": frozenset({"applied", "inert", "unrestored"}),
}


def census_dry() -> bool:
    """Whether this run records its members and does none of their work."""
    import os

    return os.environ.get(CENSUS_DRY) == "1"


def record_ran(name: str, outcome: str | None = None) -> None:
    """Record `name`, if a census was asked for: one row per member, appended
    so a run that dies part-way says how far it got. A real run's row carries
    the member's OUTCOME, written from what its work returned, never on entry
    (#268's fifth review: a row on entry said only that the loop got there).
    A dry run's row carries none."""
    import os

    path = os.environ.get(MEMBERS_RAN)
    if not path:
        return
    with open(path, "a", encoding="utf-8") as out:
        out.write(f"dry\t{name}\n" if census_dry() else f"ran\t{name}\t{outcome}\n")


def members_listed(output: str, check: str) -> list[str] | None:
    """The members a check printed after its own `LISTING` line and before
    verify.sh's `--- check: LISTED` line: every non-empty line, so no name a
    pattern would reject falls out of the proof (#268's second review). None
    when the run did not end in LISTED, or when the check printed no LISTING
    line of its own -- verify.sh says LISTED for any check that exits 0 under
    VERIFY_LIST_MEMBERS, and a check that ignored `--names` and ran must not
    pass for one that listed (#268's third)."""
    inside, names, listed, marked = False, [], False, 0
    for line in output.split("\n"):
        if line.strip() == f"=== {check} ===":
            inside = True
        elif line.startswith(f"--- {check}:"):
            inside = False
            listed = line.startswith(f"--- {check}: LISTED")
        elif inside and line.strip() == LISTING:
            marked += 1
            names = []
        elif inside and marked and line.strip():
            names.append(line.strip())
    return names if listed and marked == 1 else None


def split_failures(check: str, count: int, whole: list[str], parts: list[list[str]]) -> list[str]:
    """What is wrong with `parts` as a split of `whole` into `count` shards:
    a member in no shard is a member nothing runs -- a test that cannot fail,
    #239's lesson -- and a member in two is the budget spent twice."""
    import collections

    failures = []
    seen = collections.Counter(name for part in parts for name in part)
    missing = [name for name in whole if seen[name] == 0]
    twice = sorted(name for name, n in seen.items() if n > 1)
    stray = sorted(set(seen) - set(whole))
    if not whole:
        failures.append(f"`{check}` lists no member at all; a split of nothing is not a split")
    if missing:
        failures.append(f"`{check}`'s {count} shards run none of {len(missing)} member(s), so nothing runs them: {', '.join(missing[:5])}")
    if twice:
        failures.append(f"`{check}`'s shards run {len(twice)} member(s) more than once: {', '.join(twice[:5])}")
    if stray:
        failures.append(f"`{check}`'s shards run {len(stray)} member(s) the unsplit check does not: {', '.join(stray[:5])}")
    return failures


# WHEN THE SELFTEST RUNS (#369): the events `.github/gate-budget.tsv`'s
# `selftest_events` row names, one word each. `schedule` is the nightly; and
# `release` is the release path, a pull request into the release branch and a
# push to it, the branch read from `.github/branches.tsv`. Read here by both
# sides of the one claim -- check-ci-coverage.py, which holds verify.yml's
# `selftest` job's `if:` to selftest_expression(), and check-job-results.py,
# which accepts a skipped `selftest` only on an event this says is off -- so
# the workflow and the verdict cannot read the row two ways.
SELFTEST_EVENT_WORDS = ("schedule", "release")


def table_value(path, key: str) -> str | None:
    """`key`'s value in a `constant<TAB>value` table, or None."""
    for line in path.read_text(encoding="utf-8").split("\n"):
        name, tab, value = line.partition("\t")
        if tab and name == key and not line.startswith("#"):
            return value.strip()
    return None


def selftest_events(value: str | None) -> tuple[str, ...] | None:
    """The row's words, in the order SELFTEST_EVENT_WORDS gives them, or None
    when the row is missing, empty, repeats a word or names one it does not
    know -- a word nothing reads would declare an event nobody runs on."""
    words = (value or "").split()
    if not words or len(set(words)) != len(words) or set(words) - set(SELFTEST_EVENT_WORDS):
        return None
    return tuple(w for w in SELFTEST_EVENT_WORDS if w in words)


def selftest_expression(events: tuple[str, ...], release: str) -> str:
    """The `if:` verify.yml's `selftest` job carries for `events`."""
    clauses = []
    if "schedule" in events:
        clauses.append("github.event_name == 'schedule'")
    if "release" in events:
        clauses.append(f"(github.event_name == 'pull_request' && github.base_ref == '{release}')")
        clauses.append(f"(github.event_name == 'push' && github.ref_name == '{release}')")
    return "${{ " + " || ".join(clauses) + " }}"


def selftest_runs(events: tuple[str, ...], release: str, event: str, base_ref: str, ref_name: str) -> bool:
    """Whether a run with this event, base and ref runs the selftest -- the
    same answer selftest_expression() gives GitHub, in Python."""
    if "schedule" in events and event == "schedule":
        return True
    if "release" in events:
        if event == "pull_request" and base_ref == release:
            return True
        if event == "push" and ref_name == release:
            return True
    return False
