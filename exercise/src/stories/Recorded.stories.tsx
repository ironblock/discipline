import type { Meta, StoryObj } from '@storybook/react-vite';
import { useState } from 'react';
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
  readonly condensed?: boolean;
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
  render: ({ t, curtain, gaps, condensed = false }) => (
    <SessionView session={fold(recordedAt(recording, t))} surface={{ curtain, gaps, condensed }} composer={{ phases: PHASES }} />
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
    // Every side call is cabled to the trunk node it came from, across the whole gutter.
    await waitFor(async () => expect(canvasElement.querySelectorAll('.ex-branchcell .ex-cable')).toHaveLength(94));
    const edge = canvasElement.querySelector('.ex-trunk')?.getBoundingClientRect().right ?? 0;
    const short = [...canvasElement.querySelectorAll('.ex-cable')].filter((c) => Math.abs(c.getBoundingClientRect().left - edge) > 1);
    await expect(short).toHaveLength(0);
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

/**
 * A side call starts below the bottom of the last thing that finished before
 * it started. In this drive the extraction after turn 2's first reply waited
 * out 56 trunk nodes; it is drawn beside where it ran, cabled back up.
 */
export const StartsWhereItRan: Story = {
  name: '7 · a side call is drawn where it ran, not where it was asked',
  play: async ({ canvasElement }) => {
    const events = recording.events as readonly Record<string, unknown>[];
    const request = events.find((e) => e['kind'] === 'request' && e['fork'] === 'e0100');
    const start = Number(request?.['t']);
    const trunkRequests = new Set(events.filter((e) => e['kind'] === 'request' && e['lane'] === 'trunk').map((e) => e['id']));
    const finished = events.filter(
      (e) => Number(e['t']) <= start && ((e['kind'] === 'response' && trunkRequests.has(e['to_request'])) || e['kind'] === 'tool.end'),
    );
    const last = finished.at(-1);
    const id = String(last?.['kind'] === 'response' ? last['to_request'] : last?.['id']);
    const cell = canvasElement.querySelector('[data-branch="e0100"]');
    const node = canvasElement.querySelector(`.ex-trunk [data-id="${id}"]`);
    await expect(node).not.toBeNull();
    await waitFor(async () => expect(cell?.getBoundingClientRect().top).toBeGreaterThanOrEqual(node?.getBoundingClientRect().bottom ?? Number.POSITIVE_INFINITY));
    // Its cable still runs back up to the node it was asked about.
    const cable = cell?.querySelector('.ex-cable')?.getBoundingClientRect();
    await expect(cable?.height).toBeGreaterThan(1000);
  },
};

/** The whole drive condensed: 94 side calls as bars, each level with its trunk node, the trunk given back its width. */
export const WholeCondensed: Story = {
  name: '6 · the whole drive, condensed',
  args: { condensed: true },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelectorAll('.ex-bar')).toHaveLength(94);
    await expect(canvasElement.querySelector('.ex-branch')).toBeNull();
    // Mimicry is still visible condensed: its bar carries the outcome's alarm.
    await expect(canvasElement.querySelectorAll('.ex-bar[data-alarm]').length).toBeGreaterThanOrEqual(7);
  },
};

/** Three moments in era 0's queue: the trunk went idle at 101 s, and side calls ran one after another until the refill. */
const QUEUE = [161_000, 261_000, 361_000] as const;

function Following() {
  const [at, setAt] = useState(0);
  return (
    <>
      <button type="button" data-advance="" style={{ position: 'fixed', top: 0, left: 0, zIndex: 9, opacity: 0 }} onClick={() => setAt((i) => Math.min(i + 1, QUEUE.length - 1))}>
        later
      </button>
      <SessionView session={fold(recordedAt(recording, QUEUE[at] ?? 0))} surface={{ curtain: true, gaps: false }} composer={{ phases: PHASES }} follow />
    </>
  );
}

/**
 * The bottom of the page is now. Following, the view stays locked to it as
 * the page grows -- here with side calls queued past the trunk's end -- and
 * lets go the moment the person scrolls up, staying where they left it.
 */
