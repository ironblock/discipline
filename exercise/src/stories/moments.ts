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
import type { DriveEvent } from '../drive/events.ts';
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

/**
 * The specimen at a cursor with its log edited before folding: how a story
 * shows what the specimen never says -- a lane, a tool, an outcome this
 * surface does not know -- while every node still comes out of `fold()`.
 * `edit` sees events as plain records, since the point is to say things the
 * types do not name.
 */
type Loose = Readonly<Record<string, unknown>>;

export function variantAt(
  cursor: Cursor,
  edit: (event: Loose) => Loose | readonly Loose[],
  /** Events to add after the moment, given the log so far: what happens next, in this variant. */
  append: (log: readonly Loose[]) => readonly Loose[] = () => [],
): Session {
  const log = snapshot(SPECIMEN, cursor).flatMap((e) => edit(e as unknown as Loose));
  return fold([...log, ...append(log)].map((e, seq) => ({ ...e, seq }) as unknown as DriveEvent));
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
