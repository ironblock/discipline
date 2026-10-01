#!/usr/bin/env python3
"""
render-ledger.py's own tests (#32 I2). Run directly:

    python3 exercise/scripts/test_render_ledger.py

Each test builds a ledger the way `check-results.py --ledger` emits one, over a
results root of its own, and runs the renderer as the gate runs it.
"""

import json
import pathlib
import subprocess
import sys
import tempfile
import unittest

RENDER = pathlib.Path(__file__).resolve().parent / 'render-ledger.py'
DIGEST = 'a' * 64
RULE = {'path': 'decision-rule.toml', 'sha256': 'b' * 64}


def row(directory, result='supported', rules=(RULE,), **extra):
    base = {
        'directory': directory,
        'result': result,
        'product_sha256': DIGEST,
        'hypothesis': 'A <hypothesis> & its claim.',
        'claims': [{'id': 'c1', 'result': result, 'consumes': list(rules)}],
        'rules': list(rules),
    }
    base.update(extra)
    return base


class Rendered(unittest.TestCase):
    def render(self, rows, present=None):
        """Run the renderer over ROWS; PRESENT names the directories that exist (default: every row's)."""
        box = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        results, out = box / 'results', box / 'out'
        for name in present if present is not None else [r.get('directory') for r in rows]:
            (results / name).mkdir(parents=True)
        ledger = box / 'ledger.json'
        ledger.write_text(json.dumps({'version': 1, 'directories': rows}), encoding='utf-8')
        run = subprocess.run([sys.executable, str(RENDER), str(ledger), str(out), '--results', str(results)], capture_output=True, text=True)
        page = (out / 'index.html').read_text(encoding='utf-8') if (out / 'index.html').is_file() else None
        return run, page

    def test_a_row_naming_a_directory_not_there_is_refused_naming_it(self):
        run, page = self.render([row('2026-01-01-here'), row('2026-01-02-gone')], present=['2026-01-01-here'])
        self.assertEqual(run.returncode, 1)
        self.assertIn('render-ledger: 2026-01-02-gone: no such directory', run.stderr)
        self.assertIsNone(page)

    def test_a_row_with_no_word_is_refused_naming_its_directory(self):
        bare = row('2026-01-01-wordless')
        del bare['result']
        run, page = self.render([bare])
        self.assertEqual(run.returncode, 1)
        self.assertIn('render-ledger: 2026-01-01-wordless: row carries no result', run.stderr)
        self.assertIsNone(page)

    def test_a_row_with_no_product_digest_is_refused(self):
        run, _ = self.render([row('2026-01-01-undigested', product_sha256='')])
        self.assertEqual(run.returncode, 1)
        self.assertIn('render-ledger: 2026-01-01-undigested: row carries no product digest', run.stderr)

    def test_a_ledger_of_nothing_is_not_a_page(self):
        run, page = self.render([])
        self.assertEqual(run.returncode, 2)
        self.assertIsNone(page)

    def test_a_result_with_no_rule_is_in_its_own_section_labelled_no_rule(self):
        run, page = self.render([row('2026-01-01-ruled'), row('2026-01-02-notebook', rules=())])
        self.assertEqual(run.returncode, 0, run.stderr)
        ruled, unruled = page.split('Results without a pre-registered rule')
        self.assertIn('2026-01-01-ruled', ruled)
        self.assertNotIn('2026-01-02-notebook', ruled)
        self.assertIn('2026-01-02-notebook', unruled)
        self.assertIn('no rule', unruled)

    def test_the_rule_digest_and_the_product_digest_are_both_there(self):
        _, page = self.render([row('2026-01-01-ruled')])
        self.assertIn('b' * 64, page)
        self.assertIn(DIGEST, page)

    def test_every_row_links_its_directory(self):
        _, page = self.render([row('2026-01-01-one'), row('2026-01-02-two', rules=())])
        for name in ('2026-01-01-one', '2026-01-02-two'):
            self.assertIn(f'href="https://github.com/ironblock/discipline/tree/main/results/{name}"', page)

    def test_an_unadjudicated_word_is_shown_as_it_is(self):
        _, page = self.render([row('2026-01-01-pending', result='unadjudicated')])
        self.assertIn('>unadjudicated<', page)

    def test_it_says_what_it_does_not_show_yet(self):
        _, page = self.render([row('2026-01-01-one')])
        self.assertIn('issue', page)
        self.assertIn('supersed', page)

    def test_record_text_is_escaped(self):
        _, page = self.render([row('2026-01-01-one')])
        self.assertIn('A &lt;hypothesis&gt; &amp; its claim.', page)
        self.assertNotIn('<hypothesis>', page)


if __name__ == '__main__':
    unittest.main()
