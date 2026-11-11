import { act } from 'react';
import { createRoot } from 'react-dom/client';
import type { Root } from 'react-dom/client';
import { afterEach, expect, test } from 'vitest';
import { page } from 'vitest/browser';

import type { LogLine } from '../drive/log.ts';
import { fold } from '../session/fold.ts';
import { PHASES } from './phases.ts';
import { SessionView, TRUNK_SHOWN } from './SessionView.tsx';

/**
 * The trunk's window holds its start while a dialog is open on one of its blocks: a keep, fork or wrap picker opened
 * near the window's top stays open, its block mounted, however many blocks land meanwhile -- in a build turn,
 * TRUNK_SHOWN of them is minutes.
 */

/** One running turn of STEPS tool steps, each a step text and a `bash` call with a short output. */
function session(steps: number): LogLine[] {
  const lines: Record<string, unknown>[] = [
    { kind: 'session.start', t: 0, version: 8, opened: 1790000000000, model: 'm', head: [{ role: 'system', content: 's' }] },
    { kind: 'ask', t: 1, turn: 1, text: 'build it' },
    { kind: 'settlement', t: 1, from: 'awaiting', to: 'turn' },
  ];
  for (let i = 0; i < steps; i++) {
    const request = lines.length;
    lines.push({ kind: 'request', t: 2 + i, lane: 'trunk', turn: 1 });
    lines.push({ kind: 'response', t: 2 + i, to_request: request, text: `Step ${i}.`, finish_reason: 'tool_calls' });
    lines.push({ kind: 'tool_call', t: 2 + i, request, turn: 1, id: `call_${i}`, name: 'bash', arguments: '{"command":"ls"}', outcome: 'ran', exit: 0, stdout: `out ${i}\n` });
  }
  return lines.map((line, seq) => ({ ...line, seq })) as unknown as LogLine[];
}

let root: Root | undefined;
afterEach(() => {
  act(() => root?.unmount());
  root = undefined;
  document.body.replaceChildren();
});

test('a picker open on the oldest mounted block stays open while TRUNK_SHOWN more blocks land', async () => {
  const view = (steps: number) => (
    <SessionView session={fold(session(steps))} surface={{ curtain: false, gaps: false }} composer={{ phases: PHASES, dispatch: async () => ({ ok: true }), tangents: true }} follow />
  );
  const host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  const steps = 2 * TRUNK_SHOWN;
  await act(async () => root!.render(view(steps)));
  // The oldest block mounted: the window holds the newest TRUNK_SHOWN steps' blocks, each step a text and a call.
  const oldest = [...document.querySelectorAll<HTMLButtonElement>('button[aria-label^="Keep from "]')][0]!;
  const name = oldest.getAttribute('aria-label')!;
  // A corner action shows on hover; the click is the button's own.
  await act(async () => oldest.click());
  await expect.poll(() => document.querySelector('dialog[open]') !== null).toBe(true);
  await act(async () => root!.render(view(steps + TRUNK_SHOWN)));
  expect(document.querySelector('dialog[open]'), 'the picker is still open').not.toBeNull();
  expect(document.querySelector(`button[aria-label="${name}"]`), `${name} is still mounted`).not.toBeNull();
  await expect.element(page.getByRole('dialog')).toBeVisible();
});
