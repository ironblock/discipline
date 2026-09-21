import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect } from 'storybook/test';

import { Pending } from './Pending.tsx';

const meta = {
  title: 'Fields/Pending',
  component: Pending,
} satisfies Meta<typeof Pending>;

export default meta;
type Story = StoryObj<typeof meta>;

/** In a row header, where the field would sit. */
export const Inline: Story = {
  args: { issue: '#92.1', atom: 'seam.reason', why: 'ruled on #27 and never landed' },
  play: async ({ canvas }) => {
    await expect(canvas.getByText('#92.1')).toBeInTheDocument();
  },
};

/** Standing where a whole row would be. */
export const Block: Story = {
  args: {
    as: 'block',
    issue: '#92.2',
    atom: 'phase_proposal',
    why: 'seam/phase.rs keeps refused proposals with a typed Refusal; the record has no kind for one.',
  },
};

/** Waiting on nothing filed yet: the slug is the join to the drafted issue. */
export const Unfiled: Story = {
  args: { issue: 'unfiled/entries-created', atom: 'capture.entries_created' },
};
