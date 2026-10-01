#!/usr/bin/env python3
"""
A recording's admission to publication (#32, ruling 1, and the ruling on
#210): the table it was scanned under, kept, and its scrub, recorded beside
it -- and that same table re-run over it at every build, so neither a later
edit to the recording nor a change to the live table moves what it was
admitted under. Re-admitting under a newer table is a deliberate change.

    python3 exercise/scripts/admission.py admit NAME     scan NAME; if clean, write its admission
    python3 exercise/scripts/admission.py tables DIR     each admitted table, and the published recordings it governs
    python3 exercise/scripts/admission.py verify DIR     every DIR/<name>.js against DIR/<name>.admission.json

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

`verify` checks digests: the recording each payload carries is the one that
was admitted, and the snapshot it names is byte for byte the one admitted.
The scan is `verify.sh`'s `check_site`, which runs each snapshot over the
recordings it governs (`tables`). A payload with no admission beside it
fails: a recording whose admission is unknown does not publish.

Exit 0 if every payload checks, 1 if any does not (each named), 2 if there was
nothing to check or the check could not run -- a check of nothing is not a pass.
"""

import hashlib
import json
import pathlib
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
RECORDED = ROOT / 'exercise/src/drive/recorded'
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
PREFIX = 'export default '  # src/replay/payload.ts


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


def scan(recording: pathlib.Path, table: dict) -> int:
    with tempfile.TemporaryDirectory() as box:
        shutil.copy(recording, box)
        return subprocess.run(
            ['bash', str(ROOT / 'scripts/hygiene.sh'), '--patterns', str(ROOT / table['patterns']), '--hashes', str(ROOT / table['hashes']), '--tree', box],
            cwd=ROOT,
        ).returncode


def admit(name: str) -> int:
    recording = RECORDED / f'{name}.json'
    rel = recording.relative_to(ROOT).as_posix()
    if not recording.is_file():
        print(f'admission: {rel}: no such recording', file=sys.stderr)
        return 2
    data = recording.read_bytes()
    scrub = [line for line in json.loads(data).get('migration', []) if isinstance(line, str) and line.startswith('Scrubbed:')]
    if len(scrub) != 1:
        print(f'admission: {rel}: its migration header says how it was scrubbed in exactly one "Scrubbed:" line, or it is not admitted', file=sys.stderr)
        return 1
    ident, table = snapshot_of_live()
    status = scan(recording, table)
    if status != 0:
        print(f'admission: {rel}: not admitted; the genesis table finds something in it (exit {status})', file=sys.stderr)
        return 1
    admission = {
        'recording': rel,
        'recording_sha256': sha256(data),
        'table': {'id': ident, **digests(table)},
        'taken_from': digests(LIVE),
        'cites': 'the snapshot (table): the rule this recording was admitted under. At admission it was the live genesis table (taken_from), byte for byte; the live table may move on, and this recording stays under its snapshot until it is admitted again.',
        'scrub': scrub[0],
    }
    out = RECORDED / f'{name}.admission.json'
    out.write_text(json.dumps(admission, indent=2) + '\n', encoding='utf-8')
    print(f'admission: {rel}: admitted under {table["patterns"]}, {out.relative_to(ROOT).as_posix()}')
    return 0


def admissions(directory: str) -> tuple[list, list[str]]:
    """Each payload in DIRECTORY with its admission, and what is wrong with any that has none usable."""
    data_dir = pathlib.Path(directory)
    found, problems = [], []
    for payload in sorted(data_dir.glob('*.js')) if data_dir.is_dir() else []:
        sidecar = data_dir / f'{payload.stem}.admission.json'
        if not sidecar.is_file():
            problems.append(f'admission: {payload}: published with no admission beside it')
            continue
        admission = json.loads(sidecar.read_text(encoding='utf-8'))
        if set(admission.get('table', {})) != {'id', *SNAPSHOT}:
            problems.append(f'admission: {admission.get("recording", payload)}: its admission does not name an admitted genesis table (patterns, exceptions, hashes)')
            continue
        found.append((payload, admission))
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
    for payload, admission in found:
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
    for payload, admission in found:
        rel = admission.get('recording', payload.name)
        text = payload.read_text(encoding='utf-8')
        failures = []
        if not text.startswith(PREFIX):
            failures.append(f'admission: {payload}: not a published recording (it does not start "{PREFIX.strip()}")')
        elif sha256(text[len(PREFIX):].encode('utf-8')) != admission.get('recording_sha256'):
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
    print(f'admission: {total - bad} of {total} published recording(s) check against their admissions')
    return 1 if bad else 0


def main(argv: list[str]) -> int:
    commands = {'admit': admit, 'tables': tables, 'verify': verify}
    if len(argv) == 3 and argv[1] in commands:
        return commands[argv[1]](argv[2])
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == '__main__':
    sys.exit(main(sys.argv))
