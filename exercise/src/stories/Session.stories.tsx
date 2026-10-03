import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect, userEvent, waitFor, within } from 'storybook/test';

import { PHASES } from '../App.tsx';
import type { Cursor } from '../drive/canned.ts';
import { useState } from 'react';

import { SessionView } from '../ui/SessionView.tsx';
import type { Surface } from '../ui/surface.tsx';
import { cableOf } from './cables.ts';
import { pointAt } from './pointing.ts';
import { idAt, MOMENTS, sessionAt, variantAt } from './moments.ts';

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
    const id = (label: string) => idAt(MOMENTS.bigRead, label);
    // The call is one block after the message that wrote it: the command, then its first lines, not all 1,860.
    await expect(q(canvasElement, `[data-id="${id('t/2')}"] .ex-tool__call`)?.textContent).toBe('$ cat src/report.rs');
    await expect(q(canvasElement, `[data-id="${id('t/2')}"] .ex-tool__output`)?.textContent?.split('\n')).toHaveLength(3);
  },
};

/** DoD 3: settled; the interview runs in slot 1 while the operator reads. */
export const IdleGapInterview: Story = {
  name: '4 · an interview in the idle gap',
  args: { cursor: MOMENTS.idleGapInterview },
  play: async ({ canvasElement }) => {
    const id = (label: string) => idAt(MOMENTS.idleGapInterview, label);
    await expect(q(canvasElement, '[data-state="capture"]')).not.toBeNull();
    await expect(q(canvasElement, `[data-branch="${id('i/1')}"] [data-outcome="running"]`)).not.toBeNull();
    // It started when the answer settled: it is drawn below the answer, in the idle gap it ran in.
    await waitFor(async () => expect(top(canvasElement, `[data-branch="${id('i/1')}"]`)).toBeGreaterThanOrEqual(bottom(canvasElement, `[data-id="${id('q/3')}"]`)));
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
    const id = (label: string) => idAt(MOMENTS.idleGapInterview, label);
    const cell = `[data-branch="${id('i/1')}"]`;
    await waitFor(async () => expect(q(canvasElement, `${cell}[data-pending]`)).not.toBeNull());
    await expect(q(canvasElement, `${cell} [data-outcome="pending"]`)?.textContent).toContain('queued');
    // Its cable is laid, dashed, and carries no light yet.
    await waitFor(async () => expect(cableOf(canvasElement, id('i/1'))).toEqual({ pending: true, live: false }));
    await expect(top(canvasElement, cell)).toBeGreaterThanOrEqual(bottom(canvasElement, `[data-id="${id('q/3')}"]`));
  },
};

/** The same, with connectors drawn as sweeps. */
export const InterviewQueuedSwept: Story = { ...InterviewQueued, name: '4b · an interview waiting for its slot, swept', globals: { connectors: 'sweep' } };

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
  play: async ({ canvasElement }) => {
    await expect(q(canvasElement, '.ex-session')?.getAttribute('data-state')).toBe('awaiting');
    await expect(q(canvasElement, '[id="memory/o1"]')?.getAttribute('data-state')).toBe('superseded');
    await expect(q(canvasElement, '[id="memory/d3"]')?.getAttribute('data-state')).toBe('live');
  },
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

/**
 * After the refill: a targeted read, because working memory said where to look -- Report::print, where the
 * first turn read the whole file. (The specimen's read is authored, 28 lines of the 49 that `sed` names.)
 */
export const BuildReading: Story = {
  name: '9 · the build turn reads Report::print, not all 1,860 lines',
  args: { cursor: MOMENTS.buildReading },
  play: async ({ canvasElement }) => {
    const said = (command: string) => [...canvasElement.querySelectorAll('.ex-block')].find((b) => b.textContent?.includes(command))?.textContent ?? '';
    await expect(said('sed -n 40,88p src/report.rs')).toMatch(/impl Report[\s\S]*(?<!\d)28 lines · /);
    await expect(said('cat src/report.rs')).toContain('1,860 lines');
  },
};

