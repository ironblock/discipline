import { act } from 'react';
import type { ReactNode } from 'react';
import { createRoot } from 'react-dom/client';
import type { Root } from 'react-dom/client';
import { afterEach, expect, test } from 'vitest';
import { page, userEvent } from 'vitest/browser';

import gestures from '../../../diet/formats/log/fixtures/valid/a-v32-operator-keep-fork-and-shadow.jsonl?raw';
import type { LogLine } from '../drive/log.ts';
import { respelled } from '../drive/spelling.ts';
import type { Command } from '../drive/transport.ts';
import { fold } from '../session/fold.ts';
import { PHASES } from './phases.ts';
import { Composer } from './Composer.tsx';
import { ForkAction, GestureContext, KeepAction } from './Gestures.tsx';
import { Memory } from './Memory.tsx';

/**
 * The page's dialogs and keys under the browser's own keys and focus (the app audit's page walk, F4, F5, F11, F18):
 * what a key does is what a person's key does, and where the focus lands is where it lands.
 */

let root: Root | undefined;
afterEach(() => {
  act(() => root?.unmount());
  root = undefined;
  document.body.replaceChildren();
});

/** Mount NODE, and gather what it posts. */
async function mount(node: (act: (command: Command) => Promise<{ ok: true }>) => ReactNode): Promise<Command[]> {
  const sent: Command[] = [];
  const host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(node(async (command) => (sent.push(command), { ok: true }))));
  return sent;
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 600));

test('F4: the fork picker opens on "your own words", so an Enter right after opening sends nothing', async () => {
  const sent = await mount((post) => (
    <GestureContext.Provider value={post}>
      <ForkAction at={{ seq: 9, text: 'Read it.' }} label="step 7" />
    </GestureContext.Provider>
  ));
  await userEvent.click(page.getByRole('button', { name: 'Fork at step 7' }));
  await expect.poll(() => document.activeElement?.getAttribute('aria-label')).toBe('Your own question at step 7');
  await userEvent.keyboard('{Enter}');
  await settle();
  expect(sent).toEqual([]);
});

test('F4: a pinned ask opens to its whole text before it is sent, and sends only from its own button', async () => {
  const sent = await mount((post) => (
    <GestureContext.Provider value={post}>
      <ForkAction at={{ seq: 9, text: 'Read it.' }} label="step 7" />
    </GestureContext.Provider>
  ));
  await userEvent.click(page.getByRole('button', { name: 'Fork at step 7' }));
  await userEvent.click(page.getByRole('button', { name: 'Fork at step 7 asking GOTCHAS' }));
  await settle();
  expect(sent).toEqual([]);
  await userEvent.click(page.getByRole('button', { name: 'Ask GOTCHAS at step 7' }));
  await expect.poll(() => sent).toEqual([{ kind: 'fork', at: 9, ask: 'GOTCHAS' }]);
});

test('F5: Esc while a turn runs arms a cancel, from anywhere on the page; a second Esc sends it', async () => {
  const sent = await mount((post) => <Composer state="turn" phase="spec" phases={PHASES} dispatch={post} />);
  // Not from the ask box: the page itself has the focus.
  (document.activeElement as HTMLElement | null)?.blur();
  await userEvent.keyboard('{Escape}');
  await settle();
  expect(sent).toEqual([]);
  await expect.poll(() => document.querySelector('.ex-composer__state')?.textContent).toBe('cancel the turn? esc again');
  await userEvent.keyboard('{Escape}');
  await expect.poll(() => sent.map((c) => c.kind)).toEqual(['cancel']);
});

test('P14: Esc in an open picker or dialog while a turn runs closes it, and neither arms nor sends a cancel', async () => {
  const sent = await mount((post) => (
    <GestureContext.Provider value={post}>
      <Composer state="turn" phase="spec" phases={PHASES} dispatch={post} />
      <ForkAction at={{ seq: 9, text: 'Read it.' }} label="step 7" />
      <KeepAction logged={{ seq: 6, text: 'a note' }} label="call call-1" />
    </GestureContext.Provider>
  ));
  for (const opener of ['Fork at step 7', 'Keep from call call-1']) {
    await userEvent.click(page.getByRole('button', { name: opener }));
    await expect.poll(() => document.querySelector('dialog[open]') !== null).toBe(true);
    await userEvent.keyboard('{Escape}');
    await expect.poll(() => document.querySelector('dialog[open]')).toBe(null);
    await settle();
    expect(document.querySelector('.ex-composer__state')?.textContent).toBe('working · esc cancels');
    expect(sent).toEqual([]);
  }
});

test('F5: an armed cancel lets go after a few seconds, so a later Esc only arms it again', async () => {
  const sent = await mount((post) => <Composer state="turn" phase="spec" phases={PHASES} dispatch={post} />);
  (document.activeElement as HTMLElement | null)?.blur();
  await userEvent.keyboard('{Escape}');
  await expect.poll(() => document.querySelector('.ex-composer__state')?.textContent).toBe('cancel the turn? esc again');
  await new Promise((resolve) => setTimeout(resolve, 4_000));
  await expect.poll(() => document.querySelector('.ex-composer__state')?.textContent).toBe('working · esc cancels');
  await userEvent.keyboard('{Escape}');
  await settle();
  expect(sent).toEqual([]);
});

test('F5: an armed cancel lets go on any other key', async () => {
  const sent = await mount((post) => <Composer state="turn" phase="spec" phases={PHASES} dispatch={post} />);
  (document.activeElement as HTMLElement | null)?.blur();
  await userEvent.keyboard('{Escape}');
  await settle();
  await userEvent.keyboard('a');
  await expect.poll(() => document.querySelector('.ex-composer__state')?.textContent).toBe('working · esc cancels');
  await userEvent.keyboard('{Escape}');
  await settle();
  expect(sent).toEqual([]);
});

test('F11: retire on an entry of yours asks first; a second press retires it', async () => {
  const session = fold(gestures.trimEnd().split('\n').map((line) => respelled(JSON.parse(line) as LogLine)));
  const entry = session.memory.find((e) => e.lane === 'operator');
  expect(entry, 'the fixture keeps an entry of the operator’s').toBeDefined();
  const sent = await mount((post) => <Memory entries={session.memory} act={post} />);
  await userEvent.click(page.getByRole('button', { name: `Retire ${entry!.id}` }));
  await settle();
  expect(sent).toEqual([]);
  await userEvent.click(page.getByRole('button', { name: `Retire ${entry!.id}` }));
  await expect.poll(() => sent).toEqual([{ kind: 'retire-entry', id: entry!.id }]);
});

test('F18: "/" anywhere but a text field takes the focus to the ask box', async () => {
  await mount((post) => <Composer state="awaiting" phase="spec" phases={PHASES} dispatch={post} />);
  (document.activeElement as HTMLElement | null)?.blur();
  await userEvent.keyboard('/');
  await expect.poll(() => document.activeElement?.getAttribute('aria-label')).toBe('your ask');
  expect((document.activeElement as HTMLTextAreaElement).value).toBe('');
});

test('a keep of a line that only reasoned opens with the focus on its reasoning', async () => {
  await mount((post) => (
    <GestureContext.Provider value={post}>
      <KeepAction logged={{ seq: 9, text: '', reasoning: 'Read it.' }} label="step 7" />
    </GestureContext.Provider>
  ));
  await userEvent.click(page.getByRole('button', { name: 'Keep from step 7' }));
  await expect.poll(() => document.activeElement?.getAttribute('aria-label')).toBe('What step 7 reasoned');
});
