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
import { Preferred, Settings } from '../ui/Prefs.tsx';
import { Memory } from '../ui/Memory.tsx';
import { Prose, ProseProbe } from '../ui/Prose.tsx';
import { AssistantMessage, SystemMessage, UserMessage } from '../ui/Message.tsx';
import { Seam } from '../ui/Seam.tsx';
import { SessionHeader } from '../ui/SessionHeader.tsx';
import { laneStyle } from '../ui/sets.ts';
import { ToolBlock } from '../ui/ToolCall.tsx';
import { PHASES } from '../App.tsx';
import { apart, contrast } from './contrast.ts';
import { UNCLOSED_FENCE, WHAT_MODELS_WRITE } from './markdown.ts';
import type { Cursor } from '../drive/canned.ts';
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
 * The header and footer are quiet, not illegible: on every trunk fill, what
 * went in and came out, and a number, read at WCAG AA for small text
 * (4.5:1), and a unit and the role chip at 3:1.
 * Checked in every canonical look (colo and paper, dark and light), and with bloom's footer inline.
 */
export const BlockFooterContrast: Story = {
  name: 'Block · footer contrast',
  render: () => (
    <div style={{ display: 'grid', gap: '0.9rem' }}>
      {TRUNK_TONES.map((tone) => (
        <Block key={tone} tone={tone} label={tone} input="+1,507 tok in 1.2 s (1,256 t/s pp)" output="+64 tok in 2.6 s (24.2 t/s tg)" stats={[{ value: 'exit 0' }, { value: '64', unit: 'tok' }]} provenance={{ from: [0], needs: [] }}>
          <p style={{ margin: 0 }}>The {tone} fill.</p>
        </Block>
      ))}
    </div>
  ),
  play: async ({ canvasElement }) => {
    const floors = [['.ex-block__flow', 4.5], ['.ex-stat', 4.5], ['.ex-stat__unit', 3], ['.ex-block__label', 3]] as const;
    const failing = floors.flatMap(([selector, floor]) =>
      [...canvasElement.querySelectorAll(selector)]
        .map((el) => ({ el, ratio: contrast(el) }))
        .filter(({ ratio }) => ratio < floor)
        .map(({ el, ratio }) => `${el.closest('[data-tone]')?.getAttribute('data-tone') ?? '?'} ${selector} ${ratio.toFixed(2)} < ${floor}`),
    );
    await expect([...new Set(failing)]).toEqual([]);
  },
};

export const BlockFooterContrastLight: Story = { ...BlockFooterContrast, name: 'Block · footer contrast, bloom in daylight', globals: { theme: 'bloom', mode: 'light' } };
export const BlockFooterContrastPaper: Story = { ...BlockFooterContrast, name: 'Block · footer contrast, paper', globals: { theme: 'paper', mode: 'light' } };
export const BlockFooterContrastPaperDark: Story = { ...BlockFooterContrast, name: 'Block · footer contrast, paper in the dark', globals: { theme: 'paper', mode: 'dark' } };
export const BlockFooterContrastEmboss: Story = { ...BlockFooterContrast, name: 'Block · footer contrast, emboss', globals: { theme: 'emboss', mode: 'dark' } };
export const BlockFooterContrastEmbossLight: Story = { ...BlockFooterContrast, name: 'Block · footer contrast, emboss in daylight', globals: { theme: 'emboss', mode: 'light' } };

/**
 * The settings, behind a button: each pick redraws the root as it asks, a
 * trace's details show only while connectors are traces, and every pick is
 * remembered.
 */
