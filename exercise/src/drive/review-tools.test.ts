// Review probes (#733, the adversarial review of 2026-10-11), kept as the fix's proof: one tool node per trunk tool_call line, per committed run; no call left running at the end.
import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { expect, it } from 'vitest';

import { fold } from '../session/fold.ts';
import type { LogLine } from './log.ts';
import { respelled } from './spelling.ts';

const runs = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../../runs');

for (const dir of readdirSync(runs, { withFileTypes: true }).filter((d) => d.isDirectory()).map((d) => d.name).sort()) {
  it(`${dir}: one trunk tool node per trunk tool_call line, and none running once the session ended`, () => {
    const lines = readFileSync(path.join(runs, dir, 'session.log'), 'utf8').split('\n').filter((l) => l.trim() !== '').map((l) => respelled(JSON.parse(l) as LogLine));
    const trunk = new Set(lines.filter((l) => l.kind === 'request' && l.lane === 'trunk').map((l) => l.seq));
    const s = fold(lines);
    const tools = s.eras.flatMap((e) => e.nodes).filter((n) => n.kind === 'tool');
    expect(tools.length).toBe(lines.filter((l) => l.kind === 'tool_call' && trunk.has(l.request)).length);
    if (s.state === 'ended') expect(tools.filter((t) => t.kind === 'tool' && t.running).length).toBe(0);
  });
}
