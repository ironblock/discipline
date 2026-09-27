import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect, waitFor } from 'storybook/test';

import { PHASES } from '../App.tsx';
import { KITCHEN_SINK } from '../drive/kitchen-sink.ts';
import { recordedAt } from '../drive/recorded.ts';
import { fold } from '../session/fold.ts';
import { SessionView } from '../ui/SessionView.tsx';
import { cableOf } from './cables.ts';

interface Args {
  /** Session time, ms. */
  readonly t: number;
  readonly condensed?: boolean;
}

const events = KITCHEN_SINK.events as readonly Record<string, unknown>[];

/**
 * The kitchen sink: a session at the cadence a working drive should have --
 * authored (src/drive/kitchen-sink.ts), where `Session/Recorded` is what the
 * predecessor actually did. Six asks, three phases, two refills; side calls
 * where #31 wants them. Hold a recording's receipt up against this one's.
 */
const meta = {
  title: 'Session/Kitchen sink',
  parameters: { layout: 'fullscreen' },
  args: { t: Number.POSITIVE_INFINITY },
  render: ({ t, condensed = false }) => (
    <div style={{ width: 1900 }}>
      <SessionView session={fold(recordedAt(KITCHEN_SINK, t))} surface={{ curtain: true, gaps: false, condensed }} composer={{ phases: PHASES }} />
    </div>
  ),
} satisfies Meta<Args>;

export default meta;
type Story = StoryObj<typeof meta>;

/** The whole session, and its receipt: the happy path's six numbers. */
export const Whole: Story = {
  name: '1 · the whole session, and its receipt',
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelectorAll('.ex-era')).toHaveLength(3);
    const row = (name: string) => canvasElement.querySelector(`.ex-receipt [data-measure="${name}"] .ex-receipt__value`)?.textContent;
    await expect(row('side-calls-per-ask')).toBe('2.0');
    await expect(row('patches-per-ask')).toBe('3.7');
    await expect(row('mimicry')).toBe('0');
    // Every side call is cabled to the node it came from.
    const ids = [...canvasElement.querySelectorAll('.ex-branchcell')].map((c) => c.getAttribute('data-branch') ?? '');
    await expect(ids).toHaveLength(12);
    await waitFor(async () => expect(ids.filter((id) => cableOf(canvasElement, id) === undefined)).toEqual([]));
  },
};

/** Mid-build: the tests run on the trunk while an extraction reads what just changed, beside it. */
export const WhileTestsRun: Story = {
  name: '2 · tests running, an extraction beside them',
  args: { t: Number(events.find((e) => e['kind'] === 'fork' && e['lane'] === 'extraction' && e['of_turn'] === 3)?.['t']) + 400 },
  play: async ({ canvasElement }) => {
    const running = canvasElement.querySelector('.ex-branchcell [data-outcome="running"]')?.closest('.ex-branchcell');
    await expect(running?.getAttribute('data-branch')).toMatch(/^e\//);
    await waitFor(async () => expect(cableOf(canvasElement, running?.getAttribute('data-branch') ?? '')).toMatchObject({ live: true }));
  },
};

/** Condensed: each side call a bar that keeps its place, the whole session at a glance. */
export const Condensed: Story = {
  name: '3 · the whole session, condensed',
  args: { condensed: true },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelectorAll('.ex-bar')).toHaveLength(12);
  },
};
