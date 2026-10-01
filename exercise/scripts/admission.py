#!/usr/bin/env python3
"""
A recording's admission to publication (#32, ruling 1): the table it was
scanned under, by digest, and its scrub, recorded beside it -- and re-checked
at every build, so a later edit to the recording, or to the table, cannot slip
past the admission.

    python3 exercise/scripts/admission.py admit NAME     scan NAME under the genesis table; if clean, write
                                                         exercise/src/drive/recorded/NAME.admission.json
    python3 exercise/scripts/admission.py verify DIR     every DIR/<name>.js (a published recording,
                                                         src/replay/payload.ts) against DIR/<name>.admission.json

`verify` checks digests: the recording the payload carries is the one that was
admitted, and the table it was admitted under is the table as it is now. The
scan itself is re-run by `verify.sh`'s `check_site`, over the same files,
under that table. A payload with no admission beside it fails: a recording
whose admission is unknown does not publish.

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
# The genesis table, as hygiene.sh reads it by default: the patterns, their exceptions, and the salted digests.
TABLE = {
    'patterns': 'scripts/hygiene-patterns.tsv',
    'exceptions': 'scripts/hygiene-exceptions.tsv',
    'hashes': 'scripts/hygiene-hashes.txt',
}
PREFIX = 'export default '  # src/replay/payload.ts


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def table_now() -> dict:
    return {part: {'path': rel, 'sha256': sha256((ROOT / rel).read_bytes())} for part, rel in TABLE.items()}


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
    with tempfile.TemporaryDirectory() as box:
        shutil.copy(recording, box)
        scan = subprocess.run(['bash', str(ROOT / 'scripts/hygiene.sh'), '--tree', box], cwd=ROOT)
    if scan.returncode != 0:
        print(f'admission: {rel}: not admitted; the genesis table finds something in it (exit {scan.returncode})', file=sys.stderr)
        return 1
    admission = {'recording': rel, 'recording_sha256': sha256(data), 'table': table_now(), 'scrub': scrub[0]}
    out = RECORDED / f'{name}.admission.json'
    out.write_text(json.dumps(admission, indent=2) + '\n', encoding='utf-8')
    print(f'admission: {rel}: admitted, {out.relative_to(ROOT).as_posix()}')
    return 0


def verify(directory: str) -> int:
    data_dir = pathlib.Path(directory)
    payloads = sorted(data_dir.glob('*.js')) if data_dir.is_dir() else []
    if not payloads:
        print(f'admission: {directory}: no published recording to check', file=sys.stderr)
        return 2
    now = table_now()
    failed = 0
    bad: set[str] = set()
    for payload in payloads:
        name = payload.stem
        sidecar = data_dir / f'{name}.admission.json'
        if not sidecar.is_file():
            print(f'admission: {payload}: published with no admission beside it', file=sys.stderr)
            failed += 1
            bad.add(payload.name)
            continue
        admission = json.loads(sidecar.read_text(encoding='utf-8'))
        rel = admission.get('recording', f'exercise/src/drive/recorded/{name}.json')
        text = payload.read_text(encoding='utf-8')
        body = text[len(PREFIX):] if text.startswith(PREFIX) else None
        if body is None:
            print(f'admission: {payload}: not a published recording (it does not start "{PREFIX.strip()}")', file=sys.stderr)
            failed += 1
            bad.add(payload.name)
            continue
        if sha256(body.encode('utf-8')) != admission.get('recording_sha256'):
            print(f'admission: {rel}: edited since it was admitted; scan it and admit it again (admission.py admit {name})', file=sys.stderr)
            failed += 1
            bad.add(payload.name)
        for part, pinned in admission.get('table', {}).items():
            if now.get(part, {}).get('sha256') != pinned.get('sha256'):
                print(f'admission: {rel}: admitted under {pinned.get("path")} at {str(pinned.get("sha256"))[:12]}, which is now {str(now.get(part, {}).get("sha256"))[:12]}; admit it again', file=sys.stderr)
                failed += 1
                bad.add(payload.name)
        if set(admission.get('table', {})) != set(TABLE):
            print(f'admission: {rel}: its admission does not name the genesis table this site checks recordings under', file=sys.stderr)
            failed += 1
            bad.add(payload.name)
    print(f'admission: {len(payloads) - len(bad)} of {len(payloads)} published recording(s) check against their admissions')
    return 1 if failed else 0


def main(argv: list[str]) -> int:
    if len(argv) == 3 and argv[1] == 'admit':
        return admit(argv[2])
    if len(argv) == 3 and argv[1] == 'verify':
        return verify(argv[2])
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == '__main__':
    sys.exit(main(sys.argv))
