#!/usr/bin/env python3
"""
One-time migration: a session the predecessor recorded, into the surface's
provisional event vocabulary (src/drive/events.ts), as a fixture.

    python3 scripts/migrate-recorded.py <events.jsonl> <out.json> --scrub <name> [--scrub <name> ...]

Not a second parser of a `diet` format: the predecessor's log is not one, and
this runs once, by hand, with its output reviewed and committed. Every rule it
applies is listed in the output's `migration` header, so a reader of the
fixture knows which values are the record's and which are the migration's.

PRIVACY. Every string in the output is scrubbed: each `--scrub` name (the
account the session ran under) and the home directory it owns become `user`
and `/work`, the predecessor's run directories become `/tmp/session`, and its
internal ticket ids are removed. The script then refuses to write if any
scrubbed name, a home-directory path, or a ticket id survives anywhere in the
output. The repository's hygiene gate checks the committed file again.
"""

import argparse
import json
import re
import sys

# Human reading time is real but long (minutes); a replay caps each gap so a
# session plays in minutes. Only gaps between turns are capped.
MAX_GAP_MS = 30_000
# Bounds on what the fixture carries of large bodies. The cut is marked in the text.
MAX_OUTPUT_LINES = 120
MAX_OUTPUT_CHARS = 12_000
MAX_QUESTION_CHARS = 1_500
MAX_RENDER_CHARS = 24_000


def cap(text, max_chars, max_lines=None, what='the fixture migration'):
    lines = text.split('\n')
    cut = False
    if max_lines is not None and len(lines) > max_lines:
        lines, cut = lines[:max_lines], True
    out = '\n'.join(lines)
    if len(out) > max_chars:
        out, cut = out[:max_chars], True
    if cut:
        total = text.count('\n') + 1
        out += f'\n… [{total:,} lines, {len(text):,} characters in the record; cut here by {what}]'
    return out


def scrubber(names):
    rules = []
    for name in names:
        rules.append((re.compile(r'/Users/' + re.escape(name) + r'\b'), '/work'))
        rules.append((re.compile(r'/home/' + re.escape(name) + r'\b'), '/work'))
        rules.append((re.compile(re.escape(name), re.IGNORECASE), 'user'))
    rules.append((re.compile(r'/tmp/die\d+-[0-9TZ]+'), '/tmp/session'))
    rules.append((re.compile(r'\bdie-?\d+(-[\w-]+)?', re.IGNORECASE), 'session'))

    def scrub(value):
        if isinstance(value, str):
            for pattern, replacement in rules:
                value = pattern.sub(replacement, value)
            return value
        if isinstance(value, list):
            return [scrub(v) for v in value]
        if isinstance(value, dict):
            return {k: scrub(v) for k, v in value.items()}
        return value

    return scrub


def timings(t):
    if not t:
        return {'prompt_n': 0, 'cache_n': 0, 'prompt_ms': 0, 'predicted_n': 0, 'predicted_ms': 0}
    return {k: t.get(k, 0) for k in ('prompt_n', 'cache_n', 'prompt_ms', 'predicted_n', 'predicted_ms')}


STOPS = {'tool_call': 'tool', 'final': 'stop'}

# The predecessor's protocol: a reply ends in one ```bash block, and that block
# is the tool call. Here a tool call is its own event, so the block leaves the text.
TRAILING_CALL = re.compile(r'\n*```(?:bash|sh)?\n.*?\n```\s*$', re.S)


