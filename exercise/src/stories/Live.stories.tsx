import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect, userEvent, waitFor } from 'storybook/test';

import { App } from '../App.tsx';
import type { EventSourceLike, Web } from '../drive/http.ts';
import rehearsal from '../drive/served/rehearsal-turns-1-4.log?raw';
import { STOPPED_IN_PREFILL, STOPPED_IN_PREFILL_AFTER } from '../drive/served/stopped-in-prefill.ts';

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

/**
 * A stand-in for `diet-drive serve` (#288): LOG, line by line, as the server-sent events its `/events` sends --
 * each line a `data:` with its `<opened>-<seq>` id -- once the stream opens; every `fetch` answered 200.
 */
function serving(log: string): Web {
  const lines = log.trimEnd().split('\n');
  const opened = (JSON.parse(lines[0] ?? '{}') as { opened?: number }).opened;
  class Served implements EventSourceLike {
    readyState = 0;
    onopen: ((event: Event) => void) | null = null;
    onmessage: ((event: MessageEvent<string>) => void) | null = null;
    onerror: ((event: Event) => void) | null = null;
    constructor(readonly url: string) {
      setTimeout(() => {
        if (this.readyState === 2) return;
        this.readyState = 1;
        this.onopen?.(new Event('open'));
        for (const data of lines) this.onmessage?.(new MessageEvent('message', { data, lastEventId: `${opened}-${(JSON.parse(data) as { seq: number }).seq}` }));
      }, 0);
    }
    close(): void {
      this.readyState = 2;
    }
  }
  return { EventSource: Served, fetch: async () => new Response('{}', { status: 200 }) };
}

/** The rehearsal's log up to the line with SEQ (#177): a session as `serve` wrote it, from its start. */
const rehearsalTo = (seq?: number) => {
  const lines = rehearsal.trimEnd().split('\n');
  const end = seq === undefined ? lines.length : lines.findIndex((line) => (JSON.parse(line) as { seq: number }).seq === seq) + 1;
  return lines.slice(0, end).join('\n') + '\n';
};

/**
 * `?drive`, served a real session (#288): four turns from the rehearsal drive, the fourth stopped mid-answer with
 * its prefill's progress lines and no response. The page went blank on such a line until it read the log's own
 * frame shape; here it draws all four.
 */
export const Served: Story = {
  name: '?drive: a session serve wrote, a stopped turn among it',
  args: { drive: true, web: serving(rehearsalTo()) },
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(canvasElement.querySelectorAll('.ex-trunk [data-tone="assistant"]').length).toBe(4));
    await expect(canvasElement.querySelectorAll('.ex-trunk [data-tone="user"]').length).toBe(4);
    await waitFor(async () => expect(says(canvasElement)).toBe('your turn'));
    // Turn 4 was stopped while writing, its prompt read whole: its footer says how far it wrote, beside the `cancelled` badge (#294).
    const feet = [...canvasElement.querySelectorAll('.ex-trunk [data-tone="assistant"] .ex-block__foot')].map((f) => f.textContent ?? '');
    await expect(feet.some((line) => line.includes('wrote for 3.5 s'))).toBe(true);
  },
};

/** `?drive` mid-prefill on a warm turn: the header counts the new part read, past the cache, of the new part. */
export const ServedReading: Story = {
  name: '?drive: a warm prefill in flight, metered as serve framed it',
  // Turn 4's third progress line: total 2334, cache 1767, processed 1818 -- 51 of 567 new tokens read.
  args: { drive: true, web: serving(rehearsalTo(1185)) },
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(canvasElement.querySelectorAll('.ex-trunk [data-tone="assistant"]').length).toBe(4));
    const reading = [...canvasElement.querySelectorAll('.ex-trunk .ex-block__head .ex-block__flow')].map((f) => f.textContent ?? '');
    await expect(reading.some((line) => line.startsWith('+51 of 567 tok in '))).toBe(true);
  },
};

/**
 * `?drive`, a turn stopped mid-prefill (#294). No turn in the rehearsal stopped in prefill, so the log past turn 4's
 * third progress line is CONSTRUCTED (`served/stopped-in-prefill.ts`, ruled on #294): the block says how far the
 * read got and that it was cut short, and that nothing was written.
 */
export const ServedStoppedReading: Story = {
  name: '?drive: a turn stopped mid-prefill, drawn as read that far (a constructed stop)',
  args: {
    drive: true,
    web: serving(rehearsalTo(STOPPED_IN_PREFILL_AFTER) + STOPPED_IN_PREFILL.map((line) => JSON.stringify(line)).join('\n') + '\n'),
  },
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(says(canvasElement)).toBe('your turn'));
    const last = [...canvasElement.querySelectorAll('.ex-trunk [data-tone="assistant"]')].at(-1) as HTMLElement;
    await expect(last.querySelector('.ex-block__head .ex-block__flow')?.textContent).toMatch(/^\+51 of 567 tok in 148 ms \(.+ t\/s pp\) · cut short$/);
    await expect(last.querySelector('.ex-block__foot')?.textContent).toContain('wrote nothing');
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
