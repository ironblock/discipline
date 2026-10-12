import type { LogLine } from '../log.ts';

/**
 * The two keys (#788), WRITTEN HERE -- not recorded: no diet fixture carries `phase.proposed`, `phase.answered` or
 * `phase.started` yet. A v8 session in a three-phase library opens in `plan` (build suggested, test allowed) and settles
 * one turn. From there: the model proposes, with its evidence; the operator asks the model, which declines or accepts;
 * or, before any ask, the operator starts in another phase. Each line is shaped as `diet/formats/log/log.ts` declares it.
 */
const PLAN = {
  name: 'plan',
  digest: 'c'.repeat(64),
  ground_rules: 'Settle the approach before any change.',
  transitions: [
    { to: 'build', kind: 'suggested' },
    { to: 'test', kind: 'allowed' },
  ],
};
const BUILD = { name: 'build', digest: 'd'.repeat(64), ground_rules: 'Make the change the plan settled.', transitions: [] };
const EVIDENCE = { phase_steps: 6, phase_tokens: 12_400, tokens_since_meditation: 12_400, meditated_by_hand: false };

/** The session opened, nothing asked: the starting phase may still be chosen. */
export const PHASE_OPENED: readonly LogLine[] = [
  {
    seq: 0,
    t: 0,
    kind: 'session.start',
    version: 8,
    opened: 1790000000000,
    model: 'a-model',
    head: [{ role: 'system', content: 'you are the trunk' }],
    phases: ['plan', 'build', 'test'],
    opening_phase: 'plan',
    phase_document: PLAN,
  },
] as LogLine[];

/** The operator chose `build` before the first ask. */
export const PHASE_STARTED: readonly LogLine[] = [...PHASE_OPENED, { seq: 1, t: 5, kind: 'phase.started', phase: 'build', phase_document: BUILD }] as LogLine[];

/** One turn asked and settled, in `plan`. */
const SETTLED: readonly LogLine[] = [
  ...PHASE_OPENED,
  { seq: 1, t: 5, kind: 'ask', turn: 1, text: 'how should we approach it' },
  { seq: 2, t: 10, kind: 'settlement', from: 'awaiting', to: 'turn' },
  { seq: 3, t: 15, kind: 'request', lane: 'trunk', turn: 1 },
  { seq: 4, t: 20, kind: 'response', to_request: 3, text: 'two tables, bugs and people' },
  { seq: 5, t: 25, kind: 'turn.settled', turn: 1, reason: 'final' },
  { seq: 6, t: 30, kind: 'settlement', from: 'turn', to: 'awaiting' },
] as LogLine[];

/** Idle in `plan`, nothing proposed: the operator's key. */
export const PHASE_IDLE: readonly LogLine[] = SETTLED;

/** The model proposed `build`, ending its turn: the operator rules, with the evidence. */
export const MODEL_PROPOSED: readonly LogLine[] = [
  ...PHASE_OPENED,
  { seq: 1, t: 5, kind: 'ask', turn: 1, text: 'how should we approach it' },
  { seq: 2, t: 10, kind: 'settlement', from: 'awaiting', to: 'turn' },
  { seq: 3, t: 15, kind: 'request', lane: 'trunk', turn: 1 },
  { seq: 4, t: 20, kind: 'response', to_request: 3, text: '', finish_reason: 'tool_calls' },
  { seq: 5, t: 25, kind: 'tool_call', request: 3, turn: 1, id: 'p1', name: 'propose_phase_transition', arguments: '{"to":"build","reason":"the plan is settled"}', outcome: 'ran', stdout: 'proposed' },
  { seq: 6, t: 25, kind: 'phase.proposed', by: 'model', call: 'p1', from: 'plan', to: 'build', reason: 'the plan is settled', ...EVIDENCE },
  { seq: 7, t: 30, kind: 'turn.settled', turn: 1, reason: 'final' },
  { seq: 8, t: 35, kind: 'settlement', from: 'turn', to: 'awaiting' },
] as LogLine[];

/** Ruled keep: the phase moves, the trunk is kept. */
export const MODEL_PROPOSAL_KEPT: readonly LogLine[] = [
  ...MODEL_PROPOSED,
  { seq: 9, t: 40, kind: 'phase.ruled', by: 'operator', call: 'p1', choice: 'keep', from: 'plan', to: 'build', phase_document: BUILD },
] as LogLine[];

/** The operator asked the model to move to `build`, keeping the trunk; the model has not answered. */
export const OPERATOR_ASKED: readonly LogLine[] = [
  ...SETTLED,
  { seq: 7, t: 35, kind: 'phase.proposed', by: 'operator', from: 'plan', to: 'build', meditate: false, ...EVIDENCE },
] as LogLine[];

/** The model declined, with its reason. */
export const OPERATOR_DECLINED: readonly LogLine[] = [
  ...OPERATOR_ASKED,
  { seq: 8, t: 40, kind: 'phase.answered', call: 'a1', accepted: false, reason: 'the tables are not settled yet' },
] as LogLine[];

/** The model accepted: the move follows the operator's choice, ruled by the model. */
export const OPERATOR_ACCEPTED: readonly LogLine[] = [
  ...OPERATOR_ASKED,
  { seq: 8, t: 40, kind: 'phase.answered', call: 'a1', accepted: true },
  { seq: 9, t: 40, kind: 'phase.ruled', by: 'model', call: 'a1', choice: 'keep', from: 'plan', to: 'build', phase_document: BUILD },
] as LogLine[];