export const SettingsPanel: Story = {
  name: 'Settings · the preferences',
  render: () => (
    <Preferred>
      <Settings />
    </Preferred>
  ),
  play: async ({ canvasElement }) => {
    const root = canvasElement.querySelector('.ex-settings')?.closest('.ex-root');
    const group = (label: string) => canvas(canvasElement).getByRole('radiogroup', { name: label });
    await userEvent.click(canvas(canvasElement).getByRole('button', { name: 'settings' }));
    await userEvent.click(canvas(group('theme')).getByRole('radio', { name: 'paper' }));
    await userEvent.click(canvas(group('mode')).getByRole('radio', { name: 'light' }));
    await expect([root?.getAttribute('data-theme'), root?.getAttribute('data-mode')]).toEqual(['paper', 'light']);
    await userEvent.click(canvas(group('motion')).getByRole('radio', { name: 'off' }));
    await expect(root?.getAttribute('data-motion')).toBe('still');
    await expect(canvas(canvasElement).queryByRole('radiogroup', { name: 'corners' })).not.toBeNull();
    await userEvent.click(canvas(group('connectors')).getByRole('radio', { name: 'sweep' }));
    await expect(canvas(canvasElement).queryByRole('radiogroup', { name: 'corners' })).toBeNull();
    await expect(JSON.parse(localStorage.getItem('exercise.prefs') ?? '{}')).toMatchObject({ theme: 'paper', mode: 'light', motion: 'off', connectors: 'sweep' });
    localStorage.removeItem('exercise.prefs');
    await userEvent.keyboard('{Escape}');
    await expect(canvas(canvasElement).queryByRole('group', { name: 'settings' })).toBeNull();
  },
};

// ---------------------------------------------------------------- Cables

