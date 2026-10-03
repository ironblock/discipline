import type { LogLine } from '../log.ts';

/**
 * A stop in prefill, WRITTEN HERE -- not recorded (#294, ruled): no turn in the rehearsal drive's log stopped
 * before its prompt was read, so this is what the drive would log after `rehearsal-turns-1-4.log`'s seq 1185
 * (turn 4's third progress line, 51 of 567 new tokens read): the stop asked, the call cancelled with nothing
 * written, the turn settled, the session awaiting. One source for the test and the story; a real mid-prefill
 * cancel, if one is ever captured from the floor, replaces it.
 */
export const STOPPED_IN_PREFILL_AFTER = 1185;

export const STOPPED_IN_PREFILL: readonly LogLine[] = [
  { seq: 1186, t: 288100, kind: 'stop.asked', turn: 4 },
  { seq: 1187, t: 288101, kind: 'cancelled', request: 1182, partial: '' },
  { seq: 1188, t: 288101, kind: 'turn.settled', turn: 4, reason: 'cancelled' },
  { seq: 1189, t: 288101, kind: 'settlement', from: 'turn', to: 'awaiting' },
] as LogLine[];