/** An interview in the idle gap of a running tool call: `cargo test` leaves the trunk waiting. */
export const TestsRunning: Story = {
  name: '10 · an interview while cargo test runs',
  args: { cursor: MOMENTS.testsRunning },
  play: async ({ canvasElement }) => {
    const id = (label: string) => idAt(MOMENTS.testsRunning, label);
    await expect(q(canvasElement, `[data-id="${id('t/5')}"].ex-block--live`)).not.toBeNull();
    await expect(q(canvasElement, `[data-branch="${id('i/4')}"]`)).not.toBeNull();
  },
};

export const Done: Story = {
  name: '11 · done: the definition of done, end to end',
  args: { cursor: MOMENTS.done },
  play: async ({ canvasElement }) => {
    // The session at rest after both phases: one refill between them, every side call settled, memory kept.
    await expect(q(canvasElement, '.ex-session')?.getAttribute('data-state')).toBe('awaiting');
    await expect(canvasElement.querySelectorAll('.ex-era__seam .ex-seam')).toHaveLength(1);
    await expect(canvasElement.querySelectorAll('.ex-branch[data-outcome="running"], .ex-branch[data-outcome="pending"]')).toHaveLength(0);
    await expect(canvasElement.querySelectorAll('.ex-branch').length).toBeGreaterThan(0);
    await expect(canvasElement.querySelectorAll('.ex-memory__entry')).toHaveLength(11);
  },
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
  render: ({ cursor, curtain, gaps }) => <SessionView session={sessionAt(cursor)} surface={{ curtain, gaps }} composer={{ phases: PHASES }} />,
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
    await expect(q(canvasElement, '.ex-session__memory .ex-memory')).not.toBeNull();
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
    // A block names what it waits on, and keeps its actions and cites, inside its own header: nothing floats
    // in the gap between blocks, where the one above would be under it.
    const blocks = [...canvasElement.querySelectorAll('.ex-trunk .ex-block:not(.ex-block--thin)')] as HTMLElement[];
    const named = blocks.filter((b) => b.dataset['needs']);
    await expect(named.length).toBeGreaterThan(0);
    for (const block of named) await expect(block.querySelector(':scope > .ex-block__head > .ex-block__needs')?.textContent).toBe(`needs ${block.dataset['needs']}`);
    for (const block of blocks) {
      const box = block.getBoundingClientRect();
      for (const part of block.querySelectorAll('.ex-block__corner, .ex-block__needs')) {
        const at = part.getBoundingClientRect();
        await expect(at.top >= box.top && at.bottom <= box.bottom && at.right <= box.right).toBe(true);
      }
    }
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
    const id = (label: string) => idAt(MOMENTS.idleGapInterview, label);
    await expect(q(canvasElement, '.ex-session--condensed')).not.toBeNull();
    await expect(q(canvasElement, '.ex-lane')?.getBoundingClientRect().width).toBeLessThan(24);
    await expect(q(canvasElement, '.ex-branch')).toBeNull();
    const bars = canvasElement.querySelectorAll('.ex-bar');
    await expect(bars.length).toBeGreaterThan(0);
    await expect(bars.length).toBe(canvasElement.querySelectorAll('.ex-branchcell').length);
    await expect(q(canvasElement, `[data-branch="${id('i/1')}"] .ex-bar[data-live]`)).not.toBeNull();
    // Every bar keeps its cable.
    const ids = [...canvasElement.querySelectorAll('.ex-branchcell')].map((c) => c.getAttribute('data-branch') ?? '');
    await waitFor(async () => expect(ids.filter((id) => cableOf(canvasElement, id) === undefined)).toEqual([]));
  },
};

