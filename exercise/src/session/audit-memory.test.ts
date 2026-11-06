import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import type { LogLine } from '../drive/log.ts';
import { fold } from './fold.ts';

/**
 * The app audit's page walk, F8's unconfirmed lead: a keep of a model note's own text endorses that note. diet logs it
 * as an `add` on the note's id from the operator's lane, `authority: operator` (`drive::session`'s keep, its
 * `kept_patch`), and the page must then take the note as the operator's: their lane, and so their retire.
 */
const valid = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../../diet/formats/log/fixtures/valid');
const START = (JSON.parse(readFileSync(path.join(valid, 'an-answered-turn.expected.json'), 'utf8')) as { events: LogLine[] }).events[0]!;

describe("a keep of a model note's text", () => {
  it("makes the note the operator's", () => {
    const lines = [
      START,
      { seq: 1, t: 0, kind: 'patch', op: 'add', lane: 'self-capture', entry: { id: 'r3/call-1/note', text: 'two tables', authority: 'self-stated' } },
      { seq: 2, t: 0, kind: 'patch', op: 'add', lane: 'operator', entry: { id: 'r3/call-1/note', text: 'two tables', category: 'schema', authority: 'operator', kept_from: 'answer' } },
    ] as unknown as LogLine[];
    const note = fold(lines).memory.find((e) => e.id === 'r3/call-1/note');
    expect(note?.lane).toBe('operator');
    expect(note?.authority).toBe('operator');
  });
});
