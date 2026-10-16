import { describe, expect, it } from 'vitest';

import { RATES, retime, tokensOf } from './retime.ts';
import type { LogLine } from './log.ts';

/** A scenario's log on its own clock is a canned substrate's: replies after their `delay_ms`, the operator instant. */
const ANSWER = 'A long answer, the kind a person reads before they ask again. '.repeat(6);
const RAN = [
  { seq: 0, t: 0, kind: 'session.start', version: 8, opened: 1, model: 'm', head: [{ role: 'system', content: 's' }] },
  { seq: 1, t: 2, kind: 'ask', turn: 1, text: 'Scope: a notes file' },
  { seq: 2, t: 3, kind: 'request', lane: 'trunk', turn: 1 },
  { seq: 3, t: 403, kind: 'delta', request: 2, text: ANSWER },
  { seq: 4, t: 404, kind: 'response', to_request: 2, text: ANSWER },
  { seq: 5, t: 405, kind: 'turn.settled', turn: 1, reason: 'final' },
  { seq: 6, t: 406, kind: 'ask', turn: 2, text: 'Now write it' },
  { seq: 7, t: 407, kind: 'request', lane: 'trunk', turn: 2 },
  { seq: 8, t: 900, kind: 'response', to_request: 7, text: 'Written.' },
] as unknown as LogLine[];

describe('the kitchen sink on the compose clock (#798)', () => {
  const placed = retime(RAN);
  const t = (seq: number) => placed.find((line) => line.seq === seq)!.t;

  it('keeps every line, in order, and changes nothing but `t`', () => {
    const untimed = (log: readonly LogLine[]) => log.map((line) => ({ ...line, t: 0 }));
    expect(untimed(placed)).toEqual(untimed(RAN));
  });

  it('never runs backwards', () => {
    for (let i = 1; i < placed.length; i++) expect(placed[i]!.t).toBeGreaterThanOrEqual(placed[i - 1]!.t);
  });

  it('gives the operator time to ask, after the turn before is read', () => {
    expect(t(6) - t(5)).toBeGreaterThanOrEqual(1500);
    expect(t(1) - t(0)).toBeGreaterThanOrEqual(1500);
  });

  it('decodes an answer at the assumed rate', () => {
    expect(t(4) - t(2)).toBeGreaterThanOrEqual((tokensOf(ANSWER) / RATES.trunkDecode) * 1000);
  });
});

/** Two runs of one scenario: the same lines, each tool's wall-clock run its own (#891). */
const RUN = (wall: (seq: number) => number) =>
  [
    { seq: 0, t: 0, kind: 'session.start', version: 8, opened: 1, model: 'm', head: [{ role: 'system', content: 's' }] },
    { seq: 1, t: 2, kind: 'ask', turn: 1, text: 'Build it' },
    { seq: 2, t: 3, kind: 'request', lane: 'trunk', turn: 1 },
    { seq: 3, t: 5, kind: 'delta', request: 2, tool_call: { index: 0, id: 'c1', name: 'bash', arguments: '{"command":"make"}' } },
    { seq: 4, t: 6 + wall(4), kind: 'tool_call', request: 2, turn: 1, id: 'c1', name: 'bash', arguments: '{"command":"make"}', outcome: 'ran', exit: 0 },
    { seq: 5, t: 7 + wall(4), kind: 'request', lane: 'trunk', turn: 1 },
    { seq: 6, t: 9 + wall(4), kind: 'delta', request: 5, tool_call: { index: 0, id: 'c2', name: 'bash', arguments: '{"command":"make slow"}' } },
    { seq: 7, t: 10 + wall(4) + wall(7), kind: 'tool_call', request: 5, turn: 1, id: 'c2', name: 'bash', arguments: '{"command":"make slow"}', outcome: 'ran', timeout_ms: 16000 },
    { seq: 8, t: 11 + wall(4) + wall(7), kind: 'request', lane: 'trunk', turn: 1 },
    { seq: 9, t: 12 + wall(4) + wall(7), kind: 'response', to_request: 8, text: 'Built, slowly.' },
  ] as unknown as LogLine[];

describe('a tool’s run placed from its line, not its wall clock (#891)', () => {
  const once = retime(RUN((seq) => (seq === 4 ? 3 : 15990)));
  const again = retime(RUN((seq) => (seq === 4 ? 912 : 16007)));
  const t = (log: readonly LogLine[], seq: number) => log.find((line) => line.seq === seq)!.t;

  it('places two runs of one scenario on the same `t`s', () => {
    expect(again.map((line) => line.t)).toEqual(once.map((line) => line.t));
  });

  it('runs a command at the floor, and a timed-out one for its timeout', () => {
    expect(t(once, 4) - t(once, 3)).toBe(150);
    expect(t(once, 7) - t(once, 6)).toBe(16000);
  });
});
