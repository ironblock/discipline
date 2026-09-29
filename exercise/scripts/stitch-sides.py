#!/usr/bin/env python3
"""
One-time stitch: authored side calls onto a recorded trunk that ran none (a
fixture from scripts/migrate-opencode.py), so a real session can be drawn
with lanes at the cadence the surface expects. Close, fast and cheap, and
meant to be replaced by a recording of a drive that ran them.

    python3 scripts/stitch-sides.py plan  <trunk.json> <work.json>
    python3 scripts/stitch-sides.py merge <trunk.json> <work.json> <answers.json> <out.json>

`plan` places the side calls by rule and writes, for each, what it would be
asked and the excerpt of the trunk it would see. Someone -- a model, told
to answer only from the excerpt -- writes each answer as tagged lines into
answers.json ({id: answer}). `merge` places them on the clock (prefill and
decode from assumed rates, one side call at a time per slot) and turns each
tagged line into a patch. The output's `migration` header says all of it.
"""

import json
import math
import sys

# Where side calls go. An extraction reads what a tool just returned, while the trunk goes on.
EXTRACT_TOOLS = {'read', 'bash', 'webfetch'}
EXTRACT_MIN_LINES = 20
EXTRACT_EVERY_MS = 45_000
# An interview asks the model, in a gap or at a plan it just wrote, what it decided.
INTERVIEW_TOOLS = {'todowrite'}
EXCERPT_LINES = 60
# The assumed rates and token estimate of src/drive/compose.ts (RATES, tokensOf), copied: this ran once.
PREFILL, DECODE = 1400, 40
SLOTS = {'extraction': 2, 'interview': 1}
TAGS = {'FACT': 'Facts', 'DECISION': 'Decisions', 'OPEN': 'Open', 'CONSTRAINT': 'Constraints', 'NEXT': 'Next'}
QUESTIONS = {
    'extraction': 'From the output you just read, what facts will matter later? FACT: lines, citing files, commands or URLs from it.',
    'interview': 'What did you decide, and what is still open? DECISION:, OPEN: or NEXT: lines.',
}


def tokens(text):
    return max(1, math.ceil(len(text) / 3.8))


def clip(text, lines=EXCERPT_LINES):
    head = text.split('\n')[:lines]
    return '\n'.join(head)


def plan(trunk):
    events = trunk['events']
    responses = {e['id']: e for e in events if e['kind'] == 'response'}
    begins = {e['id']: e for e in events if e['kind'] == 'tool.begin'}
    context = 0
    last_extract = -EXTRACT_EVERY_MS
    last_text = ''
    ask = ''
    work = []
    per = {}

    def next_id(lane):
        per[lane] = per.get(lane, 0) + 1
        return f'{lane[0]}/{per[lane]}'
    for e in events:
        if e['kind'] == 'ask':
            ask = e['text']
        if e['kind'] == 'response':
            t = e['timings']
            context = t['prompt_n'] + t['cache_n'] + t['predicted_n']
            if e['text'].strip():
                last_text = e['text']
        if e['kind'] == 'tool.end':
            b = begins[e['id']]
            lines = e['output'].count('\n') + 1
            if b['tool'] in EXTRACT_TOOLS and lines >= EXTRACT_MIN_LINES and e['t'] - last_extract >= EXTRACT_EVERY_MS:
                last_extract = e['t']
                call = b['args'].get('command') or b['args'].get('filePath') or b['args'].get('url') or json.dumps(b['args'])
                work.append({'id': next_id('extraction'), 'lane': 'extraction', 't': e['t'] + 50, 'at': e['id'], 'turn': b['turn'], 'prefix_tokens': context,
                             'why': f'{b["tool"]} returned {lines:,} lines', 'question': QUESTIONS['extraction'],
                             'excerpt': f'{b["tool"]}: {call}\n---\n{clip(e["output"])}'})
            elif b['tool'] in INTERVIEW_TOOLS:
                work.append({'id': next_id('interview'), 'lane': 'interview', 't': e['t'] + 50, 'at': e['id'], 'turn': b['turn'], 'prefix_tokens': context,
                             'why': 'the agent wrote its plan', 'question': QUESTIONS['interview'],
                             'excerpt': f'The operator asked: {clip(ask, 12)}\n---\nThe plan it wrote: {json.dumps(b["args"].get("todos"), ensure_ascii=False)[:2500]}'})
        if e['kind'] == 'turn.settled':
            at = next(r['id'] for r in reversed([x for x in events if x['kind'] == 'response' and x['t'] <= e['t']]))
            work.append({'id': next_id('interview'), 'lane': 'interview', 't': e['t'] + 60, 'at': at, 'turn': e['turn'], 'prefix_tokens': context,
                         'why': 'the turn settled', 'question': QUESTIONS['interview'],
                         'excerpt': f'The operator asked: {clip(ask, 12)}\n---\nThe agent ended the turn with: {clip(last_text, 40)}'})
    assert all(w['at'] in responses or w['at'] in begins for w in work)
    return work


