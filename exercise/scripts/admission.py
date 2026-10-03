#!/usr/bin/env python3
"""
A recording's admission to publication (#32, ruling 1, and the ruling on
#210): the table it was scanned under, kept, and its scrub, recorded beside
it -- and that same table re-run over it at every build, so neither a later
edit to the recording nor a change to the live table moves what it was
admitted under. Re-admitting under a newer table is a deliberate change.

AN AUTHORED EXAMPLE (#272; the maintainer's ruling on #32, 2026-10-02) is
admitted by the same scan under the same snapshot, from
src/drive/examples/<name>.json. Where a recording's migration header says how
it was scrubbed in one "Scrubbed:" line, an example's says it was authored, in
one "Authored: " line carrying the maintainer's sentence (EXAMPLE_LABEL in
src/replay/published.ts) verbatim. Which line is required is decided by the
list the name is on (PUBLISHED or EXAMPLES); a header carrying both, or
neither, is not admitted.

    python3 exercise/scripts/admission.py admit NAME     scan NAME; if clean, write its admission; then remove every snapshot no admission names
    python3 exercise/scripts/admission.py tables DIR     each admitted table, and the published recordings it governs
    python3 exercise/scripts/admission.py verify DIR     every DIR/<name>.js against DIR/<name>.admission.json, and no snapshot no admission names

THE ADMITTED TABLE is a snapshot of the genesis table -- its patterns, its
exceptions and its salted digests -- written once, by `admit` and nothing
else, into scripts/ as `hygiene-admitted-<id>-patterns.tsv`,
`-exceptions.tsv` and `-hashes.txt`, where hygiene.sh reads a pattern table
as a table (it exempts `scripts/*-patterns.tsv`) and finds its exceptions by
name. <id> is the first 12 hex digits of the sha256 over the three files'
own sha256s, so a change to any of the three is a new snapshot and the same
three are always the same one. The snapshot is the admitted rule; the live
table is the current rule. An admission names its snapshot and the live
table it was taken from, and says which it cites.

A SNAPSHOT NO ADMISSION NAMES is a rule nothing is admitted under, and a
reader of scripts/ cannot tell it from one in force (#257). An admission
names a snapshot by the three paths it lists under `table`, each exactly as
`admit` writes it (`scripts/hygiene-admitted-<id>-...`, for the id beside
them); every `*.admission.json` under exercise/src/drive/ counts. Any file
under scripts/ whose name starts `hygiene-admitted-` and that no admission
names -- whatever the rest of its name, and in any subdirectory -- is such a
snapshot. `admit`, once it has run (admitted or refused), removes every one
at scripts/' top level, where it writes them, and says which; one in a
subdirectory it names and leaves, since that file is someone else's. `verify`
refuses a tree that still holds either. An admission this script cannot read,
or one listing a path in any other form, leaves what it names unknown: all
three commands exit 2 naming it, and `admit` reads them before it writes
anything, so it writes and removes nothing.

`verify` reads every admission from the tree, never from the site it is
checking, and holds the site to it: exactly the published recordings and
examples (src/replay/published.ts) and nothing else in the payload directory; each
payload's bytes the committed recording's, behind one prefix; the
recording's digest the admitted one; each snapshot byte for byte the one
admitted; the site's copy of each admission the tree's; and no snapshot in
scripts/ that no admission names.
The scan is `verify.sh`'s `check_site`, which runs each snapshot over the
recordings it governs (`tables`). A payload with no admission beside it
fails: a recording whose admission is unknown does not publish.

Exit 0 if every payload checks and no snapshot is unnamed, 1 if any payload
does not check or any snapshot is unnamed (each named), 2 if there was nothing
to check or the check could not run -- a check of nothing is not a pass.
"""

import hashlib
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
RECORDED = ROOT / 'exercise/src/drive/recorded'
EXAMPLES = ROOT / 'exercise/src/drive/examples'
# The live genesis table, as hygiene.sh reads it by default.
LIVE = {
    'patterns': 'scripts/hygiene-patterns.tsv',
    'exceptions': 'scripts/hygiene-exceptions.tsv',
    'hashes': 'scripts/hygiene-hashes.txt',
}
SNAPSHOT = {
    'patterns': 'scripts/hygiene-admitted-{id}-patterns.tsv',
    'exceptions': 'scripts/hygiene-admitted-{id}-exceptions.tsv',
    'hashes': 'scripts/hygiene-admitted-{id}-hashes.txt',
}
PREFIX = b'export default '  # src/replay/payload.ts
PUBLISHED_TS = ROOT / 'exercise/src/replay/published.ts'


