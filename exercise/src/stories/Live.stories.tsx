import type { Meta, StoryObj } from '@storybook/react-vite';

import { App } from '../App.tsx';

/**
 * The canned transport, driven for real: type an ask, watch it stream, let
 * the interviews run in the idle gap, move to build, refill. The model's
 * side is scripted (the specimen); the timing is real unless sped up.
 */
const meta = {
  title: 'Session/Live',
  component: App,
  parameters: { layout: 'fullscreen' },
  args: { speed: 1 },
  argTypes: { speed: { control: { type: 'select' }, options: [1, 2, 4, 10] } },
  tags: ['!test'],
} satisfies Meta<typeof App>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Canned: Story = {
  name: 'drive the canned session',
};
