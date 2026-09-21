import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect } from 'storybook/test';

import { TokenCount } from './TokenCount.tsx';

const meta = {
  title: 'Fields/TokenCount',
  component: TokenCount,
} satisfies Meta<typeof TokenCount>;

export default meta;
type Story = StoryObj<typeof meta>;

/** `turn.prefill_tokens`: the number the program exists to keep small. */
export const Prefill: Story = {
  args: { tokens: 1024, of: 'prefill' },
  play: async ({ canvas }) => {
    await expect(canvas.getByText('1,024')).toBeInTheDocument();
  },
};

/** `response.output_tokens` of zero is a measurement, not an absence. */
export const ZeroOutput: Story = { args: { tokens: 0, of: 'output' } };

/** A session total, thousands-separated. */
export const Total: Story = { args: { tokens: 1_234_567, of: 'total' } };
