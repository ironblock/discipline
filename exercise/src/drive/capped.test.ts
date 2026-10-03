import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import { fold } from '../session/fold.ts';
import type { LogLine } from './log.ts';

/**
 * A turn that hit its output cap, as `diet`'s drive logs it: a `response`
 * with `capped`, then `turn.settled` with `failed` (#290, ruled 5969297103).
 * The fixture is track three's (`diet/drive/fixtures/a-capped-turn.jsonl`),
 * held equal to what a capped session logs by `session.rs`'s own test. The
 * pair is the record; neither half alone is: an older log has `capped` on a
 * turn settled `final`, and a request that failed has `failed` and no cap.
 *
 * Read line by line: the file ends in a newline, so no final line is torn
 * (the case `diet`'s own reader sets aside, #230).
 */
const here = path.dirname(fileURLToPath(import.meta.url));
const read = (file: string) => {
  const text = readFileSync(path.join(here, '../../../diet', file), 'utf8');
  expect(text.endsWith('\n')).toBe(true);
  return text.trimEnd().split('\n').map((line) => JSON.parse(line) as LogLine);
};
const nodesOf = (file: string) => fold(read(file)).eras.flatMap((era) => era.nodes);

describe('a capped turn (#290)', () => {
  it('ends on a mark that says it hit max tokens, from capped on a failed settle', () => {
    const end = nodesOf('drive/fixtures/a-capped-turn.jsonl').find((n) => n.kind === 'settled');
    expect(end?.kind === 'settled' && end.reason).toBe('failed');
    expect(end?.kind === 'settled' && end.capped).toBe(true);
  });

  it('keeps its answer and its ask out of the model’s context, as the drive does (ruled 5969941559)', () => {
    const marks = nodesOf('drive/fixtures/a-capped-turn.jsonl')
      .filter((n) => n.kind === 'user' || n.kind === 'assistant')
      .map((n) => [n.kind, 'outOfContext' in n ? n.outOfContext : undefined]);
    expect(marks).toEqual([
      ['user', 'failed'],
      ['assistant', 'failed'],
    ]);
  });

  it('carries the cap on the answer that hit it, with what it wrote', () => {
    const answer = nodesOf('drive/fixtures/a-capped-turn.jsonl').find((n) => n.kind === 'assistant');
    expect(answer?.kind === 'assistant' && answer.capped).toBe(true);
    expect(answer?.kind === 'assistant' && answer.text).toBe('The answer');
  });

  it('is not read from the settle word alone: a failed request is not capped', () => {
    const end = nodesOf('formats/log/fixtures/valid/a-turn-whose-connection-failed.jsonl').find((n) => n.kind === 'settled');
    expect(end?.kind === 'settled' && end.reason).toBe('failed');
    expect(end?.kind === 'settled' && 'capped' in end).toBe(false);
  });

  it('is not read from capped alone: a capped response on a turn settled final draws no turn end', () => {
    const nodes = nodesOf('formats/log/fixtures/valid/a-v2-capped-response.jsonl');
    expect(nodes.filter((n) => n.kind === 'settled')).toEqual([]);
    expect(nodes.filter((n) => n.kind === 'user' || n.kind === 'assistant').map((n) => 'outOfContext' in n)).toEqual([false, false]);
  });
});