/** The ways a cable can end, as token values a theme sets (tokens.css, `--cable-*`). */
const CABLE_OPTIONS = [
  { name: 'line', title: 'A · a hairline and an arrow, no glow', vars: { '--cable-width': '1px', '--cable-rest': '60%', '--cable-arrow': 'inline', '--cable-ports': 'none', '--cable-glow': '0px' } },
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
 * ones carry travelling light. `bloom` draws C, `paper` and `emboss` A.
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

/**
 * A cable says which lane it runs to by its colour, at rest as well as lit:
 * any two lanes' cables at rest are visibly apart, in every theme and mode
 * (OKLab distance; 0.02 is about the least a person can see, and this asks
 * for four times that -- the closest pair is diet asking and deciding,
 * violet and magenta).
 */
export const CableLanesApart: Story = {
  name: 'Cable · lanes tell apart at rest, every look',
  render: () => (
    <div style={{ display: 'flex', gap: '2rem' }}>
      {['interview', 'extraction', 'ratify'].map((lane) => (
        <div key={lane} data-lane={lane} style={{ position: 'relative', width: 120, height: 40, ...laneStyle(lane) }}>
          <div style={{ position: 'absolute', left: 60, top: 0 }}>
            <Cable reach={60} drop={20} top={10} live={false} />
          </div>
        </div>
      ))}
    </div>
  ),
  play: async ({ canvasElement }) => {
    const root = canvasElement.querySelector('.ex-root') as HTMLElement;
    const cable = (lane: string) => canvasElement.querySelector(`[data-lane="${lane}"] .ex-cable`) as Element;
    // A cable fades between colours; measure where it lands, not where it is fading from.
    for (const c of canvasElement.querySelectorAll<SVGElement>('.ex-cable')) c.style.transition = 'none';
    const close: string[] = [];
    for (const theme of ['bloom', 'paper', 'emboss'])
      for (const mode of ['dark', 'light']) {
        root.setAttribute('data-theme', theme);
        root.setAttribute('data-mode', mode);
        for (const [a, b] of [['interview', 'extraction'], ['interview', 'ratify'], ['extraction', 'ratify']] as const) {
          const d = apart(cable(a), cable(b));
          if (d < 0.08) close.push(`${theme} ${mode}: ${a}/${b} ${d.toFixed(3)}`);
        }
      }
    await expect(close).toEqual([]);
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
  name: 'Message · user, after the refill (12 new, 1.5k warm)',
  render: () => <UserMessage node={trunkNodeAt(MOMENTS.done, 'ask/3', 'user')} />,
  play: async ({ canvasElement }) => {
    // What the ask put in front of the model: new tokens, in its header; the warm part says why so few.
    // A line after the chip, as the assistant's reading is: not a chip of its own.
    await expect(canvasElement.querySelector('.ex-block__head .ex-stat')).toBeNull();
    const line = canvasElement.querySelector('.ex-block__head > .ex-block__label + .ex-block__flow') as HTMLElement;
    await expect(line.textContent).toBe('+12 tok');
    await expect((line.querySelector('[title]') as HTMLElement).title).toMatch(/1\.5k before it were warm/);
  },
};

/** Before the server has said how far it has read: one line, the cursor where the first token will land. */
export const AssistantPrefill: Story = {
  name: 'Message · assistant, in prefill',
  render: () => {
    // No progress frame yet: the first arrives a quarter-second in.
    const unread = variantAt({ beat: 2, t: 5_000 }, (e) => (e['kind'] === 'progress' ? [] : e));
    const node = unread.eras.flatMap((era) => era.nodes).find((n) => n.id === 'q/3' && n.kind === 'assistant');
    if (node?.kind !== 'assistant') throw new Error('no assistant q/3 in prefill');
    return <AssistantMessage node={node} />;
  },
  play: async ({ canvasElement }) => {
    // One line, the answer's first: the cursor where its first token will land.
    const waiting = canvasElement.querySelector('.ex-waiting') as HTMLElement;
    await expect(waiting.querySelectorAll('.ex-caret')).toHaveLength(1);
    const line = parseFloat(getComputedStyle(waiting).lineHeight);
    await expect(waiting.getBoundingClientRect().height).toBeLessThanOrEqual(line + 1);
    // The header says it is reading, with nothing known yet: light sweeps the top edge, and follows the corners.
    await expect(canvasElement.querySelector('.ex-block__head .ex-block__flow')?.textContent).toMatch(/^reading · \d/);
    const block = canvasElement.querySelector('.ex-block--live') as HTMLElement;
    const edge = block.querySelector('.ex-block__edge[data-unknown]') as HTMLElement;
    await expect(getComputedStyle(edge).borderRadius).toBe(getComputedStyle(block).borderRadius);
    await expect(edge.getBoundingClientRect().height).toBeCloseTo(block.getBoundingClientRect().height, 0);
    // Nothing is written yet: the bottom edge rests, and there is no footer.
    await expect(getComputedStyle(block, '::after').display).toBe('none');
    await expect(block.querySelector('.ex-block__foot')).toBeNull();
  },
};

/**
 * A 16k-token prompt, half read: the header says how many of how many new,
 * over how much warm, how fast and how long is left, and the top edge fills
 * with the new part -- only the new part, so a long session's warm prefix
 * never dominates it, and a cache miss is a long fill.
 */
export const AssistantPrefillMetered: Story = {
  name: 'Message · assistant, prefill metered',
  render: () => <AssistantMessage node={trunkNodeAt({ beat: 2, t: 10_000 }, 'q/3', 'assistant')} />,
  play: async ({ canvasElement }) => {
    // New tokens read of those to read, for how long so far, and the rate those two make.
    const line = canvasElement.querySelector('.ex-block__head .ex-block__flow')?.textContent ?? '';
    await expect(line).toMatch(/^\+\d+(\.\d)?k of 16\.4k tok in \d+\.\d s \([\d,]+ t\/s pp\)$/);
    const edge = canvasElement.querySelector('.ex-block__edge[data-reading]') as HTMLElement;
    // The edge is the new part alone: nothing of the warm part is drawn.
    await expect(edge.style.getPropertyValue('--warm')).toBe('');
    // As far through the new part as the server has read it -- not through the whole prompt.
    const m = trunkNodeAt({ beat: 2, t: 10_000 }, 'q/3', 'assistant').meter;
    await expect(m).toBeDefined();
    await expect(Number(edge.style.getPropertyValue('--read'))).toBeCloseTo((m?.processed ?? 0) / ((m?.total ?? 0) - (m?.cache ?? 0)), 2);
    // The role's chip leads the header, before what it reads.
    await expect(canvasElement.querySelector('.ex-block__head > .ex-block__label')?.textContent).toBe('assistant');
  },
};

export const AssistantStreaming: Story = {
  name: 'Message · assistant, streaming',
  render: () => <AssistantMessage node={trunkNodeAt(MOMENTS.streaming, 'q/3', 'assistant')} />,
  // At this moment the answer ends in a list item with nothing in it yet: the caret still has to show.
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelectorAll('.ex-caret')).toHaveLength(1);
    // Writing, the footer counts tokens and time as they come, from the first token.
    const foot = canvasElement.querySelector('.ex-block__foot')?.textContent ?? '';
    await expect(foot).toMatch(/^\+\d+ tok in \d+\.\d s \([\d.]+ t\/s tg\)$/);
  },
};

/**
 * What a generation read, apart from what it wrote: the reading is a header
 * along the block's top edge -- right under the ask or tool output it mostly
 * is -- and the writing is the body, from line 1, and the footer. A side
 * call's bar reads along its top edge, and its opened exchange starts with
 * what it read.
 */
