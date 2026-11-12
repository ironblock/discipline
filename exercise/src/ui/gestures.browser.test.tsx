import { act } from 'react';
import { createRoot } from 'react-dom/client';
import type { Root } from 'react-dom/client';
import { afterEach, expect, test } from 'vitest';
import { page, userEvent } from 'vitest/browser';

import type { Command } from '../drive/transport.ts';
import { ForkAction, GestureContext, GestureMomentContext, KeepAction, WrapAction } from './Gestures.tsx';
import type { GestureMoment } from './Gestures.tsx';

/**
 * The keep dialog under the browser's own clicks and keys (the app audit, finding 2): a person selects a span of the
 * logged text, then clicks into the tag field and types, then keeps. A click moves the document's selection; a
 * story's synthetic events never do, so only here is that order tried as a person tries it.
 */

let root: Root | undefined;
afterEach(() => {
  act(() => root?.unmount());
  root = undefined;
  document.body.replaceChildren();
});

/** The keep action for a line that logged TEXT, its posts gathered. */
async function keeping(text: string): Promise<Command[]> {
  const sent: Command[] = [];
  const host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root!.render(
      <GestureContext.Provider value={async (command) => (sent.push(command), { ok: true })}>
        <KeepAction logged={{ seq: 6, text }} label="call call-1" />
      </GestureContext.Provider>,
    ),
  );
  await userEvent.click(page.getByRole('button', { name: 'Keep from call call-1' }));
  return sent;
}

/** Select the first LENGTH characters of the logged text, as the end of a person's drag leaves them. */
function select(length: number) {
  const raw = document.querySelector('[aria-label="What call call-1 logged"]')!;
  const range = document.createRange();
  range.setStart(raw.firstChild!, 0);
  range.setEnd(raw.firstChild!, length);
  const selection = window.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
}

test('a span selected and kept at once is the span kept', async () => {
  const sent = await keeping('a note\nand more\n');
  select('a note'.length);
  await userEvent.click(page.getByRole('button', { name: 'Keep the selection from call call-1' }));
  await expect.poll(() => sent).toEqual([{ kind: 'keep', from: 6, span: 'a note' }]);
});

test('a span selected, then a tag clicked into and typed, is still the span kept', async () => {
  const sent = await keeping('a note\nand more\n');
  select('a note'.length);
  await userEvent.click(page.getByRole('textbox', { name: 'Tag for the keep from call call-1' }));
  await userEvent.keyboard('notes');
  await userEvent.click(page.getByRole('button', { name: 'Keep the selection from call call-1' }));
  await expect.poll(() => sent).toEqual([{ kind: 'keep', from: 6, span: 'a note', tag: 'notes' }]);
});

/** The wrap action for a call's result that logged TEXT, its posts gathered. */
async function wrapping(text: string): Promise<Command[]> {
  const sent: Command[] = [];
  const host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root!.render(
      <GestureContext.Provider value={async (command) => (sent.push(command), { ok: true })}>
        <WrapAction call={6} from={{ seq: 6, text }} label="call call-1" />
      </GestureContext.Provider>,
    ),
  );
  await userEvent.click(page.getByRole('button', { name: 'Wrap from call call-1' }));
  return sent;
}

test('a span selected and wrapped with the mouse is the span the wrap keeps, as a keep is', async () => {
  const sent = await wrapping('a note\nand more\n');
  const raw = document.querySelector('[aria-label="What call call-1 logged, to wrap"]')!;
  const range = document.createRange();
  range.setStart(raw.firstChild!, 0);
  range.setEnd(raw.firstChild!, 'a note'.length);
  const selection = window.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
  await userEvent.click(page.getByRole('button', { name: 'Wrap with the selection from call call-1' }));
  await expect.poll(() => sent).toEqual([{ kind: 'wrap', call: 6, from: 6, span: 'a note' }]);
});

/** The fork action at a step, during a turn, its posts gathered and answered `{queued: true}` (N4). */
async function forkingMidTurn(moment: Partial<GestureMoment> = {}): Promise<Command[]> {
  const sent: Command[] = [];
  const host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  const during: GestureMoment = { awaiting: false, queueing: true, onTrunk: () => true, ...moment };
  await act(async () =>
    root!.render(
      <GestureContext.Provider value={async (command) => (sent.push(command), { ok: true, answer: { queued: true } })}>
        <GestureMomentContext.Provider value={during}>
          <ForkAction at={{ seq: 6, text: 'a note' }} label="call call-1" />
        </GestureMomentContext.Provider>
      </GestureContext.Provider>,
    ),
  );
  return sent;
}

test('a fork asked during a turn names no line, and is queued for the next step', async () => {
  const sent = await forkingMidTurn();
  await userEvent.click(page.getByRole('button', { name: 'Fork at call call-1' }));
  await expect.element(page.getByRole('heading', { name: "fork at the turn's next step" })).toBeVisible();
  await userEvent.click(page.getByRole('textbox', { name: 'Your own question at call call-1' }));
  await userEvent.keyboard('why that file?');
  await userEvent.click(page.getByRole('button', { name: 'Ask at call call-1' }));
  await expect.poll(() => sent).toEqual([{ kind: 'fork', text: 'why that file?' }]);
});

test('while a fork is queued, the fork action waits, saying what is queued', async () => {
  await forkingMidTurn({ queued: 'why that file?' });
  const action = page.getByRole('button', { name: 'Fork at call call-1' });
  await expect.element(action).toBeDisabled();
  await expect.element(action).toHaveAttribute('title', 'a fork is queued for the next step: why that file?');
});
