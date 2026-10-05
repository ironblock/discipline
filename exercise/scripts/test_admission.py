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
        (self.box / 'exercise/src/replay/published.ts').write_text("export const PUBLISHED = ['fixture'] as const;\nexport const EXAMPLES = [] as const;\n", encoding='utf-8')
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
        return subprocess.run([sys.executable, str(self.box / 'exercise/scripts/admission.py'), *args], cwd=self.box, capture_output=True, text=True, errors='replace')

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
        top, deeper = self.UNNAMED[:-1], self.UNNAMED[-1]
        for name in top:
            self.assertIn(f'admission: removed {name}: a snapshot no admission names', run.stdout)
        # Below scripts/, where admit never writes one, it is someone else's file: named, left, and refused by verify.
        self.assertIn(f'admission: {deeper}: named like a snapshot no admission names, below scripts/ where admit never writes one; not removed', run.stdout)
        self.assertEqual(sorted(self.snapshots()), sorted([*self.cited, deeper]))

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

    def unread(self, expected, *commands):
        """Each command exits 2 naming EXPECTED, and admit -- under a live table that moved -- writes and removes nothing."""
        admission = self.recording.with_name('fixture.admission.json')
        before = (admission.read_bytes(), sorted(self.snapshots()))
        with (self.box / 'scripts/hygiene-patterns.tsv').open('a', encoding='utf-8') as live:
            live.write('# the live table, moved on\n')
        for args in commands:
            run = self.run_admission(*args)
            self.assertEqual(run.returncode, 2, (args, run.stdout, run.stderr))
            self.assertIn(expected, run.stderr, args)
        self.assertEqual((admission.read_bytes(), sorted(self.snapshots())), before)

    def test_an_unreadable_admission_stops_every_command_and_writes_nothing(self):
        data = self.site()
        self.plant(self.UNNAMED[0])
        (self.box / 'exercise/src/drive/recorded/stray.admission.json').write_text('{"table": "x"}', encoding='utf-8')
        self.unread('admission: exercise/src/drive/recorded/stray.admission.json: not an admission this script can read', ('admit', 'fixture'), ('verify', data), ('tables', data))

    def test_a_published_admission_that_does_not_parse_stops_every_command(self):
        data = self.site()
        for copy in (self.recording.with_name('fixture.admission.json'), pathlib.Path(data) / 'fixture.admission.json'):
            copy.write_text('{', encoding='utf-8')
        self.unread('admission: exercise/src/drive/recorded/fixture.admission.json: not an admission this script can read', ('admit', 'fixture'), ('verify', data), ('tables', data))

    def test_an_admission_with_no_digest_stops_every_command(self):
        data = self.site()
        for copy in (self.recording.with_name('fixture.admission.json'), pathlib.Path(data) / 'fixture.admission.json'):
            admission = json.loads(copy.read_text(encoding='utf-8'))
            del admission['table']['hashes']['sha256']
            copy.write_text(json.dumps(admission, indent=2) + '\n', encoding='utf-8')
        self.unread('admission: exercise/src/drive/recorded/fixture.admission.json: not an admission this script can read', ('admit', 'fixture'), ('verify', data), ('tables', data))

    def test_an_admission_that_is_a_directory_stops_every_command(self):
        data = self.site()
        (self.box / 'exercise/src/drive/recorded/stray.admission.json').mkdir()
        self.unread('admission: exercise/src/drive/recorded/stray.admission.json: not an admission this script can read', ('admit', 'fixture'), ('verify', data), ('tables', data))

    def test_a_link_to_a_cited_snapshot_under_an_unnamed_id_is_unnamed(self):
        cited = self.box / next(p for p in self.cited if p.endswith('-patterns.tsv'))
        symlink = self.box / 'scripts/hygiene-admitted-000000000000-patterns.tsv'
        hardlink = self.box / 'scripts/hygiene-admitted-111111111111-patterns.tsv'
        symlink.symlink_to(cited.name)
        hardlink.hardlink_to(cited)
        names = [p.relative_to(self.box).as_posix() for p in (symlink, hardlink)]
        refused = self.run_admission('verify', self.site())
        self.assertEqual(refused.returncode, 1)
        for name in names:
            self.assertIn(f'admission: {name}: a snapshot no admission names', refused.stderr)
        run = self.run_admission('admit', 'fixture')
        self.assertEqual(run.returncode, 0, run.stderr)
        for name in names:
            self.assertIn(f'admission: removed {name}: a snapshot no admission names', run.stdout)
        self.assertEqual(sorted(self.snapshots()), self.cited)

    def test_a_cited_snapshot_that_is_a_link_stops_every_command(self):
        data = self.site()
        cited = self.box / next(p for p in self.cited if p.endswith('-patterns.tsv'))
        target = cited.with_name('hygiene-admitted-000000000000-patterns.tsv')
        cited.rename(target)
        cited.symlink_to(target.name)
        self.unread(f'its table lists {cited.relative_to(self.box).as_posix()}, which is a link', ('admit', 'fixture'), ('verify', data), ('tables', data))
        self.assertTrue(target.exists())

    def test_a_digest_that_is_not_a_string_stops_every_command(self):
        data = self.site()
        for copy in (self.recording.with_name('fixture.admission.json'), pathlib.Path(data) / 'fixture.admission.json'):
            admission = json.loads(copy.read_text(encoding='utf-8'))
            admission['table']['hashes']['sha256'] = None
            copy.write_text(json.dumps(admission, indent=2) + '\n', encoding='utf-8')
        self.unread('its table gives a snapshot digest that is not a string', ('admit', 'fixture'), ('verify', data), ('tables', data))

    def test_an_unreadable_admission_says_the_way_out(self):
        (self.box / 'exercise/src/drive/recorded/stray.admission.json').write_text('{', encoding='utf-8')
        run = self.run_admission('admit', 'fixture')
        self.assertEqual(run.returncode, 2)
        self.assertIn('Restore that file from git, or delete it, then admit again.', run.stderr)

    def test_a_cited_snapshot_under_another_spelling_of_its_name_is_kept(self):
        patterns = self.box / next(p for p in self.cited if p.endswith('-patterns.tsv'))
        respelled = patterns.with_name(patterns.name.replace('-patterns.tsv', '-Patterns.tsv'))
        patterns.rename(respelled)
        if not patterns.exists():
            self.skipTest('this disk tells names apart by case, so the cited path no longer reaches the file')
        run = self.run_admission('admit', 'fixture')
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertNotIn('removed', run.stdout)
        self.assertTrue(respelled.exists())
        self.assertEqual(self.run_admission('verify', self.site()).returncode, 0)

    def test_a_snapshot_path_in_another_form_is_not_read_as_naming_it(self):
        data = self.site()
        for copy in (self.recording.with_name('fixture.admission.json'), pathlib.Path(data) / 'fixture.admission.json'):
            admission = json.loads(copy.read_text(encoding='utf-8'))
            for part in ('patterns', 'exceptions', 'hashes'):
                admission['table'][part]['path'] = './' + admission['table'][part]['path']
            copy.write_text(json.dumps(admission, indent=2) + '\n', encoding='utf-8')
        self.unread("where admit writes 'scripts/hygiene-admitted-", ('admit', 'fixture'), ('verify', data), ('tables', data))


    # ---- #372's record half: the assets a recording's tool calls left ----

    SHOT = b'\x89PNG\r\n\x1a\n a canvas, nothing in it'

    def with_asset(self, *, header=(), body=None, extra=None, commit=True, kind='ask'):
        """The fixture recording with one line naming a PNG -- the operator's ask by default, T1's path -- the asset committed beside it under its digest."""
        import hashlib
        digest = hashlib.sha256(self.SHOT).hexdigest()
        recording = dict(FIXTURE, migration=[*FIXTURE['migration'], *(line.format(digest=digest) for line in header)])
        recording['events'] = [{'kind': kind, 'seq': 1, 't': 5, 'files': [{'path': 'files/shot.png', 'sha256': digest, 'media_type': 'image/png', 'bytes': len(self.SHOT)}]}]
        self.recording.write_text(json.dumps(recording), encoding='utf-8')
        files = self.recording.parent / 'fixture/files'
        files.mkdir(parents=True, exist_ok=True)
        if commit:
            (files / digest).write_bytes(self.SHOT if body is None else body)
        if extra:
            (files / extra).write_bytes(b'stray')
        return digest

    def admitted_files(self):
        return json.loads(self.recording.with_name('fixture.admission.json').read_text(encoding='utf-8')).get('files')

    def test_an_asset_declared_clean_is_admitted_declared_clean(self):
        digest = self.with_asset(header=['Declared-clean: {digest} by the maintainer on 2026-10-04'])
        run = self.run_admission('admit', 'fixture')
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(self.admitted_files(), [{'sha256': digest, 'path': 'files/shot.png', 'media_type': 'image/png', 'bytes': len(self.SHOT), 'scrub': 'declared-clean', 'declared': f'Declared-clean: {digest} by the maintainer on 2026-10-04'}])

    def test_a_credential_shape_inside_a_binary_asset_is_refused(self):
        # Spelled in two halves so this file carries no key shape of its own. Run as admit runs, with the default
        # environment: the scan must read the asset as bytes whatever the locale (#464's review, NB1).
        self.SHOT = b'\x89PNG\r\n\x1a\n\x00\x00 ' + b'AKIA' + b'ABCDEFGHIJKLMNOP' + b' \x00\xff'
        self.with_asset(header=['Declared-clean: {digest} by the maintainer on 2026-10-04'])
        run = self.run_admission('admit', 'fixture')
        self.assertEqual(run.returncode, 1, run.stdout + run.stderr)
        self.assertIn('not admitted; the genesis table finds something in it', run.stderr)

    def test_a_tool_calls_asset_is_admitted_as_an_asks_is(self):
        digest = self.with_asset(kind='tool_call', header=['Declared-clean: {digest} by the maintainer on 2026-10-04'])
        run = self.run_admission('admit', 'fixture')
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual([(row['sha256'], row['scrub']) for row in self.admitted_files()], [(digest, 'declared-clean')])

    def test_an_asset_with_no_declaration_is_admitted_undeclared(self):
        digest = self.with_asset()
        run = self.run_admission('admit', 'fixture')
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual([(row['sha256'], row['scrub']) for row in self.admitted_files()], [(digest, 'undeclared')])

    def test_an_asset_whose_bytes_are_not_its_digest_is_refused(self):
        digest = self.with_asset(body=b'other bytes')
        run = self.run_admission('admit', 'fixture')
        self.assertEqual(run.returncode, 1)
        self.assertIn(f'fixture/files/{digest}: its bytes hash to', run.stderr)

    def test_an_asset_not_committed_beside_the_recording_is_refused(self):
        digest = self.with_asset(commit=False)
        run = self.run_admission('admit', 'fixture')
        self.assertEqual(run.returncode, 1)
        self.assertIn(f'fixture/files/{digest}: named by a line\'s files and not committed beside the recording', run.stderr)

    def test_a_file_beside_the_recording_that_no_line_names_is_refused(self):
        self.with_asset(extra='stray.bin')
        run = self.run_admission('admit', 'fixture')
        self.assertEqual(run.returncode, 1)
        self.assertIn('fixture/files/stray.bin: no line\'s files names it', run.stderr)

    def test_a_declaration_of_a_digest_no_line_names_is_refused(self):
        self.with_asset(header=['Declared-clean: ' + '0' * 64 + ' by the maintainer on 2026-10-04'])
        run = self.run_admission('admit', 'fixture')
        self.assertEqual(run.returncode, 1)
        self.assertIn('declares a digest no line\'s files names', run.stderr)


if __name__ == '__main__':
    unittest.main()
