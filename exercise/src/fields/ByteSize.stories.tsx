import type { Meta, StoryObj } from '@storybook/react-vite';

import { ByteSize } from './ByteSize.tsx';

const meta = {
  title: 'Fields/ByteSize',
  component: ByteSize,
} satisfies Meta<typeof ByteSize>;

export default meta;
type Story = StoryObj<typeof meta>;

/** `seam.rendered_bytes` for a small working set. */
export const Exact: Story = { args: { bytes: 2048 } };

/** The 300 KB read: a size, never a scroll. */
export const TheFoundingGrievance: Story = { args: { bytes: 319_488 } };

/** Above a megabyte. */
export const Megabytes: Story = { args: { bytes: 4_500_000 } };
