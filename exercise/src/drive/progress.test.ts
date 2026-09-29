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

  it('fill the new part of the prompt over prefill, the warm part there from the start', () => {
    const prefill = made.filter((f) => f.t <= 3000);
    expect(prefill[0]).toMatchObject({ t: 1000, request: 'q', prompt: { total: 16000, cache: 12000, processed: 0 }, decoded: 0 });
    expect(prefill.at(-1)?.prompt.processed).toBe(4000);
    expect(prefill.every((f, i) => i === 0 || f.t - (prefill[i - 1]?.t ?? 0) === FRAME_MS)).toBe(true);
  });

  it('then count tokens decoded, up to all of them by the response', () => {
    expect(made.at(-1)).toMatchObject({ t: 7000, decoded: 100, prompt: { processed: 4000 } });
    const decoded = made.map((f) => f.decoded);
    expect(decoded.every((d, i) => i === 0 || d >= (decoded[i - 1] ?? 0))).toBe(true);
  });
});

describe('the meter a running request carries', () => {
  const log = (rows: readonly Record<string, unknown>[]) => place(rows as unknown as Unplaced[]).log;
  const base = [
    { kind: 'session.start', t: 0, arm: 'a', model: 'm', slots: 2, trunk_slot: 0, phase: 'p', system: { text: '' } },
    { kind: 'ask', t: 0, turn: 1, text: 'hi' },
    { kind: 'request', t: 1000, id: 'q', lane: 'trunk', slot: 0, turn: 1 },
  ];
  const frame = (t: number, processed: number, decoded = 0) => ({ kind: 'progress', t, request: 'q', prompt: { total: 16000, cache: 12000, processed }, decoded });
  const assistant = (rows: readonly Record<string, unknown>[]) => fold(log([...base, ...rows])).eras[0]?.nodes.find((n) => n.kind === 'assistant');

  it('says how far prefill has got, and how fast, held at the most it has seen', () => {
    const node = assistant([frame(1000, 0), frame(2000, 2000), frame(2250, 1500)]);
    expect(node?.kind === 'assistant' && node.meter).toMatchObject({ total: 16000, cache: 12000, processed: 2000, ppRate: 2000 });
  });

  it('counts tokens decoded, and how fast, while it generates', () => {
    const node = assistant([frame(3000, 4000, 0), frame(5000, 4000, 50)]);
    expect(node?.kind === 'assistant' && node.meter).toMatchObject({ decoded: 50, tgRate: 25 });
  });

  it('has none once the response is in: its timings say the rest', () => {
    const node = assistant([frame(2000, 2000), { ...response, t: 7000 }]);
    expect(node?.kind === 'assistant' && node.meter).toBeUndefined();
  });
});
