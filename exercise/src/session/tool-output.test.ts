import { describe, expect, it } from 'vitest';

import type { LogLine } from '../drive/log.ts';
import { fold } from './fold.ts';

/**
 * What a call's block shows as its output: a `bash` line's `stdout`, and, for a tool whose line carries no `stdout`
 * -- `read`, `grep`, `glob` -- what the model was shown, its `shown`. A refused call's `shown` is its refusal, never
 * an output.
 */
const call = (seq: number, line: Record<string, unknown>): LogLine[] =>
  [
    { seq, t: seq, kind: 'request', lane: 'trunk', turn: 1 },
    { seq: seq + 1, t: seq, kind: 'response', to_request: seq, text: '', finish_reason: 'tool_calls' },
    { seq: seq + 2, t: seq, kind: 'tool_call', request: seq, turn: 1, id: `call_${seq}`, arguments: '{}', ...line },
  ] as LogLine[];

const LOG: LogLine[] = [
  { seq: 0, t: 0, kind: 'session.start', version: 8, opened: 1790000000000, model: 'm', head: [{ role: 'system', content: 's' }] },
  { seq: 1, t: 1, kind: 'ask', turn: 1, text: 'look' },
  { seq: 2, t: 1, kind: 'settlement', from: 'awaiting', to: 'turn' },
  ...call(3, { name: 'read', outcome: 'ran', shown: 'abc' }),
  ...call(6, { name: 'bash', outcome: 'ran', stdout: 'out', shown: 'out' }),
  ...call(9, { name: 'read', outcome: 'refused', reason: 'outside', shown: 'refused: outside the read scope' }),
  { seq: 12, t: 12, kind: 'turn.settled', turn: 1, reason: 'final' },
] as LogLine[];

describe('a call block’s output', () => {
  const outputs = fold(LOG)
    .eras.flatMap((era) => era.nodes)
    .filter((node) => node.kind === 'tool')
    .map((node) => (node.kind === 'tool' ? (node.output ?? null) : null));

  it('is what a read was shown, a command’s stdout, and nothing for a refused call', () => {
    expect(outputs).toEqual(['abc', 'out', null]);
  });
});
