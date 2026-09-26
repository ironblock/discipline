import type { Meta, StoryObj } from '@storybook/react-vite';
import { useEffect, useState } from 'react';
import type { ReactNode } from 'react';
import { flushSync } from 'react-dom';
import { expect, userEvent, within as canvas } from 'storybook/test';

import { Block } from '../ui/Block.tsx';
import type { Tone } from '../ui/Block.tsx';
import { Branch } from '../ui/Branch.tsx';
import { Cable } from '../ui/Cable.tsx';
import { Composer } from '../ui/Composer.tsx';
import type { ComposerProps } from '../ui/Composer.tsx';
import { LookSetting, Looked } from '../ui/look.tsx';
import { Memory } from '../ui/Memory.tsx';
import { Prose, ProseProbe } from '../ui/Prose.tsx';
import { AssistantMessage, SystemMessage, UserMessage } from '../ui/Message.tsx';
import { Seam } from '../ui/Seam.tsx';
import { SessionHeader } from '../ui/SessionHeader.tsx';
import { laneStyle } from '../ui/sets.ts';
import { ToolCall } from '../ui/ToolCall.tsx';
import { PHASES } from '../App.tsx';
import { layersOf } from '../theme/themes/index.ts';
import { contrast } from './contrast.ts';
import { UNCLOSED_FENCE, WHAT_MODELS_WRITE } from './markdown.ts';
import { MOMENTS, branchAt, sessionAt, trunkNodeAt, variantAt } from './moments.ts';

/**
 * One component, one story per state it distinguishes. Every node comes out
 * of the folded specimen at a named moment; only `Block` -- the primitive
 * every other part refines -- `Prose`, which takes text rather than a node
 * (its stories use a hand-written fixture, so the specimen never grows to
 * show off a table), and `Composer`, which takes no node, get plain props.
 */
const meta = {
  title: 'Parts',
  parameters: { layout: 'padded' },
  decorators: [
    (Story) => (
      <div style={{ maxWidth: 'var(--trunk-width)' }}>
        <Story />
      </div>
    ),
  ],
} satisfies Meta;

export default meta;
type Story = StoryObj<typeof meta>;

// ---------------------------------------------------------------- Block

const TRUNK_TONES: readonly Tone[] = ['system', 'user', 'assistant', 'tool'];
/** Every lane the registry knows, and one it does not. */
const LANES = ['interview', 'ratify', 'extraction', 'tangent'] as const;

/** The session event: every fill, with and without a body. The footer is what the harness measured. */
export const BlockTones: Story = {
  name: 'Block · every tone',
  render: () => (
    <div style={{ display: 'grid', gap: '0.9rem' }}>
      {TRUNK_TONES.map((tone) => (
        <Block key={tone} tone={tone} label={tone} stats={[{ value: '2.12 s' }, { value: '71', unit: 'tok' }]} provenance={{ from: [0], needs: [] }}>
          <p style={{ margin: 0 }}>The {tone} fill.</p>
        </Block>
      ))}
      {LANES.map((lane) => (
        <Block key={lane} tone="lane" lane={lane} label={lane} thin stats={[{ value: '2.12 s' }, { value: '71', unit: 'tok' }]} provenance={{ from: [0], needs: [] }} />
      ))}
    </div>
  ),
  // A lane the registry does not know is drawn, neutrally, under its own name.
  play: async ({ canvasElement }) => {
    const bar = (lane: string) => canvasElement.querySelector(`[data-lane='${lane}']`);
    await expect(bar('tangent')?.textContent).toContain('tangent');
    const ink = (lane: string) => getComputedStyle(bar(lane) as Element).color;
    await expect(ink('tangent')).not.toBe(ink('interview'));
    await expect(ink('extraction')).not.toBe(ink('interview'));
    await expect(new Set(LANES.map(ink)).size).toBe(LANES.length);
  },
};

/**
 * The footer is quiet, not illegible: on every trunk fill, a number reads at
 * WCAG AA for small text (4.5:1), and its unit and the role chip at 3:1.
 * Checked in every canonical look (colo and paper, dark and light), and with bloom's footer inline.
 */
