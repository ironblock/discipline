import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect, userEvent, waitFor } from 'storybook/test';

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
} satisfies Meta<typeof App>;

export default meta;
type Story = StoryObj<typeof meta>;

/** What the composer says about the session now. */
const says = (root: HTMLElement) => root.querySelector('.ex-composer__state')?.textContent;

/** To drive by hand. As a test it only looks: the session opened, primed, and it is your turn. */
export const Canned: Story = {
  name: 'drive the canned session',
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(canvasElement.querySelector('.ex-trunk .ex-block')).not.toBeNull());
    await waitFor(async () => expect(says(canvasElement)).toBe('your turn'));
  },
};

/** Driven: an ask, sent from the composer, runs its turn -- the trunk writes, the turn settles -- and hands it back. */
export const Driven: Story = {
  name: 'an ask, driven, settles and hands back the turn',
  // Faster than a person would watch: the turn is the test, not its pace.
  args: { speed: 40 },
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(says(canvasElement)).toBe('your turn'));
    const blocks = () => canvasElement.querySelectorAll('.ex-trunk .ex-block').length;
    const before = blocks();
    await userEvent.type(canvasElement.querySelector('textarea') as HTMLTextAreaElement, 'Where is the output format decided?{Enter}');
    await waitFor(async () => expect(says(canvasElement)).not.toBe('your turn'));
    await waitFor(async () => expect(says(canvasElement)).toBe('your turn'), { timeout: 20_000 });
    await expect(blocks()).toBeGreaterThan(before + 1);
    await expect(canvasElement.querySelector('.ex-trunk [data-tone="assistant"]')).not.toBeNull();
  },
};