def listed(which: str) -> list[str]:
    """A published list (PUBLISHED or EXAMPLES), read from the one place it is written."""
    found = re.search(rf"export const {which} = \[([^\]]*)\] as const", PUBLISHED_TS.read_text(encoding='utf-8'))
    if not found:
        raise SystemExit(f'admission: {PUBLISHED_TS.relative_to(ROOT)}: no {which} list to check the site against')
    return re.findall(r"'([a-z0-9-]+)'", found.group(1))


def published() -> list[str]:
    return listed('PUBLISHED')


def example_label() -> str:
    """The maintainer's sentence an example is admitted under, read from where the page reads it."""
    found = re.search(r"export const EXAMPLE_LABEL = '([^'\\]*)';", PUBLISHED_TS.read_text(encoding='utf-8'))
    if not found:
        raise SystemExit(f'admission: {PUBLISHED_TS.relative_to(ROOT)}: no EXAMPLE_LABEL to admit an example under')
    return found.group(1)


def kind_of(name: str) -> dict:
    """What NAME is published as: where its file is, the admission's key for it, and the one header line it needs."""
    if name in listed('EXAMPLES'):
        return {'key': 'example', 'dir': EXAMPLES, 'line': 'Authored:', 'field': 'authored'}
    return {'key': 'recording', 'dir': RECORDED, 'line': 'Scrubbed:', 'field': 'scrub'}


def published_names() -> list[str]:
    """Every name the site publishes: the recordings, then the examples. A name on both is refused."""
    recordings, examples = published(), listed('EXAMPLES')
    both = sorted(set(recordings) & set(examples))
    if both:
        raise SystemExit(f'admission: {PUBLISHED_TS.relative_to(ROOT)}: {", ".join(both)} on both PUBLISHED and EXAMPLES; a recording is not an example')
    return recordings + examples


class Unreadable(Exception):
    """An admission whose snapshot paths cannot be read: what it names is unknown, so nothing may be removed."""


def cited() -> set[str]:
    """Every snapshot file an admission under exercise/src/drive/ names, by the paths it lists."""
    named = set()
    for admitted in sorted((ROOT / 'exercise/src/drive').rglob('*.admission.json')):
        rel = admitted.relative_to(ROOT).as_posix()
        try:
            table = json.loads(admitted.read_text(encoding='utf-8'))['table']
            ident = table['id']
            paths = {part: table[part]['path'] for part in SNAPSHOT}
            digests_given = [table[part]['sha256'] for part in SNAPSHOT]
        except (OSError, ValueError, KeyError, TypeError) as err:
            raise Unreadable(f'admission: {rel}: not an admission this script can read ({type(err).__name__}: {err})') from err
        if not all(isinstance(d, str) for d in digests_given):
            raise Unreadable(f'admission: {rel}: its table gives a snapshot digest that is not a string')
        if not (isinstance(ident, str) and re.fullmatch(r'[0-9a-f]{12}', ident)):
            raise Unreadable(f'admission: {rel}: its table id {ident!r} is not a snapshot id')
        for part, path in paths.items():
            # Compared as strings, so only the form admit writes is read: `./scripts/...`, or another case of it, is not.
            if path != SNAPSHOT[part].format(id=ident):
                raise Unreadable(f'admission: {rel}: its table lists {path!r} where admit writes {SNAPSHOT[part].format(id=ident)!r}')
            # admit writes a snapshot as a file; a link there reaches something else, which removing could break.
            if (ROOT / path).is_symlink():
                raise Unreadable(f'admission: {rel}: its table lists {path}, which is a link; admit writes a snapshot as a file')
        named.update(paths.values())
    return named


def orphans() -> list[pathlib.Path]:
    """Every file under scripts/ named `hygiene-admitted-*` that no admission names."""
    named = cited()
    by_case = {path.casefold(): path for path in named}

    def respelled(f: pathlib.Path, rel: str) -> bool:
        # A cited path reaches this very entry under another case of its name (a case-insensitive disk): it is cited.
        # Only that: the entries are compared, not what they lead to, so a link or another name is not.
        if rel.casefold() not in by_case:
            return False
        try:
            mine, cited_entry = os.lstat(f), os.lstat(ROOT / by_case[rel.casefold()])
        except OSError:
            return False
        return (mine.st_dev, mine.st_ino) == (cited_entry.st_dev, cited_entry.st_ino)

    found = [f for f in (ROOT / 'scripts').rglob('hygiene-admitted-*') if f.is_file() or f.is_symlink()]
    return sorted(f for f in found if (rel := f.relative_to(ROOT).as_posix()) not in named and not respelled(f, rel))