export const BlockFooterContrast: Story = {
  name: 'Block · footer contrast',
  render: () => (
    <div style={{ display: 'grid', gap: '0.9rem' }}>
      {TRUNK_TONES.map((tone) => (
        <Block key={tone} tone={tone} label={tone} stats={[{ value: '2.65 s' }, { value: '64', unit: 'tok' }, { value: '1,507', unit: 'pp t/s' }]} provenance={{ from: [0], needs: [] }}>
          <p style={{ margin: 0 }}>The {tone} fill.</p>
        </Block>
      ))}
    </div>
  ),
  play: async ({ canvasElement }) => {
    const floors = [['.ex-stat', 4.5], ['.ex-stat__unit', 3], ['.ex-block__label', 3]] as const;
    const failing = floors.flatMap(([selector, floor]) =>
      [...canvasElement.querySelectorAll(selector)]
        .map((el) => ({ el, ratio: contrast(el) }))
        .filter(({ ratio }) => ratio < floor)
        .map(({ el, ratio }) => `${el.closest('[data-tone]')?.getAttribute('data-tone') ?? '?'} ${selector} ${ratio.toFixed(2)} < ${floor}`),
    );
    await expect([...new Set(failing)]).toEqual([]);
  },
};

export const BlockFooterContrastInline: Story = { ...BlockFooterContrast, name: 'Block · footer contrast, inline', globals: { theme: 'bloom-inline' } };
export const BlockFooterContrastPaper: Story = { ...BlockFooterContrast, name: 'Block · footer contrast, paper', globals: { theme: 'paper' } };
export const BlockFooterContrastLight: Story = { ...BlockFooterContrast, name: 'Block · footer contrast, bloom in daylight', globals: { theme: 'bloom-light' } };
export const BlockFooterContrastPaperDark: Story = { ...BlockFooterContrast, name: 'Block · footer contrast, paper in the dark', globals: { theme: 'paper-dark' } };

/** The look is two settings; each pick redraws the root in the canonical theme it names, and is remembered. */
export const LookSettings: Story = {
  name: 'Look · the two settings',
  render: () => (
    <Looked>
      <LookSetting />
    </Looked>
  ),
  play: async ({ canvasElement }) => {
    const root = canvasElement.querySelector('.ex-look')?.closest('.ex-root');
    const pick = async (name: string) => userEvent.click(canvas(canvasElement).getByRole('radio', { name }));
    await pick('paper');
    await pick('light');
    await expect(root?.getAttribute('data-theme')).toBe(layersOf('paper'));
    await pick('dark');
    await expect(root?.getAttribute('data-theme')).toBe(layersOf('paper-dark'));
    await pick('colo');
    await expect(root?.getAttribute('data-theme')).toBe(layersOf('bloom'));
    await expect(JSON.parse(localStorage.getItem('exercise.look') ?? '{}')).toEqual({ material: 'colo', scheme: 'dark' });
    localStorage.removeItem('exercise.look');
  },
};

// ---------------------------------------------------------------- Cables

/** The ways a cable can end, as token values a theme sets (tokens.css, `--cable-*`). */
const CABLE_OPTIONS = [
  { name: 'line', title: 'A · a line and an arrow, in the faint ink', vars: { '--cable-width': '1px', '--cable-rest': '0%', '--cable-arrow': 'inline', '--cable-ports': 'none', '--cable-glow': '0px' } },
  { name: 'lane', title: 'B · a line and an arrow, in its lane’s colour', vars: { '--cable-width': '1.5px', '--cable-rest': '60%', '--cable-arrow': 'inline', '--cable-ports': 'none', '--cable-glow': '3px' } },
  { name: 'patch', title: 'C · a patch cable, jacked in at both ends', vars: { '--cable-width': '1.5px', '--cable-rest': '45%', '--cable-arrow': 'none', '--cable-ports': 'inline', '--cable-glow': '3px' } },
] as const;

