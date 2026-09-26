import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect } from 'storybook/test';

import { PHASES } from '../App.tsx';
import type { Link } from '../drive/transport.ts';
import type { Session } from '../session/fold.ts';
import { SessionView } from '../ui/SessionView.tsx';
import { MOMENTS, sessionAt, variantAt } from './moments.ts';

interface FailureArgs {
  readonly session: () => Session;
  readonly link: Link;
}

/**
 * How the surface looks when things go wrong. Each story is the specimen with
 * one failure edited in -- the kinds the predecessor's drives actually hit,
 * and the ones a real drive will -- so what it draws is what `fold()` makes of
 * it. For failures that really happened, see `Session/Recorded`.
 */
const meta = {
  title: 'Session/Failures',
  parameters: { layout: 'fullscreen' },
  args: { session: () => sessionAt(MOMENTS.firstSettled), link: 'live' },
  render: ({ session, link }) => <SessionView session={session()} link={link} surface={{ curtain: true, gaps: false }} composer={{ phases: PHASES }} />,
} satisfies Meta<FailureArgs>;

export default meta;
type Story = StoryObj<typeof meta>;

/** What the trunk had streamed of `q/3` by the moment. */
const streamedSoFar = (log: readonly Readonly<Record<string, unknown>>[]) =>
  log
    .filter((e) => e['kind'] === 'delta' && e['request'] === 'q/3')
    .map((e) => String(e['text'] ?? ''))
    .join('');
const at = (log: readonly Readonly<Record<string, unknown>>[]) => Number(log.at(-1)?.['t'] ?? 0) + 100;

export const Cancelled: Story = {
  name: 'the person cancelled mid-answer',
  args: {
    session: () =>
      variantAt(MOMENTS.streaming, (e) => e, (log) => [
        { kind: 'response', t: at(log), id: 'q/3#response', to_request: 'q/3', text: streamedSoFar(log), stop: 'cancelled', timings: { prompt_n: 0, cache_n: 0, prompt_ms: 0, predicted_n: 0, predicted_ms: 0 } },
        { kind: 'turn.settled', t: at(log), turn: 1, reason: 'cancelled' },
      ]),
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector('.ex-cancelled')).not.toBeNull();
    await expect(canvasElement.querySelector('[data-state="awaiting"]')).not.toBeNull();
  },
};

export const RequestFailed: Story = {
  name: 'the server failed a request',
  args: {
    session: () =>
      variantAt(MOMENTS.streaming, (e) => e, (log) => [
        { kind: 'request.failed', t: at(log), request: 'q/3', reason: 'context_overflow', message: 'the request (33,120 tokens) exceeds the available context size (32,768 tokens)' },
        { kind: 'turn.settled', t: at(log), turn: 1, reason: 'failed' },
      ]),
  },
  play: async ({ canvasElement }) => {
    const failed = canvasElement.querySelector('.ex-failed');
    await expect(failed?.textContent).toContain('the prompt no longer fits');
    await expect(failed?.textContent).toContain('exceeds the available context size');
    await expect(canvasElement.querySelector('.ex-turnend[data-level="bad"]')?.textContent).toContain('a request failed');
  },
};

export const MaxTokens: Story = {
  name: 'an answer cut off at max tokens',
  args: { session: () => variantAt(MOMENTS.firstSettled, (e) => (e['kind'] === 'response' && e['to_request'] === 'q/3' ? { ...e, stop: 'length' } : e)) },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector('.ex-stop[data-level="warn"]')?.textContent).toBe('hit max tokens');
  },
};

export const StepLimit: Story = {
  name: 'a turn stopped at the step limit',
  args: { session: () => variantAt(MOMENTS.firstSettled, (e) => (e['kind'] === 'turn.settled' && e['turn'] === 1 ? { ...e, reason: 'max_steps' } : e)) },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector('.ex-turnend[data-level="warn"]')?.textContent).toContain('stopped at the step limit');
  },
};

export const ToolFailed: Story = {
  name: 'a command failed',
  args: {
    session: () =>
      variantAt(MOMENTS.done, (e) =>
        e['kind'] === 'tool.end' && e['id'] === 't/5'
          ? { ...e, exit: 101, output: 'running 3 tests\ntest json_lines ... FAILED\n\nfailures:\n    json_lines: left: 4, right: 3\n\ntest result: FAILED. 2 passed; 1 failed' }
          : e,
      ),
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector('[data-id="t/5"] .ex-exit--bad')?.textContent).toBe('exit 101');
  },
};

export const SideCallTimedOut: Story = {
  name: 'a side call timed out',
  args: {
    session: () =>
      variantAt(MOMENTS.firstSettled, (e) => {
        if (e['kind'] === 'response' && e['to_request'] === 'i/2/q') return { kind: 'request.failed', t: e['t'], request: 'i/2/q', reason: 'timeout', message: 'no response within 120 s' };
        if (e['kind'] === 'patch' && e['from'] === 'i/2') return [];
        if (e['kind'] === 'fork.settled' && e['id'] === 'i/2') return { ...e, outcome: 'timeout' };
        return e;
      }),
  },
  play: async ({ canvasElement }) => {
    const bar = canvasElement.querySelector('[data-branch="i/2"]');
    await expect(bar?.querySelector('.ex-branch__outcome[data-level="bad"]')?.textContent).toBe('timed out');
    await expect(bar?.querySelector('.ex-patch')).toBeNull();
  },
};

export const Reconnecting: Story = {
  name: 'the connection dropped, reconnecting',
  args: { link: 'reconnecting' },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector('.ex-header__link')?.textContent).toBe('reconnecting…');
    await expect(canvasElement.querySelector('.ex-composer__state')?.textContent).toContain('your draft is kept');
    await expect(canvasElement.querySelector('.ex-composer__send')?.hasAttribute('disabled')).toBe(true);
  },
};

export const Lost: Story = {
  name: 'the connection is lost',
  args: { link: 'lost' },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector('.ex-header__link[data-link="lost"]')?.textContent).toBe('connection lost');
  },
};
