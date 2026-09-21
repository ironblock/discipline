import type { Meta, StoryObj } from '@storybook/react-vite';

import { ReasoningBadge, VerdictBadge } from './Badge.tsx';

const meta = {
  title: 'Fields/Badges',
  component: ReasoningBadge,
} satisfies Meta<typeof ReasoningBadge>;

export default meta;
type Story = StoryObj<typeof meta>;

/** `substrate.reasoning`: four states, and `suppressed` is its own because it is a known footgun. */
export const Reasoning: Story = {
  args: { reasoning: 'on' },
  render: () => (
    <div style={{ display: 'flex', gap: 8 }}>
      <ReasoningBadge reasoning="on" />
      <ReasoningBadge reasoning="off" />
      <ReasoningBadge reasoning="suppressed" />
      <ReasoningBadge reasoning="undeclared" />
    </div>
  ),
};

/** `claim.result`: three verdicts and one non-verdict. */
export const Verdicts: Story = {
  args: { reasoning: 'on' },
  render: () => (
    <div style={{ display: 'flex', gap: 8 }}>
      <VerdictBadge verdict="supported" />
      <VerdictBadge verdict="refuted" />
      <VerdictBadge verdict="inconclusive" />
      <VerdictBadge verdict="unadjudicated" />
    </div>
  ),
};