function CableBench({ vars }: { readonly vars: Readonly<Record<string, string>> }) {
  const side = (lane: string, top: number, live: boolean, drop: number) => (
    <div style={{ position: 'absolute', left: 170, width: 190, top, ...laneStyle(lane) }}>
      <Cable reach={40} drop={drop} top={15 - drop} live={live} />
      <Block tone="lane" lane={lane} label={lane} thin live={live} stats={[{ value: live ? '4.1 s' : '2.12 s' }]} provenance={{ from: [0], needs: [] }} />
    </div>
  );
  return (
    <div style={{ position: 'relative', height: 190, ...vars }}>
      <div className="ex-block ex-block--assistant" style={{ position: 'absolute', left: 0, top: 0, width: 130, height: 56 }} />
      <div className="ex-block ex-block--user" style={{ position: 'absolute', left: 0, top: 72, width: 130, height: 56 }} />
      {side('extraction', 0, false, 0)}
      {side('interview', 72, true, 0)}
      {side('ratify', 130, true, 58)}
    </div>
  );
}

/**
 * How a cable ends, three ways, each at rest (extraction), running straight
 * across (interview) and stacked below a busy slot (ratify): the running
 * ones carry travelling light. `colo` draws C, `paper` A.
 */
export const CableOptions: Story = {
  name: 'Cable · three ways to end',
  render: () => (
    <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, 380px)', gap: '2rem' }}>
      {CABLE_OPTIONS.map((o) => (
        <div key={o.name} data-option={o.name}>
          <p style={{ margin: '0 0 1rem', color: 'var(--ink-muted)' }}>{o.title}</p>
          <CableBench vars={o.vars} />
        </div>
      ))}
    </div>
  ),
  play: async ({ canvasElement }) => {
    const shown = (option: string, selector: string) =>
      [...canvasElement.querySelectorAll(`[data-option='${option}'] ${selector}`)].some((el) => getComputedStyle(el).display !== 'none');
    await expect(shown('line', '.ex-cable__arrow')).toBe(true);
    await expect(shown('line', '.ex-cable__port')).toBe(false);
    await expect(shown('patch', '.ex-cable__port')).toBe(true);
    await expect(shown('patch', '.ex-cable__arrow')).toBe(false);
    // Light travels only down a cable whose side call is running.
    const moving = (el: Element) => getComputedStyle(el).display !== 'none' && el.getAnimations().length > 0;
    const pulses = [...canvasElement.querySelectorAll('.ex-cable')].map((c) => [c.hasAttribute('data-live'), moving(c.querySelector('.ex-cable__pulse') as Element)]);
    await expect(pulses.every(([live, travels]) => live === travels)).toBe(true);
    await expect(pulses.filter(([live]) => live)).toHaveLength(6);
  },
};

// ---------------------------------------------------------------- Messages

export const SystemPriming: Story = {
  name: 'Message · system, the phase’s priming',
  render: () => <SystemMessage node={sessionAt(MOMENTS.opened).eras[0]!.system} />,
};

export const SystemRender: Story = {
  name: 'Message · system, working memory rendered',
  render: () => <SystemMessage node={sessionAt(MOMENTS.refilled).eras[1]!.system} />,
  play: async ({ canvas }) => {
    await expect(canvas.getByText(/render v1/)).toBeInTheDocument();
  },
};

export const UserCold: Story = {
  name: 'Message · user, the first ask (a cold prefix)',
  render: () => <UserMessage node={trunkNodeAt(MOMENTS.firstSettled, 'ask/1', 'user')} />,
};

export const UserAfterRefill: Story = {
  name: 'Message · user, after the refill (12 new, 1.5k cached)',
  render: () => <UserMessage node={trunkNodeAt(MOMENTS.done, 'ask/3', 'user')} />,
  play: async ({ canvas }) => {
    await expect(canvas.getByText('12')).toBeInTheDocument();
  },
};

export const AssistantPrefill: Story = {
  name: 'Message · assistant, in prefill',
  render: () => <AssistantMessage node={trunkNodeAt({ beat: 2, t: 5_000 }, 'q/3', 'assistant')} />,
};

export const AssistantStreaming: Story = {
  name: 'Message · assistant, streaming',
  render: () => <AssistantMessage node={trunkNodeAt(MOMENTS.streaming, 'q/3', 'assistant')} />,
  // At this moment the answer ends in a list item with nothing in it yet: the caret still has to show.
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelectorAll('.ex-caret')).toHaveLength(1);
  },
};

export const AssistantDone: Story = {
  name: 'Message · assistant, done, long reasoning clipped',
  render: () => <AssistantMessage node={trunkNodeAt(MOMENTS.firstSettled, 'q/3', 'assistant')} />,
  play: async ({ canvas }) => {
    await expect(canvas.getByText('all reasoning')).toBeInTheDocument();
  },
};