/** A bar is a way in: pressing it opens the curtain on that side call. */
export const CondensedOpens: Story = {
  name: 'condensed · pressing a bar opens it',
  args: { cursor: MOMENTS.done, condensed: true },
  play: async ({ canvasElement }) => {
    const id = (label: string) => idAt(MOMENTS.done, label);
    const bar = q(canvasElement, `[data-branch="${id('i/1')}"] .ex-bar`) as HTMLElement;
    await expect(bar.getAttribute('aria-label')).toContain(id('i/1'));
    await userEvent.click(bar);
    await expect(q(canvasElement, '.ex-session--condensed')).toBeNull();
    await expect(q(canvasElement, `[data-branch="${id('i/1')}"] .ex-branch`)).not.toBeNull();
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
    try {
      window.scrollTo(0, 0);
      // A side call's address is its line's `seq`, as every node's is.
      const target = idAt(MOMENTS.done, 'i/1');
      window.location.hash = `#${target}`;
      (canvasElement.querySelector('[data-open]') as HTMLElement).click();
      await waitFor(async () => expect(canvasElement.querySelector(`[id="${target}"]`)?.hasAttribute('data-target')).toBe(true));
      await waitFor(async () => {
        const box = canvasElement.querySelector(`[id="${target}"]`)?.getBoundingClientRect();
        await expect((box?.top ?? -1) >= 0 && (box?.bottom ?? Infinity) <= window.innerHeight).toBe(true);
      });
      const ids = [...canvasElement.querySelectorAll('[data-id]')].map((el) => [el.id, el.getAttribute('data-id')]);
      await expect(ids.filter(([id, data]) => id !== data)).toEqual([]);
      await expect(new Set(ids.map(([id]) => id)).size).toBe(ids.length);
      await expect(canvasElement.querySelector('[id="memory/d1"]')).not.toBeNull();
    } finally {
      // Whatever happened, the address is left as found: a later story does not open on this one's hash.
      history.replaceState(null, '', window.location.pathname + window.location.search);
    }
  },
};

/**
 * The lines into working memory are drawn over the page, so a scroll moves
 * the side call under them and they must follow at once: in the frame the
 * scroll is handled in, not a render later. Checked by the end of that
 * frame's animation callbacks -- a line still where it was has lagged.
 */
