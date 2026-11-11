import { act } from 'react';
import { createRoot } from 'react-dom/client';
import type { Root } from 'react-dom/client';
import { afterEach, expect, test } from 'vitest';
import { page, userEvent } from 'vitest/browser';

import type { Command } from '../drive/transport.ts';
import type { Folded, MemoryEntry } from '../session/fold.ts';
import { Memory } from './Memory.tsx';

/**
 * An entry's actions under the browser's own clicks: a click on `flag` opens the note to write, and sends nothing
 * until the note is sent. A story's synthetic events never run a click's default action, so only here is it tried.
 */

let root: Root | undefined;
afterEach(() => {
  act(() => root?.unmount());
  root = undefined;
  document.body.replaceChildren();
});

const entry = { id: 'd1', category: 'decision', text: 'a note', state: 'live', by: 'model', landedAt: 1, fresh: false, from: [1], needs: [] } as unknown as Folded<MemoryEntry>;

test('a click on flag opens the note and sends nothing until it is sent', async () => {
  const sent: Command[] = [];
  const host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<Memory entries={[entry]} act={async (command) => (sent.push(command), { ok: true })} />));
  await userEvent.click(page.getByRole('button', { name: 'Flag d1' }));
  await expect.element(page.getByRole('textbox', { name: 'Flag d1' })).toBeVisible();
  expect(sent).toEqual([]);
  await userEvent.keyboard('still true?');
  await userEvent.click(page.getByRole('button', { name: 'Send flag d1' }));
  await expect.poll(() => sent).toEqual([{ kind: 'flag-entry', id: 'd1', note: 'still true?' }]);
});
