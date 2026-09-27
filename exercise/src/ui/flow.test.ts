import { describe, expect, it } from 'vitest';

import { edgeOf, flowText, readingOf, writingOf, writtenApart } from './flow.ts';

const meter = { at: 0, total: 17_830, cache: 1_410, processed: 7_900, decoded: 0 };

describe('a flow: tokens in or out, and how long they took', () => {
  it('reads as new tokens, the time they took, and the rate derived from those two', () => {
    expect(flowText({ phase: 'pp', n: 100, ms: 5_000, running: false })).toBe('+100 tok in 5.0 s (20.0 t/s pp)');
    expect(flowText({ phase: 'tg', n: 207, ms: 5_750, running: false })).toBe('+207 tok in 5.8 s (36.0 t/s tg)');
  });

  it('counts up while reading: how many of the new tokens, for how long so far', () => {
    const f = readingOf({ progress: 'prefill', meter, startedAt: 1_000 }, 6_700);
    expect(f).toEqual({ phase: 'pp', n: 7_900, of: 16_420, ms: 5_700, running: true });
    expect(flowText(f!)).toBe('+7.9k of 16.4k tok in 5.7 s (1,386 t/s pp)');
    expect(edgeOf({ progress: 'prefill', meter })?.read).toBeCloseTo(7_900 / 16_420, 5);
  });

  it('says only how long, while nothing has said how far', () => {
    expect(flowText(readingOf({ progress: 'prefill', startedAt: 0 }, 1_200)!)).toBe('reading · 1.2 s');
    expect(edgeOf({ progress: 'prefill' })).toBeUndefined();
  });

  it('counts writing from the first token, not the request', () => {
    const f = writingOf({ progress: 'streaming', meter: { ...meter, at: 14_000, decoded: 50 }, startedAt: 1_000, writingSince: 13_000 }, 15_000);
    expect(f).toMatchObject({ n: 50, ms: 2_000, running: true });
  });

  it('says only how long it has written, until a frame since the first token says how much', () => {
    const stale = writingOf({ progress: 'streaming', meter: { ...meter, at: 12_000, decoded: 0 }, startedAt: 1_000, writingSince: 13_000 }, 13_400);
    expect(flowText(stale!)).toBe('writing · 400 ms');
  });

  it('splits what was written into the text and the calls where the drive said the calls began, and not where it did not', () => {
    const timings = { prompt_n: 108, cache_n: 1_300, prompt_ms: 90, predicted_n: 42, predicted_ms: 1_200 };
    expect(writtenApart({ timings, callsFrom: { predicted_n: 14, predicted_ms: 400 } })).toEqual({
      text: { phase: 'tg', n: 14, ms: 400, running: false },
      calls: { phase: 'tg', n: 28, ms: 800, running: false },
    });
    expect(writtenApart({ timings })).toBeUndefined();
    // Only the time kept: the shares' times are known, their tokens are not.
    const timed = writtenApart({ timings, callsFrom: { predicted_ms: 500 } });
    expect([flowText(timed!.text), flowText(timed!.calls)]).toEqual(['+? tok in 500 ms', '+? tok in 700 ms']);
  });

  it('once answered, is what the response measured', () => {
    const timings = { prompt_n: 108, cache_n: 1_300, prompt_ms: 90, predicted_n: 41, predicted_ms: 1_150 };
    expect(readingOf({ progress: 'done', timings }, 0)).toEqual({ phase: 'pp', n: 108, ms: 90, running: false });
    expect(writingOf({ progress: 'done', timings }, 0)).toEqual({ phase: 'tg', n: 41, ms: 1_150, running: false });
  });
});
