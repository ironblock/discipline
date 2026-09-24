/**
 * How a story reaches the surface: a moment of the specimen, folded.
 *
 * Stories never build a node by hand. They stop the specimen at a cursor,
 * fold it with the same `fold()` the app uses, and pick what they show out
 * of the result -- so the brand on every node is honest, and a story shows
 * exactly what the app would at that moment.
 */

import { snapshot } from '../drive/canned.ts';
import type { Cursor } from '../drive/canned.ts';
import { SPECIMEN } from '../drive/specimen.ts';
import { fold } from '../session/fold.ts';
import type { BranchNode, Folded, Session, TrunkNode } from '../session/fold.ts';

export function sessionAt(cursor: Cursor): Session {
  return fold(snapshot(SPECIMEN, cursor));
}

/** The trunk node with this id, of this kind, at the cursor. */
export function trunkNodeAt<K extends TrunkNode['kind']>(cursor: Cursor, id: string, kind: K): Extract<TrunkNode, { kind: K }> {
  const session = sessionAt(cursor);
  for (const era of session.eras) {
    for (const node of era.nodes) {
      if (node.id === id && node.kind === kind) return node as Extract<TrunkNode, { kind: K }>;
    }
  }
  throw new Error(`no ${kind} node ${id} at beat ${cursor.beat}${cursor.t === undefined ? '' : ` t=${cursor.t}`}`);
}

export function branchAt(cursor: Cursor, id: string): Folded<BranchNode> {
  for (const list of sessionAt(cursor).branches.values()) {
    const found = list.find((b) => b.id === id);
    if (found) return found;
  }
  throw new Error(`no branch ${id} at beat ${cursor.beat}`);
}

/** Named moments of the specimen, in session order. */
export const MOMENTS = {
  opened: { beat: 1 },
  streaming: { beat: 2, t: 22_000 },
  bigRead: { beat: 2, t: 4_050 },
  idleGapInterview: { beat: 2, t: 28_600 },
  firstSettled: { beat: 2 },
  specSettled: { beat: 3 },
  ratifying: { beat: 4, t: 2_000 },
  refilled: { beat: 4 },
  buildReading: { beat: 5, t: 1_000 },
  testsRunning: { beat: 5, t: 12_500 },
  done: { beat: 5 },
} as const satisfies Record<string, Cursor>;
