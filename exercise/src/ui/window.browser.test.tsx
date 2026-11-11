import { act } from 'react';
import { createRoot } from 'react-dom/client';
import type { Root } from 'react-dom/client';
import { afterEach, expect, test } from 'vitest';
import { page, userEvent } from 'vitest/browser';

import type { LogLine } from '../drive/log.ts';
import type { Command } from '../drive/transport.ts';
import { fold } from '../session/fold.ts';
import { PHASES } from './phases.ts';
import { SessionView } from './SessionView.tsx';

/**
 * A long session stays responsive: what the page mounts is bounded, however long the session. A driven session of
 * three hours (105k log lines) mounted 28.5k DOM nodes, with a 423 MB heap at 37k lines; dialogs opened seconds
 * after a click and two cancel clicks missed. That log is private (its paths and ids are the run's), so the session
 * here is generated in its shape: one running turn of tool steps, each a short step text and a `bash` call whose
 * output is thirty lines, and a working memory of 180 entries, 163 of them the mechanical lane's `files:` lines --
 * the drive's count. 800 steps is past the bound: unwindowed, the trunk alone mounts more than 5,000 nodes.
 */

const STEPS = 800;
const MEMORY = 180;

function session(): LogLine[] {
  const lines: Record<string, unknown>[] = [
    { kind: 'session.start', t: 0, version: 8, opened: 1790000000000, model: 'm', head: [{ role: 'system', content: 's' }] },
    { kind: 'ask', t: 1, turn: 1, text: 'build it' },
    { kind: 'settlement', t: 1, from: 'awaiting', to: 'turn' },
  ];
  const output = Array.from({ length: 30 }, (_, i) => `line ${i} of the listing`).join('\n') + '\n';
  for (let i = 0; i < STEPS; i++) {
    const request = lines.length;
    lines.push({ kind: 'request', t: 2 + i, lane: 'trunk', turn: 1 });
    lines.push({ kind: 'response', t: 2 + i, to_request: request, text: `Step ${i}: listing the tree.`, finish_reason: 'tool_calls' });
    lines.push({ kind: 'tool_call', t: 2 + i, request, turn: 1, id: `call_${i}`, name: 'bash', arguments: '{"command":"ls -R"}', outcome: 'ran', exit: 0, stdout: output });
  }
  for (let i = 0; i < MEMORY; i++) {
    const files = i < 163;
    lines.push({
      kind: 'patch',
      t: 2 + STEPS,
      op: 'add',
      lane: files ? 'mechanical' : 'self-capture',
      entry: { id: files ? `m/${i}` : `r/${i}`, category: files ? 'files' : 'notes', text: files ? `files: src/part-${i}.ts` : `a note, number ${i}` },
    });
  }
  return lines.map((line, seq) => ({ ...line, seq })) as unknown as LogLine[];
}

let root: Root | undefined;
afterEach(() => {
  act(() => root?.unmount());
  root = undefined;
  document.body.replaceChildren();
});

test('a three-hour session mounts under 5,000 DOM nodes, and a cancel click posts within 500 ms', async () => {
  const sent: { command: Command; at: number }[] = [];
  const host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  const folded = fold(session());
  await act(async () =>
    root!.render(
      <SessionView
        session={folded}
        surface={{ curtain: true, gaps: false }}
        composer={{ phases: PHASES, dispatch: async (command) => (sent.push({ command, at: performance.now() }), { ok: true }) }}
        follow
      />,
    ),
  );
  const nodes = document.querySelectorAll('*').length;
  expect(nodes, `${nodes} DOM nodes mounted`).toBeLessThan(5000);
  const clicked = performance.now();
  await userEvent.click(page.getByRole('button', { name: /cancel/ }).first());
  await expect.poll(() => sent.map((s) => s.command.kind)).toEqual(['cancel']);
  expect(sent[0]!.at - clicked).toBeLessThan(500);
});
