import type { Meta, StoryObj } from '@storybook/react-vite';

import { IdChip } from './IdChip.tsx';

const meta = {
  title: 'Fields/IdChip',
  component: IdChip,
} satisfies Meta<typeof IdChip>;

export default meta;
type Story = StoryObj<typeof meta>;

/** A fixture's id. */
export const Fixture: Story = { args: { id: 'q1', kind: 'request' } };

/** The drive's own spelling: `lane/n` and `lane/n#response`. */
export const DriveRequest: Story = { args: { id: 'q/1', kind: 'request' } };
export const DriveResponse: Story = { args: { id: 'q/1#response', kind: 'response' } };

/** `fork-{turn}`: the drive's fork id is a property of its turn. */
export const DriveFork: Story = { args: { id: 'fork-2', kind: 'fork' } };
