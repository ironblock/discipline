// Review probes (#733, the adversarial review of 2026-10-11), kept as the fix's proof: a fork's own tool-call fragments are not the trunk's calls.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { expect, it } from 'vitest';

import { fold } from '../session/fold.ts';
import type { LogLine } from './log.ts';
import { respelled } from './spelling.ts';

const runs = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../../runs');

it('levers-run-b: every tool node on the trunk was made by a trunk request, and none is still running once the session ended', () => {
  const lines = readFileSync(path.join(runs, '2026-10-10-levers-run-b-agent-driven', 'session.log'), 'utf8')
    .split('\n')
    .filter((l) => l.trim() !== '')
    .map((l) => respelled(JSON.parse(l) as LogLine));
  const trunkRequests = new Set(lines.filter((l) => l.kind === 'request' && l.lane === 'trunk').map((l) => String(l.seq)));
  const session = fold(lines);
  expect(session.state).toBe('ended');
  const tools = session.eras.flatMap((e) => e.nodes).filter((n) => n.kind === 'tool');
  expect(tools.filter((t) => t.kind === 'tool' && !trunkRequests.has(t.after)).map((t) => t.kind === 'tool' && `${t.tool} after request ${t.after}`)).toEqual([]);
  expect(tools.filter((t) => t.kind === 'tool' && t.running).length).toBe(0);
});
