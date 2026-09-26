import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect, userEvent, waitFor } from 'storybook/test';

import { PHASES } from '../App.tsx';
import { RECORDINGS, recordedAt } from '../drive/recorded.ts';
import { fold } from '../session/fold.ts';
import { SessionView } from '../ui/SessionView.tsx';

interface RecordedArgs {
  /** Session time, ms. */
  readonly t: number;
  readonly curtain: boolean;
  readonly gaps: boolean;
}

const recording = RECORDINGS['first-drive'];

/**
 * A session that happened: the predecessor's first drive against a local
 * 27B model, migrated into this vocabulary (see the recording's `migration`
 * header for what the migration decided). Where `Session/Moments` is what
 * the surface should look like, this is what it has to survive -- 150 trunk
 * nodes, 94 side calls, side calls that answered as the agent, a refill
 * that lost the thread, and junk in working memory.
 */
const meta = {
  title: 'Session/Recorded',
  parameters: { layout: 'fullscreen' },
  args: { t: Number.POSITIVE_INFINITY, curtain: true, gaps: false },
  render: ({ t, curtain, gaps }) => (
    <SessionView session={fold(recordedAt(recording, t))} surface={{ curtain, gaps }} composer={{ phases: PHASES }} />
  ),
} satisfies Meta<RecordedArgs>;

export default meta;
type Story = StoryObj<typeof meta>;

/** Turn 2: extraction side calls, run while the trunk reads, answer as the agent -- a bash block where facts were asked for. */
export const Mimicry: Story = {
  name: '1 · side calls that answered as the agent',
  args: { t: 107_000 },
  play: async ({ canvasElement }) => {
    const outcomes = [...canvasElement.querySelectorAll('.ex-branch__outcome')].map((el) => el.textContent);
    await expect(outcomes.filter((o) => o === 'mimicry').length).toBeGreaterThanOrEqual(2);
  },
};

/** The first phase boundary: the audit runs in the ratify lane before the refill. */
export const Ratifying: Story = {
  name: '2 · the audit before the first refill',
  args: { t: 530_000 },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector('[data-state="ratify"]')).not.toBeNull();
    await expect(canvasElement.querySelector('.ex-lanehead[data-lane="ratify"]')).not.toBeNull();
  },
};

/** After the refill: the render leads with junk, and the trunk has lost where the checkout is. */
export const LostAfterRefill: Story = {
  name: '3 · the refill lost the thread',
  args: { t: 595_000 },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelectorAll('.ex-era')).toHaveLength(2);
    const exits = [...canvasElement.querySelectorAll('.ex-era[data-era="1"] .ex-exit--bad')].map((el) => el.textContent);
    await expect(exits).toContain('exit 128');
  },
};

/** The whole drive: three eras, every side call in its slot, nothing the surface does not know. */
export const Whole: Story = {
  name: '4 · the whole drive',
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelectorAll('.ex-era')).toHaveLength(3);
    await expect(canvasElement.querySelectorAll('.ex-branchcell')).toHaveLength(94);
    await expect(canvasElement.querySelector('.ex-header__unknown')).toBeNull();
    // Real content is wide -- long commands, wide tables -- and never pushes the trunk into the lanes.
    const lane = canvasElement.querySelector('.ex-lane')?.getBoundingClientRect().left ?? Number.POSITIVE_INFINITY;
    const over = [...canvasElement.querySelectorAll('.ex-trunk .ex-block')].filter((b) => b.getBoundingClientRect().right > lane);
    await expect(over.map((b) => b.getAttribute('data-id'))).toEqual([]);
  },
};

/** The minimap: the whole drive down the left edge -- every message and side call, both refills, every alarm -- and a press goes there. */
export const Minimap: Story = {
  name: '5 · the minimap',
  play: async ({ canvasElement }) => {
    const map = canvasElement.querySelector<HTMLElement>('.ex-minimap');
    await expect(map).not.toBeNull();
    if (!map) return;
    const stage = canvasElement.querySelector('.ex-stage');
    await waitFor(() => expect(map.querySelectorAll('.ex-mm__lane')).toHaveLength(94));
    await expect(map.querySelectorAll('.ex-mm__trunk')).toHaveLength(stage?.querySelectorAll('.ex-trunk .ex-block, .ex-trunk .ex-turnend').length ?? -1);
    await expect(map.querySelectorAll('.ex-mm__seam')).toHaveLength(2);
    const alarms = stage?.querySelectorAll('[data-alarm]').length ?? 0;
    await expect(alarms).toBeGreaterThan(0);
    await expect(map.querySelectorAll('[data-alarm]')).toHaveLength(alarms);
    // Press near the bottom of the map: the page goes to the end of the drive.
    window.scrollTo({ top: 0 });
    const box = map.getBoundingClientRect();
    await userEvent.pointer({ keys: '[MouseLeft]', target: map, coords: { clientX: box.left + box.width / 2, clientY: box.bottom - 2 } });
    await waitFor(() => expect(window.scrollY).toBeGreaterThan(0.9 * (document.documentElement.scrollHeight - window.innerHeight)));
  },
};
