/**
 * AN AUTHORED SESSION for the approval prompt (#389): T1's opening (#177), the
 * operator asking for the dependencies and the dev server. Not a record:
 * hand-written in the script language of `script.ts` so the canned transport
 * can hold a call on the operator before `serve` does (#298 point 8). Its
 * numbers are plausible, not measured.
 *
 * `npm install` waits on the operator. Approved, the beat plays on: it runs,
 * carrying the decision; the model then tries to clear a cache with `rm -rf`,
 * which the denylist refuses without asking; and it answers. Declined, the
 * call is refused `declined` and the model says it will not install.
 *
 * Placed, it carries log v4's keys (`cwd`, `approval`, the reasons `denylist`
 * and `declined`, #388) in a log that declares v3, which `diet`'s v3 reader
 * refuses. So it is not among the sessions `projections.ts` hands to
 * `diet check-log` until the v4 courier lands; then it joins them.
 */

import type { Beat } from './specimen.ts';
import type { Timings } from './script.ts';

const timings = (prompt_n: number, cache_n: number, prompt_ms: number, predicted_n: number, predicted_ms: number): Timings => ({ prompt_n, cache_n, prompt_ms, predicted_n, predicted_ms });

/** Where T1's experiment lives: a tilde path, never expanded (#388 5982826236). */
export const T1_CWD = '~/git/experiments/t1';

export const APPROVAL: readonly Beat[] = [
  {
    trigger: 'open',
    events: [
      {
        kind: 'session.start',
        t: 0,
        arm: 'diet',
        model: 'a 27B instruct model, Q4, one 24 GB GPU',
        slots: 1,
        trunk_slot: 0,
        phase: 'build',
        system: { text: 'You are working in a fresh Vite project. Run commands with the bash tool.', tokens: 640 },
      },
    ],
  },
  {
    trigger: 'send',
    events: [
      { kind: 'ask', t: 0, turn: 1, text: 'Install the dependencies and start the dev server.' },
      { kind: 'request', t: 30, id: 'q/1', lane: 'trunk', slot: 0, turn: 1 },
      {
        kind: 'response',
        t: 1400,
        id: 'q/1#response',
        to_request: 'q/1',
        reasoning: 'A fresh project: install first.',
        text: 'Installing the dependencies first.',
        stop: 'tool',
        timings: timings(700, 0, 410, 38, 940),
      },
      {
        kind: 'tool.begin',
        t: 1420,
        id: 't/1',
        turn: 1,
        after: 'q/1#response',
        tool: 'bash',
        args: { command: 'npm install' },
        cwd: T1_CWD,
        prompt: {
          reason: 'not_approved',
          segments: [{ text: 'npm install', program: 'npm', subcommand: 'install', verdict: 'prompt', why: 'not_approved' }],
          declined: [
            { kind: 'request', t: 20, id: 'q/1d', lane: 'trunk', slot: 0, turn: 1 },
            {
              kind: 'response',
              t: 900,
              id: 'q/1d#response',
              to_request: 'q/1d',
              text: 'Understood: I will not install anything. Tell me how you would like the dependencies installed and I will go from there.',
              stop: 'stop',
              timings: timings(64, 738, 60, 31, 820),
            },
            { kind: 'turn.settled', t: 910, turn: 1, reason: 'final' },
          ],
        },
      },
      { kind: 'tool.end', t: 9400, id: 't/1', exit: 0, output: '\nadded 212 packages, and audited 213 packages in 8s\n\nfound 0 vulnerabilities\n' },
      { kind: 'request', t: 9420, id: 'q/2', lane: 'trunk', slot: 0, turn: 1 },
      {
        kind: 'response',
        t: 10600,
        id: 'q/2#response',
        to_request: 'q/2',
        reasoning: 'Clear the Vite cache before the first start, to be safe.',
        text: 'Clearing the build cache before starting.',
        stop: 'tool',
        timings: timings(96, 738, 80, 30, 1050),
      },
      { kind: 'tool.begin', t: 10620, id: 't/2', turn: 1, after: 'q/2#response', tool: 'bash', args: { command: 'rm -rf node_modules/.vite' }, cwd: T1_CWD },
      { kind: 'tool.end', t: 10625, id: 't/2', exit: 0, output: '', refused: 'denylist' },
      { kind: 'request', t: 10640, id: 'q/3', lane: 'trunk', slot: 0, turn: 1 },
      {
        kind: 'response',
        t: 12100,
        id: 'q/3#response',
        to_request: 'q/3',
        text: 'The dependencies are installed. Removing the cache is not allowed here, and it is not needed: a fresh install has none. Run `npm run dev` to start the server when you want it.',
        stop: 'stop',
        timings: timings(58, 864, 50, 44, 1400),
      },
      { kind: 'turn.settled', t: 12110, turn: 1, reason: 'final' },
    ],
  },
];
