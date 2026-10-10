import type { LogLine } from '../log.ts';

/**
 * A phase proposal, WRITTEN HERE -- not recorded: no diet fixture carries `phase.ruled` yet (#651). The session opens
 * in `plan` under a plan → build graph; the model calls `propose_phase_transition` (to `build`, with its reason),
 * logged beside its `capture` (`proposed`); the turn settles and the session awaits the operator's ruling.
 * `PHASE_RULED_CONTINUE` is the ruling the drive would log for "continue": the phase moves, no seam.
 */
export const PHASE_PROPOSED: readonly LogLine[] = [
  {
    seq: 0,
    t: 0,
    kind: 'session.start',
    version: 7,
    opened: 1790000000000,
    model: 'a-model',
    head: [{ role: 'system', content: 'you are the trunk' }],
    phases: ['plan', 'build'],
    phase_transitions: [{ from: 'plan', to: 'build' }],
    opening_phase: 'plan',
  },
  { seq: 1, t: 5, kind: 'ask', turn: 1, text: 'the spec is settled, I think' },
  { seq: 2, t: 10, kind: 'settlement', from: 'awaiting', to: 'turn' },
  { seq: 3, t: 15, kind: 'request', lane: 'trunk', turn: 1 },
  { seq: 4, t: 20, kind: 'delta', request: 3, tool_call: { index: 0, id: 'p1', name: 'propose_phase_transition', arguments: '{"to":"build","reason":"the spec is settled"}' } },
  { seq: 5, t: 25, kind: 'response', to_request: 3, text: '', finish_reason: 'tool_calls' },
  { seq: 6, t: 30, kind: 'tool_call', request: 3, turn: 1, id: 'p1', name: 'propose_phase_transition', arguments: '{"to":"build","reason":"the spec is settled"}', outcome: 'ran', stdout: 'proposed: plan -> build' },
  { seq: 7, t: 30, kind: 'capture', request: 3, call: 'p1', tool: 'propose_phase_transition', outcome: 'proposed', entries: [], from: 'plan', to: 'build' },
  { seq: 8, t: 35, kind: 'request', lane: 'trunk', turn: 1 },
  { seq: 9, t: 40, kind: 'response', to_request: 8, text: 'I have proposed moving to build.' },
  { seq: 10, t: 45, kind: 'turn.settled', turn: 1, reason: 'final' },
  { seq: 11, t: 50, kind: 'settlement', from: 'turn', to: 'awaiting' },
] as LogLine[];

export const PHASE_RULED_CONTINUE: readonly LogLine[] = [
  ...PHASE_PROPOSED,
  { seq: 12, t: 55, kind: 'phase.ruled', call: 'p1', choice: 'continue', from: 'plan', to: 'build' },
] as LogLine[];
