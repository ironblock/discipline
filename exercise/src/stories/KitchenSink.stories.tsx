import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect, waitFor } from 'storybook/test';

import { PHASES } from '../App.tsx';
import { AUTHORED } from '../drive/examples.ts';
import { placed, recordedAt } from '../drive/recorded.ts';
import { fold } from '../session/fold.ts';
import { SessionView } from '../ui/SessionView.tsx';
import { cableOf } from './cables.ts';

interface Args {
  /** Session time, ms. */
  readonly t: number;
  readonly condensed?: boolean;
}

const KITCHEN_SINK = AUTHORED['kitchen-sink'];
const log = placed(KITCHEN_SINK);
const forks = log.filter((line) => line.kind === 'fork');
/** A fork cut at a tool call (#782), the moment its request is sent: it runs in the spare slot. */
const cut = forks.find((line) => (line as { cut?: number }).cut !== undefined);
const cutSent = log.find((line) => line.kind === 'request' && (line as { fork?: number }).fork === cut?.seq);

/**
 * The kitchen sink: every lever in one session, as `diet/scenarios/kitchen-sink.jsonl` drives it on the canned
 * scenario substrate (#798) -- every line `diet`'s own, the model's replies the scenario's. Where `Session/Recorded` is
 * what a model actually did, this is what the drive does with every lever on. RETIMED: its times are placed on assumed
 * rates (`src/drive/retime.ts`), not measured -- the example's `timing` says so.
 */
const meta = {
  title: 'Session/Kitchen sink',
  parameters: { layout: 'fullscreen' },
  args: { t: Number.POSITIVE_INFINITY },
  render: ({ t, condensed = false }) => (
    <SessionView session={fold(recordedAt(KITCHEN_SINK, t))} surface={{ curtain: true, gaps: false, condensed }} composer={{ phases: PHASES }} />
  ),
} satisfies Meta<Args>;

export default meta;
type Story = StoryObj<typeof meta>;

/** The whole session: every kind of line drawn, an era per meditation, and every fork cabled to where it came from. */
export const Whole: Story = {
  name: '1 · the whole session, every fork cabled',
  play: async ({ canvasElement }) => {
    const session = fold(log);
    await expect([...session.unknown.keys()]).toEqual([]);
    await expect(canvasElement.querySelectorAll('.ex-era')).toHaveLength(session.eras.length);
    const ids = [...canvasElement.querySelectorAll('.ex-branchcell')].map((c) => c.getAttribute('data-branch') ?? '');
    await expect(ids).toHaveLength(forks.length);
    await waitFor(async () => expect(ids.filter((id) => cableOf(canvasElement, id) === undefined)).toEqual([]));
  },
};

/** Mid-turn: an interview cut at a tool call runs in the spare slot, beside the trunk (#782). */
export const WhileAForkRuns: Story = {
  name: '2 · a fork running beside the trunk',
  args: { t: (cutSent?.t ?? 0) + 1 },
  play: async ({ canvasElement }) => {
    const running = canvasElement.querySelector('.ex-branchcell [data-outcome="running"]')?.closest('.ex-branchcell');
    await expect(running).toBeTruthy();
    await waitFor(async () => expect(cableOf(canvasElement, running?.getAttribute('data-branch') ?? '')).toMatchObject({ live: true }));
  },
};

/** Condensed: each fork a bar that keeps its place, the whole session at a glance. */
export const Condensed: Story = {
  name: '3 · the whole session, condensed',
  args: { condensed: true },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelectorAll('.ex-bar')).toHaveLength(forks.length);
  },
};
