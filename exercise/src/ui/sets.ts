/**
 * The registries for the surface's open sets (see `Open` in
 * `src/drive/log.ts`): each known member and how it is drawn, and the
 * one rule for everything else -- an unknown member is drawn neutrally,
 * labelled with its own name, never as a crash or a blank.
 *
 * Adding a member is one entry here (and, for a lane, its two colour
 * tokens in `src/theme/tokens.css`).
 */

import type { CSSProperties } from 'react';

import type { ApprovalScope, FailReason, ForkLane, ForkOutcome, PatchOp, SeamReason, SettleReason, Tool, ToolOutcome, ToolRefusal } from '../drive/log.ts';
import type { Stop } from '../session/fold.ts';
import type { Refusal } from '../drive/transport.ts';

/** How a member reads at a glance. `quiet` is the neutral every unknown member gets. */
export type Level = 'ok' | 'quiet' | 'warn' | 'bad';

/** The alarm a level raises: warn and bad do, ok and quiet do not. */
export function alarmOf(level: Level): 'warn' | 'bad' | undefined {
  return level === 'warn' || level === 'bad' ? level : undefined;
}

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
const LANES: ReadonlySet<string> = new Set(['interview', 'ratify', 'extraction', 'audit']);

/**
 * A lane's colours, as custom properties for whatever draws it -- a bar, a
 * slot's LED, its column head. An unknown lane gets none and falls back to
 * the neutral `--fill-lane` / `--ink-lane`.
 */
