import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect, userEvent, waitFor, within } from 'storybook/test';

import { App, PHASES } from '../App.tsx';
import { APPROVAL, T1_CWD } from '../drive/approval.ts';
import { snapshot } from '../drive/canned.ts';
import type { Unplaced } from '../drive/script.ts';
import type { Beat } from '../drive/specimen.ts';
import { fold } from '../session/fold.ts';
import { SessionView } from '../ui/SessionView.tsx';

/**
 * The approval prompt (#389): a command the gate holds waits on the operator,
 * who answers once, for this session, for this workspace, or declines -- one
 * click -- and the call says what it ran under. Driven on the canned
 * transport (`drive/approval.ts`), and replayed from the log alone.
 */
const meta = {
  title: 'Session/Approval',
  component: App,
  parameters: { layout: 'fullscreen' },
  args: { speed: 40, script: APPROVAL },
} satisfies Meta<typeof App>;

export default meta;
type Story = StoryObj<typeof meta>;

const says = (root: HTMLElement) => root.querySelector('.ex-composer__state')?.textContent;
const calls = (root: HTMLElement) => [...root.querySelectorAll<HTMLElement>('.ex-trunk [data-tone="tool"]')];

/** Ask, and wait for the prompt: docked over the composer, and the call it holds says it waits on you. */
async function prompted(root: HTMLElement): Promise<HTMLElement> {
  await waitFor(async () => expect(says(root)).toBe('your turn'));
  await userEvent.type(root.querySelector('textarea') as HTMLTextAreaElement, 'Install the dependencies and start the dev server.{Enter}');
  const dock = await waitFor(async () => {
    const found = root.querySelector<HTMLElement>('.ex-session__composer .ex-approval');
    await expect(found).not.toBeNull();
    return found!;
  });
  await expect(dock.querySelector('.ex-approval__command')?.textContent).toBe('$ npm install');
  await expect(dock.textContent).toContain(T1_CWD);
  await expect(dock.textContent).toContain('not_approved');
  await expect([...dock.querySelectorAll('.ex-approval__answer')].map((b) => b.textContent)).toEqual(['once', 'for this session', 'for this workspace', 'decline']);
  await expect(calls(root)[0]?.textContent).toContain('waiting on you');
  return dock;
}

export const Approved: Story = {
  name: 'a command waits on you; approved for the session, it runs and says so',
  play: async ({ canvasElement }) => {
    const dock = await prompted(canvasElement);
    await userEvent.click(within(dock).getByText('for this session'));
    await waitFor(async () => expect(canvasElement.querySelector('.ex-approval')).toBeNull());
    await waitFor(async () => expect(says(canvasElement)).toBe('your turn'), { timeout: 10_000 });
    const [ran, refused] = calls(canvasElement);
    await expect(ran?.querySelector('.ex-approval-chip')?.textContent).toMatch(/^approved for this session · .+ to decide$/);
    await expect(ran?.textContent).toContain('held: not_approved');
    await expect(ran?.textContent).toContain('added 212 packages');
    await expect(refused?.querySelector('.ex-call-outcome')?.textContent).toBe('refused · on the denylist');
    await expect(refused?.textContent).toContain('did not run');
  },
};

export const Declined: Story = {
  name: 'a command waits on you; declined, it is refused and never runs',
  play: async ({ canvasElement }) => {
    const dock = await prompted(canvasElement);
    await userEvent.click(within(dock).getByText('decline'));
    await waitFor(async () => expect(says(canvasElement)).toBe('your turn'), { timeout: 10_000 });
    await expect(canvasElement.querySelector('.ex-approval')).toBeNull();
    const [declined, ...rest] = calls(canvasElement);
    await expect(rest).toHaveLength(0);
    await expect(declined?.querySelector('.ex-call-outcome')?.textContent).toBe('refused · declined by the operator');
    await expect(declined?.querySelector('.ex-approval-chip')).toBeNull();
  },
};

/** The approved session as its log has it: `t/1` decided for this session 4.2 s after it was held. */
const decided: readonly Beat[] = APPROVAL.map((beat) => ({
  ...beat,
  events: beat.events.map((e): Unplaced => {
    // The log has no prompt: the call as it ran, with no hold on it.
    if (e.kind === 'tool.begin') return Object.fromEntries(Object.entries(e).filter(([key]) => key !== 'prompt')) as Unplaced;
    return e.kind === 'tool.end' && e.id === 't/1' ? { ...e, approval: { scope: 'session', decided_at: 1420 + 4200, why: 'not_approved' } } : e;
  }),
}));

/** Replayed from the log alone: no prompt waits and nothing can answer, and the call says the same as it did live. */
export const Replayed: StoryObj<{ readonly beats: readonly Beat[] }> = {
  name: 'replayed from the log: what it ran under, why it was held, where, and the denylist',
  args: { beats: decided },
  render: ({ beats }) => <SessionView session={fold(snapshot(beats, { beat: beats.length }))} surface={{ curtain: true, gaps: false }} composer={{ phases: PHASES }} />,
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector('.ex-approval')).toBeNull();
    const [ran, refused] = calls(canvasElement);
    await expect(ran?.querySelector('.ex-approval-chip')?.textContent).toBe('approved for this session · 4.2 s to decide');
    await expect(ran?.textContent).toContain(`in ${T1_CWD}`);
    await expect(ran?.textContent).toContain('held: not_approved');
    await expect(refused?.querySelector('.ex-call-outcome')?.textContent).toBe('refused · on the denylist');
    await expect(refused?.textContent).toContain(`in ${T1_CWD}`);
  },
};
