import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect, waitFor } from 'storybook/test';

import { AUTHORED } from '../drive/examples.ts';
import { RECORDINGS } from '../drive/recordings.ts';
import { EXAMPLE_LABEL } from '../replay/published.ts';
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
    // What the page does not show yet, and whose it is, on the recording's page too (#32, ruling 6).
    await expect(canvasElement.querySelector('.ex-replay__gaps')?.textContent).toContain('#31');
    await waitFor(async () => expect(canvasElement.querySelector('.ex-trunk .ex-block')).not.toBeNull());
  },
};

export const Example: Story = {
  name: 'an authored example, under its label for the whole replay',
  render: () => <Replay name="kitchen-sink" recording={AUTHORED['kitchen-sink']} speed={100_000} example />,
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector('.ex-replay__label')?.textContent).toBe(EXAMPLE_LABEL);
    const lines = [...canvasElement.querySelectorAll('.ex-replay__source li')].map((li) => li.textContent ?? '');
    await expect(lines).toContain(`Authored: ${EXAMPLE_LABEL}`);
    await expect(lines.some((line) => line.startsWith('Scrubbed:'))).toBe(false);
    await waitFor(async () => expect(canvasElement.querySelector('.ex-trunk .ex-block')).not.toBeNull());
    // Still there once the session has played: the label is the page's, not the opening moment's.
    await expect(canvasElement.querySelector('.ex-replay__label')?.textContent).toBe(EXAMPLE_LABEL);
  },
};

export const Index: Story = {
  name: 'the index: what is published, the examples apart under their label, what is not and why, what is not here yet',
  render: () => <ReplayIndex asked="voxel-stress" />,
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector('.ex-replay__note')?.textContent).toContain('voxel-stress');
    await expect(canvasElement.querySelector('.ex-replay__note')?.textContent).toContain('written by a model after the session');
    await expect([...canvasElement.querySelectorAll('.ex-replay__recordings a')].map((a) => a.textContent)).toEqual(['first-drive', 'cancelled-capture', 'step-limit']);
    const examples = canvasElement.querySelector('.ex-replay__examples');
    await expect(examples?.querySelector('h2')?.textContent).toBe('Authored examples (not sessions)');
    await expect(examples?.querySelector('.ex-replay__label')?.textContent).toBe(EXAMPLE_LABEL);
    await expect([...(examples?.querySelectorAll('a') ?? [])].map((a) => a.textContent)).toEqual(['kitchen-sink']);
    await expect(canvasElement.textContent).toContain('#31');
  },
};

export const Drive: Story = {
  name: '?drive: this page replays; it names the landing page',
  render: () => <ReplayIndex drive />,
  play: async ({ canvasElement }) => {
    const note = canvasElement.querySelector('.ex-replay__note');
    await expect(note?.textContent).toContain('does not drive');
    await expect(note?.querySelector('a')?.getAttribute('href')).toBe('../');
  },
};

export const NotAReason: Story = {
  name: 'a name that is not a recording gets no reason it does not have',
  render: () => <ReplayIndex asked="constructor" />,
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector('.ex-replay__note')?.textContent).toBe('Not published here: constructor.');
  },
};