export function laneStyle(lane: ForkLane | undefined): CSSProperties | undefined {
  if (lane === undefined || !LANES.has(lane)) return undefined;
  return { ['--lane-fill' as string]: `var(--fill-${lane})`, ['--lane-ink' as string]: `var(--ink-${lane})`, ['--lane-lamp' as string]: `var(--lamp-${lane})` };
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

/** What became of a call (v3, #297). `ran` draws nothing of its own: its exit says how it went. */
export const callOutcomeOf = registry<ToolOutcome>({
  ran: { label: 'ran', level: 'ok' },
  refused: { label: 'refused', level: 'warn' },
  command_failed: { label: 'failed under policy', level: 'bad' },
  cancelled: { label: 'cancelled', level: 'quiet' },
  // A result over the cap (#94): a file too large to carry is this outcome too, its sibling (#372 5983588781).
  output_too_large: { label: 'output too large', level: 'warn' },
});

/** Why the drive refused a call (v3, #297 Q2). */
export const callRefusalOf = registry<ToolRefusal>({
  not_allowed: { label: 'not on the allowlist', level: 'warn' },
  max_steps: { label: 'past the step limit', level: 'warn' },
  unparsable: { label: 'arguments unparsable', level: 'warn' },
  unknown_tool: { label: 'no such tool', level: 'warn' },
  denylist: { label: 'on the denylist', level: 'warn' },
  declined: { label: 'declined by the operator', level: 'warn' },
});

/** What a call ran under (log v4's `approval`, #388): the operator's scope, or the pre-seeded set. */
/** Why the gate held a command (`diet`'s `Why`, `shell_gate.rs`): the waiting event's `reason`, a segment's `why`. */
export const heldOf = registry<string>({
  not_approved: { label: 'no approval covers it yet', level: 'warn' },
  dynamic: { label: 'what it runs is decided as it runs: only once can answer it', level: 'warn' },
  unparsable: { label: 'the gate cannot read it', level: 'warn' },
});

/** What the gate made of one segment of a command. */
export const verdictOf = registry<string>({
  prompt: { label: 'asks you', level: 'warn' },
  free: { label: 'free', level: 'quiet' },
  approved: { label: 'approved', level: 'ok' },
  refused: { label: 'refused', level: 'bad' },
});

export const approvalOf = registry<ApprovalScope>({
  once: { label: 'approved once', level: 'ok' },
  session: { label: 'approved for this session', level: 'ok' },
  workspace: { label: 'approved for this workspace', level: 'ok' },
  preseeded: { label: 'pre-seeded', level: 'quiet' },
  off: { label: 'approvals off', level: 'quiet' },
});

// ------------------------------------------------------------------ fork outcomes

/** `value` is the expected end and draws nothing; everything else says what happened. */
export const outcomeOf = registry<ForkOutcome>({
  value: { label: 'value', level: 'ok' },
  decline: { label: 'declined', level: 'quiet' },
  truncated: { label: 'truncated', level: 'warn' },
  output_too_large: { label: 'output too large', level: 'warn' },
  thinking_exhausted: { label: 'thinking exhausted', level: 'warn' },
  timeout: { label: 'timed out', level: 'bad' },
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
  window: { label: 'the window', level: 'warn' },
  prune: { label: 'the model pruned output', level: 'quiet' },
});

/** A hazard a fork was sent knowing (#637). */
export const hazardOf = registry<string>({
  'may-displace-trunk-cache': { label: 'may displace the trunk’s cache', level: 'warn' },
});

/** Why a generation stopped. The expected ones draw nothing; the rest say so. */
export const stopOf = registry<Stop>({
  stop: { label: 'stop', level: 'ok' },
  tool_calls: { label: 'tool call', level: 'ok' },
  length: { label: 'hit max tokens', level: 'warn' },
  cancelled: { label: 'cancelled', level: 'quiet' },
});

export const settleOf = registry<SettleReason>({
  final: { label: 'final', level: 'ok' },
  cancelled: { label: 'cancelled', level: 'quiet' },
  max_steps: { label: 'stopped at the step limit', level: 'warn' },
  timeout: { label: 'timed out', level: 'bad' },
  failed: { label: 'a request failed', level: 'bad' },
  capped: { label: 'hit max tokens, so no answer', level: 'warn' },
});

/** Why a request produced no response. Every one is a failure; the label says which. */
export const failOf = registry<FailReason>({
  server: { label: 'the server failed it', level: 'bad' },
  context_overflow: { label: 'the prompt no longer fits', level: 'bad' },
  timeout: { label: 'timed out', level: 'bad' },
  transport: { label: 'the connection dropped', level: 'bad' },
  crashed: { label: 'the call\'s thread crashed', level: 'bad' },
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
  // diet's own (log v0's Refusal), and the transport's.
  'in-flight': { label: 'not taken: something is still running', level: 'warn' },
  'nothing-in-flight': { label: 'nothing is running', level: 'quiet' },
  'seam-not-built': { label: 'not taken: the drive cannot refill yet', level: 'quiet' },
  stale: { label: 'not taken: that turn is over', level: 'quiet' },
  unreachable: { label: 'not taken: the drive cannot be reached', level: 'bad' },
  // An attachment's (`POST /files`), and an ask naming one the drive was never sent.
  'not-a-png': { label: 'not attached: the file is not a PNG', level: 'warn' },
  'too-large': { label: 'not attached: the file is over the drive\'s size cap', level: 'warn' },
  'not-uploaded': { label: 'not taken: an attachment never reached the drive', level: 'bad' },
  // A tangent's (#608).
  'tangent-open': { label: 'not taken: a tangent is open -- end it first', level: 'warn' },
  'no-tangent': { label: 'not taken: no tangent is open', level: 'quiet' },
  'bad-tangent': { label: 'not taken: a tangent needs working memory, and a new id', level: 'warn' },
  'not-the-scope': { label: 'not taken: rule on exactly the tangent’s entries', level: 'warn' },
  'nothing-running': { label: 'not taken: no command is running to move', level: 'quiet' },
  // A seam's phase the graph would not take (#563).
  'no-phase-graph': { label: 'not taken: this session declares no phases', level: 'quiet' },
  'not-a-phase': { label: 'not taken: that is not one of the session’s phases', level: 'warn' },
  'already-in-phase': { label: 'not taken: the session is already in that phase', level: 'quiet' },
  'no-phase-edge': { label: 'not taken: the phase graph has no move from here to there', level: 'warn' },
});
