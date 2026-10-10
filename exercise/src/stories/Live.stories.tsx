import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect, userEvent, waitFor } from 'storybook/test';

import capped from '../../../diet/drive/fixtures/a-capped-turn.jsonl?raw';
import toolCallFailed from '../../../diet/formats/log/fixtures/valid/a-v3-tool-call-failed-under-policy.jsonl?raw';
import toolCallRefused from '../../../diet/formats/log/fixtures/valid/a-v3-tool-call-refused.jsonl?raw';
import approvalsOff from '../../../diet/formats/log/fixtures/valid/a-v7-call-that-ran-with-approvals-off.jsonl?raw';
import phaseMoved from '../../../diet/formats/log/fixtures/valid/a-v7-seam-that-moved-a-phase.jsonl?raw';
import deliveredNote from '../../../diet/formats/log/fixtures/valid/a-v7-imperative-delivery-after-an-ask.jsonl?raw';
import recalledNote from '../../../diet/formats/log/fixtures/valid/a-v7-recall-after-an-ask.jsonl?raw';
import refillMessage from '../../../diet/formats/log/fixtures/valid/a-v7-seam-whose-refill-is-a-message.jsonl?raw';
import leversDeclared from '../../../diet/formats/log/fixtures/valid/a-v7-session-declaring-its-levers.jsonl?raw';
import selfCapturePatch from '../../../diet/formats/log/fixtures/valid/a-v7-self-capture-patch-named-by-its-lane.jsonl?raw';
import answeredTurn from '../../../diet/formats/log/fixtures/valid/an-answered-turn.jsonl?raw';
import toolCallRan from '../../../diet/formats/log/fixtures/valid/a-v3-tool-call-that-ran.jsonl?raw';
import forks from '../../../diet/formats/log/fixtures/valid/a-v5-scoping-fork-that-patched-and-a-read-fork-that-declined.jsonl?raw';
import { App } from '../App.tsx';
import { png } from '../drive/png.ts';
import type { EventSourceLike, Web } from '../drive/http.ts';
import rehearsal from '../drive/served/rehearsal-turns-1-4.log?raw';
import { TANGENT_OPEN } from '../drive/served/tangent.ts';
import { SELF_CAPTURE } from '../drive/served/self-capture.ts';
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
    addEventListener(): void {}
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

/** What the stand-in below was posted, in order: each command's JSON body. */
const posted: unknown[] = [];

/** `serving`, keeping what the page posts to `/commands`. */
function posting(log: string): Web {
  return {
    ...serving(log),
    fetch: async (url, init) => {
      if (String(url).endsWith('/commands') && typeof init?.body === 'string') posted.push(JSON.parse(init.body));
      return new Response('{}', { status: 200 });
    },
  };
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

/**
 * `?drive`, a turn that hit its output cap (#290): `capped` on the response, the turn settled `failed` -- track three's
 * fixture. The answer says it hit max tokens, the turn's end says so in place of a failed request, and neither the
 * ask nor what it wrote is in the model's context (ruled 5969941559).
 */
export const ServedCapped: Story = {
  name: '?drive: a turn that hit max tokens, so no answer',
  args: { drive: true, web: serving(capped) },
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(canvasElement.querySelector('.ex-turnend')).not.toBeNull());
    const answer = canvasElement.querySelector('.ex-trunk [data-tone="assistant"]') as HTMLElement;
    await expect(answer.querySelector('.ex-stop[data-level="warn"]')?.textContent).toBe('hit max tokens');
    await expect(answer.querySelector('.ex-context-out')).not.toBeNull();
    await expect(canvasElement.querySelector('.ex-trunk [data-tone="user"] .ex-context-out')).not.toBeNull();
    const end = canvasElement.querySelector('.ex-turnend') as HTMLElement;
    await expect(end.dataset['level']).toBe('warn');
    await expect(end.textContent).toContain('hit max tokens, so no answer');
    await expect(end.textContent).not.toContain('a request failed');
  },
};

/** The same turn with the finish spelled `max_tokens`, diet's other capped spelling: the badge reads `capped`, not the word. */
export const ServedCappedMaxTokens: Story = {
  name: '?drive: a turn capped under another finish spelling, still hit max tokens',
  args: { drive: true, web: serving(capped.replace('"finish_reason":"length"', '"finish_reason":"max_tokens"')) },
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(canvasElement.querySelector('.ex-turnend')).not.toBeNull());
    const answer = canvasElement.querySelector('.ex-trunk [data-tone="assistant"]') as HTMLElement;
    await expect(answer.querySelector('.ex-stop[data-level="warn"]')?.textContent).toBe('hit max tokens');
  },
};