export const LinesKeepUp: Story = {
  name: 'memory · lines keep up with a scroll',
  args: { cursor: MOMENTS.done },
  play: async ({ canvasElement }) => {
    const id = (label: string) => idAt(MOMENTS.done, label);
    const line = () => document.querySelector(`.ex-links .ex-link[data-from="${id('i/1')}"]`)?.getAttribute('d') ?? '';
    const cell = q(canvasElement, `[data-branch="${id('i/1')}"]`) as HTMLElement;
    await waitFor(async () => expect(cell.style.visibility).not.toBe('hidden'));
    cell.scrollIntoView({ block: 'center' });
    await waitFor(async () => expect(line()).not.toBe(''));
    await new Promise((settle) => setTimeout(settle, 300));
    for (const by of [120, -80, 60]) {
      const before = line();
      const moved = new Promise<void>((done) =>
        window.addEventListener('scroll', () => requestAnimationFrame(() => done()), { once: true }),
      );
      window.scrollBy(0, by);
      await moved;
      await expect(line()).not.toBe(before);
    }
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
  play: async ({ canvasElement }) => {
    const id = (label: string) => idAt(MOMENTS.done, label);
    const cell = q(canvasElement, `[data-branch="${id('i/1')}"]`) as HTMLElement;
    await expect(cell.querySelector('.ex-branch__patches')).toBeNull();
    await expect(cell.querySelector('.ex-patchsum')?.textContent).toMatch(/\+\s*\d/);
    // Lines are drawn for the side calls on screen.
    await waitFor(async () => expect(cell.style.visibility).not.toBe('hidden'));
    cell.scrollIntoView({ block: 'center' });
    const lines = () => [...document.querySelectorAll(`.ex-links path.ex-link[data-from="${id('i/1')}"]`)];
    await waitFor(async () => expect(lines().length).toBeGreaterThan(0));
    const entries = [...new Set(lines().map((l) => l.getAttribute('data-entry')))];
    for (const entry of entries) await expect(canvasElement.querySelector(`[id="memory/${entry}"]`)).not.toBeNull();
    await expect(lines().some((l) => l.hasAttribute('data-hot'))).toBe(false);
    await pointAt(cell.querySelector('.ex-block') as HTMLElement, async () => {
      await expect(lines().every((l) => l.hasAttribute('data-hot'))).toBe(true);
      for (const entry of entries) await expect(canvasElement.querySelector(`[id="memory/${entry}"]`)?.hasAttribute('data-hot')).toBe(true);
    });
    await pointAt(canvasElement.querySelector(`[id="memory/${entries[0]}"]`) as HTMLElement, async () => {
      await expect(cell.hasAttribute('data-hot')).toBe(true);
    });
    // Opened, the side call lists its patches again.
    await userEvent.click(cell.querySelector('.ex-branch__why') as HTMLElement);
    await expect(cell.querySelector('.ex-branch__patches')).not.toBeNull();
  },
};

/**
 * Pointing at a trunk node lights its chain: the node, the side calls off
 * it, their cables, the lines into memory and the entries they wrote.
 * Pointing at a cable lights the two things it joins, and their chain.
 */
export const ChainFromANode: Story = {
  name: 'chain · a trunk node, and a cable, light what they join',
  args: { cursor: MOMENTS.done },
  play: async ({ canvasElement }) => {
    const node = q(canvasElement, '.ex-trunk__node[data-node]') as HTMLElement;
    const id = node.getAttribute('data-node') ?? '';
    const sides = [...canvasElement.querySelectorAll('.ex-branchcell')].filter((c) => {
      const net = [...canvasElement.querySelectorAll(`.ex-wiring [data-point][data-node="${id}"]`)];
      return net.some((h) => (h.getAttribute('data-branches') ?? '').split(' ').includes(c.getAttribute('data-branch') ?? ''));
    });
    await expect(sides.length).toBeGreaterThan(0);
    const lit = () => ({
      node: node.hasAttribute('data-hot'),
      sides: sides.every((c) => c.hasAttribute('data-hot')),
      cables: sides.every((c) => q(canvasElement, `.ex-wiring [data-hot][data-from="${c.getAttribute('data-branch')}"]`) !== null),
      others: [...canvasElement.querySelectorAll('.ex-branchcell[data-hot]')].length === sides.length,
    });
    await pointAt(node.querySelector('.ex-block') as HTMLElement, async () => {
      await expect(lit()).toEqual({ node: true, sides: true, cables: true, others: true });
      // Its entries are lit too.
      await expect(q(canvasElement, '.ex-memory__entry[data-hot]')).not.toBeNull();
    });
    await userEvent.unhover(node.querySelector('.ex-block') as HTMLElement);
    await waitFor(async () => expect(node.hasAttribute('data-hot')).toBe(false));
    // The cable itself: its node and the side calls on it.
    await pointAt(q(canvasElement, `.ex-wiring [data-point][data-node="${id}"]`) as Element, async () => {
      await expect(lit()).toMatchObject({ node: true, cables: true });
    });
  },
};

/** A line into memory, pointed at: its side call and that one entry, not the rest of what the side call wrote. */
export const ChainFromALine: Story = {
  name: 'chain · a line into memory lights its two ends',
  args: { cursor: MOMENTS.done },
  globals: { connectors: 'sweep' },
  play: async ({ canvasElement }) => {
    const id = (label: string) => idAt(MOMENTS.done, label);
    const cell = q(canvasElement, `.ex-branchcell[data-branch="${id('i/1')}"]`) as HTMLElement;
    cell.scrollIntoView({ block: 'center' });
    const hits = () => [...document.querySelectorAll(`.ex-links [data-point][data-branches="${id('i/1')}"][data-entry]`)];
    await waitFor(async () => expect(hits().length).toBeGreaterThan(1));
    const hit = hits()[0] as Element;
    const entry = hit.getAttribute('data-entry') ?? '';
    await pointAt(hit, async () => {
      await expect(cell.hasAttribute('data-hot')).toBe(true);
      const hotEntries = [...canvasElement.querySelectorAll('.ex-memory__entry[data-hot]')].map((e) => e.id);
      await expect(hotEntries).toEqual([`memory/${entry}`]);
      await expect([...document.querySelectorAll('.ex-links .ex-link[data-hot]')].map((l) => l.getAttribute('data-entry'))).toEqual([entry]);
      // Its sweep cable to the trunk is lit, and the node it came from.
      await expect(cell.querySelector('.ex-cable')?.hasAttribute('data-hot')).toBe(true);
      await expect(q(canvasElement, '.ex-trunk__node[data-hot]')).not.toBeNull();
    });
  },
};

/** Focus lights a chain as the pointer does: a control inside a side call, reached by keyboard. */
export const ChainByFocus: Story = {
  name: 'chain · focus lights it too',
  args: { cursor: MOMENTS.done },
  play: async ({ canvasElement }) => {
    const id = (label: string) => idAt(MOMENTS.done, label);
    const cell = q(canvasElement, `.ex-branchcell[data-branch="${id('i/1')}"]`) as HTMLElement;
    // Placed first: until then the cell is `visibility: hidden`, and nothing in it can take focus (a slow
    // runner reached the focus before the placement, and the focus silently went nowhere).
    await waitFor(async () => expect(cell.style.visibility).not.toBe('hidden'));
    const target = cell.querySelector('button, summary, [tabindex]') as HTMLElement;
    target.focus();
    await expect(document.activeElement).toBe(target);
    await waitFor(async () => expect(cell.hasAttribute('data-hot')).toBe(true));
    await expect(q(canvasElement, '.ex-trunk__node[data-hot]')).not.toBeNull();
    // A pointer over nothing that points -- as Chromium reports under a mouse left still while the page moves
    // beneath it -- does not take away what the focus lit (CI saw it do so, twice).
    const bare = q(canvasElement, '.ex-session__need') as HTMLElement;
    bare.dispatchEvent(new PointerEvent('pointerover', { bubbles: true }));
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    await expect(cell.hasAttribute('data-hot')).toBe(true);
    await expect(q(canvasElement, '.ex-trunk__node[data-hot]')).not.toBeNull();
    (document.activeElement as HTMLElement).blur();
    await waitFor(async () => expect(cell.hasAttribute('data-hot')).toBe(false));
  },
};

/** Working memory shut in its drawer: no lines into it. */
export const LinesDrawerShut: Story = {
  name: 'memory · no lines into a shut drawer',
  args: { cursor: MOMENTS.done },
  render: (args) => <div style={{ width: 900 }}>{meta.render(args)}</div>,
  play: async ({ canvasElement }) => {
    const id = (label: string) => idAt(MOMENTS.done, label);
    await waitFor(async () => expect(q(canvasElement, '.ex-session[data-drawer]')).not.toBeNull());
    // A side call with patches on screen: the only thing a line could come from.
    const cell = q(canvasElement, `[data-branch="${id('i/1')}"]`) as HTMLElement;
    await waitFor(async () => expect(cell.style.visibility).not.toBe('hidden'));
    cell.scrollIntoView({ block: 'center' });
    // Open, the drawer has lines into it -- so none, shut, is the drawer's doing, not lines not drawn yet.
    const lines = () => document.querySelectorAll('.ex-links path.ex-link').length;
    const tab = await within(canvasElement).findByRole('button', { name: /working memory/ });
    await userEvent.click(tab);
    await waitFor(async () => expect(lines()).toBeGreaterThan(0));
    await userEvent.click(tab);
    await waitFor(async () => expect(lines()).toBe(0));
  },
};

/** The whole surface in a column as wide as a device: SessionView measures its own width, not the window's. */
const narrow = (width: number) =>
  function Render(args: MomentArgs) {
    return <div style={{ width }}>{meta.render(args)}</div>;
  };

/** Every horizontal extent inside the surface: nothing may reach past its right edge. */
const overflow = (root: HTMLElement) => {
  const edge = (q(root, '.ex-session') as HTMLElement).getBoundingClientRect().right;
  return [...root.querySelectorAll('.ex-block, .ex-composer, .ex-header')].filter((e) => e.getBoundingClientRect().right > edge + 1).length;
};

/**
 * A phone: no room beside the trunk for side calls even as bars, so the
 * curtain draws closed -- the header says why the rest cannot be picked --
 * and a message's marker opens its side calls under it, in the trunk.
 */
export const NarrowPhone: Story = {
  name: 'narrow · a phone: side calls open under their message',
  args: { cursor: MOMENTS.done },
  globals: { minimap: 'off' },
  render: narrow(375),
  play: async ({ canvasElement }) => {
    const session = q(canvasElement, '.ex-session') as HTMLElement;
    await waitFor(async () => expect(session.dataset['room']).toBe('none'));
    await expect(session.classList.contains('ex-session--curtain')).toBe(false);
    await expect((q(canvasElement, 'input[name="ex-curtain"][value="open"]') as HTMLInputElement).disabled).toBe(true);
    await expect((q(canvasElement, 'input[name="ex-curtain"][value="closed"]') as HTMLInputElement).checked).toBe(true);
    await expect(overflow(canvasElement)).toBe(0);
    await expect(q(canvasElement, '.ex-trunk')!.getBoundingClientRect().width).toBeGreaterThan(300);
    const node = q(canvasElement, '.ex-trunk__node:has(> .ex-peek)') as HTMLElement;
    await userEvent.click(node.querySelector('.ex-peek') as HTMLElement);
    const opened = node.querySelector('.ex-inline .ex-block--lane') as HTMLElement;
    await expect(opened.classList.contains('ex-block--thin')).toBe(false);
    await expect(overflow(canvasElement)).toBe(0);
    await userEvent.click(node.querySelector('.ex-peek') as HTMLElement);
    await expect(node.querySelector('.ex-inline')).toBeNull();
  },
};

/**
 * A tablet: room for side calls as bars, not whole, so they condense; a
 * bar pressed opens its side call under the message it came from.
 */
export const NarrowTablet: Story = {
  name: 'narrow · a tablet: side calls condense to bars',
  args: { cursor: MOMENTS.done },
  globals: { minimap: 'off' },
  render: narrow(700),
  play: async ({ canvasElement }) => {
    const session = q(canvasElement, '.ex-session') as HTMLElement;
    await waitFor(async () => expect(session.dataset['room']).toBe('bars'));
    await expect(session.classList.contains('ex-session--condensed')).toBe(true);
    await expect((q(canvasElement, 'input[name="ex-curtain"][value="open"]') as HTMLInputElement).disabled).toBe(true);
    await expect(overflow(canvasElement)).toBe(0);
    await expect(q(canvasElement, '.ex-trunk')!.getBoundingClientRect().width).toBeGreaterThanOrEqual(320);
    const bar = q(canvasElement, '.ex-bar') as HTMLElement;
    const id = bar.closest('.ex-branchcell')?.getAttribute('data-branch') ?? '';
    await userEvent.click(bar);
    await expect(session.classList.contains('ex-session--condensed')).toBe(true);
    const opened = q(canvasElement, `.ex-trunk .ex-inline[data-branch-inline="${id}"]`) as HTMLElement;
    await expect(opened.querySelector('.ex-block--lane')).not.toBeNull();
    // Opened, it is tied to its bar: the two are lit as one chain, and the cable leaves the trunk level
    // with the opened block's header, not the message above it.
    await expect(bar.closest('.ex-branchcell')?.hasAttribute('data-hot')).toBe(true);
    const stage = (q(canvasElement, '.ex-stage') as HTMLElement).getBoundingClientRect();
    const head = (opened.querySelector('.ex-block__head') as HTMLElement).getBoundingClientRect();
    await waitFor(async () => {
      const d = q(canvasElement, `.ex-wiring [data-from="${id}"] path`)?.getAttribute('d') ?? '';
      const y = Number(/^M\s*[\d.]+[ ,]([\d.]+)/.exec(d)?.[1]);
      await expect(y >= head.top - stage.top && y <= head.bottom - stage.top).toBe(true);
    });
  },
};
