import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import * as V0 from '../../../diet/formats/log/log.ts';
import { fold } from '../session/fold.ts';
import { authorityOf, refusalOf } from '../ui/sets.ts';
import type { LogLine } from './log.ts';
import { respelled } from './spelling.ts';

/**
 * The page against diet's own sets (#1050's audit, findings 7, 8 and 16): each test runs over what diet's generated
 * bindings list (`diet/formats/log/log.ts`), so a member diet adds that the page does not read fails here, the day
 * the bindings are regenerated, and not in a live session.
 */

/** One line of `kind` and nothing else: what the fold makes of a kind, whatever its fields. */
const bare = (kind: string) => ({ seq: 0, t: 0, kind }) as unknown as LogLine;

describe('every kind diet writes, the fold reads', () => {
  for (const kind of V0.KINDS) {
    it(kind, () => {
      // A kind the fold handles may need its fields, and throw without them; only one it does not know is counted.
      let unknown = false;
      try {
        unknown = fold([bare(kind)]).unknown.has(kind);
      } catch {
        unknown = false;
      }
      expect(unknown, `${kind}: a kind the fold does not know`).toBe(false);
    });
  }
});

const valid = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../../diet/formats/log/fixtures/valid');

/**
 * The kinds no valid fixture of diet's carries, so `log.test.ts`'s folding of them reads none: named here, so a kind
 * diet adds without a fixture is a decision someone writes down, and one that gains a fixture leaves the list.
 */
const UNCOVERED = ['phase.proposed', 'phase.answered', 'phase.started', 'tangent.open', 'tangent.close', 'reminded', 'trunk.clamped', 'trunk.waited', 'warrant.gated'];

describe("diet's valid fixtures, by kind", () => {
  it('carry every kind but the ones named uncovered', () => {
    const seen = new Set<string>();
    for (const file of readdirSync(valid).filter((f) => f.endsWith('.expected.json'))) {
      const { events } = JSON.parse(readFileSync(path.join(valid, file), 'utf8')) as { events: LogLine[] };
      for (const line of events) seen.add(respelled(line).kind);
    }
    expect(V0.KINDS.filter((kind) => !seen.has(kind))).toEqual(UNCOVERED);
  });
});

describe('every refusal diet writes has a label', () => {
  for (const refusal of V0.REFUSALS) {
    it(refusal, () => {
      expect(refusalOf(refusal).known, `${refusal}: drawn as its raw word`).toBe(true);
    });
  }
});

describe("every authority diet writes on an entry is the entry's, and has a label", () => {
  for (const authority of V0.AUTHORITIES) {
    it(authority, () => {
      const patch = { seq: 0, t: 0, kind: 'patch', op: 'add', lane: 'self-capture', entry: { id: 'e1', text: 'a note', authority } } as unknown as LogLine;
      expect(fold([patch]).memory[0]?.authority).toBe(authority);
      expect(authorityOf(authority).known, `${authority}: drawn as its raw word`).toBe(true);
    });
  }
});
