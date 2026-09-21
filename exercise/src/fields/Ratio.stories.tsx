import type { Meta, StoryObj } from '@storybook/react-vite';

import { Ratio } from './Ratio.tsx';

const meta = {
  title: 'Fields/Ratio',
  component: Ratio,
} satisfies Meta<typeof Ratio>;

export default meta;
type Story = StoryObj<typeof meta>;

/** `rejected.grounded / of`: the score a rejection carries so it can be audited. */
export const Grounded: Story = { args: { numerator: 3, denominator: 30, of: 'grounded' } };

/** `summary.targets_matched / targets_checked` on a recompute. */
export const Matched: Story = { args: { numerator: 2, denominator: 2, of: 'matched' } };
