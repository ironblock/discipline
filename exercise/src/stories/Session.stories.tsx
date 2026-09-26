import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect, userEvent, waitFor, within } from 'storybook/test';

import { PHASES } from '../App.tsx';
import type { Cursor } from '../drive/canned.ts';
import { useState } from 'react';

import { SessionView } from '../ui/SessionView.tsx';
import type { Surface } from '../ui/surface.tsx';
import { MOMENTS, sessionAt, variantAt } from './moments.ts';

interface MomentArgs {
  readonly cursor: Cursor;
  /** Behind the curtain: slot lanes, working memory, provenance. */
  readonly curtain: boolean;
  /** Outline what `diet` cannot emit yet. */
  readonly gaps: boolean;
  /** Behind the curtain, each side call a bar that keeps its place. */
  readonly condensed?: boolean;
}

/**
 * The whole surface at a moment of the specimen: #31's definition of done,
 * one stop at a time. The composer is live-looking but inert here; the
 * `Live` story drives the canned transport for real.
 */
const meta = {
  title: 'Session/Moments',
  parameters: { layout: 'fullscreen' },
  args: { cursor: MOMENTS.opened, curtain: true, gaps: false, condensed: false },
  render: function Render({ cursor, curtain, gaps, condensed = false }) {
    // Stateful, so a story can press what changes the surface (a condensed bar opens the curtain).
    const [surface, setSurface] = useState<Surface>({ curtain, gaps, condensed });
    return <SessionView session={sessionAt(cursor)} surface={surface} onSurface={setSurface} composer={{ phases: PHASES }} />;
  },
} satisfies Meta<MomentArgs>;

export default meta;
type Story = StoryObj<typeof meta>;

const q = (root: HTMLElement, selector: string) => root.querySelector(selector);
const top = (root: HTMLElement, selector: string) => q(root, selector)?.getBoundingClientRect().top ?? Number.NaN;
const bottom = (root: HTMLElement, selector: string) => q(root, selector)?.getBoundingClientRect().bottom ?? Number.NaN;

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
    // It started when the answer settled: it is drawn below the answer, in the idle gap it ran in.
    await waitFor(async () => expect(top(canvasElement, '[data-branch="i/1"]')).toBeGreaterThanOrEqual(bottom(canvasElement, '[data-id="q/3"]')));
  },
};

/**
 * An interview declared but waiting for its slot: it collects below
 * everything finished so far, unlit and dashed, and is not pinned until its
 * request starts.
 */