def migrate(records):
    by_id = {r['id']: r for r in records}
    begin = next(r for r in records if r['event'] == 'run.begin')
    responses = {r['parent_id']: r for r in records if r['event'] in ('turn.response', 'fork.response')}

    # Session time: ms from the first request. Time is kept wherever anything
    # runs (a turn, a tool, a side call); a stretch where nothing runs -- a
    # person reading -- is cut to MAX_GAP_MS. Monotonic, so order survives.
    stamps = sorted({r['start'] for r in records if r['event'] != 'run.begin'} | {r['end'] for r in records if r.get('end') and r['event'] != 'run.begin'})
    active = []
    turn_start = {}
    for r in records:
        if r['event'] == 'turn.request' and r.get('step') == 0:
            turn_start[r['turn']] = r['start']
        if r['event'] == 'turn.settled' and r['turn'] in turn_start:
            active.append((turn_start[r['turn']], r['start']))
        if r['event'] in ('tool.exec', 'fork.response', 'turn.response') and r.get('end'):
            parent = by_id.get(r['parent_id'], {})
            active.append((parent.get('start', r['start']) if r['event'] != 'tool.exec' else r['start'], r['end']))
    mapped = {stamps[0]: 0.0}
    for a, b in zip(stamps, stamps[1:]):
        mid = (a + b) / 2
        gap = (b - a) * 1000
        if not any(s0 <= mid <= s1 for s0, s1 in active):
            gap = min(gap, MAX_GAP_MS)
        mapped[b] = mapped[a] + gap

    def clock(stamp):
        # Anything before the first request (the run's own start) is time 0.
        return round(mapped.get(stamp, 0.0))

    # Slots: the trunk holds 0; each fork takes the lowest slot free for its span.
    fork_span = {}
    for r in records:
        if r['event'] == 'fork.request':
            resp = responses.get(r['id'])
            fork_span[r['id']] = (r['start'], resp['end'] if resp else r['start'])
    busy, slot_of = [], {}
    for fid, (s, e) in sorted(fork_span.items(), key=lambda kv: kv[1][0]):
        slot = 1
        while any(b_slot == slot and b_end > s for b_slot, b_end in busy):
            slot += 1
        slot_of[fid] = slot
        busy.append((slot, e))
    slots = 1 + max(slot_of.values(), default=1)

    first_request = next(r for r in records if r['event'] == 'turn.request')
    system = first_request['messages'][0]['content']

    out = [{
        'kind': 'session.start', 't': 0, 'arm': 'recorded', 'model': begin.get('model_id', 'not recorded'),
        'slots': slots, 'trunk_slot': 0, 'phase': 'not recorded', 'system': {'text': system},
    }]
    last_trunk_response = None
    last_settled_turn = 0
    render_hash = 'not recorded'
    classified = {'mimicry': 0, 'empty': 0}
    pending_seam = None
    entry_text = {}
    called = {r['parent_id'] for r in records if r['event'] == 'tool.exec'}

    for r in records:
        kind = r['event']
        t0, t1 = clock(r['start']), clock(r.get('end') or r['start'])
        if kind == 'turn.request':
            if r.get('step') == 0:
                if pending_seam is not None:
                    # The predecessor refilled the trunk with the render as its first user message.
                    render = next((m['content'] for m in r['messages'][1:] if m['content'].startswith('COMPACTED WORKING RECORD')), '')
                    pending_seam['render']['text'] = cap(render, MAX_RENDER_CHARS)
                    out.append(pending_seam)
                    pending_seam = None
                out.append({'kind': 'ask', 't': t0, 'turn': r['turn'], 'text': r['messages'][-1]['content']})
            out.append({'kind': 'request', 't': t0, 'id': r['id'], 'lane': 'trunk', 'slot': 0, 'turn': r['turn']})
        elif kind == 'turn.response':
            last_trunk_response = r['id']
            text = r.get('content') or ''
            if r['id'] in called:
                text = TRAILING_CALL.sub('', text)
            ev = {'kind': 'response', 't': t1, 'id': r['id'], 'to_request': r['parent_id'], 'text': text,
                  'stop': STOPS.get(r.get('stop'), r.get('stop') or 'stop'), 'timings': timings(r.get('timings'))}
            if r.get('reasoning'):
                ev['reasoning'] = r['reasoning']
            out.append(ev)
        elif kind == 'tool.exec':
            output = (r.get('stdout') or '') + (('\n' if r.get('stdout') else '') + r['stderr'] if r.get('stderr') else '')
            out.append({'kind': 'tool.begin', 't': t0, 'id': r['id'], 'turn': r['turn'], 'after': r['parent_id'], 'tool': 'bash', 'args': {'command': r['command']}})
            end = {'kind': 'tool.end', 't': max(t1, t0 + int(r.get('duration_ms') or 0)), 'id': r['id'], 'exit': r['exit'],
                   'output': cap(output, MAX_OUTPUT_CHARS, MAX_OUTPUT_LINES)}
            if r.get('truncated'):
                end['truncated'] = True
            out.append(end)
        elif kind == 'turn.settled':
            last_settled_turn = r['turn']
            out.append({'kind': 'turn.settled', 't': t0, 'turn': r['turn'], 'reason': r.get('reason') or 'final'})
        elif kind == 'fork.request':
            resp = responses.get(r['id'])
            ask = (resp or {}).get('ask') or r['lane']
            # The audit at a phase boundary is what the charter calls ratify.
            lane = 'ratify' if ask.startswith('audit-') or ask == 'seam-add' else r['lane']
            parent = by_id.get(r['parent_id'])
            at = parent['id'] if parent and parent['event'] in ('turn.response', 'tool.exec') else last_trunk_response
            out.append({'kind': 'fork', 't': t0, 'id': r['id'], 'lane': lane, 'slot': slot_of[r['id']], 'of_turn': r.get('parent_turn', 0),
                        'at': at, 'why': ask.split(':')[0], 'question': cap(r['messages'][-1]['content'], MAX_QUESTION_CHARS),
                        'prefix_tokens': ((resp or {}).get('timings') or {}).get('cache_n', 0)})
            out.append({'kind': 'request', 't': t0, 'id': r['id'] + '/q', 'lane': lane, 'slot': slot_of[r['id']], 'turn': r.get('parent_turn', 0), 'fork': r['id']})
        elif kind == 'fork.response':
            content = r.get('content') or ''
            ev = {'kind': 'response', 't': t1, 'id': r['id'], 'to_request': r['parent_id'] + '/q', 'text': content, 'stop': 'stop', 'timings': timings(r.get('timings'))}
            if r.get('reasoning'):
                ev['reasoning'] = r['reasoning']
            out.append(ev)
            if not content.strip():
                outcome = 'empty'
            elif '```bash' in content:
                # Classified by this migration: a side call that answers with a
                # shell command has answered as the agent, not as asked.
                outcome = 'mimicry'
            else:
                outcome = 'complete'
            classified[outcome] = classified.get(outcome, 0) + 1
            out.append({'kind': 'fork.settled', 't': t1, 'id': r['parent_id'], 'outcome': outcome})
        elif kind == 'object.patch':
            parent = by_id.get(r['parent_id'])
            source = parent['parent_id'] if parent and parent['event'] == 'fork.response' else r['parent_id']
            added = [re.match(r'#(\d+)\s?(.*)', a, re.S) for a in r.get('added') or []]
            added = [(m.group(1), m.group(2)) for m in added if m]
            superseded = [s.lstrip('#') for s in r.get('superseded') or []]
            common = {'kind': 'patch', 't': t0, 'from': source}
            if r.get('provenance'):
                common['provenance'] = r['provenance']
            for i, (eid, text) in enumerate(added):
                entry_text[eid] = text
                ev = {**common, 'id': f"{r['id']}/{eid}", 'op': 'add', 'entry': {'id': eid, 'text': text}}
                if i == 0 and superseded:
                    ev['op'], ev['supersedes'] = 'supersede', superseded[0]
                out.append(ev)
            for sid in superseded[1 if added else 0:]:
                out.append({**common, 'id': f"{r['id']}/-{sid}", 'op': 'retire', 'entry': {'id': sid, 'text': entry_text.get(sid, '')}})
        elif kind == 'seam.render':
            before, render_hash = render_hash, r.get('object_hash') or 'not recorded'
            pending_seam = {'kind': 'seam', 't': t0, 'id': r['id'], 'at_turn': last_settled_turn, 'reason': 'not recorded',
                            'prefix_hash_before': before, 'prefix_hash_after': render_hash,
                            'render': {'version': r.get('render_version') or 0, 'text': ''}}
        elif kind == 'run.end':
            out.append({'kind': 'session.end', 't': t0})

    out.sort(key=lambda e: e['t'])
    return out, slots, classified


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[1])
    parser.add_argument('source')
    parser.add_argument('target')
    parser.add_argument('--scrub', action='append', required=True, help='a name to remove everywhere (the account the session ran under)')
    parser.add_argument('--title', required=True)
    args = parser.parse_args()

    records = [json.loads(line) for line in open(args.source)]
    events, slots, classified = migrate(records)
    scrub = scrubber(args.scrub)
    events = scrub(events)
    fixture = {
        'title': args.title,
        'migration': [
            'Recorded by the predecessor harness against a local model, and migrated once into the provisional vocabulary by scripts/migrate-recorded.py.',
            f'Session time is milliseconds from the first request; a stretch where nothing runs (a person reading) longer than {MAX_GAP_MS // 1000} s is cut to {MAX_GAP_MS // 1000} s.',
            f'Slots are assigned by the migration: the trunk holds 0, each side call the lowest slot free for its span ({slots} in all).',
            'A trunk reply that called a tool ended in the bash block that was the call; the block is removed from the text, since the call is its own event.',
            'Deltas were never recorded; a replay synthesizes them. The system prompt, renders and pre-warms carry no token counts: the record did not measure them.',
            'Phases and seam reasons were not recorded. The audit forks at a phase boundary are drawn in the ratify lane, which is what the charter calls them.',
            f'Fork outcomes are classified by the migration: empty if the answer is empty, mimicry if it answers with a bash block (it answered as the agent), otherwise complete ({classified}).',
            f'Tool output is cut at {MAX_OUTPUT_LINES} lines or {MAX_OUTPUT_CHARS:,} characters, a fork question at {MAX_QUESTION_CHARS:,}, a render at {MAX_RENDER_CHARS:,}; each cut says so in the text.',
            'Scrubbed: the account name and its home directory (now user and /work), run directories (/tmp/session), internal ticket ids.',
        ],
        'events': events,
    }
    text = json.dumps(fixture, ensure_ascii=False, indent=0)

    # Refuse to write anything a scrub missed.
    leaks = [n for n in args.scrub if re.search(re.escape(n), text, re.IGNORECASE)]
    if re.search(r'(/Users/|/home/)[A-Za-z0-9._-]+', text):
        leaks.append('a home-directory path')
    if re.search(r'(^|[^A-Za-z0-9])DIE-?[0-9]+', text, re.IGNORECASE):
        leaks.append('an internal ticket id')
    if leaks:
        sys.exit(f'refusing to write {args.target}: {len(leaks)} leak(s) survived the scrub')

    with open(args.target, 'w') as f:
        f.write(text + '\n')
    print(f'{args.target}: {len(events):,} events, {len(text):,} bytes, {slots} slots, outcomes {classified}')


if __name__ == '__main__':
    main()
