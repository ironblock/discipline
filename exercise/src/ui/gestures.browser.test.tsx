import { act } from 'react';
import { createRoot } from 'react-dom/client';
import type { Root } from 'react-dom/client';
import { afterEach, expect, test } from 'vitest';
import { page, userEvent } from 'vitest/browser';

import type { Command } from '../drive/transport.ts';
import { GestureContext, KeepAction } from './Gestures.tsx';

/**
 * The keep dialog under the browser's own clicks and keys (#1050's audit, finding 2): a person selects a span of the
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