/**
 * The one tool call `serve` drew from a courier's v3 fixture (#297), once its outcome line has arrived: the fixture
 * ends there, the turn still open, as a drive's log does between a call and the step after it.
 */
const servedCall = async (root: HTMLElement) => {
  await waitFor(async () => {
    const calls = root.querySelectorAll<HTMLElement>('.ex-trunk [data-tone="tool"]');
    await expect(calls.length).toBe(1);
    await expect(calls[0]!.textContent).not.toContain('running ·');
  });
  return root.querySelector<HTMLElement>('.ex-trunk [data-tone="tool"]')!;
};

/** `?drive`, log v3 (#300): a call that ran -- I0's streamed turn, its fragments assembled -- and what confined it. */
export const ServedToolCallRan: Story = {
  name: '?drive: a tool call that ran, and what confined it (v3)',
  args: { drive: true, web: serving(toolCallRan) },
  play: async ({ canvasElement }) => {
    const call = await servedCall(canvasElement);
    await expect(call.querySelector('.ex-call-outcome')).toBeNull();
    await expect(call.querySelector('.ex-exit')?.textContent).toBe('exit 0');
    await expect(call.querySelector('.ex-confinement')?.textContent).toBe('isolation sandbox · network none');
    await expect(call.querySelector('.ex-tool__call')?.textContent).toContain('ls | wc -l');
  },
};

/** `?drive`, log v3 (#300): a call the drive refused, drawn as refused with its reason, never as ran. */
export const ServedToolCallRefused: Story = {
  name: '?drive: a tool call the drive refused (v3)',
  args: { drive: true, web: serving(toolCallRefused) },
  play: async ({ canvasElement }) => {
    const call = await servedCall(canvasElement);
    await expect(call.querySelector('.ex-call-outcome[data-outcome="refused"]')?.textContent).toBe('refused · not on the allowlist');
    await expect(call.querySelector('.ex-exit')).toBeNull();
    await expect(call.querySelector('.ex-confinement')).toBeNull();
  },
};

/** `?drive`, log v3 (#300): a call that failed under its policy, drawn as that, never as a plain failure. */
export const ServedToolCallFailedUnderPolicy: Story = {
  name: '?drive: a tool call that failed under its policy (v3)',
  args: { drive: true, web: serving(toolCallFailed) },
  play: async ({ canvasElement }) => {
    const call = await servedCall(canvasElement);
    await expect(call.querySelector('.ex-call-outcome[data-outcome="command_failed"]')?.textContent).toBe('failed under policy');
    await expect(call.querySelector('.ex-tool__stderr')?.textContent).toContain('Operation not permitted');
  },
};

/** `?drive`, log v5: `diet`'s forks, whose lines name no slot, drawn in a lane behind the curtain -- one branch per fork. */
export const ServedForks: Story = {
  name: '?drive: the interview forks, drawn in their lane (v5)',
  args: { drive: true, web: serving(forks) },
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(canvasElement.querySelectorAll('.ex-lane .ex-branch')).toHaveLength(2));
    // Settled, each is a thin bar: its lane and how it ended -- the scoping fork patched, the read fork declined.
    const drawn = [...canvasElement.querySelectorAll<HTMLElement>('.ex-lane .ex-branch')].map((b) => [b.dataset['lane'], b.dataset['outcome']]);
    await expect(drawn).toEqual([
      ['interview', 'value'],
      ['interview', 'decline'],
    ]);
  },
};

/**
 * The operator's screenshot (#372), attached in the composer: picked, shown as a chip, sent ahead of the ask by
 * the transport's upload, named on the ask by digest -- and drawn on the operator's message, read back by that digest.
 */
export const AttachAScreenshot: Story = {
  name: 'attach a screenshot to the ask',
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(says(canvasElement)).toBe('your turn'));
    const shot = new File([png(8, 6, (x) => [x * 30, 90, 160]) as Uint8Array<ArrayBuffer>], 'shot.png', { type: 'image/png' });
    await userEvent.upload(canvasElement.querySelector('input[aria-label="attach a PNG"]') as HTMLInputElement, shot);
    await waitFor(async () => expect(canvasElement.querySelectorAll('.ex-composer__file img')).toHaveLength(1));
    await userEvent.type(canvasElement.querySelector('textarea') as HTMLTextAreaElement, 'Where is the output format decided?{Enter}');
    await waitFor(async () => expect(canvasElement.querySelector('.ex-trunk [aria-label="what the operator attached"] img')).not.toBeNull());
    // Taken, the chip is gone: the next ask attaches nothing unless the operator says so.
    await expect(canvasElement.querySelector('.ex-composer__file')).toBeNull();
  },
};

