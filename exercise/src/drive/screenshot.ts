/**
 * AN AUTHORED SESSION for a tool call whose result is a file (#372): T1's
 * screenshot, a canvas the model rendered, carried by reference -- path,
 * sha256, media type, size -- and drawn from its bytes, checked. Not a
 * record: hand-written, and the image is made by `png.ts` from the few
 * numbers below, so nothing about it has a provenance but this file. Its
 * timings are plausible, not measured.
 *
 * Placed, it carries log v4's `files` (#372 5983588781) in a log that
 * declares v3, so it is not among the sessions `projections.ts` hands to
 * `diet check-log` until the v4 courier lands.
 */

import { png } from './png.ts';
import type { ScriptedFile, Timings } from './script.ts';
import type { Beat } from './specimen.ts';

const timings = (prompt_n: number, cache_n: number, prompt_ms: number, predicted_n: number, predicted_ms: number): Timings => ({ prompt_n, cache_n, prompt_ms, predicted_n, predicted_ms });

/** A lit sphere over a ground plane, 160 × 96: what a first Babylon scene looks like, roughly. */
export const SCENE_PNG = png(160, 96, (x, y) => {
  const dx = (x - 80) / 30;
  const dy = (y - 44) / 30;
  const r2 = dx * dx + dy * dy;
  if (r2 <= 1) {
    const light = Math.max(0, -0.5 * dx - 0.6 * dy + 0.62 * Math.sqrt(1 - r2));
    return [Math.round(40 + 190 * light), Math.round(70 + 150 * light), Math.round(140 + 110 * light)];
  }
  if (y > 70) return [52 + ((x >> 3) + (y >> 3)) % 2 * 18, 58, 64];
  return [18, 22 + (y >> 2), 38 + (y >> 1)];
});

/** Its sha256, as the drive would log it: `files.test.ts` holds it to the bytes. */
export const SCENE_SHA256 = '181894c2119af02298cc86a30ed1f490cf95c79c8567022e1f102511d010bf77';

export const SCENE: ScriptedFile = { path: 'shots/scene.png', media_type: 'image/png', sha256: SCENE_SHA256, bytes: SCENE_PNG };

export const SCREENSHOT: readonly Beat[] = [
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
        system: { text: 'You are working in a Babylon.js project. Run commands with the bash tool.', tokens: 610 },
      },
    ],
  },
  {
    trigger: 'send',
    events: [
      { kind: 'ask', t: 0, turn: 1, text: 'Render the scene and show me a screenshot.' },
      { kind: 'request', t: 30, id: 'q/1', lane: 'trunk', slot: 0, turn: 1 },
      {
        kind: 'response',
        t: 1300,
        id: 'q/1#response',
        to_request: 'q/1',
        reasoning: 'The project has a headless screenshot script.',
        text: 'Rendering it headless and taking a screenshot.',
        stop: 'tool',
        timings: timings(680, 0, 400, 36, 880),
      },
      { kind: 'tool.begin', t: 1320, id: 't/1', turn: 1, after: 'q/1#response', tool: 'bash', args: { command: 'node shot.mjs --out shots/scene.png' } },
      { kind: 'tool.end', t: 4100, id: 't/1', exit: 0, output: 'wrote shots/scene.png (160x96)\n', files: [SCENE] },
      { kind: 'request', t: 4120, id: 'q/2', lane: 'trunk', slot: 0, turn: 1 },
      {
        kind: 'response',
        t: 5600,
        id: 'q/2#response',
        to_request: 'q/2',
        text: 'Here it is: a lit sphere above a checkered ground plane, lit from the upper left.',
        stop: 'stop',
        timings: timings(240, 716, 180, 28, 1200),
      },
      { kind: 'turn.settled', t: 5610, turn: 1, reason: 'final' },
    ],
  },
];
