import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect, userEvent, waitFor } from 'storybook/test';

import { PHASES } from '../App.tsx';
import { AUTHORED } from '../drive/examples.ts';
import { CHUNKED_AUDITS, GATED, MERGE, QUIET_STEP, WHOLE_AUDITS, firedStep, latencyOf } from '../drive/moments.ts';
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
    // The layout is a fixed point (#862): a meditation's pad resizes the page, which lays it out again, so pads that a
    // pass changes are padded again on the next -- the trunk ran off the page that way. Once settled, another pass gives
    // the same pads; and none is more than every side call stacked, which a pass pinned at the browser's limit is.
    const pads = () => [...canvasElement.querySelectorAll<HTMLElement>('.ex-era__meditation')].map((m) => parseFloat(m.style.paddingTop) || 0);
    const settled = await waitFor(async () => {
      const first = pads();
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      await expect(pads()).toEqual(first);
      return first;
    });
    await new Promise((resolve) => setTimeout(resolve, 250));
    await expect(pads()).toEqual(settled);
    const stacked = [...canvasElement.querySelectorAll<HTMLElement>('.ex-branchcell')].reduce((sum, cell) => sum + cell.offsetHeight, 0);
    await expect(settled.filter((pad) => pad > stacked)).toEqual([]);
    // And it is a barrier: every side call settled before a meditation is drawn above it -- its audit's chunks among them,
    // stacked in their slot below one another (#854). One still running when a window meditation refilled runs across it.
    const settledAt = new Map(log.flatMap((line) => (line.kind === 'fork.settled' ? [[String(line.fork), line.seq] as const] : [])));
    const meditations = [...canvasElement.querySelectorAll<HTMLElement>('.ex-era__meditation .ex-meditation')];
    const over = meditations.flatMap((meditation) =>
      [...canvasElement.querySelectorAll<HTMLElement>('.ex-branchcell')]
        .filter((cell) => (settledAt.get(cell.dataset['branch'] ?? '') ?? Infinity) < Number(meditation.dataset['id']) && cell.getBoundingClientRect().bottom > meditation.getBoundingClientRect().top)
        .map((cell) => `${cell.dataset['branch']} over ${meditation.dataset['id']}`),
    );
    await expect(over).toEqual([]);
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

/** The warrant gate on each step (#850, #862): which probes fired and how strongly, or that none did -- as logged. */
export const WarrantGate: Story = {
  name: '4 · the warrant gate on each step',
  play: async ({ canvasElement }) => {
    const gate = (step: number) => canvasElement.querySelector<HTMLElement>(`.ex-trunk [data-id="${step}"] .ex-gate`);
    await waitFor(async () => expect(canvasElement.querySelectorAll('.ex-trunk .ex-gate')).toHaveLength(GATED.size));
    // The missing-file step: three probes fired, strongest first; the background build's: one.
    const missing = firedStep('constraint', 'decision', 'gotcha');
    await expect([gate(missing)?.textContent, gate(missing)?.dataset['fired']]).toEqual(['gate · gotcha 0.93 · decision 0.71 · constraint 0.64', '']);
    await expect(gate(missing)?.closest<HTMLElement>('.ex-stat')?.title).toBe(
      `decided by canned-scenario-classifier@0000000000000000000000000000000000000000 · threshold 0.5 · compute none · ${latencyOf(missing)} ms\nconstraint 0.64 fired · decision 0.71 fired · gotcha 0.93 fired · open 0.04`,
    );
    await expect(gate(firedStep('open'))?.textContent).toBe('gate · open 0.88');
    // A quiet step: decided, nothing fired.
    await expect([gate(QUIET_STEP)?.textContent, gate(QUIET_STEP)?.dataset['fired']]).toEqual(['gate · quiet · ≤ 0.04', undefined]);
  },
};

/** A meditation's audit in two chunks at once (#854, #862): each numbered, each overlapping the other, both answers reconciled. */
export const AuditChunks: Story = {
  name: '5 · an audit in concurrent chunks',
  play: async ({ canvasElement }) => {
    const chunk = (fork: { readonly seq: number }) => canvasElement.querySelector(`[data-branch="${fork.seq}"] .ex-branch__chunk`)?.textContent;
    await waitFor(async () => expect(chunk(CHUNKED_AUDITS[0]![0]!)).toBe('chunk 1 of 2 · overlapped'));
    await expect(CHUNKED_AUDITS.map((chunks) => chunks.map(chunk))).toEqual(CHUNKED_AUDITS.map(() => ['chunk 1 of 2 · overlapped', 'chunk 2 of 2 · overlapped']));
    await expect(WHOLE_AUDITS.map(chunk)).toEqual([undefined]);
    const answers = [...canvasElement.querySelectorAll('.ex-meditation__reconcile [data-part="answer"] pre')].map((pre) => pre.textContent ?? '');
    await expect(answers.filter((answer) => answer.includes('chunk 1 of 2\n') && answer.includes('chunk 2 of 2\n'))).toHaveLength(2);
  },
};

/** A note merged into a live one as similar (#872, #875): drawn on the note it joined, and on its fork as a merge, not a new entry. */
export const NoteMerged: Story = {
  name: '6 · a note merged as similar',
  play: async ({ canvasElement }) => {
    const merge = MERGE!;
    // Working memory: the note that absorbed it says what it absorbed, how similar, and which patch merged it.
    const into = await waitFor(async () => {
      const found = canvasElement.querySelector<HTMLElement>(`[id="memory/${merge.merged}"]`);
      await expect(found).not.toBeNull();
      return found!;
    });
    const merged = [...into.querySelectorAll<HTMLElement>('.ex-memory__merge')];
    await expect(merged.map((m) => m.textContent)).toEqual([`merged #${merge.entry.id} · cosine ${merge.cosine} · ${merge.entry.text}`]);
    await expect(merged[0]?.title).toBe(`merged as similar by patch #${merge.seq}`);
    await expect(canvasElement.querySelector(`[id="memory/${merge.entry.id}"]`)).toBeNull();
    // Its fork: the patch is a merge into that note, not a new entry.
    const fork = canvasElement.querySelector<HTMLElement>(`[data-branch="${merge.fork}"]`)!;
    await userEvent.click(fork.querySelector<HTMLElement>('.ex-branch__why')!);
    const patch = await waitFor(async () => {
      const found = [...fork.querySelectorAll<HTMLElement>('.ex-patch')].find((p) => p.querySelector('.ex-patch__id')?.textContent === `#${merge.entry.id}`);
      await expect(found).toBeTruthy();
      return found!;
    });
    await expect([patch.dataset['merged'], patch.querySelector('.ex-patch__op')?.textContent, patch.querySelector('.ex-patch__note')?.textContent]).toEqual([
      merge.merged,
      '≈',
      `into #${merge.merged} · cosine ${merge.cosine}`,
    ]);
  },
};
