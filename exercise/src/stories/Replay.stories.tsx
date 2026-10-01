import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect, waitFor } from 'storybook/test';

import { RECORDINGS } from '../drive/recordings.ts';
import { Replay, ReplayIndex } from '../replay/Replay.tsx';

/** The replay page (#32): a recording with where it came from drawn above it, and the page's index. */
const meta = {
  title: 'Pages/Replay',
  parameters: { layout: 'fullscreen' },
} satisfies Meta;

export default meta;
type Story = StoryObj<typeof meta>;

export const FirstDrive: Story = {
  name: 'a recording, its source and scrub drawn above it',
  render: () => <Replay name="first-drive" recording={RECORDINGS['first-drive']} speed={100_000} />,
  play: async ({ canvasElement }) => {
    const source = canvasElement.querySelector('.ex-replay__source') as HTMLElement;
    await expect(source.querySelector('h1')?.textContent).toBe(RECORDINGS['first-drive'].title);
    const lines = [...source.querySelectorAll('li')].map((li) => li.textContent ?? '');
    await expect(lines.some((line) => line.startsWith('Recorded by the predecessor harness'))).toBe(true);
    await expect(lines.some((line) => line.startsWith('Scrubbed:'))).toBe(true);
    await expect(canvasElement.querySelector('.ex-replay__foot')?.textContent).toContain('Apache-2.0');
    await waitFor(async () => expect(canvasElement.querySelector('.ex-trunk .ex-block')).not.toBeNull());
  },
};

export const Index: Story = {
  name: 'the index: what is published, what is not and why, what is not here yet',
  render: () => <ReplayIndex asked="voxel-stress" />,
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector('.ex-replay__note')?.textContent).toContain('voxel-stress');
    await expect(canvasElement.querySelector('.ex-replay__note')?.textContent).toContain('written by a model after the session');
    await expect([...canvasElement.querySelectorAll('.ex-replay__list a')].map((a) => a.textContent)).toEqual(['first-drive', 'cancelled-capture', 'step-limit']);
    await expect(canvasElement.textContent).toContain('#31');
  },
};
