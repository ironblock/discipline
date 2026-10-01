#!/usr/bin/env python3
"""
The results ledger, as a page (#32 I2): rendered from what
`scripts/check-results.py --ledger` emits and from nothing else -- the linter
reads the records through `diet` and checks them; this only draws what it
handed over, so a row on the page is a row the gate passed.

    python3 exercise/scripts/render-ledger.py LEDGER OUT_DIR --results RESULTS

Every directory at its word. One section for results decided by a
pre-registered rule, the rule's digest beside the word (the
`decision-rule*.toml` a claim consumed); a second for results without one --
the two notebook-era directories migrated under gate 0 -- shown with their
product digest and labelled `no rule` (#32, ruling 4). The product digest is
the link to the directory.

Not yet: which issue a result answers, and which directory supersedes which.
Neither is recorded in the results' front matter, and the page draws only what
is recorded; it says so.

Refuses -- exit 1, naming the directory -- a row whose directory is not under
RESULTS, a row with no word, a row with no product digest. Exit 2 if the
ledger holds no row or cannot be read: a ledger of nothing is not a page.
"""

import argparse
import html
import json
import pathlib
import re
import sys

REPO = 'https://github.com/ironblock/discipline'
HEX64 = re.compile(r'[0-9a-f]{64}')

PAGE = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>discipline · results ledger</title>
<style>
  :root {{ color-scheme: light dark; --ink: #1d1f22; --faint: #5c636b; --rule: #d7dade; --page: #fbfbfc; }}
  @media (prefers-color-scheme: dark) {{ :root {{ --ink: #e6e7e9; --faint: #9aa1a9; --rule: #2c3036; --page: #131415; }} }}
  body {{ margin: 0; background: var(--page); color: var(--ink); font: 15px/1.5 system-ui, sans-serif; }}
  main {{ max-width: 64rem; margin: 0 auto; padding: 1.5rem 1rem 3rem; }}
  h1 {{ font-size: 1.4rem; }} h2 {{ font-size: 1.1rem; margin-top: 2rem; }}
  p.note, footer, .claims {{ color: var(--faint); }}
  table {{ width: 100%; border-collapse: collapse; }}
  th, td {{ text-align: left; vertical-align: top; padding: 0.5rem 0.4rem; border-top: 1px solid var(--rule); }}
  code {{ font: 12px ui-monospace, monospace; word-break: break-all; }}
  .word {{ font-weight: 600; white-space: nowrap; }}
  footer {{ margin-top: 3rem; font-size: 0.85rem; }}
</style>
</head>
<body>
<main>
<h1>Results ledger</h1>
<p>Every results directory in this repository at its word, rendered on every build from what the results linter
(<code>scripts/check-results.py</code>) read and checked through <code>diet</code>. Nothing here is written by hand.</p>
<p class="note">Not shown yet: which issue each result answers, and which directory supersedes which. Neither is recorded in
the results' front matter yet (#32), and this page draws only what is recorded.</p>
<h2>Results under a pre-registered rule</h2>
{ruled}
<h2>Results without a pre-registered rule</h2>
<p class="note">Migrated under gate 0 from before rules were pre-registered: no rule decided their word.</p>
{unruled}
<footer><a href="../">discipline</a> · <a href="{repo}">{repo_short}</a> · Apache-2.0</footer>
</main>
</body>
</html>
"""


def esc(value: object) -> str:
    return html.escape(str(value), quote=True)


def claims_of(row: dict) -> str:
    items = ''.join(f'<li><code>{esc(c.get("id"))}</code>: {esc(c.get("result"))}</li>' for c in row.get('claims', []))
    return f'<ul class="claims">{items}</ul>' if items else ''


def table(rows: list[dict], ruled: bool) -> str:
    if not rows:
        return '<p class="note">None.</p>'
    head = '<tr><th>result</th><th>word</th><th>rule</th><th>product</th></tr>'
    body = []
    for row in rows:
        directory, link = row['directory'], f'{REPO}/tree/main/results/{row["directory"]}'
        rule = '<br>'.join(f'<code>{esc(r.get("path"))}</code> <code>{esc(r.get("sha256"))}</code>' for r in row.get('rules', [])) if ruled else 'no rule'
        body.append(
            '<tr>'
            f'<td><a href="{esc(link)}">{esc(directory)}</a><br>{esc(row.get("hypothesis", ""))}{claims_of(row)}</td>'
            f'<td class="word">{esc(row["result"])}</td>'
            f'<td>{rule}</td>'
            f'<td><a href="{esc(link)}"><code>{esc(row["product_sha256"])}</code></a></td>'
            '</tr>'
        )
    return f'<table>{head}{"".join(body)}</table>'


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('ledger', type=pathlib.Path)
    parser.add_argument('out', type=pathlib.Path)
    parser.add_argument('--results', type=pathlib.Path, required=True)
    args = parser.parse_args(argv)
    try:
        rows = json.loads(args.ledger.read_text(encoding='utf-8')).get('directories', [])
    except (OSError, ValueError) as err:
        print(f'render-ledger: {args.ledger}: cannot be read: {err}', file=sys.stderr)
        return 2
    if not rows:
        print(f'render-ledger: {args.ledger}: holds no result; a ledger of nothing is not a page', file=sys.stderr)
        return 2
    refused = []
    for row in rows:
        name = row.get('directory') or '(no directory)'
        if not row.get('directory') or not (args.results / row['directory']).is_dir():
            refused.append(f'render-ledger: {name}: no such directory under {args.results}')
        if not isinstance(row.get('result'), str) or not row['result']:
            refused.append(f'render-ledger: {name}: row carries no result')
        if not isinstance(row.get('product_sha256'), str) or not HEX64.fullmatch(row['product_sha256']):
            refused.append(f'render-ledger: {name}: row carries no product digest')
    if refused:
        for line in refused:
            print(line, file=sys.stderr)
        return 1
    rows = sorted(rows, key=lambda r: r['directory'])
    page = PAGE.format(
        ruled=table([r for r in rows if r.get('rules')], ruled=True),
        unruled=table([r for r in rows if not r.get('rules')], ruled=False),
        repo=REPO,
        repo_short=REPO.removeprefix('https://'),
    )
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / 'index.html').write_text(page, encoding='utf-8')
    print(f'render-ledger: {len(rows)} result(s), {sum(1 for r in rows if r.get("rules"))} under a pre-registered rule, to {args.out / "index.html"}')
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
