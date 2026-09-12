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