/**
 * `?drive`: a log that declares no phase graph -- so the composer offers no move, says so, and its refill sends a
 * declare-seam naming no phase.
 */
export const ServedRefillNamesNoPhase: Story = {
  name: '?drive: refill offers no phase, since the drive declares none',
  args: { drive: true, web: posting(answeredTurn) },
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(says(canvasElement)).toBe('your turn'));
    await expect(canvasElement.querySelector('select[aria-label="move to"]')).toBeNull();
    await expect(canvasElement.querySelector('.ex-composer__phase')?.textContent).toBe('phase not declared');
    posted.length = 0;
    await userEvent.click([...canvasElement.querySelectorAll('button')].find((b) => b.textContent === 'refill')!);
    // The bare declare-seam (with the idle gap it ends): no phase named.
    await waitFor(async () => expect(posted.map((p) => (p as { kind: string }).kind)).toEqual(['declare-seam']));
    await expect(posted.some((p) => 'to' in (p as object))).toBe(false);
  },
};

/** The canned demo declares its phases: the composer offers the move. */
export const CannedRefillOffersPhases: Story = {
  name: 'canned: refill offers the next phase',
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(says(canvasElement)).toBe('your turn'));
    await expect(canvasElement.querySelector('select[aria-label="move to"]')).not.toBeNull();
  },
};

/**
 * `?drive`, log v7 (#544): a session run with approvals off. The header says so for the whole session, and the
 * call says so on itself -- every command ran with no gate decision and no prompt, the sandbox still confining it.
 */
export const ServedApprovalsOff: Story = {
  name: '?drive: a session with approvals off says so, and so does each call (v7)',
  args: { drive: true, web: serving(approvalsOff) },
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(canvasElement.querySelectorAll('.ex-trunk [data-tone="tool"]').length).toBeGreaterThan(0));
    await expect(canvasElement.querySelector('.ex-header__lever[data-lever="approvals"]')?.textContent).toBe('approvals off');
    const call = canvasElement.querySelector('.ex-trunk [data-tone="tool"]') as HTMLElement;
    await expect(call.textContent).toContain('approvals off');
  },
};

/** A log from before the lever was declared (v3) says nothing about it: the header says so, rather than assume the gate. */
export const ServedApprovalsUndeclared: Story = {
  name: '?drive: a log that does not declare the approval lever shows it undeclared',
  args: { drive: true, web: serving(toolCallRan) },
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(canvasElement.querySelectorAll('.ex-trunk [data-tone="tool"]').length).toBeGreaterThan(0));
    const approvals = canvasElement.querySelector<HTMLElement>('.ex-header__lever[data-lever="approvals"]');
    await expect(approvals?.textContent).toBe('approvals undeclared');
    await expect(approvals?.dataset['undeclared']).toBe('');
  },
};

/** A served log, from lines the surface holds: one per line, as `serving` streams it. */
const jsonl = (lines: readonly unknown[]) => lines.map((l) => JSON.stringify(l)).join('\n');

/** `?drive` (#608): with no tangent open, the composer offers to open one, as `t/1`. */
export const ServedOpenTangent: Story = {
  name: '?drive: open a tangent',
  args: { drive: true, web: posting(answeredTurn) },
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(says(canvasElement)).toBe('your turn'));
    posted.length = 0;
    await userEvent.click([...canvasElement.querySelectorAll('button')].find((b) => b.textContent === 'open tangent')!);
    await waitFor(async () => expect(posted).toEqual([{ kind: 'open-tangent', id: 't/1' }]));
  },
};

/** `?drive` (#608): a tangent open with two entries of its own -- end it, keeping one and dropping the other. */
export const ServedEndTangent: Story = {
  name: '?drive: end a tangent, ruling on each of its entries',
  args: { drive: true, web: posting(jsonl(TANGENT_OPEN)) },
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(says(canvasElement)).toBe('your turn'));
    await userEvent.click([...canvasElement.querySelectorAll('button')].find((b) => b.textContent === 'end tangent…')!);
    const panel = await waitFor(async () => {
      const found = canvasElement.querySelector<HTMLElement>('fieldset[aria-label="end tangent t/1"]');
      await expect(found).not.toBeNull();
      return found!;
    });
    await expect([...panel.querySelectorAll('li')].map((li) => li.dataset['entry'])).toEqual(['e1', 'e2']);
    await userEvent.click(panel.querySelector('li[data-entry="e2"] input[value="drop"]')!);
    posted.length = 0;
    await userEvent.click([...panel.querySelectorAll('button')].find((b) => b.textContent === 'close tangent')!);
    await waitFor(async () => expect(posted).toEqual([{ kind: 'close-tangent', dispositions: { e1: 'keep', e2: 'drop' } }]));
  },
};

