// Review probes (#733, the adversarial review of 2026-10-11), kept as the fix's proof: the operator's flags after the meditation that addressed them.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { expect, it } from 'vitest';

import { fold } from '../session/fold.ts';
import type { LogLine } from './log.ts';
import { respelled } from './spelling.ts';

const runs = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../../runs');
const log = (dir: string): LogLine[] =>
  readFileSync(path.join(runs, dir, 'session.log'), 'utf8').split('\n').filter((l) => l.trim() !== '').map((l) => respelled(JSON.parse(l) as LogLine));

for (const dir of ['2026-10-10-gate-and-tail-agent-driven', '2026-10-10-operator-edits-agent-driven']) {
  it(`${dir}: every flag was addressed (the last meditation left none unaddressed), so no entry still shows one`, () => {
    const lines = log(dir);
    const last = lines.filter((l) => l.kind === 'meditation').at(-1);
    expect(last?.kind === 'meditation' ? last.unaddressed : undefined).toEqual([]);
    // No flag line after the last meditation.
    expect(lines.filter((l) => l.kind === 'flag' && l.seq > (last?.seq ?? 0))).toEqual([]);
    const flagged = fold(lines).memory.filter((m) => m.flag !== undefined).map((m) => `${m.id} [${m.state}] flag · ${m.flag}`);
    expect(flagged).toEqual([]);
  });
}
