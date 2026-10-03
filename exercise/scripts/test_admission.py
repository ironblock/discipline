#!/usr/bin/env python3
"""
admission.py's own tests: the snapshots no admission names (#257). Run directly:

    python3 exercise/scripts/test_admission.py

Each test runs admission.py in a box of its own, as the gate runs it, since
`admit` writes and removes files in the tree it runs from. The box holds
scripts/ with no admitted snapshot in it, admission.py, and a fixture of its
own -- one recording, published, admitted in setUp -- never the tree's
recordings or snapshots, so a fault seeded in those is reported by the check
that guards them, not by these tests.
"""

import json
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
PREFIX = 'export default '  # src/replay/payload.ts
FIXTURE = {'title': 'a fixture', 'migration': ['Scrubbed: nothing; written by this test'], 'carried': {}, 'events': []}


class Admission(unittest.TestCase):
    def setUp(self):
        self.box = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        shutil.copytree(ROOT / 'scripts', self.box / 'scripts', ignore=shutil.ignore_patterns('__pycache__', 'hygiene-admitted-*'))
        (self.box / 'exercise/scripts').mkdir(parents=True)
        shutil.copyfile(ROOT / 'exercise/scripts/admission.py', self.box / 'exercise/scripts/admission.py')
        (self.box / 'exercise/src/replay').mkdir(parents=True)
        (self.box / 'exercise/src/replay/published.ts').write_text("export const PUBLISHED = ['fixture'] as const;\n", encoding='utf-8')
        self.recording = self.box / 'exercise/src/drive/recorded/fixture.json'
        self.recording.parent.mkdir(parents=True)
        self.recording.write_text(json.dumps(FIXTURE), encoding='utf-8')
        admitted = self.run_admission('admit', 'fixture')
        self.assertEqual(admitted.returncode, 0, admitted.stderr)
        self.cited = sorted(self.snapshots())
        self.assertEqual(len(self.cited), 3, 'the fixture is admitted under one snapshot of three files')

    def snapshots(self):
        return {p.relative_to(self.box).as_posix() for p in (self.box / 'scripts').rglob('hygiene-admitted-*')}

    def run_admission(self, *args):
        return subprocess.run([sys.executable, str(self.box / 'exercise/scripts/admission.py'), *args], cwd=self.box, capture_output=True, text=True)

    def plant(self, *names):
        """Copies of a cited patterns table under names no admission gives."""
        source = self.box / next(p for p in self.cited if p.endswith('-patterns.tsv'))
        for name in names:
            (self.box / name).parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, self.box / name)

    def site(self):
        """The payload directory the build writes, from the copied tree."""
        data = self.box / '_site/replay/data'
        data.mkdir(parents=True)
        (data / 'fixture.js').write_text(PREFIX + self.recording.read_text(encoding='utf-8'), encoding='utf-8')
        shutil.copyfile(self.recording.with_name('fixture.admission.json'), data / 'fixture.admission.json')
        return str(data)

    UNNAMED = (
        'scripts/hygiene-admitted-000000000000-patterns.tsv',
        'scripts/hygiene-admitted-ABCDEF012345-patterns.tsv',
        'scripts/hygiene-admitted-000000000000-patterns.tsv.orig',
        'scripts/old/hygiene-admitted-000000000000-patterns.tsv',
    )

    def test_admit_removes_a_snapshot_no_admission_names_and_says_so(self):
        self.plant(*self.UNNAMED)
        run = self.run_admission('admit', 'fixture')
        self.assertEqual(run.returncode, 0, run.stderr)
        for name in self.UNNAMED:
            self.assertIn(f'admission: removed {name}: a snapshot no admission names', run.stdout)
        self.assertEqual(sorted(self.snapshots()), self.cited)

    def test_a_refused_admission_removes_the_snapshot_it_wrote(self):
        with (self.box / 'scripts/hygiene-patterns.tsv').open('a', encoding='utf-8') as live:
            live.write('# the live table, moved on\n')
        self.recording.write_text(json.dumps({**FIXTURE, 'migration': [*FIXTURE['migration'], 'ghp_' + 'A' * 36]}), encoding='utf-8')
        run = self.run_admission('admit', 'fixture')
        self.assertEqual(run.returncode, 1)
        self.assertIn('not admitted', run.stderr)
        self.assertIn('admission: removed scripts/hygiene-admitted-', run.stdout)
        self.assertEqual(sorted(self.snapshots()), self.cited)

    def test_verify_refuses_a_snapshot_no_admission_names(self):
        data = self.site()
        self.assertEqual(self.run_admission('verify', data).returncode, 0)
        self.plant(*self.UNNAMED)
        run = self.run_admission('verify', data)
        self.assertEqual(run.returncode, 1)
        for name in self.UNNAMED:
            self.assertIn(f'admission: {name}: a snapshot no admission names', run.stderr)

    def test_an_unreadable_admission_stops_both_and_removes_nothing(self):
        data = self.site()
        self.plant(self.UNNAMED[0])
        (self.box / 'exercise/src/drive/recorded/stray.admission.json').write_text('{"table": "x"}', encoding='utf-8')
        for args in (('admit', 'fixture'), ('verify', data)):
            run = self.run_admission(*args)
            self.assertEqual(run.returncode, 2, args)
            self.assertIn('admission: exercise/src/drive/recorded/stray.admission.json: not an admission this script can read', run.stderr)
        self.assertIn(self.UNNAMED[0], self.snapshots())


if __name__ == '__main__':
    unittest.main()
