import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import { fold } from '../session/fold.ts';
import { readingOf } from '../ui/flow.ts';
import type { LogLine } from './log.ts';

/**
 * A session `diet-drive serve` served, as its `--log` wrote it: lines 1-1440
 * of the rehearsal drive's log (#177, 2026-10-03), scanned and committed
 * under the recorded-fixture rule. Turn 1 cold, turns 2 and 3 warm, each
 * with its prefill's `progress` lines; turn 4 stopped mid-answer, its
 * `progress` lines followed by no response. The page went blank on the first
 * of those lines until #288: it read an older frame shape than the log's.
 *
 * Read line by line: the file ends in a newline, so no final line is torn
 * (the case `diet`'s own reader sets aside, #230), and `diet check-log`
 * accepts it whole.
 */
const text = readFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)), 'served/rehearsal-turns-1-4.log'), 'utf8');
const log = text.trimEnd().split('\n').map((line) => JSON.parse(line) as LogLine);
const upTo = (seq: number) => log.slice(0, log.findIndex((l) => l.seq === seq) + 1);
const assistants = (lines: readonly LogLine[]) => fold(lines).eras.flatMap((era) => era.nodes).filter((n) => n.kind === 'assistant');

describe('a served session, as the page reads it (#288)', () => {
  it('is whole: no line torn, every line a kind the fold knows', () => {
    expect(text.endsWith('\n')).toBe(true);
    expect(log).toHaveLength(1440);
    expect([...fold(log).unknown.keys()]).toEqual([]);
  });

  it('folds at every line, as the page folds the log while it arrives', () => {
    for (let n = 1; n <= log.length; n++) expect(() => fold(log.slice(0, n)), `the log to seq ${log[n - 1]?.seq}`).not.toThrow();
  });

  it('folds its four turns: three answered, the fourth stopped with what had arrived', () => {
    const answers = assistants(log);
    expect(answers.map((a) => a.kind === 'assistant' && a.progress)).toEqual(['done', 'done', 'done', 'cancelled']);
    const cancelled = log.find((l) => l.kind === 'cancelled');
    expect(answers[3]?.kind === 'assistant' && answers[3].text).toBe(cancelled?.kind === 'cancelled' ? cancelled.partial : undefined);
  });

  it('meters a warm prefill as the server framed it: the cache counted in, the new part read over its own time', () => {
    // Turn 4's third frame: total 2334, cache 1767, processed 1818 after 125 ms of prefill.
    const reading = assistants(upTo(1185))[3];
    expect(reading?.kind === 'assistant' && reading.progress).toBe('prefill');
    expect(reading?.kind === 'assistant' && reading.meter).toEqual({ at: 288037, total: 2334, cache: 1767, processed: 1818, ppRate: (1000 * 51) / 125 });
    expect(reading?.kind === 'assistant' && readingOf(reading, 288037)).toMatchObject({ n: 51, of: 567 });
  });

  it('reads a cold prefill from nothing', () => {
    const reading = assistants(upTo(6))[0];
    expect(reading?.kind === 'assistant' && reading.meter).toMatchObject({ total: 523, cache: 0, processed: 111 });
    expect(reading?.kind === 'assistant' && readingOf(reading, 34391)).toMatchObject({ n: 111, of: 523 });
  });
});
