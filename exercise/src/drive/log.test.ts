import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import { fold } from '../session/fold.ts';
import type { LogLine } from './log.ts';
import { V0_KEYS, V0_KINDS, V0_SETS } from './log.ts';

/**
 * The hand-mirrored v0 (`log.ts`) held to `diet`'s own fixtures, until the
 * generated bindings replace it (#117, 2026-09-28): every valid log in
 * `diet/formats/log/fixtures/valid/` is read line by line against the
 * mirror's keys and sets, and folded. Anything the surface cannot read --
 * a kind, a key, a member it does not know, a line the fold counts as
 * unknown -- fails here, where it is found, not in a session.
 */
const valid = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../../diet/formats/log/fixtures/valid');
const fixtures = readdirSync(valid).filter((f) => f.endsWith('.jsonl')).sort();

const logOf = (file: string): LogLine[] =>
  readFileSync(path.join(valid, file), 'utf8')
    .split('\n')
    .filter((line) => line !== '')
    .map((line) => JSON.parse(line) as LogLine);

/** Which v0 set each key's values belong to. */
const SET_OF: Readonly<Record<string, readonly string[]>> = {
  'settlement.from': V0_SETS.state,
  'settlement.to': V0_SETS.state,
  'refused.command': V0_SETS.command,
  'refused.because': V0_SETS.refusal,
  'refused.during': V0_SETS.state,
  'request.lane': V0_SETS.lane,
  'request.failed.reason': V0_SETS.fail,
  'turn.settled.reason': V0_SETS.settle,
  'idle.gap.ended_by': V0_SETS.gapEnd,
};

describe("diet's valid v0 logs, as the surface reads them", () => {
  it('finds the fixtures', () => {
    expect(fixtures.length).toBeGreaterThan(0);
  });

  for (const file of fixtures) {
    it(file, () => {
      const log = logOf(file);
      for (const line of log) {
        const where = `${file} seq ${line.seq}`;
        expect(V0_KINDS.has(line.kind), `${where}: kind ${line.kind}`).toBe(true);
        const [required, optional] = V0_KEYS[line.kind]!;
        const keys = Object.keys(line).filter((k) => k !== 'seq' && k !== 't' && k !== 'kind');
        for (const key of required) expect(keys, `${where}: ${line.kind} lacks ${key}`).toContain(key);
        for (const key of keys) expect([...required, ...optional], `${where}: ${line.kind} has ${key}, which the mirror does not`).toContain(key);
        for (const [key, value] of Object.entries(line)) {
          const set = SET_OF[`${line.kind}.${key}`];
          if (set) expect(set, `${where}: ${line.kind}.${key} = ${String(value)}`).toContain(value);
        }
        if (line.kind === 'session.start') for (const m of line.head) expect(V0_SETS.role, `${where}: role ${m.role}`).toContain(m.role);
      }
      const session = fold(log);
      expect([...session.unknown.keys()], `${file}: kinds the fold does not know`).toEqual([]);
      expect(session.state).not.toBe('connecting');
    });
  }

  it('folds an answered turn into an ask and its answer', () => {
    const session = fold(logOf('an-answered-turn.jsonl'));
    const nodes = session.eras[0]?.nodes ?? [];
    expect(nodes.map((n) => n.kind)).toEqual(['user', 'assistant']);
    const answer = nodes[1];
    const response = logOf('an-answered-turn.jsonl').find((l) => l.kind === 'response');
    expect(answer?.kind === 'assistant' && answer.progress).toBe('done');
    expect(answer?.kind === 'assistant' && answer.text).toBe(response?.kind === 'response' ? response.text : undefined);
  });

  it('folds a stopped call as cancelled, keeping what arrived', () => {
    const answer = fold(logOf('a-cancelled-turn.jsonl')).eras[0]?.nodes.find((n) => n.kind === 'assistant');
    expect(answer?.kind === 'assistant' && answer.progress).toBe('cancelled');
    expect(answer?.kind === 'assistant' && answer.text).toBe('Hel');
  });

  it('folds a reasoning delta as reasoning, not answer', () => {
    const log = logOf('a-reasoning-delta.jsonl');
    const answer = fold(log).eras[0]?.nodes.find((n) => n.kind === 'assistant');
    const reasoned = log.filter((l) => l.kind === 'delta' && l.reasoning !== undefined).map((l) => (l.kind === 'delta' ? l.reasoning : ''));
    expect(reasoned.length).toBeGreaterThan(0);
    expect(answer?.kind === 'assistant' && answer.reasoning.startsWith(reasoned.join('').slice(0, 1))).toBe(true);
  });

  it('takes the state from the log: an ended session is ended', () => {
    expect(fold(logOf('an-ended-session.jsonl')).state).toBe('ended');
  });
});
