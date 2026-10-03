#!/usr/bin/env python3
"""
The results ledger, as a page (#32 I2): rendered from what
`scripts/check-results.py --ledger` emits and from nothing else -- the linter
reads the records through `diet` and checks them; this only draws what it
handed over, so a row on the page is a row the gate passed.

    python3 exercise/scripts/render-ledger.py LEDGER OUT_DIR --results RESULTS [--commit SHA]

Every directory at its word. One section for results whose claims consumed a
rule file -- a `decision-rule*.toml` -- the rule's digest beside the word; a
second for results that consumed none, shown with their product digest and
labelled `no rule` (#32, ruling 4: today, the two notebook-era directories).
The page says no more about a rule than the record does: whether a rule file
was written before its numbers is in its own header, not in the record, so
the page does not call it pre-registered. The product digest is the link to
the directory, at the commit the page was rendered from (--commit, else
`HEAD`, which GitHub resolves to the default branch: no branch is named here,
#326).

Not yet: which issue a result answers, and which directory supersedes which.
Neither is recorded in the results' front matter, and the page draws only what
is recorded; it says so.

Refuses -- exit 1, naming the directory -- a row whose directory is not a
results directory name (`YYYY-MM-DD-<slug>`, one path segment) under
RESULTS, a row with no word, a row with no product digest, a rule that is not
a rule file with its digest. Exit 2 if the ledger cannot be read, is not the
shape the emitter writes, or holds no row: a ledger of nothing is not a page.
"""

import argparse
import html
import json
import pathlib
import re
import sys

REPO = 'https://github.com/ironblock/discipline'
HEX64 = re.compile(r'[0-9a-f]{64}')
# A results directory's name, as the results linter requires it (scripts/check-results.py DIR_NAME): one segment.
RESULTS_NAME = re.compile(r'[0-9]{4}-[0-9]{2}-[0-9]{2}-[a-z0-9]+(?:-[a-z0-9]+)*')
RULE_FILE = re.compile(r'decision-rule[^/]*\.toml')

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
<h2>Results decided by a rule file</h2>
<p class="note">Each consumed a <code>decision-rule*.toml</code>; its digest is beside the word. When the rule was written is in
the rule file's own header, which this page does not read.</p>
{ruled}
<h2>Results with no rule file</h2>
<p class="note">These consumed no <code>decision-rule*.toml</code>: no rule file decided their word.</p>
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


def table(rows: list[dict], ruled: bool, commit: str) -> str:
    if not rows:
        return '<p class="note">None.</p>'
    head = '<tr><th>result</th><th>word</th><th>rule</th><th>product</th></tr>'
    body = []
    for row in rows:
        directory, link = row['directory'], f'{REPO}/tree/{commit}/results/{row["directory"]}'
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


def refusals(row: dict, results: pathlib.Path) -> list[str]:
    """What is wrong with one row, each naming its directory; empty if it renders."""
    directory = row.get('directory')
    name = directory if isinstance(directory, str) and directory else '(no directory)'
    found = []
    if not isinstance(directory, str) or not RESULTS_NAME.fullmatch(directory):
        found.append(f'render-ledger: {name}: is not a results directory name (YYYY-MM-DD-<slug>, one path segment)')
    elif not (results / directory).is_dir():
        found.append(f'render-ledger: {name}: no such directory under {results}')
    if not isinstance(row.get('result'), str) or not row['result']:
        found.append(f'render-ledger: {name}: row carries no result')
    if not isinstance(row.get('product_sha256'), str) or not HEX64.fullmatch(row['product_sha256']):
        found.append(f'render-ledger: {name}: row carries no product digest')
    rules = row.get('rules', [])
    if not isinstance(rules, list):
        found.append(f'render-ledger: {name}: its rules are not a list')
    else:
        for rule in rules:
            if not (isinstance(rule, dict) and isinstance(rule.get('path'), str) and RULE_FILE.fullmatch(rule['path'])
                    and isinstance(rule.get('sha256'), str) and HEX64.fullmatch(rule['sha256'])):
                found.append(f'render-ledger: {name}: a rule that is not a decision-rule*.toml with its sha256: {rule!r}'[:300])
    claims = row.get('claims', [])
    if not isinstance(claims, list) or not all(isinstance(c, dict) for c in claims):
        found.append(f'render-ledger: {name}: its claims are not a list of claims')
    return found


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('ledger', type=pathlib.Path)
    parser.add_argument('out', type=pathlib.Path)
    parser.add_argument('--results', type=pathlib.Path, required=True)
    parser.add_argument('--commit', default='HEAD', help='the commit the page links into (default: HEAD, the default branch)')
    args = parser.parse_args(argv)
    try:
        ledger = json.loads(args.ledger.read_text(encoding='utf-8'))
    except (OSError, ValueError) as err:
        print(f'render-ledger: {args.ledger}: cannot be read: {err}', file=sys.stderr)
        return 2
    rows = ledger.get('directories') if isinstance(ledger, dict) else None
    if not isinstance(rows, list) or not all(isinstance(r, dict) for r in rows):
        print(f'render-ledger: {args.ledger}: not a ledger (an object whose `directories` is a list of rows)', file=sys.stderr)
        return 2
    if not rows:
        print(f'render-ledger: {args.ledger}: holds no result; a ledger of nothing is not a page', file=sys.stderr)
        return 2
    if not re.fullmatch(r'[0-9a-f]{7,40}|HEAD', args.commit):
        print(f'render-ledger: --commit {args.commit!r} is not a commit', file=sys.stderr)
        return 2
    refused = [line for row in rows for line in refusals(row, args.results)]
    if refused:
        for line in refused:
            print(line, file=sys.stderr)
        return 1
    rows = sorted(rows, key=lambda r: r['directory'])
    page = PAGE.format(
        ruled=table([r for r in rows if r.get('rules')], ruled=True, commit=args.commit),
        unruled=table([r for r in rows if not r.get('rules')], ruled=False, commit=args.commit),
        repo=REPO,
        repo_short=REPO.removeprefix('https://'),
    )
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / 'index.html').write_text(page, encoding='utf-8')
    print(f'render-ledger: {len(rows)} result(s), {sum(1 for r in rows if r.get("rules"))} decided by a rule file, to {args.out / "index.html"}')
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
