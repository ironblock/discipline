import type { Meta, StoryObj } from '@storybook/react-vite';

import { LaneBadge } from './LaneBadge.tsx';

const meta = {
  title: 'Fields/LaneBadge',
  component: LaneBadge,
} satisfies Meta<typeof LaneBadge>;

export default meta;
type Story = StoryObj<typeof meta>;

/** The four lanes the drive writes. */
export const Main: Story = { args: { lane: 'main' } };
export const Interview: Story = { args: { lane: 'interview' } };
export const Ratify: Story = { args: { lane: 'ratify' } };
export const Control: Story = { args: { lane: 'control' } };

/** A lane the drive does not name -- `reformat` in the rejected-lane fixture -- is muted, not coloured by guess. */
export const Undeclared: Story = { args: { lane: 'reformat' } };