def prune() -> None:
    """Remove the snapshots no admission names at scripts/' top level, each said by name; name and leave any deeper."""
    for orphan in orphans():
        rel = orphan.relative_to(ROOT).as_posix()
        if orphan.parent != ROOT / 'scripts':
            print(f'admission: {rel}: named like a snapshot no admission names, below scripts/ where admit never writes one; not removed')
            continue
        orphan.unlink(missing_ok=True)
        print(f'admission: removed {rel}: a snapshot no admission names')


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def digests(paths: dict) -> dict:
    return {part: {'path': rel, 'sha256': sha256((ROOT / rel).read_bytes())} for part, rel in paths.items()}


def snapshot_of_live() -> tuple[str, dict]:
    """The snapshot the live table would be: its id, and its three files written if not there yet."""
    live = digests(LIVE)
    ident = sha256(''.join(live[part]['sha256'] for part in ('patterns', 'exceptions', 'hashes')).encode())[:12]
    paths = {part: rel.format(id=ident) for part, rel in SNAPSHOT.items()}
    for part, rel in paths.items():
        source, target = ROOT / LIVE[part], ROOT / rel
        if target.exists():
            if target.read_bytes() != source.read_bytes():
                raise SystemExit(f'admission: {rel} exists and is not the live table it is named for; a snapshot is written once')
        else:
            shutil.copyfile(source, target)
    return ident, paths


def scan(name: str, data: bytes, table: dict) -> int:
    """The genesis snapshot over exactly the bytes being admitted."""
    with tempfile.TemporaryDirectory() as box:
        (pathlib.Path(box) / f'{name}.json').write_bytes(data)
        return subprocess.run(
            ['bash', str(ROOT / 'scripts/hygiene.sh'), '--patterns', str(ROOT / table['patterns']), '--hashes', str(ROOT / table['hashes']), '--tree', box],
            cwd=ROOT,
        ).returncode


def admit(name: str) -> int:
    cited()  # every admission readable, before anything is written
    published_names()  # a name on both lists is refused before anything is written
    kind = kind_of(name)
    recording = kind['dir'] / f'{name}.json'
    rel = recording.relative_to(ROOT).as_posix()
    if not recording.is_file():
        print(f'admission: {rel}: no such {kind["key"]}', file=sys.stderr)
        return 2
    data = recording.read_bytes()
    header = [line for line in json.loads(data).get('migration', []) if isinstance(line, str)]
    declared = [line for line in header if line.startswith(kind["line"])]
    other = [line for line in header if line.startswith('Authored:' if kind['key'] == 'recording' else 'Scrubbed:')]
    if kind['key'] == 'recording' and (len(declared) != 1 or other):
        print(f'admission: {rel}: its migration header says how it was scrubbed in exactly one "Scrubbed:" line, and carries no "Authored:" line, or it is not admitted', file=sys.stderr)
        return 1
    if kind['key'] == 'example' and (declared != [f'Authored: {example_label()}'] or other):
        print(f'admission: {rel}: an example\'s header says it was authored in exactly one line, "Authored: " and the maintainer\'s sentence (EXAMPLE_LABEL), and carries no "Scrubbed:" line, or it is not admitted', file=sys.stderr)
        return 1
    ident, table = snapshot_of_live()
    status = scan(name, data, table)
    if status != 0:
        print(f'admission: {rel}: not admitted; the genesis table finds something in it (exit {status})', file=sys.stderr)
        prune()  # the snapshot this run wrote, if nothing else names it
        return 1
    admission = {
        kind['key']: rel,
        f'{kind["key"]}_sha256': sha256(data),
        'table': {'id': ident, **digests(table)},
        'taken_from': digests(LIVE),
        'cites': f'the snapshot (table): the rule this {kind["key"]} was admitted under. At admission it was the live genesis table (taken_from), byte for byte; the live table may move on, and this {kind["key"]} stays under its snapshot until it is admitted again.',
        kind['field']: declared[0],
    }
    out = kind['dir'] / f'{name}.admission.json'
    out.write_text(json.dumps(admission, indent=2) + '\n', encoding='utf-8')
    print(f'admission: {rel}: admitted under {table["patterns"]}, {out.relative_to(ROOT).as_posix()}')
    prune()
    return 0