export const AssistantWithCode: Story = {
  name: 'Message · assistant, with a code block',
  render: () => <AssistantMessage node={trunkNodeAt(MOMENTS.done, 'q/8', 'assistant')} />,
};

// ---------------------------------------------------------------- Prose

/** Every construct a model's answer uses, and the ones the renderer refuses: no markup, no script links, no images fetched. */
export const ProseWhatModelsWrite: Story = {
  name: 'Prose · what models write',
  render: () => <Prose text={WHAT_MODELS_WRITE} kind="answer" />,
  play: async ({ canvas, canvasElement }) => {
    await expect(canvas.getByRole('heading', { name: 'What I found' })).toBeVisible();
    const table = canvas.getByRole('table');
    await expect(table.querySelectorAll('tbody tr')).toHaveLength(2);
    await expect([...table.querySelectorAll('th')].map((th) => th.getAttribute('data-align'))).toEqual(['left', 'center', 'right']);
    // A bolded URL stays bold, and the link stops at the URL (GFM's trailing-punctuation rule).
    const url = canvas.getByRole('link', { name: 'http://localhost:5173/?speed=4' });
    await expect(url.getAttribute('href')).toBe('http://localhost:5173/?speed=4');
    await expect(url.closest('strong')).not.toBeNull();
    const boxes = canvas.getAllByRole('checkbox');
    await expect(boxes.map((b) => [(b as HTMLInputElement).checked, (b as HTMLInputElement).disabled])).toEqual([[true, true], [false, true]]);
    await expect(canvasElement.querySelector('li ul li')?.textContent).toBe('one object per file');
    await expect(canvasElement.querySelector('ol')?.children).toHaveLength(2);
    await expect(canvasElement.querySelector('blockquote del')?.textContent).toBe('a new data model');
    const text = canvasElement.textContent ?? '';
    await expect(text).toContain('the harness & its canned transport © 2026');
    await expect(text).toContain('none of this costs $5 or $10');
    await expect(canvasElement.querySelector('code')?.textContent).toBe('Report::print');
    await expect(text).toContain('<b>raw html</b>');
    await expect(canvasElement.querySelector('b')).toBeNull();
    await expect([...canvasElement.querySelectorAll('a')].some((a) => a.getAttribute('href')?.startsWith('javascript'))).toBe(false);
    await expect(text).toContain('a script');
    await expect(canvasElement.querySelector('img')).toBeNull();
    await expect(canvas.getByRole('link', { name: /diagram/ }).getAttribute('href')).toBe('https://example.com/d.png');
    await expect(canvasElement.querySelector('pre')?.textContent).toContain('let args = Args::parse();');
  },
};

export const ProseUnclosedFence: Story = {
  name: 'Prose · cut off inside a code block',
  render: () => <Prose text={UNCLOSED_FENCE} kind="answer" caret />,
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector('pre')?.textContent).toContain('cargo test --workspace');
    await expect(canvasElement.querySelector('pre .ex-caret')).not.toBeNull();
  },
};

/** Driven by the play function: the streamed text so far, and how many blocks rendered for it. */
let stream: ((length: number) => void) | undefined;
let renders = 0;
const countRender = () => {
  renders += 1;
};
const STREAMED = Array.from({ length: 6 }, () => WHAT_MODELS_WRITE).join('\n');

function Streaming() {
  const [length, setLength] = useState(0);
  useEffect(() => {
    stream = setLength;
    return () => {
      stream = undefined;
    };
  }, []);
  return <Prose text={STREAMED.slice(0, length)} kind="answer" caret />;
}

/**
 * Streaming costs the growing block, not the answer: finished blocks keep
 * their identity and are never rendered again. Every update renders the last
 * block (and, as a block closes, the one before it) -- one or two, however
 * long the answer is.
 */
export const ProseStreaming: Story = {
  name: 'Prose · streaming renders only the growing block',
  render: () => (
    <ProseProbe.Provider value={countRender}>
      <Streaming />
    </ProseProbe.Provider>
  ),
  play: async ({ canvasElement }) => {
    await expect(stream).toBeDefined();
    const perStep: number[] = [];
    for (let length = 12; length <= STREAMED.length; length += 12) {
      renders = 0;
      flushSync(() => stream?.(length));
      perStep.push(renders);
    }
    await expect(perStep.length).toBeGreaterThan(200);
    await expect(perStep.filter((n) => n < 1 || n > 2)).toEqual([]);
    await expect(canvasElement.querySelectorAll('table')).toHaveLength(6);
  },
};

