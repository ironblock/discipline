import type { Meta, StoryObj } from '@storybook/react-vite';
import type { ReactNode } from 'react';
import { expect } from 'storybook/test';

import { Block } from '../ui/Block.tsx';
import type { Tone } from '../ui/Block.tsx';
import { Branch } from '../ui/Branch.tsx';
import { Composer } from '../ui/Composer.tsx';
import type { ComposerProps } from '../ui/Composer.tsx';
import { Memory } from '../ui/Memory.tsx';
import { AssistantMessage, SystemMessage, UserMessage } from '../ui/Message.tsx';
import { Seam } from '../ui/Seam.tsx';
import { ToolCall } from '../ui/ToolCall.tsx';
import { PHASES } from '../App.tsx';
import { contrast } from './contrast.ts';
import { MOMENTS, branchAt, sessionAt, trunkNodeAt } from './moments.ts';

/**
 * One component, one story per state it distinguishes. Every node comes out
 * of the folded specimen at a named moment; only `Block` -- the primitive
 * every other part refines -- and `Composer`, which takes no node, get plain
 * props.
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

const TONES: readonly Tone[] = ['system', 'user', 'assistant', 'tool', 'interview', 'ratify'];
const TRUNK_TONES: readonly Tone[] = ['system', 'user', 'assistant', 'tool'];

/** The session event: every fill, with and without a body. The footer is what the harness measured. */
export const BlockTones: Story = {
  name: 'Block · every tone',
  render: () => (
    <div style={{ display: 'grid', gap: '0.9rem' }}>
      {TONES.map((tone) => (
        <Block
          key={tone}
          tone={tone}
          label={tone}
          thin={tone === 'interview' || tone === 'ratify'}
          stats={[{ value: '2.12 s' }, { value: '71', unit: 'tok' }]}
          provenance={{ from: [0], needs: [] }}
        >
          {tone === 'interview' || tone === 'ratify' ? undefined : <p style={{ margin: 0 }}>The {tone} fill.</p>}
        </Block>
      ))}
    </div>
  ),
};

/**
 * The footer is quiet, not illegible: on every trunk fill, a number reads at
 * WCAG AA for small text (4.5:1), and its unit and the role chip at 3:1.
 * Checked in the theme the session opens in, and in its banded variant.
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

export const BlockFooterContrastBanded: Story = { ...BlockFooterContrast, name: 'Block · footer contrast, banded', globals: { theme: 'band' } };

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

// ---------------------------------------------------------------- Composer

const composer = (state: ComposerProps['state'], phase = 'spec') => (
  <Composer state={state} phase={phase} phases={PHASES} onSend={async () => ({ ok: true })} onCancel={async () => ({ ok: true })} onSeam={async () => ({ ok: true })} />
);

export const ComposerAwaiting: Story = { name: 'Composer · your turn', render: () => composer('awaiting') };
export const ComposerTurn: Story = { name: 'Composer · the trunk is working', render: () => composer('turn') };
export const ComposerCapture: Story = { name: 'Composer · an interview in the idle gap', render: () => composer('capture') };
export const ComposerRatify: Story = { name: 'Composer · ratifying', render: () => composer('ratify', 'spec') };
export const ComposerEnded: Story = { name: 'Composer · ended', render: () => composer('ended', 'build') };
