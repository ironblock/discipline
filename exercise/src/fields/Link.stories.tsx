import type { Meta, StoryObj } from '@storybook/react-vite';

import { Link } from './Link.tsx';

const meta = {
  title: 'Fields/Link',
  component: Link,
} satisfies Meta<typeof Link>;

export default meta;
type Story = StoryObj<typeof meta>;

/** The four links the structure checker validates, and no fifth. */
export const ToRequest: Story = { args: { field: 'to_request', to: 'q1', wants: 'request' } };
export const RetryOf: Story = { args: { field: 'retry_of', to: 'q1', wants: 'request' } };
export const FromFork: Story = { args: { field: 'from_fork', to: 'f1', wants: 'fork' } };
export const Supersedes: Story = { args: { field: 'supersedes', to: 'c1', wants: 'claim' } };