// ---------------------------------------------------------------- Tool calls

export const ToolRunning: Story = {
  name: 'ToolCall · running',
  render: () => <ToolCall node={trunkNodeAt(MOMENTS.testsRunning, 't/5', 'tool')} />,
};

export const ToolLarge: Story = {
  name: 'ToolCall · 1,860 lines, collapsed',
  render: () => <ToolCall node={trunkNodeAt(MOMENTS.firstSettled, 't/2', 'tool')} />,
};

export const ToolLargeOpen: Story = {
  name: 'ToolCall · 1,860 lines, opened',
  render: () => <ToolCall node={trunkNodeAt(MOMENTS.firstSettled, 't/2', 'tool')} open />,
};

export const ToolScriptNoOutput: Story = {
  name: 'ToolCall · a multi-line command that printed nothing',
  render: () => <ToolCall node={trunkNodeAt(MOMENTS.done, 't/4', 'tool')} open />,
};

// ---------------------------------------------------------------- Branches

const lane = (children: ReactNode) => <div style={{ maxWidth: 'var(--lane-width)' }}>{children}</div>;

export const BranchRunning: Story = {
  name: 'Branch · interview, running',
  render: () => lane(<Branch node={branchAt(MOMENTS.idleGapInterview, 'i/1')} />),
};

export const BranchLanded: Story = {
  name: 'Branch · interview, three facts landed',
  render: () => lane(<Branch node={branchAt(MOMENTS.firstSettled, 'i/1')} />),
};

export const BranchOpened: Story = {
  name: 'Branch · interview, question and answer opened',
  render: () => lane(<Branch node={branchAt(MOMENTS.firstSettled, 'i/1')} open />),
};

export const BranchSupersede: Story = {
  name: 'Branch · interview, superseding an open question',
  render: () => lane(<Branch node={branchAt(MOMENTS.specSettled, 'i/3')} />),
};

export const BranchRatify: Story = {
  name: 'Branch · ratify, retiring and adding',
  render: () => lane(<Branch node={branchAt(MOMENTS.refilled, 'r/1')} />),
};

// ---------------------------------------------------------------- Working memory

export const MemoryEmpty: Story = {
  name: 'Memory · empty',
  render: () => lane(<Memory entries={sessionAt(MOMENTS.opened).memory} />),
};

export const MemoryFresh: Story = {
  name: 'Memory · six fresh entries',
  render: () => lane(<Memory entries={sessionAt(MOMENTS.firstSettled).memory} />),
};

export const MemoryAfterRefill: Story = {
  name: 'Memory · superseded and retired, kept visible',
  render: () => lane(<Memory entries={sessionAt(MOMENTS.refilled).memory} />),
};

// ---------------------------------------------------------------- Seam

export const SeamRefill: Story = {
  name: 'Seam · the refill',
  render: () => <Seam node={sessionAt(MOMENTS.refilled).eras[1]!.seam!} />,
};

// ---------------------------------------------------------------- What this surface does not know

/**
 * The open sets' fallbacks. Each story folds the specimen with one event
 * saying something this surface has no entry for; each thing is drawn
 * neutrally, under its own name, and nothing else changes.
 */
const unknownBranch = () => {
  const session = variantAt(MOMENTS.firstSettled, (e) => {
    if (e['kind'] === 'fork' && e['id'] === 'i/1') return { ...e, lane: 'tangent' };
    if (e['kind'] === 'fork.settled' && e['id'] === 'i/1') return { ...e, outcome: 'deferred' };
    if (e['kind'] === 'patch' && e['from'] === 'i/1') return { ...e, op: 'amend', authority: 'observed-momentum' };
    return e;
  });
  const branch = [...session.branches.values()].flat().find((b) => b.id === 'i/1');
  if (!branch) throw new Error('no branch i/1');
  return branch;
};

