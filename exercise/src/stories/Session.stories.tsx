import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect, userEvent, within } from 'storybook/test';

import { PHASES } from '../App.tsx';
import type { Cursor } from '../drive/canned.ts';
import { SessionView } from '../ui/SessionView.tsx';
import { MOMENTS, sessionAt } from './moments.ts';

interface MomentArgs {
  readonly cursor: Cursor;
  /** Behind the curtain: slot lanes, working memory, provenance. */
  readonly curtain: boolean;
  /** Outline what `diet` cannot emit yet. */
  readonly gaps: boolean;
}

/**
 * The whole surface at a moment of the specimen: #31's definition of done,
 * one stop at a time. The composer is live-looking but inert here; the
 * `Live` story drives the canned transport for real.
 */
const meta = {
  title: 'Session/Moments',
  parameters: { layout: 'fullscreen' },
  args: { cursor: MOMENTS.opened, curtain: true, gaps: false },
  render: ({ cursor, curtain, gaps }) => (
    <SessionView session={sessionAt(cursor)} surface={{ curtain, gaps }} composer={{ phases: PHASES }} />
  ),
} satisfies Meta<MomentArgs>;

export default meta;
type Story = StoryObj<typeof meta>;

const q = (root: HTMLElement, selector: string) => root.querySelector(selector);

/** DoD 1, before it: the phase's priming as the system prompt, nothing asked yet. */
export const Opened: Story = {
  name: '1 · opened, awaiting the first ask',
  args: { cursor: MOMENTS.opened },
  play: async ({ canvasElement }) => {
    await expect(q(canvasElement, '[data-state="awaiting"]')).not.toBeNull();
  },
};

/** DoD 1: the trunk answering, streamed; reasoning in italic first. */
export const Streaming: Story = {
  name: '2 · the trunk is streaming',
  args: { cursor: MOMENTS.streaming },
  play: async ({ canvasElement }) => {
    await expect(q(canvasElement, '.ex-block--assistant.ex-block--live')).not.toBeNull();
  },
};

/** DoD 2: bash ran, its 1,860 lines collapsed to a size. The aberration, as it happens. */
export const BigRead: Story = {
  name: '3 · a tool call returned 1,860 lines',
  args: { cursor: MOMENTS.bigRead },
  play: async ({ canvasElement }) => {
    await expect(q(canvasElement, '[data-id="t/2"] .ex-tool__output')).toBeNull();
  },
};

/** DoD 3: settled; the interview runs in slot 1 while the operator reads. */
export const IdleGapInterview: Story = {
  name: '4 · an interview in the idle gap',
  args: { cursor: MOMENTS.idleGapInterview },
  play: async ({ canvasElement }) => {
    await expect(q(canvasElement, '[data-state="capture"]')).not.toBeNull();
    await expect(q(canvasElement, '[data-branch="i/1"] [data-outcome="running"]')).not.toBeNull();
  },
};

/** DoD 3: the patches landed, fresh in working memory. */
export const FirstSettled: Story = {
  name: '5 · patches landed in working memory',
  args: { cursor: MOMENTS.firstSettled },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelectorAll('.ex-memory__entry[data-fresh]')).toHaveLength(6);
  },
};

export const SpecSettled: Story = {
  name: '6 · the spec settled; an open question superseded',
  args: { cursor: MOMENTS.specSettled },
};

/** DoD 4: the operator declared spec → build; ratify runs first. */
export const Ratifying: Story = {
  name: '7 · ratifying before the refill',
  args: { cursor: MOMENTS.ratifying },
  play: async ({ canvasElement }) => {
    await expect(q(canvasElement, '[data-state="ratify"]')).not.toBeNull();
  },
};

/** DoD 5: the refill, drawn as the one deliberate prefill event. */
export const Refilled: Story = {
  name: '8 · refilled into build',
  args: { cursor: MOMENTS.refilled },
  play: async ({ canvasElement }) => {
    await expect(q(canvasElement, '.ex-seam')).not.toBeNull();
    await expect(q(canvasElement, '[data-era="1"] .ex-block--system')).not.toBeNull();
  },
};

