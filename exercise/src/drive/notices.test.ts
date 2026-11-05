import { describe, expect, it } from 'vitest';

import { notices, shellNotify } from './notices.ts';
import type { Waits } from './notices.ts';
import type { Prompt } from './transport.ts';

/** The moments the session waits on the operator (#1049), each told once, when it begins. */
const PROMPT: Prompt = { request: 3, id: 'call-1', command: 'ls | wc -l', cwd: '~/wt', reason: 'dynamic', segments: [] };
const NONE: Waits = {};

describe('notices: a moment that waits on the operator is told once, as it begins', () => {
  it('tells a command that waits on approval, by its command', () => {
    expect(notices(NONE, { waiting: PROMPT })).toEqual([{ title: 'a command waits on you', body: 'ls | wc -l' }]);
  });

  it('tells the same wait no second time, and the next one again', () => {
    expect(notices({ waiting: PROMPT }, { waiting: PROMPT })).toEqual([]);
    expect(notices({ waiting: PROMPT }, { waiting: { ...PROMPT, id: 'call-2', command: 'pwd' } })).toEqual([{ title: 'a command waits on you', body: 'pwd' }]);
    expect(notices({ waiting: PROMPT }, NONE)).toEqual([]);
  });

  it('tells a meditation held for the operator, and a phase move the model proposes', () => {
    const held = { atTurn: 4, reason: 'move to build', unaddressed: ['m/2'] };
    expect(notices(NONE, { held })).toEqual([{ title: 'the meditation is held', body: 'the reconcile left your changes unaddressed' }]);
    expect(notices({ held }, { held: { ...held } })).toEqual([]);
    const proposal = { call: 'call-9', from: 'plan', to: 'build' };
    expect(notices(NONE, { proposal })).toEqual([{ title: 'the model proposes a phase move', body: 'plan to build' }]);
    expect(notices({ proposal }, { proposal: { ...proposal } })).toEqual([]);
  });

  it('cuts a long command to one line the notification can show', () => {
    const long = `${'x'.repeat(200)}\nsecond line`;
    const [told] = notices(NONE, { waiting: { ...PROMPT, command: long } });
    expect(told?.body).toBe(`${'x'.repeat(119)}…`);
  });
});

describe('shellNotify: the shell is asked, and a browser has none', () => {
  it('asks the shell to notify, with the notice', async () => {
    const asked: [string, unknown][] = [];
    const scope = { __TAURI_INTERNALS__: { invoke: async (command: string, args?: unknown) => void asked.push([command, args]) } };
    expect(await shellNotify({ title: 't', body: 'b' }, scope)).toBe(true);
    expect(asked).toEqual([['notify', { title: 't', body: 'b' }]]);
  });

  it('does nothing in a browser', async () => {
    expect(await shellNotify({ title: 't', body: 'b' }, {})).toBe(false);
  });
});