export const ReadApartFromWritten: Story = {
  name: 'Message · what it read, apart from what it wrote',
  render: () => {
    const moments = [
      ['reading', trunkNodeAt({ beat: 2, t: 10_000 }, 'q/3', 'assistant')],
      ['writing', trunkNodeAt(MOMENTS.streaming, 'q/3', 'assistant')],
      ['written', trunkNodeAt({ beat: 2 }, 'q/2', 'assistant')],
    ] as const;
    const side = (t: number | undefined, open: boolean) => (
      <div style={laneStyle('interview')}>
        <Branch node={branchAt(t === undefined ? MOMENTS.firstSettled : { beat: 2, t }, 'i/1')} open={open} />
      </div>
    );
    return (
      <div style={{ display: 'grid', gridTemplateColumns: '7rem minmax(0, 1fr)', gap: '1.25rem 2rem', alignItems: 'start' }}>
        {moments.map(([name, node]) => (
          <Row key={name} name={name}>
            <div data-moment={name}>
              <AssistantMessage node={node} />
            </div>
          </Row>
        ))}
        <Row name="side call, reading">
          <div data-moment="side">{side(27_800, false)}</div>
        </Row>
        <Row name="side call, opened">
          <div data-moment="side-open">{side(undefined, true)}</div>
        </Row>
      </div>
    );
  },
  play: async ({ canvasElement }) => {
    const at = (name: string) => canvasElement.querySelector(`[data-moment="${name}"]`) as HTMLElement;
    const text = (name: string, sel: string) => at(name).querySelector(sel)?.textContent ?? '';
    // Reading: the header says how far; the body is one line with the cursor; nothing is written, so no footer.
    await expect(text('reading', '.ex-block__flow')).toMatch(/^\+.+ of 16\.4k tok in .+ pp\)$/);
    await expect(at('reading').querySelectorAll('.ex-block__body .ex-caret')).toHaveLength(1);
    await expect(at('reading').querySelector('.ex-block__foot')).toBeNull();
    // Writing and written: what was read stays at the top; the footer is only what was written.
    for (const name of ['writing', 'written']) {
      await expect(text(name, '.ex-block__head .ex-block__flow')).toMatch(/^\+.+ tok in .+ t\/s pp\)$/);
      await expect(text(name, '.ex-block__foot')).toMatch(/^\+.+ tok in .+ t\/s tg\)/);
    }
    // A side call reads its edge too, and says what it read when opened: a warm fork, a few new tokens.
    await expect(at('side').querySelector('.ex-block__edge')).not.toBeNull();
    // Opened, it is a block as the trunk's: what it read heads it -- new tokens, the warm part on hover -- and what it wrote closes it.
    const head = at('side-open').querySelector('.ex-block__head .ex-block__flow') as HTMLElement;
    await expect(head.textContent).toMatch(/^\+38 tok in .+ pp\)$/);
    await expect((head.querySelector('[title]') as HTMLElement).title).toMatch(/17\.8k more were warm/);
    await expect(text('side-open', '.ex-block__foot .ex-block__flow')).toMatch(/^\+71 tok in .+ tg\)$/);
  },
};

function Row({ name, children }: { readonly name: string; readonly children: ReactNode }) {
  return (
    <>
      <span style={{ color: 'var(--ink-faint)', paddingTop: '0.5rem' }}>{name}</span>
      {children}
    </>
  );
}

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

/**
 * An answer cut off just after a bullet's marker -- a failed or cancelled
 * reply -- ends on no empty bullet. While it streams, the caret still waits
 * in that bullet, where the next word will land.
 */
