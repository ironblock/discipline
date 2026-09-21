import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect } from 'storybook/test';

import { pickGroup } from '../fixtures/pick.ts';
import { ForkRow, bindFork } from './ForkRow.tsx';

const meta = {
  title: 'Rows/fork + capture',
  component: ForkRow,
} satisfies Meta<typeof ForkRow>;

export default meta;
type Story = StoryObj<typeof meta>;

/** A fork with its capture: three entries touched. */
export const WithCapture: Story = {
  name: 'with a capture',
  args: bindFork(pickGroup('full-session', 'fork')),
  play: async ({ canvas }) => {
    await expect(canvas.getByText('from_fork')).toBeInTheDocument();
    await expect(canvas.getByText('3')).toBeInTheDocument();
  },
};

/** A fork with no capture: it happened, and nothing was written (the lane was rejected instead). */
export const WithoutCapture: Story = {
  name: 'without a capture',
  args: bindFork(pickGroup('rejected-lane', 'fork')),
  play: async ({ canvas }) => {
    await expect(canvas.getByText('no capture')).toBeInTheDocument();
  },
};

/** The drive's own: `fork-2` on the interview lane, served by the canned substrate. */
export const FromTheDrive: Story = {
  name: 'from the canned drive',
  args: bindFork(pickGroup('canned-drive', 'fork', 1)),
};

/** A fork on a different substrate from the main lane: the arrangement this repository measures. */
export const OnTheSmallSubstrate: Story = {
  name: 'on the small substrate',
  args: bindFork(pickGroup('two-substrates', 'fork')),
  play: async ({ canvas }) => {
    await expect(canvas.getByText('small')).toBeInTheDocument();
  },
};
