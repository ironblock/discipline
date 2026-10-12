"""The scrub a run or results directory is assembled with (#757).

One source with the hygiene check: the patterns are `scripts/hygiene-patterns.tsv`'s
own rows, read here, so what the scrub rewrites and what the scan refuses
cannot drift apart. A scrub rewrites exactly three things, each to the
spelling the tree already uses:

    personal-home-path        /Users/<name>, /home/<name>   ->  ~
    session-scratchpad-path   /tmp/claude-<n>/               ->  /tmp/scratch/
    the account name          <name>, standing alone         ->  user

Every other row is a shape nothing can rewrite into something true -- a LAN
address, a hostname, a credential -- so a file still matching one after the
scrub is refused, not scrubbed. The account name has no row (it is a literal,
caught by the digest half, `hygiene-hashes.txt`), so it is the home
directory's last part, read from `--home`.
"""

import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parent.parent
PATTERNS = ROOT / "scripts" / "hygiene-patterns.tsv"

# The rows a scrub rewrites, and to what. Every other row refuses.
REWRITES = {
    "personal-home-path": "~",
    "session-scratchpad-path": "/tmp/scratch/",
}
# The account name's own spelling after the scrub.
ACCOUNT = "user"
# What a name stands alone between: hygiene's own boundary for a home path.
NAME_CHARS = "A-Za-z0-9._-"


# The POSIX classes the table's extended regexes use, as Python spells them
# inside a bracket expression; a class the table gains and this does not know
# is refused below rather than read as a set of its letters.
POSIX_CLASSES = {"[:space:]": r"\s", "[:digit:]": "0-9", "[:alnum:]": "A-Za-z0-9", "[:alpha:]": "A-Za-z"}


def python(regex: str) -> str:
    """An extended regex of the table, as Python's `re` reads the same thing."""
    for posix, spelled in POSIX_CLASSES.items():
        regex = regex.replace(posix, spelled)
    if re.search(r"\[:[a-z]+:\]", regex):
        raise ValueError(f"a POSIX class this scrub cannot translate: {regex}")
    return regex


def table() -> dict[str, re.Pattern]:
    """Each row of the hygiene table, compiled: label -> pattern."""
    rows = {}
    for line in PATTERNS.read_text().splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        label, flags, regex = line.split("\t", 2)
        rows[label] = re.compile(python(regex), re.IGNORECASE if "i" in flags else 0)
    return rows


def scrub(text: str, account: str) -> tuple[str, dict[str, int]]:
    """`text` scrubbed, and how many of each replacement it made."""
    rows = table()
    made: dict[str, int] = {}
    for label, replacement in REWRITES.items():
        text, count = rows[label].subn(replacement, text)
        if count:
            made[label] = count
    if account:
        alone = re.compile(rf"(?<![{NAME_CHARS}]){re.escape(account)}(?![{NAME_CHARS}])")
        text, count = alone.subn(ACCOUNT, text)
        if count:
            made["account-name"] = count
    return text, made


def unscrubbable(text: str) -> list[str]:
    """The rows `text` still matches that no scrub rewrites, by label."""
    return [label for label, row in table().items() if label not in REWRITES and row.search(text)]
