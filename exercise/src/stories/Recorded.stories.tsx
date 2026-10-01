import type { Meta, StoryObj } from '@storybook/react-vite';
import { useState } from 'react';
import { expect, userEvent, waitFor } from 'storybook/test';

import { PHASES } from '../App.tsx';
import { cableOf } from './cables.ts';
import { pointAt } from './pointing.ts';
import { colourContrast, contrast, fillContrast } from './contrast.ts';
import { labelsOf, RECORDINGS, recordedAt } from '../drive/recorded.ts';
import type { RecordingName } from '../drive/recorded.ts';
import { fold } from '../session/fold.ts';
import { SessionView } from '../ui/SessionView.tsx';

interface RecordedArgs {
  /** Session time, ms. */
  readonly t: number;
  readonly curtain: boolean;
  readonly gaps: boolean;
  readonly condensed?: boolean;
  /** Which recording; the first drive unless a story says otherwise. */
  readonly recording?: RecordingName;
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
  render: ({ t, curtain, gaps, condensed = false, recording: name = 'first-drive' }) => (
    <SessionView session={fold(recordedAt(RECORDINGS[name], t))} surface={{ curtain, gaps, condensed }} composer={{ phases: PHASES }} />
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
    // A busy slot says which lane holds it; the side call's own id -- its line's seq -- is on hover.
    const running = [...canvasElement.querySelectorAll<HTMLElement>('.ex-branchcell')].find((c) => c.querySelector('[data-lane="ratify"][data-outcome="running"]'))?.dataset['branch'] ?? '';
    await expect(running).toMatch(/^\d+$/);
    const head = canvasElement.querySelector('.ex-lanehead[data-lane="ratify"]');
    await expect(head?.textContent).toContain('ratify');
    await expect(head?.textContent).not.toContain(running);
    await expect(head?.getAttribute('title')).toContain(running);
    const led = canvasElement.querySelector('.ex-header__slot[data-lane="ratify"]');
    await expect(led?.textContent).toContain('ratify');
    await expect(led?.textContent).not.toContain(running);
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
  // Its cables' reach is a sweep's geometry; 12b holds the traces to the same count.
  globals: { connectors: 'sweep' },
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
  // Its cable's height is a sweep's geometry.
  globals: { connectors: 'sweep' },
  play: async ({ canvasElement }) => {
    const events = recording.events as readonly Record<string, unknown>[];
    const request = events.find((e) => e['kind'] === 'request' && e['fork'] === 'e0100');
    const start = Number(request?.['t']);
    const trunkRequests = new Set(events.filter((e) => e['kind'] === 'request' && e['lane'] === 'trunk').map((e) => e['id']));
    const finished = events.filter(
      (e) => Number(e['t']) <= start && ((e['kind'] === 'response' && trunkRequests.has(e['to_request'])) || e['kind'] === 'tool.end'),
    );
    const last = finished.at(-1);
    // Labels in the recording; node ids are where they landed in its log.
    const labels = labelsOf(recording);
    const id = String(labels.get(String(last?.['kind'] === 'response' ? last['to_request'] : last?.['id'])));
    const cell = canvasElement.querySelector(`[data-branch="${labels.get('e0100')}"]`);
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
 * the page grows -- here with side calls queued past the trunk's end, and
 * through the page clamping the view as it relays itself out -- and lets go
 * the moment the person scrolls up, staying where they left it.
 */
export const FollowsTheBottom: Story = {
  name: '8 · following: locked to the bottom, where now is',
  render: () => <Following />,
  play: async ({ canvasElement }) => {
    const page = document.scrollingElement ?? document.documentElement;
    const atBottom = () => window.innerHeight + window.scrollY >= page.scrollHeight - 2;
    const where = (step: string) => `${step}: scrollY ${Math.round(window.scrollY)} + view ${window.innerHeight} of ${page.scrollHeight}`;
    const later = canvasElement.querySelector('[data-advance]') as HTMLElement;
    await waitFor(async () => expect(canvasElement.querySelectorAll('.ex-branchcell .ex-cable, .ex-wiring [data-net]').length).toBeGreaterThan(0));
    // Layout settles over a few passes (placements, seam pads); read heights only once it has.
    const settled = async () => {
      let last = -1;
      await waitFor(
        () => {
          const now = page.scrollHeight;
          const moved = now !== last;
          last = now;
          if (moved) throw new Error('still laying out');
        },
        { timeout: 4000, interval: 150 },
      );
    };
    await settled();
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
    // The page moves the view too, and that is not the person letting go: a scroll up away from the bottom
    // with no hand on it -- as WebKit reports a clamp during a layout pass, after the page has grown back --
    // and then the page grows: the view follows.
    const session = canvasElement.querySelector('.ex-session') as HTMLElement;
    window.scrollTo(0, window.scrollY - 300);
    await new Promise((r) => setTimeout(r, 100));
    const spacer = session.appendChild(Object.assign(document.createElement('div'), { style: 'height: 400px' }));
    await waitFor(async () => expect(atBottom(), where('after the page moved the view, and grew')).toBe(true));
    spacer.remove();
    await new Promise((r) => setTimeout(r, 200));
    // Scrolled away by the person -- a wheel, then the scroll it makes: what they were reading stays where
    // it was on screen while the page grows (the browser may move scrollY to keep it there).
    window.dispatchEvent(new WheelEvent('wheel', { deltaY: -800 }));
    window.scrollTo(0, window.scrollY - 800);
    await new Promise((r) => setTimeout(r, 100));
    const reading = [...canvasElement.querySelectorAll('.ex-branchcell')].find((c) => {
      const r = c.getBoundingClientRect();
      return r.top > 80 && r.bottom < window.innerHeight - 200;
    });
    await expect(reading).toBeDefined();
    const seen = reading?.getBoundingClientRect().top ?? 0;
    const tall = page.scrollHeight;
    later.click();
    // The page has grown -- only then is staying put a choice, not a page that has not moved yet.
    await waitFor(async () => expect(page.scrollHeight).toBeGreaterThan(tall));
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    await expect(atBottom(), where('scrolled away, after the page grew')).toBe(false);
    await expect(Math.abs((reading?.getBoundingClientRect().top ?? 0) - seen)).toBeLessThan(2);
    // The composer's fade is as wide as the composer: a lane passing under the strip beyond it is neither painted over nor unreachable.
    const strip = canvasElement.querySelector('.ex-session__composer') as HTMLElement;
    const composer = strip.querySelector('.ex-composer')?.getBoundingClientRect();
    const lane = canvasElement.querySelector('.ex-lane')?.getBoundingClientRect();
    const band = strip.getBoundingClientRect();
    await expect(getComputedStyle(strip).backgroundImage).toBe('none');
    const fade = parseFloat(getComputedStyle(strip, '::before').width);
    await expect(fade).toBeLessThanOrEqual((composer?.width ?? 0) + 1);
    await expect(lane?.right ?? 0).toBeGreaterThan((composer?.right ?? 0) + 40);
    const hit = document.elementFromPoint((lane?.right ?? 0) - 20, band.top + 8);
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
    // The floor's mimicry is confounded (#25): unmeasured, the migration's count of 7 kept on hover.
    await expect(row('mimicry')).toBe('unmeasured (confounded)');
    await expect(receipt?.querySelector('[data-measure="mimicry"]')?.getAttribute('title')).toContain('7 typed so in this log');
    await expect(row('idle-before-refill')).toBe('442 s · 637 s');
    await expect(row('side-call-time-in-gap')).toBe('≤ 100%');
  },
};

/** In daylight the minimap's slivers are lighter: visible, not heavy -- grey on white weighs more than light on black. */
export const MinimapInDaylight: Story = {
  name: '5b · the minimap in daylight',
  globals: { theme: 'paper', mode: 'light' },
  play: async ({ canvasElement }) => {
    const sliver = await waitFor(() => canvasElement.querySelector('.ex-mm__trunk[data-tone="assistant"]') ?? Promise.reject(new Error('no sliver yet')));
    const ratio = fillContrast(sliver);
    await expect(ratio).toBeGreaterThan(1.2);
    await expect(ratio).toBeLessThan(1.8);
  },
};

/**
 * Another drive: the person cancelled a capture round after turn 2, and the
 * side calls it had not fired never ran. The record says `capture.cancelled`,
 * which this vocabulary does not have: it is carried, counted, and named in
 * the header -- a gap to close, drawn as one.
 */
export const CancelledCapture: Story = {
  name: '10 · a capture round cancelled: a kind the vocabulary lacks',
  args: { recording: 'cancelled-capture' },
  play: async ({ canvasElement }) => {
    const unknown = canvasElement.querySelector('.ex-header__unknown');
    await expect(unknown?.textContent).toBe('1 unknown');
    await expect(unknown?.getAttribute('title')).toContain('capture.cancelled');
    await expect(canvasElement.querySelector('[data-state="ended"]')).not.toBeNull();
  },
};

/** Another drive: turn 2 ran thirty steps and was stopped at the limit; the turn end says so. */
export const StepLimit: Story = {
  name: '11 · the step limit',
  args: { recording: 'step-limit' },
  play: async ({ canvasElement }) => {
    const end = canvasElement.querySelector('.ex-turnend');
    await expect(end?.textContent).toMatch(/step limit|max_steps/);
  },
};

/**
 * The lines into memory and the trunk's cables routed as a wiring harness
 * (harness.ts) instead of curves, on the densest drive: 111 patches an ask.
 * Each net's lines share one track in a gutter and fork to where they go;
 * crossings hop.
 */
export const Harness: Story = {
  name: '12 · lines into memory as a harness',
  args: { recording: 'step-limit' },
  globals: { connectors: 'trace', crossings: 'hop', corners: 'round' },
  // Wide enough for working memory beside the lanes, not in its drawer.
  play: async ({ canvasElement }) => {
    const cell = canvasElement.querySelector('[data-branch]:has(.ex-patchsum)') as HTMLElement;
    cell.scrollIntoView({ block: 'center' });
    const nets = () => [...document.querySelectorAll('.ex-links .ex-net__run')].map((p) => p.getAttribute('d') ?? '');
    await waitFor(async () => expect(nets().length).toBeGreaterThan(0));
    // The trunk's cables too, drawn together over the stage rather than each in its side call.
    const cables = [...canvasElement.querySelectorAll('.ex-wiring [data-net] .ex-cable__line')].map((p) => p.getAttribute('d') ?? '');
    await expect(cables.length).toBeGreaterThan(0);
    await expect(canvasElement.querySelector('.ex-branchcell .ex-cable')).toBeNull();
    // Straight runs and corners only: no curve anywhere.
    await expect([...nets(), ...cables].every((d) => /^[MLA\d .-]+$/.test(d))).toBe(true);
    // Scrolled a little, step by step, the lines are redrawn, never lost.
    for (let step = 0; step < 4; step++) {
      window.scrollBy(0, 20);
      await new Promise((settle) => setTimeout(settle, 150));
      await expect(nets().length).toBeGreaterThan(0);
    }
    // At rest a line is the net's; pointed at, a side call's own lines are drawn over it.
    const lit = () => [...document.querySelectorAll(`.ex-links .ex-link[data-from="${cell.dataset.branch}"]`)].filter((l) => getComputedStyle(l).stroke !== 'rgba(0, 0, 0, 0)');
    await expect(lit()).toHaveLength(0);
    await pointAt(cell.querySelector('.ex-block') as HTMLElement, async () => {
      await expect(lit().length).toBeGreaterThan(0);
    });
  },
};

/** The same on the first drive, where more side calls write at once: breaking at crossings, corners cut at 45°. */
export const HarnessFirstDrive: Story = {
  name: '12b · a harness on the first drive, gaps and chamfers',
  globals: { connectors: 'trace', crossings: 'gap', corners: 'chamfer' },
  play: async ({ canvasElement }) => {
    // Every side call is cabled to the trunk node it came from.
    const ids = [...canvasElement.querySelectorAll('.ex-branchcell')].map((c) => c.getAttribute('data-branch') ?? '');
    await expect(ids).toHaveLength(94);
    await waitFor(async () => expect(ids.filter((id) => cableOf(canvasElement, id) === undefined)).toEqual([]));
  },
};

/**
 * OpenCode, against the same local model: native tool calls, several in one
 * step, six tools (only `bash` is one this surface knows by name). The
 * transcript kept when each call began, not how many tokens came before it,
 * so each call's share reads in time alone. Its side calls did not run --
 * they are stitched on (scripts/stitch-sides.py) and its header says so --
 * and OpenCode compacting its context is a kind this vocabulary lacks.
 */
export const VoxelStress: Story = {
  name: '13 · OpenCode: native calls, several a step',
  args: { recording: 'voxel-stress' },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelectorAll('.ex-block--tool')).toHaveLength(105);
    await expect(canvasElement.querySelectorAll('.ex-branchcell')).toHaveLength(14);
    // The first call of a step says what writing the calls took, in its header: a time, with its tokens unknown.
    const shares = [...canvasElement.querySelectorAll('.ex-block--tool > .ex-block__head .ex-block__flow')].map((f) => f.textContent ?? '');
    await expect(shares.length).toBeGreaterThan(0);
    await expect(shares.every((s) => /^\+\? tok in /.test(s))).toBe(true);
    // A step's calls are blocks in turn: nine of them after one message.
    const runs = [...canvasElement.querySelectorAll('.ex-trunk__node')].map((n) => n.getAttribute('data-kind'));
    let longest = 0;
    let run = 0;
    for (const kind of runs) {
      run = kind === 'tool' ? run + 1 : 0;
      longest = Math.max(longest, run);
    }
    await expect(longest).toBe(9);
    // A later call of a step says nothing of what writing it took: its header has no line after its chip.
    const heads = [...canvasElement.querySelectorAll('.ex-block--tool > .ex-block__head')];
    await expect(heads.filter((h) => h.querySelector('.ex-block__flow') === null).length).toBe(105 - shares.length);
    await expect(canvasElement.textContent).not.toMatch(/\b1 lines\b/);
    await expect(canvasElement.querySelector('.ex-header__unknown')?.getAttribute('title')).toContain('compaction');
  },
};

/**
 * The chrome around the trunk, legible in every look: the seam, working
 * memory, the receipt and the header read at WCAG AA for small text (4.5:1)
 * -- text set in the faint ink, a label or an id, at 3:1, as a footer's
 * units are; an entry struck out as superseded or retired is faint on
 * purpose, and not held to it. The minimap's seams stand off its track, and
 * its alarm ticks off the page-coloured halo they are drawn in, at 3:1, as a
 * mark that carries meaning must (WCAG 1.4.11).
 */
const LEGIBLE = [
  '.ex-seam__kind',
  '.ex-seam__reason',
  '.ex-header__item',
  '.ex-lanehead',
  ".ex-memory__entry:not([data-state='superseded'], [data-state='retired']) :is(.ex-memory__text, .ex-memory__id)",
  '.ex-receipt__head',
  '.ex-receipt dt',
  '.ex-receipt dd',
] as const;

/** An alarm tick against the halo it is drawn in -- which must be there, in the page's colour. */
function haloContrast(el: Element): number {
  const tick = getComputedStyle(el, '::after');
  const page = getComputedStyle(el.closest('.ex-root') ?? document.body).backgroundColor;
  if (!tick.boxShadow.startsWith(`${page} 0px 0px 0px 1px`)) return 0;
  return colourContrast(tick.backgroundColor, page);
}

/** Measured at rest: with motion still, an entry that just landed is not mid-flash. */
const ChromeLegible: Story = {
  play: async ({ canvasElement }) => {
    await waitFor(() => expect(canvasElement.querySelectorAll('.ex-mm__seam').length).toBeGreaterThan(0));
    // The faint ink, as this look resolves it: text in it is held to 3:1.
    const probe = canvasElement.querySelector('.ex-root, .ex-session')!.appendChild(document.createElement('span'));
    probe.style.color = 'var(--ink-faint)';
    const faint = getComputedStyle(probe).color;
    probe.remove();
    // A selector that matches nothing would pass vacuously: each is on screen.
    await expect(LEGIBLE.filter((selector) => canvasElement.querySelector(selector) === null)).toEqual([]);
    const failing = [
      ...LEGIBLE.flatMap((selector) =>
        [...canvasElement.querySelectorAll(selector)].map((el) => ({ what: `${selector.slice(-24)}`, floor: getComputedStyle(el).color === faint ? 3 : 4.5, ratio: contrast(el) })),
      ),
      ...[...canvasElement.querySelectorAll('.ex-mm__seam')].map((el) => ({ what: 'minimap seam', floor: 3, ratio: fillContrast(el) })),
      ...[...canvasElement.querySelectorAll('.ex-minimap > [data-alarm]')].map((el) => ({ what: `minimap ${el.getAttribute('data-alarm')} tick`, floor: 3, ratio: haloContrast(el) })),
    ]
      .filter(({ ratio, floor }) => ratio < floor)
      .map(({ what, floor, ratio }) => `${what} ${ratio.toFixed(2)} < ${floor}`);
    await expect([...new Set(failing)]).toEqual([]);
  },
};

export const ChromeBloom: Story = { ...ChromeLegible, name: 'chrome · legible, bloom', globals: { theme: 'bloom', mode: 'dark', motion: 'off' } };
export const ChromeBloomLight: Story = { ...ChromeLegible, name: 'chrome · legible, bloom in daylight', globals: { theme: 'bloom', mode: 'light', motion: 'off' } };
export const ChromePaper: Story = { ...ChromeLegible, name: 'chrome · legible, paper', globals: { theme: 'paper', mode: 'light', motion: 'off' } };
export const ChromePaperDark: Story = { ...ChromeLegible, name: 'chrome · legible, paper in the dark', globals: { theme: 'paper', mode: 'dark', motion: 'off' } };
export const ChromeEmboss: Story = { ...ChromeLegible, name: 'chrome · legible, emboss', globals: { theme: 'emboss', mode: 'dark', motion: 'off' } };
export const ChromeEmbossLight: Story = { ...ChromeLegible, name: 'chrome · legible, emboss in daylight', globals: { theme: 'emboss', mode: 'light', motion: 'off' } };
