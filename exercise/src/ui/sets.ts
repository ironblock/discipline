/**
 * The registries for the surface's open sets (see `Open` in
 * `src/drive/events.ts`): each known member and how it is drawn, and the
 * one rule for everything else -- an unknown member is drawn neutrally,
 * labelled with its own name, never as a crash or a blank.
 *
 * Adding a member is one entry here (and, for a lane, its two colour
 * tokens in `src/theme/tokens.css`).
 */

import type { CSSProperties } from 'react';

import type { FailReason, ForkLane, ForkOutcome, PatchOp, SeamReason, SettleReason, Stop, Tool } from '../drive/events.ts';
import type { Refusal } from '../drive/transport.ts';

/** How a member reads at a glance. `quiet` is the neutral every unknown member gets. */
export type Level = 'ok' | 'quiet' | 'warn' | 'bad';

export interface Drawn {
  readonly label: string;
  readonly level: Level;
  /** Known to this surface. An unknown member is drawn, and marked as such. */
  readonly known: boolean;
}

function registry<K extends string>(known: Readonly<Record<string, Omit<Drawn, 'known'>>>) {
  return (member: K): Drawn => {
    const entry = Object.hasOwn(known, member) ? known[member] : undefined;
    return entry ? { ...entry, known: true } : { label: member, level: 'quiet', known: false };
  };
}

// ------------------------------------------------------------------ lanes

/** Lanes with colour tokens of their own (`--fill-<lane>`, `--ink-<lane>`). */
const LANES: ReadonlySet<string> = new Set(['interview', 'ratify', 'extraction']);

export function laneKnown(lane: ForkLane): boolean {
  return LANES.has(lane);
}

/**
 * A lane's colours, as custom properties for whatever draws it -- a bar, a
 * slot's LED, its column head. An unknown lane gets none and falls back to
 * the neutral `--fill-lane` / `--ink-lane`.
 */
export function laneStyle(lane: ForkLane | undefined): CSSProperties | undefined {
  if (lane === undefined || !LANES.has(lane)) return undefined;
  return { ['--lane-fill' as string]: `var(--fill-${lane})`, ['--lane-ink' as string]: `var(--ink-${lane})` };
}

// ------------------------------------------------------------------ tools

export interface Call {
  /** The footer's chip: the tool's name. */
  readonly label: string;
  /** Before the call's text: `$` for a shell. */
  readonly prompt: string;
  /** The call as one copyable string: a command line, or `name(args)`. */
  readonly text: string;
  readonly known: boolean;
}

const TOOLS: Readonly<Record<string, (args: Readonly<Record<string, unknown>>) => Omit<Call, 'known'> | undefined>> = {
  bash: (args) => (typeof args['command'] === 'string' ? { label: 'bash', prompt: '$', text: args['command'] } : undefined),
};

/** How a tool call reads. A tool this surface does not know -- or a known one called oddly -- reads as `name(args)`. */
export function callOf(tool: Tool, args: Readonly<Record<string, unknown>>): Call {
  const known = Object.hasOwn(TOOLS, tool) ? TOOLS[tool]?.(args) : undefined;
  return known ? { ...known, known: true } : { label: tool, prompt: '', text: `${tool}(${JSON.stringify(args)})`, known: false };
}

// ------------------------------------------------------------------ fork outcomes

/** Complete is the expected end and draws nothing; everything else says what happened. */
export const outcomeOf = registry<ForkOutcome>({
  complete: { label: 'complete', level: 'ok' },
  value: { label: 'value', level: 'ok' },
  empty: { label: 'empty', level: 'warn' },
  decline: { label: 'declined', level: 'quiet' },
  truncated: { label: 'truncated', level: 'warn' },
  thinking_exhausted: { label: 'thinking exhausted', level: 'warn' },
  timeout: { label: 'timed out', level: 'bad' },
  cancelled: { label: 'cancelled', level: 'quiet' },
  failed: { label: 'failed', level: 'bad' },
  mimicry: { label: 'mimicry', level: 'bad' },
  unparseable: { label: 'unparseable', level: 'bad' },
  rejected: { label: 'rejected', level: 'bad' },
});

// ------------------------------------------------------------------ patch ops

export interface Op extends Drawn {
  readonly glyph: string;
}

const OPS: Readonly<Record<string, Omit<Op, 'known'>>> = {
  add: { glyph: '+', label: 'add', level: 'ok' },
  supersede: { glyph: '↻', label: 'supersede', level: 'warn' },
  retire: { glyph: '−', label: 'retire', level: 'quiet' },
  resolve: { glyph: '✓', label: 'resolve', level: 'ok' },
  park: { glyph: '‖', label: 'park', level: 'quiet' },
  edit: { glyph: '✎', label: 'edit', level: 'ok' },
  void: { glyph: '∅', label: 'void', level: 'quiet' },
};

export function opOf(op: PatchOp): Op {
  const entry = Object.hasOwn(OPS, op) ? OPS[op] : undefined;
  return entry ? { ...entry, known: true } : { glyph: '·', label: op, level: 'quiet', known: false };
}

// ------------------------------------------------------------------ seams, stops, settlements

export const seamReasonOf = registry<SeamReason>({
  operator: { label: 'declared by you', level: 'quiet' },
  phase: { label: 'phase', level: 'quiet' },
  cadence: { label: 'cadence', level: 'quiet' },
  budget: { label: 'over budget', level: 'warn' },
});

/** Why a generation stopped. The expected ones draw nothing; the rest say so. */
export const stopOf = registry<Stop>({
  stop: { label: 'stop', level: 'ok' },
  tool: { label: 'tool call', level: 'ok' },
  length: { label: 'hit max tokens', level: 'warn' },
  cancelled: { label: 'cancelled', level: 'quiet' },
});

export const settleOf = registry<SettleReason>({
  final: { label: 'final', level: 'ok' },
  cancelled: { label: 'cancelled', level: 'quiet' },
  max_steps: { label: 'stopped at the step limit', level: 'warn' },
  timeout: { label: 'timed out', level: 'bad' },
  failed: { label: 'a request failed', level: 'bad' },
});

/** Why a request produced no response. Every one is a failure; the label says which. */
export const failOf = registry<FailReason>({
  server: { label: 'the server failed it', level: 'bad' },
  context_overflow: { label: 'the prompt no longer fits', level: 'bad' },
  timeout: { label: 'timed out', level: 'bad' },
  disconnected: { label: 'the connection dropped', level: 'bad' },
});

// ------------------------------------------------------------------ refusals

/** What the composer says when the drive does not take a command. */
export const refusalOf = registry<Refusal>({
  busy: { label: 'not taken: something is still running', level: 'warn' },
  ended: { label: 'not taken: the session has ended', level: 'quiet' },
  'nothing-to-seam': { label: 'nothing to refill yet: no turn has settled', level: 'quiet' },
  'nothing-to-cancel': { label: 'nothing is running', level: 'quiet' },
  'off-script': { label: 'the canned script expects something else next', level: 'quiet' },
  recording: { label: 'a recording: it plays, and takes no commands', level: 'quiet' },
});