export const FollowsTheBottom: Story = {
  name: '8 · following: locked to the bottom, where now is',
  render: () => <Following />,
  play: async ({ canvasElement }) => {
    const page = document.scrollingElement ?? document.documentElement;
    const atBottom = () => window.innerHeight + window.scrollY >= page.scrollHeight - 2;
    const where = (step: string) => `${step}: scrollY ${Math.round(window.scrollY)} + view ${window.innerHeight} of ${page.scrollHeight}`;
    const later = canvasElement.querySelector('[data-advance]') as HTMLElement;
    await waitFor(async () => expect(canvasElement.querySelectorAll('.ex-branchcell .ex-cable').length).toBeGreaterThan(0));
    window.scrollTo(0, page.scrollHeight);
    await waitFor(async () => expect(atBottom(), where('at first')).toBe(true));
    const before = page.scrollHeight;
    later.click();
    await waitFor(async () => expect(page.scrollHeight).toBeGreaterThan(before));
    // The lanes run past the trunk's end: the case following used to lose.
    const trunkEnd = [...canvasElement.querySelectorAll('.ex-trunk__node')].at(-1)?.getBoundingClientRect().bottom ?? 0;
    const laneEnd = Math.max(...[...canvasElement.querySelectorAll('.ex-branchcell')].map((c) => c.getBoundingClientRect().bottom));
    await expect(laneEnd).toBeGreaterThan(trunkEnd);
    await waitFor(async () => expect(atBottom(), where('after the page grew')).toBe(true));
    // Scrolled away: what the person was reading stays where it was on screen
    // while the page grows (the browser may move scrollY to keep it there).
    window.scrollTo(0, window.scrollY - 800);
    await new Promise((r) => setTimeout(r, 100));
    const reading = [...canvasElement.querySelectorAll('.ex-branchcell')].find((c) => {
      const r = c.getBoundingClientRect();
      return r.top > 80 && r.bottom < window.innerHeight - 200;
    });
    await expect(reading).toBeDefined();
    const seen = reading?.getBoundingClientRect().top ?? 0;
    later.click();
    await new Promise((r) => setTimeout(r, 600));
    await expect(atBottom(), where('scrolled away, after the page grew')).toBe(false);
    await expect(Math.abs((reading?.getBoundingClientRect().top ?? 0) - seen)).toBeLessThan(2);
    // The composer's fade is the trunk's: a lane passing under the composer strip is neither painted over nor unreachable.
    const strip = canvasElement.querySelector('.ex-session__composer') as HTMLElement;
    const lane = canvasElement.querySelector('.ex-lane')?.getBoundingClientRect();
    const band = strip.getBoundingClientRect();
    await expect(getComputedStyle(strip).backgroundImage).toBe('none');
    const fade = parseFloat(getComputedStyle(strip, '::before').width);
    await expect(fade).toBeLessThanOrEqual((canvasElement.querySelector('.ex-trunk')?.getBoundingClientRect().width ?? 0) + 1);
    const hit = document.elementFromPoint((lane?.left ?? 0) + 20, band.top + 8);
    await expect(hit?.closest('.ex-session__composer')).toBeNull();
  },
};

/** The receipt #31 measures a session on, under working memory: the floor, on this drive. */
export const ItsReceipt: Story = {
  name: '9 · the receipt: six numbers, the floor',
  play: async ({ canvasElement }) => {
    const receipt = canvasElement.querySelector('.ex-receipt');
    await expect(receipt).not.toBeNull();
    const row = (name: string) => receipt?.querySelector(`[data-measure="${name}"] .ex-receipt__value`)?.textContent;
    await expect(row('side-calls-per-ask')).toBe('18.8');
    await expect(row('patches-per-ask')).toBe('52.8');
    await expect(row('live-entries')).toBe('202');
    await expect(row('mimicry')).toBe('7');
    await expect(row('idle-before-refill')).toBe('442 s · 637 s');
    await expect(row('side-call-time-in-gap')).toBe('≤ 100%');
  },
};