export const UnknownLaneOutcomeOp: Story = {
  name: 'Unknown · a lane, an outcome and a patch op',
  render: () => <Branch node={unknownBranch()} open />,
  play: async ({ canvasElement }) => {
    // At rest the footer counts the op under its own name; opened, the patch is listed.
    const counted = canvasElement.querySelector('.ex-patchsum__op:not([data-known])');
    await expect(counted?.getAttribute('title')).toBe('amend');
    const bar = canvasElement.querySelector('[data-lane="tangent"]');
    await expect(bar?.textContent).toContain('tangent');
    const outcome = canvasElement.querySelector('.ex-branch__outcome');
    await expect(outcome?.textContent).toBe('deferred');
    await expect(outcome?.hasAttribute('data-known')).toBe(false);
    const patch = canvasElement.querySelector('.ex-patch');
    await expect(patch?.textContent).toContain('amend');
    await expect(patch?.hasAttribute('data-known')).toBe(false);
    await expect(patch?.getAttribute('title')).toContain('observed-momentum');
  },
};

export const UnknownTool: Story = {
  name: 'Unknown · a tool',
  render: () => {
    const session = variantAt(MOMENTS.firstSettled, (e) =>
      e['kind'] === 'tool.begin' && e['id'] === 't/1' ? { ...e, tool: 'read', args: { path: 'src/report.rs', lines: [40, 88] } } : e,
    );
    const node = session.eras[0]?.nodes.find((n) => n.id === 't/1');
    if (node?.kind !== 'tool') throw new Error('no tool t/1');
    return <ToolCall node={node} />;
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector('.ex-block__label')?.textContent).toBe('read');
    await expect(canvasElement.textContent).toContain('read({"path":"src/report.rs","lines":[40,88]})');
    await expect(canvasElement.querySelector('.ex-tool__prompt')).toBeNull();
  },
};

export const UnknownSeam: Story = {
  name: 'Unknown · a seam reason, with no phase recorded',
  render: () => {
    const session = variantAt(MOMENTS.refilled, (e) => {
      if (e['kind'] !== 'seam') return e;
      const rest: Record<string, unknown> = { ...e, reason: 'drift' };
      delete rest['phase'];
      return rest;
    });
    const seam = session.eras[1]?.seam;
    if (!seam) throw new Error('no seam');
    return <Seam node={seam} />;
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.textContent).toContain('drift');
    await expect(canvasElement.textContent).toContain('phase not recorded');
  },
};

export const UnknownEvents: Story = {
  name: 'Unknown · event kinds, counted in the header',
  render: () => (
    <SessionHeader
      session={variantAt(MOMENTS.firstSettled, (e) => (e['kind'] === 'turn.settled' ? [e, { kind: 'gate.verdict', t: e['t'], verdict: 'pass' }] : e))}
      surface={{ curtain: true, gaps: false }}
    />
  ),
  play: async ({ canvasElement }) => {
    const chip = canvasElement.querySelector('.ex-header__unknown');
    await expect(chip?.textContent).toBe('1 unknown');
    await expect(chip?.getAttribute('title')).toContain('gate.verdict ×1');
  },
};

export const UnknownRefusal: Story = {
  name: 'Unknown · a refusal',
  render: () => <Composer state="awaiting" phase="spec" phases={PHASES} dispatch={async () => ({ ok: false, refused: 'quota' })} />,
  play: async ({ canvas, userEvent }) => {
    await userEvent.type(canvas.getByRole('textbox'), 'go{Enter}');
    await expect(await canvas.findByText('not taken: quota')).toBeVisible();
  },
};

// ---------------------------------------------------------------- Composer

const composer = (state: ComposerProps['state'], phase = 'spec') => (
  <Composer state={state} phase={phase} phases={PHASES} dispatch={async () => ({ ok: true })} />
);

export const ComposerAwaiting: Story = { name: 'Composer · your turn', render: () => composer('awaiting') };
export const ComposerTurn: Story = { name: 'Composer · the trunk is working', render: () => composer('turn') };
export const ComposerCapture: Story = { name: 'Composer · an interview in the idle gap', render: () => composer('capture') };
export const ComposerRatify: Story = { name: 'Composer · ratifying', render: () => composer('ratify', 'spec') };
export const ComposerEnded: Story = { name: 'Composer · ended', render: () => composer('ended', 'build') };
