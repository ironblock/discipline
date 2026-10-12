// Review probes (#733, the adversarial review of 2026-10-11), kept as the fix's proof: replay every committed run's session.log through the surface's reader and fold,
// and hold the fold's working memory against diet's own `product.txt` for that run.
import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import { fold } from '../session/fold.ts';
import { RUN_TIMEOUT_MS } from './committed-runs.ts';
import type { LogLine } from './log.ts';
import { respelled } from './spelling.ts';

const runs = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../../runs');
const dirs = readdirSync(runs, { withFileTypes: true }).filter((d) => d.isDirectory()).map((d) => d.name).sort();

const read = (dir: string) => {
  const text = readFileSync(path.join(runs, dir, 'session.log'), 'utf8');
  const bad: number[] = [];
  const lines: LogLine[] = [];
  text.split('\n').forEach((raw, i) => {
    if (raw.trim() === '') return;
    try {
      lines.push(respelled(JSON.parse(raw) as LogLine));
    } catch {
      bad.push(i + 1);
    }
  });
  return { lines, bad };
};

describe('committed runs through the surface', () => {
  for (const dir of dirs) {
    it(`${dir}: every line reads and folds, no kind unknown`, () => {
      const { lines, bad } = read(dir);
      expect(bad, 'lines JSON.parse refuses').toEqual([]);
      const session = fold(lines);
      expect([...session.unknown.keys()]).toEqual([]);
      expect(session.state).not.toBe('connecting');
    }, RUN_TIMEOUT_MS);

    it(`${dir}: the fold's entry states match diet's product.txt`, () => {
      const { lines } = read(dir);
      const session = fold(lines);
      const product = readFileSync(path.join(runs, dir, 'product.txt'), 'utf8')
        .split('\n')
        .filter((l) => l.trim() !== '')
        .map((l) => JSON.parse(l.slice(l.indexOf('\t') + 1)) as { id: string; state: string });
      // diet's words: live, voided, resolved, retired, parked. The surface's: live, superseded, retired, parked.
      const surfaceWord = (s: string) => (s === 'voided' ? 'superseded' : s);
      const want = Object.fromEntries(product.map((p) => [p.id, surfaceWord(p.state)]));
      const got = Object.fromEntries(session.memory.map((m) => [m.id, m.state]));
      const diff = Object.keys({ ...want, ...got })
        .filter((id) => want[id] !== got[id])
        .map((id) => `${id}: diet=${want[id]} surface=${got[id]}`);
      expect(diff).toEqual([]);
    }, RUN_TIMEOUT_MS);
  }
});