/**
 * `?drive`, log v7 (#563): a session with a phase graph. The composer names the phase the log says it is in, offers
 * exactly the moves the graph allows from there, and its refill sends the phase picked.
 */
export const ServedPhaseGraph: Story = {
  name: '?drive: refill offers the moves the logged phase graph allows, and sends the one picked',
  args: { drive: true, web: posting(phaseMoved) },
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(canvasElement.querySelector('.ex-composer__phase')?.textContent).toBe('phase build'));
    const select = canvasElement.querySelector<HTMLSelectElement>('select[aria-label="move to"]');
    await expect([...(select?.options ?? [])].map((o) => o.value)).toEqual(['review']);
    posted.length = 0;
    await userEvent.click([...canvasElement.querySelectorAll('button')].find((b) => b.textContent === 'refill')!);
    await waitFor(async () => expect(posted.map((p) => (p as { kind: string }).kind)).toEqual(['declare-seam']));
    await expect((posted[0] as { phase?: string }).phase).toBe('review');
  },
};

/** diet's call that ran, cut before its outcome line: a log mid-call, the bash command still running in the foreground. */
const callRunning = toolCallRan.trimEnd().split('\n').filter((line) => !line.includes('"kind":"tool_call"')).join('\n');

/** `?drive` (#614): while a bash call runs in the foreground, the composer offers to move it to the background. */
export const ServedMoveToBackground: Story = {
  name: '?drive: move a running command to the background',
  args: { drive: true, web: posting(callRunning) },
  play: async ({ canvasElement }) => {
    const move = await waitFor(async () => {
      const found = [...canvasElement.querySelectorAll('button')].find((b) => b.textContent === 'move to background');
      await expect(found).toBeDefined();
      return found!;
    });
    posted.length = 0;
    await userEvent.click(move);
    await waitFor(async () => expect(posted).toEqual([{ kind: 'background' }]));
  },
};

/** And with the call answered, nothing runs in the foreground: there is nothing to move. */
export const ServedNothingToBackground: Story = {
  name: '?drive: with no command running, nothing to move to the background',
  args: { drive: true, web: posting(toolCallRan) },
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(canvasElement.querySelectorAll('.ex-trunk [data-tone="tool"]').length).toBe(1));
    await expect([...canvasElement.querySelectorAll('button')].some((b) => b.textContent === 'move to background')).toBe(false);
  },
};

/** The harness's note on an ask, as drawn: under the operator's words, named as the harness's, never as theirs. */
const harnessNote = async (root: HTMLElement, note: string) =>
  waitFor(async () => {
    const found = root.querySelector<HTMLElement>(`.ex-trunk .ex-harness-note[data-note="${note}"]`);
    await expect(found).not.toBeNull();
    return found!;
  });

/** `?drive`, log v7 (#574): the fork-delivery note sent after turn 2's ask, drawn as the harness's. */
export const ServedDeliveredNote: Story = {
  name: '?drive: the fork-delivery note after an ask, marked as the harness’s',
  args: { drive: true, web: serving(deliveredNote) },
  play: async ({ canvasElement }) => {
    const note = await harnessNote(canvasElement, 'delivered');
    await expect(note.querySelector('.ex-harness-note__label')?.textContent).toBe('harness · fork delivery · imperative');
    await expect(note.textContent).toContain('Working record');
  },
};

/** `?drive`, log v7 (#574): archive recall's note, drawn as the harness's. */
export const ServedRecalledNote: Story = {
  name: '?drive: the recall note after an ask, marked as the harness’s',
  args: { drive: true, web: serving(recalledNote) },
  play: async ({ canvasElement }) => {
    const note = await harnessNote(canvasElement, 'recalled');
    await expect(note.querySelector('.ex-harness-note__label')?.textContent).toBe('harness · recall · literal');
  },
};

/** `?drive` (#574): self-capture's reminder note, and its call on the trunk with what it recorded. */
export const ServedSelfCapture: Story = {
  name: '?drive: self-capture’s reminder and its recorded call',
  args: { drive: true, web: serving(SELF_CAPTURE.map((l) => JSON.stringify(l)).join('\n')) },
  play: async ({ canvasElement }) => {
    const note = await harnessNote(canvasElement, 'reminded');
    await expect(note.querySelector('.ex-harness-note__label')?.textContent).toBe('harness · self-capture reminder');
    const call = canvasElement.querySelector<HTMLElement>('.ex-trunk [data-tone="tool"]');
    await expect(call?.querySelector('.ex-capture')?.textContent).toBe('self-capture · recorded r10/c1/fact');
  },
};

