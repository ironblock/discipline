import { describe, expect, it } from 'vitest';

import type { LogLine } from '../drive/log.ts';
import { fold } from './fold.ts';

/** Two turns whose calls carry the same id, as a model may write (#733): a ruling and a prune name only one of them. */
const turn = (n: number, at: number): LogLine[] =>
  [
    { seq: at, t: at, kind: 'ask', turn: n, text: `ask ${n}` },
    { seq: at + 1, t: at, kind: 'settlement', from: 'awaiting', to: 'turn' },
    { seq: at + 2, t: at, kind: 'request', lane: 'trunk', turn: n },
    { seq: at + 3, t: at, kind: 'response', to_request: at + 2, text: '', finish_reason: 'tool_calls' },
    { seq: at + 4, t: at, kind: 'tool_call', request: at + 2, turn: n, id: 'call_0', name: 'propose_phase_transition', arguments: '{"to":"build"}', outcome: 'ran', stdout: 'proposed' },
    { seq: at + 5, t: at, kind: 'turn.settled', turn: n, reason: 'final' },
    { seq: at + 6, t: at, kind: 'settlement', from: 'turn', to: 'awaiting' },
  ] as LogLine[];

const LOG: LogLine[] = [
  { seq: 0, t: 0, kind: 'session.start', version: 8, opened: 1790000000000, model: 'm', head: [{ role: 'system', content: 's' }], phases: ['plan', 'build'], phase_transitions: [{ from: 'plan', to: 'build' }], opening_phase: 'plan' },
  ...turn(1, 1),
  ...turn(2, 8),
  { seq: 15, t: 15, kind: 'phase.ruled', call: 'call_0', choice: 'stay', from: 'plan', to: 'build' },
  { seq: 16, t: 16, kind: 'pruned', turn: 1, call: 'call_0', sha256: 'a'.repeat(64), bytes: 10, text: 'cut' },
] as LogLine[];

describe('a call id reused across turns (#733)', () => {
  const tools = fold(LOG).eras.flatMap((era) => era.nodes).filter((node) => node.kind === 'tool');

  it('rules only the latest call with that id before the ruling', () => {
    expect(tools.map((t) => (t.kind === 'tool' ? (t.ruled?.choice ?? null) : null))).toEqual([null, 'stay']);
  });

  it('prunes only the call with that id in the prune’s own turn', () => {
    expect(tools.map((t) => (t.kind === 'tool' ? (t.pruned?.bytes ?? null) : null))).toEqual([10, null]);
  });
});
