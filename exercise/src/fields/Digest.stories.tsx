import type { Meta, StoryObj } from '@storybook/react-vite';

import { Digest } from './Digest.tsx';

const meta = {
  title: 'Fields/Digest',
  component: Digest,
} satisfies Meta<typeof Digest>;

export default meta;
type Story = StoryObj<typeof meta>;

/** `summary.product_sha256` of the canned drive's product. */
export const Product: Story = { args: { sha256: '84f1ad3b1ff3a79785d7c4d2fbaec1c9ab3f11644bff0157ef43f01c5d9fa39d' } };

/** A fixture's placeholder digest. */
export const Fixture: Story = { args: { sha256: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' } };