def admissions(directory: str) -> tuple[list, list[str]]:
    """Each published recording's and example's payload in DIRECTORY with its admission from the tree, and every problem found."""
    data_dir = pathlib.Path(directory)
    cited()  # every admission readable, before any is judged
    if not data_dir.is_dir():
        return [], []
    names = published_names()
    expected = {f'{n}.js' for n in names} | {f'{n}.admission.json' for n in names}
    problems = [f'admission: {data_dir / f.name}: not a published recording or example, or its admission; nothing else is published here' for f in sorted(data_dir.iterdir()) if f.name not in expected]
    found = []
    for name in names:
        kind = kind_of(name)
        payload, copy = data_dir / f'{name}.js', data_dir / f'{name}.admission.json'
        rel = (kind['dir'] / f'{name}.json').relative_to(ROOT).as_posix()
        admitted = kind['dir'] / f'{name}.admission.json'
        if not payload.is_file():
            problems.append(f'admission: {payload}: published ({PUBLISHED_TS.relative_to(ROOT)}) but not on the site')
            continue
        if not admitted.is_file():
            problems.append(f'admission: {rel}: published with no admission in the tree')
            continue
        if not copy.is_file() or copy.read_bytes() != admitted.read_bytes():
            problems.append(f'admission: {copy}: not the admission in the tree ({admitted.relative_to(ROOT)})')
            continue
        admission = json.loads(admitted.read_text(encoding='utf-8'))
        if admission.get(kind['key']) != rel:
            problems.append(f'admission: {admitted.relative_to(ROOT)}: admits {admission.get(kind["key"])}, not {rel}')
            continue
        if set(admission.get('table', {})) != {'id', *SNAPSHOT}:
            problems.append(f'admission: {rel}: its admission does not name an admitted genesis table (patterns, exceptions, hashes)')
            continue
        found.append((payload, admission, kind['key']))
    return found, problems


def tables(directory: str) -> int:
    found, problems = admissions(directory)
    for problem in problems:
        print(problem, file=sys.stderr)
    if problems:
        return 1
    if not found:
        print(f'admission: {directory}: no published recording to check', file=sys.stderr)
        return 2
    groups: dict = {}
    for payload, admission, _ in found:
        key = (admission['table']['patterns']['path'], admission['table']['hashes']['path'])
        groups.setdefault(key, []).append(str(payload))
    for (patterns, hashes), payloads in groups.items():
        print('\t'.join([patterns, hashes, *payloads]))
    return 0


def verify(directory: str) -> int:
    found, problems = admissions(directory)
    for problem in problems:
        print(problem, file=sys.stderr)
    if not found and not problems:
        print(f'admission: {directory}: no published recording to check', file=sys.stderr)
        return 2
    live = digests(LIVE)
    bad = len(problems)
    for payload, admission, key in found:
        rel = admission[key]
        body = payload.read_bytes()
        failures = []
        if not body.startswith(PREFIX):
            failures.append(f'admission: {payload}: not a published recording (it does not start "{PREFIX.decode().strip()}")')
        elif body[len(PREFIX):] != (ROOT / rel).read_bytes():
            failures.append(f'admission: {payload}: not {rel} byte for byte behind its prefix')
        elif sha256(body[len(PREFIX):]) != admission.get(f'{key}_sha256'):
            failures.append(f'admission: {rel}: edited since it was admitted; scan it and admit it again (admission.py admit {payload.stem})')
        for part in SNAPSHOT:
            pinned = admission['table'][part]
            path = ROOT / pinned['path']
            if not path.is_file():
                failures.append(f'admission: {rel}: its admitted table {pinned["path"]} is not there')
            elif sha256(path.read_bytes()) != pinned['sha256']:
                failures.append(f'admission: {rel}: its admitted table {pinned["path"]} is not the one admitted (a snapshot is never edited)')
        for failure in failures:
            print(failure, file=sys.stderr)
        bad += 1 if failures else 0
        if not failures and any(live[part]['sha256'] != admission['table'][part]['sha256'] for part in SNAPSHOT):
            print(f'admission: {rel}: checked under its admitted table {admission["table"]["id"]}; the live table has moved on since')
    total = len(found) + len(problems)
    print(f'admission: {total - bad} of {total} published recording(s) and example(s) check against their admissions')
    # And no snapshot in scripts/ that nothing is admitted under (#257).
    stray = orphans()
    for orphan in stray:
        print(f'admission: {orphan.relative_to(ROOT).as_posix()}: a snapshot no admission names; admission.py admit removes it', file=sys.stderr)
    return 1 if bad or stray else 0


def main(argv: list[str]) -> int:
    commands = {'admit': admit, 'tables': tables, 'verify': verify}
    if len(argv) == 3 and argv[1] in commands:
        try:
            return commands[argv[1]](argv[2])
        except Unreadable as err:
            print(f'{err}; the snapshots it names are unknown, so none is removed or judged. Restore that file from git, or delete it, then admit again.', file=sys.stderr)
            return 2
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == '__main__':
    sys.exit(main(sys.argv))
