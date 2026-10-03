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
    // Under the browser project's 15 s test timeout, so a turn that never settles fails here, saying so.
    await waitFor(async () => expect(says(canvasElement)).toBe('your turn'), { timeout: 10_000 });
    await expect(blocks()).toBeGreaterThan(before + 1);
    await expect(canvasElement.querySelector('.ex-trunk [data-tone="assistant"]')).not.toBeNull();
  },
};

/** Ended from the page (#289): the end control asks once, then sends `end`; the session says it has ended. */
export const Ended: Story = {
  name: 'end, from the page: asked once, then the session has ended',
  args: { speed: 40 },
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(says(canvasElement)).toBe('your turn'));
    const end = canvasElement.querySelector('.ex-composer__end') as HTMLButtonElement;
    await userEvent.click(end);
    await expect(end.textContent).toBe('end the session?');
    await expect(says(canvasElement)).toBe('your turn');
    // Answered once the question has been there to read.
    await new Promise((resolve) => setTimeout(resolve, 600));
    await userEvent.click(end);
    await waitFor(async () => expect(says(canvasElement)).toBe('the session has ended'));
    await expect((canvasElement.querySelector('.ex-composer__end') as HTMLButtonElement).disabled).toBe(true);
  },
};

/** Not ended by accident (#289): a double click, or Enter pressed twice at once, only asks. */
export const NotEndedByAccident: Story = {
  name: 'end, not by a double click or a double Enter: it only asks',
  args: { speed: 40 },
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(says(canvasElement)).toBe('your turn'));
    const end = canvasElement.querySelector('.ex-composer__end') as HTMLButtonElement;
    await userEvent.dblClick(end);
    await expect(end.textContent).toBe('end the session?');
    end.blur();
    await waitFor(async () => expect(end.textContent).toBe('end'));
    end.focus();
    await userEvent.keyboard('{Enter}{Enter}');
    await waitFor(async () => expect(end.textContent).toBe('end the session?'));
    await new Promise((resolve) => setTimeout(resolve, 300));
    await expect(says(canvasElement)).toBe('your turn');
  },
};