export const InterviewQueued: Story = {
  name: '4b · an interview waiting for its slot',
  render: () => (
    <SessionView
      session={variantAt(MOMENTS.idleGapInterview, (e) => (e['kind'] === 'request' && e['fork'] === 'i/1' ? [] : e))}
      surface={{ curtain: true, gaps: false }}
      composer={{ phases: PHASES }}
    />
  ),
  play: async ({ canvasElement }) => {
    const cell = '[data-branch="i/1"]';
    await waitFor(async () => expect(q(canvasElement, `${cell}[data-pending]`)).not.toBeNull());
    await expect(q(canvasElement, `${cell} [data-outcome="pending"]`)?.textContent).toContain('queued');
    await expect(q(canvasElement, `${cell} .ex-cable[data-pending]`)).not.toBeNull();
    await expect(q(canvasElement, `${cell} .ex-cable[data-live]`)).toBeNull();
    await expect(top(canvasElement, cell)).toBeGreaterThanOrEqual(bottom(canvasElement, '[data-id="q/3"]'));
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

/**
 * Condensed: every side call keeps its place beside the trunk as a bar in
 * its lane's colour, the length of what it has written so far, a tick per
 * patch it landed. The running one is lit, and grows as it streams.
 */
export const Condensed: Story = {
  name: 'condensed · an interview in the idle gap',
  args: { cursor: MOMENTS.idleGapInterview, condensed: true },
  play: async ({ canvasElement }) => {
    await expect(q(canvasElement, '.ex-session--condensed')).not.toBeNull();
    await expect(q(canvasElement, '.ex-lane')?.getBoundingClientRect().width).toBeLessThan(24);
    await expect(q(canvasElement, '.ex-branch')).toBeNull();
    const bars = canvasElement.querySelectorAll('.ex-bar');
    await expect(bars.length).toBeGreaterThan(0);
    await expect(bars.length).toBe(canvasElement.querySelectorAll('.ex-branchcell').length);
    await expect(q(canvasElement, '[data-branch="i/1"] .ex-bar[data-live]')).not.toBeNull();
    await waitFor(async () => expect(canvasElement.querySelectorAll('.ex-branchcell .ex-cable').length).toBe(bars.length));
  },
};

/** A bar is a way in: pressing it opens the curtain on that side call. */
export const CondensedOpens: Story = {
  name: 'condensed · pressing a bar opens it',
  args: { cursor: MOMENTS.done, condensed: true },
  play: async ({ canvasElement }) => {
    const bar = q(canvasElement, '[data-branch="i/1"] .ex-bar') as HTMLElement;
    await expect(bar.getAttribute('aria-label')).toContain('i/1');
    await userEvent.click(bar);
    await expect(q(canvasElement, '.ex-session--condensed')).toBeNull();
    await expect(q(canvasElement, '[data-branch="i/1"] .ex-branch')).not.toBeNull();
  },
};

/**
 * Every message, tool call and side call is addressable by its own id, and
 * every working-memory entry as `memory/<id>`: `#<id>` in the address goes
 * there and selects it -- including a link opened before the session has
 * drawn, where the browser's own jump finds nothing to go to.
 */
function OpenedByLink({ cursor }: { readonly cursor: MomentArgs['cursor'] }) {
  const [shown, setShown] = useState(false);
  return (
    <>
      <button type="button" data-open="" style={{ position: 'fixed', top: 0, left: 0, opacity: 0, zIndex: 9 }} onClick={() => setShown(true)}>
        open
      </button>
      {shown ? <SessionView session={sessionAt(cursor)} surface={{ curtain: true, gaps: false }} composer={{ phases: PHASES }} /> : null}
    </>
  );
}

export const DeepLink: Story = {
  name: 'deep link · #<id> goes there',
  args: { cursor: MOMENTS.done },
  render: ({ cursor }) => <OpenedByLink cursor={cursor} />,
  play: async ({ canvasElement }) => {
    window.scrollTo(0, 0);
    window.location.hash = '#i/1';
    (canvasElement.querySelector('[data-open]') as HTMLElement).click();
    await waitFor(async () => expect(canvasElement.querySelector('[id="i/1"]')?.hasAttribute('data-target')).toBe(true));
    await waitFor(async () => {
      const box = canvasElement.querySelector('[id="i/1"]')?.getBoundingClientRect();
      await expect((box?.top ?? -1) >= 0 && (box?.bottom ?? Infinity) <= window.innerHeight).toBe(true);
    });
    const ids = [...canvasElement.querySelectorAll('[data-id]')].map((el) => [el.id, el.getAttribute('data-id')]);
    await expect(ids.filter(([id, data]) => id !== data)).toEqual([]);
    await expect(new Set(ids.map(([id]) => id)).size).toBe(ids.length);
    await expect(canvasElement.querySelector('[id="memory/d1"]')).not.toBeNull();
    history.replaceState(null, '', window.location.pathname + window.location.search);
  },
};

/**
 * What a side call wrote is a line into working memory, not a list under the
 * side call: its footer counts its patches by op, and a line runs from it to
 * each entry it touched -- faint at rest, lit when either end is pointed at.
 * The list is still there when the side call is opened.
 */
export const LinesIntoMemory: Story = {
  name: 'memory · lines from side calls to what they wrote',
  args: { cursor: MOMENTS.done },
  render: (args) => (
    <div style={{ width: 1900 }}>
      {meta.render(args)}
    </div>
  ),
  play: async ({ canvasElement }) => {
    const cell = q(canvasElement, '[data-branch="i/1"]') as HTMLElement;
    await expect(cell.querySelector('.ex-branch__patches')).toBeNull();
    await expect(cell.querySelector('.ex-patchsum')?.textContent).toMatch(/\+\s*\d/);
    // Lines are drawn for the side calls on screen.
    await waitFor(async () => expect(cell.style.visibility).not.toBe('hidden'));
    cell.scrollIntoView({ block: 'center' });
    const lines = () => [...document.querySelectorAll('.ex-links path.ex-link[data-branch="i/1"]')];
    await waitFor(async () => expect(lines().length).toBeGreaterThan(0));
    const entries = [...new Set([...document.querySelectorAll('.ex-links path.ex-link[data-branch="i/1"]')].map((l) => l.getAttribute('data-entry')))];
    await waitFor(async () => expect(lines().length).toBeGreaterThan(0));
    for (const entry of entries) await expect(canvasElement.querySelector(`[id="memory/${entry}"]`)).not.toBeNull();
    await expect(lines().some((l) => l.hasAttribute('data-hot'))).toBe(false);
    await userEvent.hover(cell.querySelector('.ex-block') as HTMLElement);
    await waitFor(async () => expect(lines().every((l) => l.hasAttribute('data-hot'))).toBe(true));
    for (const entry of entries) await expect(canvasElement.querySelector(`[id="memory/${entry}"]`)?.hasAttribute('data-hot')).toBe(true);
    await userEvent.hover(canvasElement.querySelector(`[id="memory/${entries[0]}"]`) as HTMLElement);
    await waitFor(async () => expect(cell.hasAttribute('data-hot')).toBe(true));
    // Opened, the side call lists its patches again.
    await userEvent.click(cell.querySelector('.ex-branch__why') as HTMLElement);
    await expect(cell.querySelector('.ex-branch__patches')).not.toBeNull();
  },
};

/** Working memory shut in its drawer: no lines into it. */
export const LinesDrawerShut: Story = {
  name: 'memory · no lines into a shut drawer',
  args: { cursor: MOMENTS.done },
  render: (args) => <div style={{ width: 900 }}>{meta.render(args)}</div>,
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(q(canvasElement, '.ex-session[data-drawer]')).not.toBeNull());
    // A side call with patches on screen: the only thing a line could come from.
    const cell = q(canvasElement, '[data-branch="i/1"]') as HTMLElement;
    await waitFor(async () => expect(cell.style.visibility).not.toBe('hidden'));
    cell.scrollIntoView({ block: 'center' });
    await new Promise((r) => setTimeout(r, 300));
    await expect(document.querySelectorAll('.ex-links path.ex-link')).toHaveLength(0);
  },
};
