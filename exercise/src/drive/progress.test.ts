import { describe, expect, it } from 'vitest';

import { place } from './place.ts';
import type { Unplaced } from './script.ts';
import { FRAME_MS, frames } from './progress.ts';
import { fold } from '../session/fold.ts';

const response = {
  kind: 'response' as const,
  t: 0,
  id: 'q#r',
  to_request: 'q',
  text: 'done',
  stop: 'stop' as const,
  // 4,000 new tokens read in 2 s over 12,000 warm ones; 100 generated in 4 s.
  timings: { prompt_n: 4000, cache_n: 12000, prompt_ms: 2000, predicted_n: 100, predicted_ms: 4000 },
};

describe('progress frames, synthesized from a response’s timings', () => {
  const made = frames(response, 1000, 7000);

  it('are the log’s own shape: the cache counted in from the start, the prompt read when processed reaches total', () => {
    expect(made[0]).toEqual({ kind: 'progress', t: 1000, request: 'q', total: 16000, cache: 12000, processed: 12000, time_ms: 0 });
    expect(made.at(-1)).toEqual({ kind: 'progress', t: 3000, request: 'q', total: 16000, cache: 12000, processed: 16000, time_ms: 2000 });
    expect(made.every((f, i) => i === 0 || f.t - (made[i - 1]?.t ?? 0) === FRAME_MS)).toBe(true);
  });

  it('stop at the first token, as a served log’s do: nothing frames what is generated', () => {
    expect(made.every((f) => f.t <= 3000)).toBe(true);
  });
});

describe('the meter a running request carries', () => {
  const log = (rows: readonly Record<string, unknown>[]) => place(rows as unknown as Unplaced[]).log;
  const base = [
    { kind: 'session.start', t: 0, arm: 'a', model: 'm', slots: 2, trunk_slot: 0, phase: 'p', system: { text: '' } },
    { kind: 'ask', t: 0, turn: 1, text: 'hi' },
    { kind: 'request', t: 1000, id: 'q', lane: 'trunk', slot: 0, turn: 1 },
  ];
  // A warm prompt, as llama.cpp frames one: processed starts at the cache and ends at the total.
  const frame = (t: number, fresh: number, time_ms: number) => ({ kind: 'progress', t, request: 'q', total: 16000, cache: 12000, processed: 12000 + fresh, time_ms });
  const assistant = (rows: readonly Record<string, unknown>[]) => fold(log([...base, ...rows])).eras[0]?.nodes.find((n) => n.kind === 'assistant');

  it('says how far prefill has got, held at the most it has seen, and how fast by the server’s clock', () => {
    const node = assistant([frame(1000, 0, 0), frame(2000, 2000, 1000), frame(2250, 1500, 1250)]);
    expect(node?.kind === 'assistant' && node.meter).toEqual({ at: 2250, total: 16000, cache: 12000, processed: 14000, ppRate: 2000 });
  });

  it('has no rate before the server has timed anything', () => {
    const node = assistant([frame(1000, 0, 0)]);
    expect(node?.kind === 'assistant' && node.meter).toEqual({ at: 1000, total: 16000, cache: 12000, processed: 12000 });
  });

  it('has none once the response is in: its timings say the rest', () => {
    const node = assistant([frame(2000, 2000, 1000), { ...response, t: 7000 }]);
    expect(node?.kind === 'assistant' && node.meter).toBeUndefined();
  });
});
