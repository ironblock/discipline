import type { Meta, StoryObj } from '@storybook/react-vite';

import { ExitStatus } from './ExitStatus.tsx';

const meta = {
  title: 'Fields/ExitStatus',
  component: ExitStatus,
} satisfies Meta<typeof ExitStatus>;

export default meta;
type Story = StoryObj<typeof meta>;

/** The states the `tool-call-exit-statuses` and `archive-rows` fixtures carry. */
export const Zero: Story = { args: { exit: 0 } };
export const One: Story = { args: { exit: 1 } };
export const CommandNotFound: Story = { args: { exit: 127 } };
export const KilledBySignal: Story = { args: { exit: 137 } };
export const NotRun: Story = { args: { exit: -1 } };
