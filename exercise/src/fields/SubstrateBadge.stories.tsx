import type { Meta, StoryObj } from '@storybook/react-vite';

import { SubstrateBadge } from './SubstrateBadge.tsx';

const meta = {
  title: 'Fields/SubstrateBadge',
  component: SubstrateBadge,
} satisfies Meta<typeof SubstrateBadge>;

export default meta;
type Story = StoryObj<typeof meta>;

/** Weights on disk, by sha256: re-fired within a band. */
export const Digest: Story = { args: { id: 'local', weights: 'digest' } };

/** Weights behind an endpoint: observed, never certified. */
export const Hosted: Story = { args: { id: 'served', weights: 'hosted' } };

/** No weights: acts replayed exactly. */
export const Canned: Story = { args: { id: 'canned', weights: 'canned' } };

/** A row that knows only the id. */
export const IdOnly: Story = { args: { id: 'small' } };