def merge(trunk, work, answers):
    events = list(trunk['events'])
    free = {}
    count = {'patches': 0, 'unanswered': 0}
    ids = {}
    for w in sorted(work, key=lambda w: w['t']):
        answer = (answers.get(w['id']) or '').strip()
        if not answer:
            count['unanswered'] += 1
            continue
        slot = SLOTS[w['lane']]
        start = max(w['t'], free.get(slot, 0))
        q = tokens(w['question'])
        a = tokens(answer)
        prompt_ms, predicted_ms = round(q / PREFILL * 1000), round(a / DECODE * 1000)
        done = start + 10 + prompt_ms + predicted_ms
        events += [
            {'kind': 'fork', 't': start, 'id': w['id'], 'lane': w['lane'], 'slot': slot, 'of_turn': w['turn'], 'at': w['at'],
             'why': w['why'], 'question': w['question'], 'prefix_tokens': w['prefix_tokens']},
            {'kind': 'request', 't': start + 10, 'id': f'{w["id"]}/q', 'lane': w['lane'], 'slot': slot, 'turn': w['turn'], 'fork': w['id']},
            {'kind': 'response', 't': done, 'id': f'{w["id"]}/q#response', 'to_request': f'{w["id"]}/q', 'text': answer, 'stop': 'stop',
             'timings': {'prompt_n': q, 'cache_n': w['prefix_tokens'], 'prompt_ms': prompt_ms, 'predicted_n': a, 'predicted_ms': predicted_ms}},
            {'kind': 'fork.settled', 't': done + 10, 'id': w['id'], 'outcome': 'value'},
        ]
        for line in answer.split('\n'):
            tag, _, text = line.partition(':')
            category = TAGS.get(tag.strip().upper())
            if not category or not text.strip():
                continue
            prefix = category[0].lower()
            ids[prefix] = ids.get(prefix, 0) + 1
            count['patches'] += 1
            patch = {'kind': 'patch', 't': done + 20, 'id': f'p/{count["patches"]}', 'from': w['id'], 'op': 'add',
                     'entry': {'id': f'{prefix}{ids[prefix]}', 'category': category, 'text': text.strip()}}
            if w['lane'] == 'extraction':
                patch['authority'] = 'extracted'
            events.append(patch)
        free[slot] = done + 30
    for e in events:
        if e['kind'] == 'session.start':
            e['slots'] = 1 + len(set(SLOTS.values()))
    order = sorted(range(len(events)), key=lambda i: (events[i]['t'], i))
    fixture = dict(trunk)
    fixture['events'] = [events[i] for i in order]
    fixture['migration'] = trunk['migration'] + [
        f'Side calls are AUTHORED, stitched on by scripts/stitch-sides.py: none ran. An extraction where {"/".join(sorted(EXTRACT_TOOLS))} returned {EXTRACT_MIN_LINES}+ lines, at most one per {EXTRACT_EVERY_MS // 1000} s; an interview where the agent wrote its plan and where a turn settled ({len(work) - count["unanswered"]} in all).',
        'Each answer was written by a model given only the excerpt of the trunk its side call would see, in the interviews\' tagged grammar; each tagged line is a patch. Outcomes are all value: nothing here measures mimicry.',
        f'A side call\'s timing is derived: its question and answer in tokens from their text, prefill at {PREFILL} t/s and decode at {DECODE} t/s, one at a time per slot, its prefix the trunk\'s context at the fork.',
    ]
    return fixture, count


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else ''
    if mode == 'plan' and len(sys.argv) == 4:
        trunk = json.load(open(sys.argv[2]))
        work = plan(trunk)
        json.dump(work, open(sys.argv[3], 'w'), ensure_ascii=False, indent=1)
        print(f'{sys.argv[3]}: {len(work)} side calls placed', {lane: sum(w['lane'] == lane for w in work) for lane in SLOTS})
    elif mode == 'merge' and len(sys.argv) == 6:
        trunk, work, answers = (json.load(open(p)) for p in sys.argv[2:5])
        fixture, count = merge(trunk, work, answers)
        text = json.dumps(fixture, ensure_ascii=False, indent=0)
        with open(sys.argv[5], 'w') as f:
            f.write(text + '\n')
        print(f'{sys.argv[5]}: {len(fixture["events"]):,} events, {len(text):,} bytes, {count}')
    else:
        sys.exit(__doc__)


if __name__ == '__main__':
    main()
