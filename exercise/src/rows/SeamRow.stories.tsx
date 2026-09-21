import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect } from 'storybook/test';

import { pickGroup } from '../fixtures/pick.ts';
import { SeamRow, bindSeam } from './SeamRow.tsx';

const meta = {
  title: 'Rows/seam',
  component: SeamRow,
} satisfies Meta<typeof SeamRow>;

export default meta;
type Story = StoryObj<typeof meta>;

/** The seam as the record has it: id, turn, rendered bytes. Everything else is dotted. */
export const AsRecorded: Story = {
  name: 'as recorded — three fields, four gaps',
  args: bindSeam(pickGroup('full-session', 'seam')),
  play: async ({ canvas }) => {
    await expect(canvas.getByText('seam.reason')).toBeInTheDocument();
    await expect(canvas.getByText('2,048')).toBeInTheDocument();
  },
};

/** The drive's seam at the operator's declared boundary: 211 bytes rendered. */
export const FromTheDrive: Story = {
  name: 'from the canned drive',
  args: bindSeam(pickGroup('canned-drive', 'seam')),
};