export const ProseCutAtABullet: Story = {
  name: 'Prose · cut off at a bullet',
  render: () => (
    <div style={{ display: 'grid', gap: '1rem' }}>
      <div data-case="finished">
        <Prose text={'Next:\n\n- read the config\n- '} kind="answer" />
      </div>
      <div data-case="streaming">
        <Prose text={'Next:\n\n- read the config\n- '} kind="answer" caret />
      </div>
    </div>
  ),
  play: async ({ canvasElement }) => {
    const items = (c: string) => canvasElement.querySelectorAll(`[data-case="${c}"] li`);
    await expect(items('finished')).toHaveLength(1);
    await expect(items('streaming')).toHaveLength(2);
    await expect(items('streaming')[1]?.querySelector('.ex-caret')).not.toBeNull();
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
  name: 'Tool · running',
  render: () => <ToolBlock node={trunkNodeAt(MOMENTS.testsRunning, 't/5', 'tool')} />,
  play: async ({ canvasElement }) => {
    const block = canvasElement.querySelector('.ex-block--tool') as HTMLElement;
    await expect(block.classList.contains('ex-block--live')).toBe(true);
    await expect(block.querySelector('.ex-block__foot')?.textContent).toMatch(/^running · /);
    await expect(block.querySelector('.ex-tool__output')).toBeNull();
  },
};

/**
 * A message, and the call it ended in: one block, set as a message is --
 * its header what went in, what writing the call took; its body like a
 * REPL, the command and what it printed, their first lines until opened;
 * its footer what came out, the tool's stats (lines and bytes and how long,
 * its exit), not tokens. The drive here did not say where the call began:
 * what was written is known only as a whole, so the message has no footer
 * and the call's header carries the whole, marked as both.
 */
export const ToolWrittenAsOne: Story = {
  name: 'Tool · a message and its call -- written as one',
  render: () => <Pair at={MOMENTS.firstSettled} message="q/2" tool="t/2" />,
  play: async ({ canvasElement }) => {
    const [message, call] = [...canvasElement.querySelectorAll('.ex-block')] as HTMLElement[];
    await expect(message?.querySelector('.ex-block__foot')).toBeNull();
    await expect(call?.querySelector('.ex-block__head .ex-block__label')?.textContent).toBe('bash');
    await expect(call?.querySelector('.ex-block__head .ex-block__flow')?.textContent).toMatch(/^\+41 tok in 1\.1 s \(35\.\d t\/s tg\) · message and call$/);
    await expect(call?.querySelector('.ex-block__body > .ex-tool__call')?.textContent).toBe('$ cat src/report.rs');
    // What came out is the tool's: lines and bytes and how long, its exit; no tokens.
    const foot = call?.querySelector('.ex-block__foot')?.textContent ?? '';
    await expect(foot).toMatch(/^1,860 lines · .+ in \d+ ms/);
    await expect(foot).toMatch(/exit 0/);
    await expect(foot).not.toMatch(/tok/);
    const output = () => call?.querySelector('.ex-block__body > .ex-tool__call + .ex-tool__output')?.textContent?.split('\n');
    await expect(output()).toHaveLength(3);
    // What it holds back is said under what it shows, not in the header.
    await expect(call?.querySelector('.ex-block__head [aria-expanded]')).toBeNull();
    const more = call?.querySelector('.ex-block__body > .ex-tool__output + .ex-more') as HTMLElement;
    await expect(more.textContent).toBe('1,857 more lines');
    await userEvent.click(more);
    await expect(output()).toHaveLength(1860);
    await expect(more.textContent).toBe('less');
  },
};

/** The same, where the drive said where the call began (`calls_from`): the message counts its text, the call itself. */
export const ToolWrittenApart: Story = {
  name: 'Tool · a message and its call -- written apart',
  render: () => <Pair at={MOMENTS.firstSettled} message="q/2" tool="t/2" callsFrom={{ predicted_n: 14, predicted_ms: 400 }} />,
  play: async ({ canvasElement }) => {
    const [message, call] = [...canvasElement.querySelectorAll('.ex-block')] as HTMLElement[];
    await expect(message?.querySelector('.ex-block__foot')?.textContent).toBe('+14 tok in 400 ms (35.0 t/s tg)');
    await expect(call?.querySelector('.ex-block__head .ex-block__flow')?.textContent).toBe('+27 tok in 750 ms (36.0 t/s tg)');
    // Every header is set alike: its band meets the block's top and sides, a tool's as a message's.
    for (const block of canvasElement.querySelectorAll('.ex-block')) {
      const outer = block.getBoundingClientRect();
      const head = block.querySelector(':scope > .ex-block__head')?.getBoundingClientRect();
      await expect([head?.top, head?.left, head?.right].map((v, i) => Math.round((v ?? NaN) - [outer.top, outer.left, outer.right][i]!))).toEqual([0, 0, 0]);
    }
  },
};

/** A call whose script runs to 27 lines, and printed nothing: its first lines, cut, and what opening shows. */
export const ToolScriptNoOutput: Story = {
  name: 'Tool · a multi-line call that printed nothing',
  render: () => <Pair at={MOMENTS.done} message="q/6" tool="t/4" />,
  play: async ({ canvasElement }) => {
    const call = canvasElement.querySelector('.ex-block--tool') as HTMLElement;
    await expect(call.querySelector('.ex-block__foot')?.textContent).toMatch(/^no output in \d+ ms/);
    await expect(call.querySelector('.ex-tool__output')).toBeNull();
    const script = () => call.querySelector('.ex-tool__call')?.textContent ?? '';
    await expect(script().split('\n')).toHaveLength(4);
    await expect(script()).toMatch(/^\$ cat > \/tmp\/json\.diff <<'EOF'\n[^]*\n…$/);
    await userEvent.click(call.querySelector('.ex-more') as HTMLElement);
    await expect(script().split('\n')).toHaveLength(27);
  },
};

/** A message and the call after it, as the trunk draws them; `callsFrom` says where the call began, as a drive may. */
function Pair({ at, message, tool, callsFrom }: { readonly at: Cursor; readonly message: string; readonly tool: string; readonly callsFrom?: { readonly predicted_n: number; readonly predicted_ms: number } }) {
  const session = callsFrom ? variantAt(at, (e) => (e['kind'] === 'response' && e['to_request'] === message ? { ...e, calls_from: callsFrom } : e)) : sessionAt(at);
  const nodes = session.eras.flatMap((era) => era.nodes);
  const caller = nodes.find((n) => n.id === message);
  const call = nodes.find((n) => n.id === tool);
  if (caller?.kind !== 'assistant' || call?.kind !== 'tool') throw new Error(`no ${message} and ${tool}`);
  return (
    <div style={{ display: 'grid', gap: '0.5rem' }}>
      <AssistantMessage node={caller} calls={[call]} />
      <ToolBlock node={call} caller={caller} first />
    </div>
  );
}

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
  play: async ({ canvasElement }) => {
    const block = canvasElement.querySelector('.ex-block--lane') as HTMLElement;
    await expect(block.classList.contains('ex-block--thin')).toBe(false);
    // The body is the exchange, in order: the question, the answer, what landed.
    const body = [...(block.querySelector('.ex-block__body > .ex-branch__exchange')?.children ?? [])].map((c) => c.className);
    await expect(body).toEqual(['ex-branch__question', 'ex-tagged', 'ex-branch__patches']);
    // The footer counts what landed, beside what it wrote; the why is under the block, and closes it.
    await expect(block.querySelector('.ex-block__foot .ex-patchsum')?.textContent).toBe('+3');
    const why = canvasElement.querySelector('.ex-block--lane + .ex-branch__why') as HTMLElement;
    await userEvent.click(why);
    await expect(block.classList.contains('ex-block--thin')).toBe(true);
    await expect(block.querySelector('.ex-branch__exchange')).toBeNull();
  },
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
    return <ToolBlock node={node} first />;
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector('.ex-block--tool .ex-block__label')?.textContent).toBe('read');
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

/**
 * Narrow -- the trunk's column beside lanes and memory -- the composer's bar
 * is two rows by design, not by wrapping: what the session is doing on the
 * first, the phase controls and the one action (send or cancel) on the second,
 * the action at its right end.
 */
export const ComposerNarrow: Story = {
  name: 'Composer · narrow: two rows by design',
  render: () => (
    <div style={{ width: 560 }}>
      <Composer state="capture" phase="not recorded" phases={PHASES} dispatch={async () => ({ ok: true })} hint="recorded: a first drive, across two refills" />
    </div>
  ),
  play: async ({ canvasElement }) => {
    const box = (sel: string) => (canvasElement.querySelector(sel) as HTMLElement).getBoundingClientRect();
    const state = box('.ex-composer__state');
    const seam = box('.ex-composer__seam');
    const action = box('.ex-composer__cancel');
    const bar = box('.ex-composer__bar');
    await expect(seam.top).toBeGreaterThanOrEqual(state.bottom);
    await expect(Math.abs(action.top - seam.top)).toBeLessThan(4);
    await expect(Math.abs(action.right - bar.right)).toBeLessThan(2);
    await expect(Math.abs(box('.ex-composer__phase').top - seam.top)).toBeLessThan(6);
  },
};