/** After the refill: a targeted read, because working memory said where to look. */
export const BuildReading: Story = {
  name: '9 · the build turn reads 49 lines, not 1,860',
  args: { cursor: MOMENTS.buildReading },
};

/** An interview in the idle gap of a running tool call: `cargo test` leaves the trunk waiting. */
export const TestsRunning: Story = {
  name: '10 · an interview while cargo test runs',
  args: { cursor: MOMENTS.testsRunning },
  play: async ({ canvasElement }) => {
    await expect(q(canvasElement, '[data-id="t/5"].ex-block--live')).not.toBeNull();
    await expect(q(canvasElement, '[data-branch="i/4"]')).not.toBeNull();
  },
};

export const Done: Story = {
  name: '11 · done: the definition of done, end to end',
  args: { cursor: MOMENTS.done },
};

/** The same session with the curtain closed: a chat, a quiet marker where a branch left, and working memory still on the right. */
export const CurtainClosed: Story = {
  name: 'curtain closed',
  args: { cursor: MOMENTS.done, curtain: false },
  play: async ({ canvasElement }) => {
    await expect(q(canvasElement, '.ex-lane')).toBeNull();
    await expect(q(canvasElement, '.ex-memory')).not.toBeNull();
    await expect(canvasElement.querySelectorAll('.ex-peek').length).toBeGreaterThan(0);
  },
};

/** Room for everything: working memory is a column on the right, past the last lane. */
export const MemoryBeside: Story = {
  name: 'memory · beside, with room',
  args: { cursor: MOMENTS.done },
  render: ({ cursor, curtain, gaps }) => (
    <div style={{ width: 1900 }}>
      <SessionView session={sessionAt(cursor)} surface={{ curtain, gaps }} composer={{ phases: PHASES }} />
    </div>
  ),
  play: async ({ canvasElement }) => {
    await expect(q(canvasElement, '.ex-session[data-drawer]')).toBeNull();
    const lane = q(canvasElement, '.ex-lane')?.getBoundingClientRect().right ?? Number.POSITIVE_INFINITY;
    await expect(q(canvasElement, '.ex-memory')?.getBoundingClientRect().left).toBeGreaterThan(lane);
  },
};

/** Not enough room: working memory becomes a drawer on the right edge, a tab that opens it over the lanes. */
export const MemoryDrawer: Story = {
  name: 'memory · a drawer, when the row runs out',
  args: { cursor: MOMENTS.done },
  render: ({ cursor, curtain, gaps }) => (
    <div style={{ width: 900 }}>
      <SessionView session={sessionAt(cursor)} surface={{ curtain, gaps }} composer={{ phases: PHASES }} />
    </div>
  ),
  play: async ({ canvasElement }) => {
    const tab = await within(canvasElement).findByRole('button', { name: /working memory/ });
    const drawer = () => q(canvasElement, '.ex-session__memory') as HTMLElement;
    await expect(q(canvasElement, '.ex-session[data-drawer]')).not.toBeNull();
    await expect(drawer().inert).toBe(false);
    await expect(q(canvasElement, '.ex-session__memory .ex-memory')?.closest('[inert]')).not.toBeNull();
    await userEvent.click(tab);
    await expect(tab.getAttribute('aria-expanded')).toBe('true');
    await expect(q(canvasElement, '.ex-session__memory .ex-memory')?.closest('[inert]')).toBeNull();
    await userEvent.keyboard('{Escape}');
    await expect(tab.getAttribute('aria-expanded')).toBe('false');
  },
};

/** Everything drawn from an event `diet` cannot emit yet, outlined with the step of #117 it waits on. */
export const Gaps: Story = {
  name: 'what diet can’t emit yet',
  args: { cursor: MOMENTS.done, gaps: true },
  play: async ({ canvasElement }) => {
    await expect(q(canvasElement, '.ex-gaps')).not.toBeNull();
  },
};
