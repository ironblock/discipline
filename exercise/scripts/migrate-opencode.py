#!/usr/bin/env python3
"""
One-time migration: a session OpenCode recorded (its sqlite store), into the
surface's provisional event vocabulary (src/drive/events.ts), as a fixture.

    python3 scripts/migrate-opencode.py <opencode.db> <session title> <out.json> --scrub <name> --title <title>

A trunk only: OpenCode runs no side calls. `scripts/stitch-sides.py` may add
authored ones after. Like scripts/migrate-recorded.py -- whose caps and scrub
it loads, so the privacy rules have one source -- it runs once, by hand, its
output reviewed and committed, every rule it applies listed in the output's
`migration` header, and it refuses to write if a scrubbed name, a
home-directory path or an e-mail address survives.

OpenCode keeps, per step (an assistant message): when it began and finished,
its tokens (input, output, reasoning, cache read), and its parts -- reasoning
and text with their times, and each tool call as a part created when the
call began streaming, with the call's input, when it ran, and what it
returned. It keeps no system prompt, and no per-token timings.
"""

import argparse
import importlib.util
import json
import os
import re
import sqlite3
import sys

here = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location('migrate_recorded', os.path.join(here, 'migrate-recorded.py'))
recorded = importlib.util.module_from_spec(spec)
spec.loader.exec_module(recorded)
cap, scrubber, MAX_GAP_MS = recorded.cap, recorded.scrubber, recorded.MAX_GAP_MS
MAX_OUTPUT_CHARS, MAX_OUTPUT_LINES = recorded.MAX_OUTPUT_CHARS, recorded.MAX_OUTPUT_LINES

# A web page fetched is someone else's text: the fixture keeps its head only.
MAX_FETCH_LINES = 20
# A write's or an edit's file body is the call's input: kept to its head.
MAX_ARG_CHARS = 1_500


def rows(db, sql, *args):
    return [tuple(r) for r in db.execute(sql, args)]