/** `?drive`, log v7 (#604): the seam's refill sent as a user message -- the harness's summary, not the operator's ask. */
export const ServedRefillSummary: Story = {
  name: '?drive: the seam’s refill message is labelled as the harness’s summary',
  args: { drive: true, web: serving(refillMessage) },
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(canvasElement.querySelectorAll('.ex-trunk [data-tone="system"]').length).toBeGreaterThan(1));
    const labels = [...canvasElement.querySelectorAll('.ex-trunk [data-tone="system"] .ex-block__label')].map((l) => l.textContent ?? '');
    await expect(labels.some((l) => l.startsWith('harness summary'))).toBe(true);
    await expect(labels.some((l) => l.startsWith('user'))).toBe(false);
  },
};

/** `?drive` (#573): a session.start with the whole lever table -- the header lists every lever and its state, as given. */
export const ServedLeverTable: Story = {
  name: '?drive: the header lists every lever the session declares',
  args: { drive: true, web: serving(leversDeclared) },
  play: async ({ canvasElement }) => {
    const table = await waitFor(async () => {
      const found = canvasElement.querySelector<HTMLDetailsElement>('.ex-header__levers');
      await expect(found).not.toBeNull();
      return found!;
    });
    const start = JSON.parse(leversDeclared.split('\n')[0]!) as { levers: Record<string, string> };
    const rows = [...table.querySelectorAll<HTMLElement>('[data-lever-row]')].map((row) => [row.dataset['leverRow'], row.querySelector('dd')?.textContent]);
    await expect(rows).toEqual(Object.entries(start.levers));
  },
};

/** A log from before the table (#623) has none to list: the header keeps its three, and offers no table. */
export const ServedNoLeverTable: Story = {
  name: '?drive: a log without the lever table offers none',
  args: { drive: true, web: serving(answeredTurn) },
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(says(canvasElement)).toBe('your turn'));
    await expect(canvasElement.querySelector('.ex-header__levers')).toBeNull();
  },
};

/** diet's v5 forks with the first moved offboard (#615) -- WRITTEN HERE: no diet fixture has an offboard fork yet. */
const offboardForks = (() => {
  const lines = forks.trimEnd().split('\n').map((l) => JSON.parse(l) as Record<string, unknown>);
  const first = lines.find((l) => l['kind'] === 'fork')!;
  return lines
    .map((l) => (l === first ? { ...l, substrate: 'mac-pro-llamacpp-qwen3-4b', model: 'qwen3-4b' } : l['kind'] === 'fork.settled' && l['fork'] === first['seq'] ? { ...l, prompt_tokens: 1820, wall_ms: 4300 } : l))
    .map((l) => JSON.stringify(l))
    .join('\n');
})();

/** `?drive` (#570): an offboard fork names its seat on its branch, with its prefill and wall time; a warm one does not. */
export const ServedOffboardFork: Story = {
  name: '?drive: an offboard fork names its seat, prefill and wall time',
  args: { drive: true, web: serving(offboardForks) },
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(canvasElement.querySelectorAll('.ex-lane .ex-branch')).toHaveLength(2));
    const [offboard, warm] = [...canvasElement.querySelectorAll<HTMLElement>('.ex-lane .ex-branch')];
    await expect(offboard?.querySelector('.ex-branch__seat')?.textContent).toBe('mac-pro-llamacpp-qwen3-4b · 1.8k tok · 4.3 s');
    await expect(warm?.querySelector('.ex-branch__seat')).toBeNull();
  },
};

/** `?drive`, log v7 (#627): a self-captured entry in the working-memory panel, marked with the lane that wrote it. */
export const ServedSelfCapturedEntry: Story = {
  name: '?drive: a self-captured entry in working memory, marked self-capture',
  args: { drive: true, web: serving(selfCapturePatch) },
  play: async ({ canvasElement }) => {
    const entry = await waitFor(async () => {
      const found = canvasElement.querySelector<HTMLElement>('.ex-memory__entry[data-lane="self-capture"]');
      await expect(found).not.toBeNull();
      return found!;
    });
    await expect(entry.querySelector('.ex-memory__lane')?.textContent).toBe('self-capture');
    await expect(entry.querySelector('.ex-memory__text')?.textContent).toContain('The parser drops blank lines');
  },
};
