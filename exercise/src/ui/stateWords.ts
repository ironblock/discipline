/**
 * What the session is doing, in the operator's words (#1050's audit, finding 13): the composer's state line and the
 * header's state say the same thing. A capture is said by what runs in it -- the operator's own fork, a refill's
 * work, or an interview -- since under manual first most captures are no interview.
 */

import type { Session, SessionState } from '../session/fold.ts';

/** What a capture is running. */
export type Capturing = 'fork' | 'refill' | 'interview';

/** Lanes a meditation's refill runs on: its audit, its summary and its warm-up. */
const REFILL_LANES: ReadonlySet<string> = new Set(['audit', 'summary', 'warm']);

/** What the session's slots hold during a capture: the operator's fork if one runs, else a refill's work, else an interview. */
export function capturingOf(session: Pick<Session, 'occupancy' | 'branches'>): Capturing {
  const held = session.occupancy.filter((h) => h !== undefined);
  const branches = [...session.branches.values()].flat();
  if (held.some((h) => branches.some((b) => b.id === h.id && b.why === 'operator'))) return 'fork';
  if (held.length > 0 && held.every((h) => REFILL_LANES.has(h.lane))) return 'refill';
  return 'interview';
}

/** The composer's line for a capture. */
export const CAPTURE_LINE: Readonly<Record<Capturing, string>> = {
  fork: 'your fork is running · send when it settles',
  refill: 'refilling · send when it settles',
  interview: 'interview running · send when it settles',
};

/** The header's word for the state. */
export function stateWord(state: SessionState, capturing: Capturing): string {
  switch (state) {
    case 'connecting':
      return 'connecting';
    case 'awaiting':
      return 'your turn';
    case 'turn':
      return 'working';
    case 'capture':
      return capturing === 'fork' ? 'your fork' : capturing === 'refill' ? 'refilling' : 'interview';
    case 'ratify':
      return 'ratifying';
    case 'ended':
      return 'ended';
  }
}