def migrate(db, session_id):
    messages = []
    for mid, created, data in rows(db, 'select id, time_created, data from message where session_id=? order by time_created, id', session_id):
        m = json.loads(data)
        parts = [(pc, json.loads(d)) for pc, d in rows(db, 'select time_created, data from part where message_id=? order by id', mid)]
        messages.append((created, m, parts))

    # When anything ran: each step from its start to its finish, each tool while it ran.
    active = []
    for created, m, parts in messages:
        if m['role'] == 'assistant':
            active.append((m['time']['created'], m['time'].get('completed') or created))
        for pc, p in parts:
            st = (p.get('state') or {}).get('time') or {}
            if p['type'] == 'tool' and st.get('start') and st.get('end'):
                active.append((st['start'], st['end']))
    active.sort()
    merged = []
    for a, b in active:
        if merged and a <= merged[-1][1]:
            merged[-1][1] = max(merged[-1][1], b)
        else:
            merged.append([a, b])
    t0 = messages[0][0]
    # Session time: ms from the first message; a stretch where nothing runs longer than MAX_GAP_MS is cut to it.
    cuts = []
    for (a1, b1), (a2, _) in zip(merged, merged[1:]):
        if a2 - b1 > MAX_GAP_MS:
            cuts.append((b1, a2 - b1 - MAX_GAP_MS))

    def at(ms):
        return int(ms - t0 - sum(excess for start, excess in cuts if start < ms))

    out = []
    model = next((m.get('modelID') for _, m, _ in messages if m['role'] == 'assistant'), 'unknown')
    out.append({'kind': 'session.start', 't': 0, 'arm': 'opencode', 'model': model, 'slots': 1, 'trunk_slot': 0, 'phase': 'not recorded',
                'system': {'text': 'OpenCode keeps its system prompt out of the session: not recorded.'}})
    turn = 0
    step = 0
    tool = 0
    last = 0
    carried = {}
    tools = {}
    for created, m, parts in messages:
        if m['role'] == 'user' and any(p['type'] == 'compaction' for _, p in parts):
            # OpenCode compacting its own context: not an ask. Carried under its own name.
            carried['compaction'] = carried.get('compaction', 0) + 1
            out.append({'kind': 'compaction', 't': at(created), 'auto': any(bool(p.get('auto')) for _, p in parts)})
            continue
        if m['role'] == 'user':
            if turn:
                out.append({'kind': 'turn.settled', 't': last, 'turn': turn, 'reason': 'final'})
            turn += 1
            text = '\n\n'.join(p['text'] for _, p in parts if p['type'] == 'text' and not p.get('synthetic'))
            last = at(created)
            out.append({'kind': 'ask', 't': last, 'turn': turn, 'text': text})
            continue
        step += 1
        q = f'q/{step}'
        begun = m['time']['created']
        finished = m['time'].get('completed') or begun
        writes = [pc for pc, p in parts if p['type'] in ('reasoning', 'text', 'tool')]
        first = min(writes) if writes else finished
        calls = [(pc, p) for pc, p in parts if p['type'] == 'tool']
        tok = m.get('tokens') or {}
        cache = (tok.get('cache') or {}).get('read', 0)
        timings = {'prompt_n': tok.get('input', 0), 'cache_n': cache, 'prompt_ms': max(0, first - begun),
                   'predicted_n': tok.get('output', 0), 'predicted_ms': max(0, finished - first)}
        out.append({'kind': 'request', 't': at(begun), 'id': q, 'lane': 'trunk', 'slot': 0, 'turn': turn})
        response = {'kind': 'response', 't': at(finished), 'id': f'{q}#response', 'to_request': q,
                    'text': '\n\n'.join(p['text'] for _, p in parts if p['type'] == 'text'),
                    'stop': 'tool' if calls else 'stop', 'timings': timings}
        reasoning = '\n\n'.join(p['text'] for _, p in parts if p['type'] == 'reasoning')
        if reasoning:
            response['reasoning'] = reasoning
        if calls:
            # When the first call began streaming: the time of the split is kept, its tokens are not.
            response['calls_from'] = {'predicted_ms': max(0, min(pc for pc, _ in calls) - first)}
        out.append(response)
        last = at(finished)
        for pc, p in calls:
            tool += 1
            st = p.get('state') or {}
            ran = st.get('time') or {}
            name = p['tool']
            tools[name] = tools.get(name, 0) + 1
            args = {k: (cap(v, MAX_ARG_CHARS, what='the fixture migration') if isinstance(v, str) else v) for k, v in (st.get('input') or {}).items()}
            begin = max(at(finished), at(ran.get('start') or finished))
            end = begin + max(0, (ran.get('end') or 0) - (ran.get('start') or 0))
            output = st.get('output') or st.get('error') or ''
            output = cap(output, MAX_OUTPUT_CHARS, MAX_FETCH_LINES if name == 'webfetch' else MAX_OUTPUT_LINES)
            meta = st.get('metadata') or {}
            exit_code = meta['exit'] if isinstance(meta.get('exit'), int) else (1 if st.get('status') == 'error' else 0)
            out.append({'kind': 'tool.begin', 't': begin, 'id': f't/{tool}', 'turn': turn, 'after': f'{q}#response', 'tool': name, 'args': args})
            done = {'kind': 'tool.end', 't': end, 'id': f't/{tool}', 'exit': exit_code, 'output': output}
            if meta.get('truncated'):
                done['truncated'] = True
            out.append(done)
            last = max(last, end)
    out.append({'kind': 'turn.settled', 't': last, 'turn': turn, 'reason': 'final'})
    order = sorted(range(len(out)), key=lambda i: (out[i]['t'], i))
    return [out[i] for i in order], {'steps': step, 'tools': tools, 'carried': carried, 'cut': len(cuts)}


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[1])
    parser.add_argument('db')
    parser.add_argument('session', help='the session\'s title, or enough of it to be the only match')
    parser.add_argument('target')
    parser.add_argument('--scrub', action='append', required=True, help='a name to remove everywhere (the account the session ran under)')
    parser.add_argument('--title', required=True)
    args = parser.parse_args()

    db = sqlite3.connect(f'file:{args.db}?mode=ro', uri=True)
    found = rows(db, 'select id from session where title like ?', f'%{args.session}%')
    if len(found) != 1:
        sys.exit(f'{len(found)} sessions match {args.session!r}: name exactly one')
    events, counts = migrate(db, found[0][0])
    events = scrubber(args.scrub)(events)
    # A private network address is the machine's, not the session's: it becomes localhost.
    private = re.compile(r'\b(?:10|192\.168|172\.(?:1[6-9]|2\d|3[01]))(?:\.\d{1,3}){2,3}\b')
    events = json.loads(private.sub('localhost', json.dumps(events, ensure_ascii=False)))
    fixture = {
        'title': args.title,
        'migration': [
            'Recorded by OpenCode against a local model, and migrated once into the provisional vocabulary by scripts/migrate-opencode.py: a trunk only, since OpenCode runs no side calls.',
            f'Session time is milliseconds from the first message; a stretch where nothing runs longer than {MAX_GAP_MS // 1000} s is cut to {MAX_GAP_MS // 1000} s ({counts["cut"]} cut).',
            'Each step is a request and a response: its prompt tokens are OpenCode\'s input and cache read, its prefill time from the step\'s start to its first part, its generation from there to the step\'s finish; generated tokens are OpenCode\'s output. No per-token timings were kept.',
            'Tool calls are native: each began streaming when OpenCode created its part, kept as `calls_from` -- a time, not a token count. A tool runs from when OpenCode ran it, but never before the step that wrote it finished.',
            f'Tools, by name: {counts["tools"]}. A call\'s string arguments are cut at {MAX_ARG_CHARS:,} characters; its output at {MAX_OUTPUT_LINES} lines or {MAX_OUTPUT_CHARS:,} characters, a fetched page at {MAX_FETCH_LINES} lines; each cut says so in the text.',
            'The system prompt, phases and slots beyond the trunk\'s were not recorded.',
            f'Events of a kind this vocabulary does not have are carried under their own name, not dropped ({counts["carried"] or "none"}).',
            'Scrubbed: the account name and its home directory (now user and /work), and private network addresses (now localhost).',
        ],
        'events': events,
    }
    text = json.dumps(fixture, ensure_ascii=False, indent=0)
    leaks = [n for n in args.scrub if re.search(re.escape(n), text, re.IGNORECASE)]
    if re.search(r'(/Users/|/home/)[A-Za-z0-9._-]+', text):
        leaks.append('a home-directory path')
    if re.search(r'[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}', text):
        leaks.append('an e-mail address')
    if private.search(text):
        leaks.append('a private network address')
    if leaks:
        sys.exit(f'refusing to write {args.target}: {len(leaks)} leak(s) survived the scrub')
    with open(args.target, 'w') as f:
        f.write(text + '\n')
    print(f'{args.target}: {len(events):,} events, {len(text):,} bytes, {counts}')


if __name__ == '__main__':
    main()
