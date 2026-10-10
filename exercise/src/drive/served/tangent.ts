import type { LogLine } from '../log.ts';

/**
 * A tangent, WRITTEN HERE -- not recorded: no served session has opened one yet (#608 landed the commands). Turn 1
 * settles; the operator opens tangent `t/1`; turn 2 runs inside it and its interview patches two entries, each
 * carrying the tangent; the session awaits. `TANGENT_CLOSED` is the close the drive would log for keep `e1`, drop
 * `e2`. One source for the fold test and the stories; a recorded tangent, once one is driven, replaces it.
 */
export const TANGENT_OPEN: readonly LogLine[] = [
  { seq: 0, t: 0, kind: 'session.start', version: 7, opened: 1790000000000, model: 'a-model', head: [{ role: 'system', content: 'you are the trunk' }] },
  { seq: 1, t: 5, kind: 'ask', turn: 1, text: 'what are we building' },
  { seq: 2, t: 10, kind: 'settlement', from: 'awaiting', to: 'turn' },
  { seq: 3, t: 15, kind: 'request', lane: 'trunk', turn: 1 },
  { seq: 4, t: 20, kind: 'response', to_request: 3, text: 'a tracker for the team' },
  { seq: 5, t: 25, kind: 'turn.settled', turn: 1, reason: 'final' },
  { seq: 6, t: 30, kind: 'settlement', from: 'turn', to: 'awaiting' },
  { seq: 7, t: 35, kind: 'tangent.open', id: 't/1', at_turn: 1, trunk_messages: 3 },
  { seq: 8, t: 40, kind: 'ask', turn: 2, text: 'what if it were a game instead', scoping: true },
  { seq: 9, t: 45, kind: 'settlement', from: 'awaiting', to: 'turn' },
  { seq: 10, t: 50, kind: 'request', lane: 'trunk', turn: 2 },
  { seq: 11, t: 55, kind: 'response', to_request: 10, text: 'a game, then: a voxel arena' },
  { seq: 12, t: 60, kind: 'turn.settled', turn: 2, reason: 'final' },
  { seq: 13, t: 65, kind: 'settlement', from: 'turn', to: 'capture' },
  { seq: 14, t: 70, kind: 'fork', lane: 'interview', of_turn: 2, at: 10, why: 'scoping', question: 'what did you decide' },
  { seq: 15, t: 75, kind: 'request', lane: 'interview', turn: 2, fork: 14 },
  { seq: 16, t: 80, kind: 'response', to_request: 15, text: 'DECISION: a voxel arena\nDECISION: no login\nPLAN: NONE' },
  { seq: 17, t: 85, kind: 'fork.settled', fork: 14, outcome: 'value' },
  { seq: 18, t: 90, kind: 'patch', fork: 14, op: 'add', entry: { id: 'e1', text: 'decision: no login' }, tangent: 't/1' },
  { seq: 19, t: 95, kind: 'patch', fork: 14, op: 'add', entry: { id: 'e2', text: 'decision: a voxel arena' }, tangent: 't/1' },
  { seq: 20, t: 100, kind: 'settlement', from: 'capture', to: 'awaiting' },
] as LogLine[];

export const TANGENT_CLOSED: readonly LogLine[] = [
  ...TANGENT_OPEN,
  { seq: 21, t: 105, kind: 'tangent.close', id: 't/1', at_turn: 2, kept: ['e1'], dropped: ['e2'], parked: [], prefix_intact: true, rolled_back: 2 },
] as LogLine[];
