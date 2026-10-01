/**
 * How a story reaches the surface: a moment of the specimen, placed in the
 * log and folded.
 *
 * Stories never build a node by hand. They stop the specimen at a cursor,
 * fold it with the same `fold()` the app uses, and pick what they show out
 * of the result -- so the brand on every node is honest, and a story shows
 * exactly what the app would at that moment.
 */

import { scriptAt } from '../drive/canned.ts';
import type { Cursor } from '../drive/canned.ts';
import type { LogLine } from '../drive/log.ts';
import { place } from '../drive/place.ts';
import type { Unplaced } from '../drive/script.ts';
import { SPECIMEN } from '../drive/specimen.ts';
import { fold } from '../session/fold.ts';
import type { BranchNode, Folded, Session, TrunkNode } from '../session/fold.ts';

/**
 * A session a story made, and the node id each of the script's labels names
 * in it: its line's `seq`, as the fold keys nodes. `ask/N` names turn N's
 * ask, `settled/N` its settling -- the script never labelled those.
 */
export type Labelled = Session & { readonly idOf: (label: string) => string };

function labelled(script: readonly Unplaced[]): Labelled {
  const { log, labels } = place(script);
  const idOf = (label: string): string => {
    const seq = labels.get(label) ?? byTurn(log, label);
    if (seq === undefined) throw new Error(`no line labelled ${label}`);
    return String(seq);
  };
  return Object.assign(fold(log), { idOf });
}

function byTurn(log: readonly LogLine[], label: string): number | undefined {
  const [kind, n] = label.split('/');
  const turn = Number(n);
  const line = log.find((l) => (kind === 'ask' && l.kind === 'ask' && l.turn === turn) || (kind === 'settled' && l.kind === 'turn.settled' && l.turn === turn));
  return line?.seq;
}

export function sessionAt(cursor: Cursor): Labelled {
  return labelled(scriptAt(SPECIMEN, cursor));
}

/** The node id a label names at the cursor. */
export function idAt(cursor: Cursor, label: string): string {
  return sessionAt(cursor).idOf(label);
}

/** The trunk node a label names, of this kind, at the cursor. */
export function trunkNodeAt<K extends TrunkNode['kind']>(cursor: Cursor, label: string, kind: K): Extract<TrunkNode, { kind: K }> {
  const session = sessionAt(cursor);
  const id = session.idOf(label);
  for (const era of session.eras) {
    for (const node of era.nodes) {
      if (node.id === id && node.kind === kind) return node as Extract<TrunkNode, { kind: K }>;
    }
  }
  throw new Error(`no ${kind} node ${label} at beat ${cursor.beat}${cursor.t === undefined ? '' : ` t=${cursor.t}`}`);
}

export function branchAt(cursor: Cursor, label: string): Folded<BranchNode> {
  const session = sessionAt(cursor);
  const id = session.idOf(label);
  for (const list of session.branches.values()) {
    const found = list.find((b) => b.id === id);
    if (found) return found;
  }
  throw new Error(`no branch ${label} at beat ${cursor.beat}`);
}

/**
 * The specimen at a cursor with its script edited before it is placed and
 * folded: how a story shows what the specimen never says -- a lane, a tool,
 * an outcome this surface does not know -- while every node still comes out
 * of `fold()`. `edit` sees the script's events as plain records, labels and
 * all, since the point is to say things the types do not name.
 */
type Loose = Readonly<Record<string, unknown>>;

export function variantAt(
  cursor: Cursor,
  edit: (event: Loose) => Loose | readonly Loose[],
  /** Events to add after the moment, given the script so far: what happens next, in this variant. */
  append: (script: readonly Loose[]) => readonly Loose[] = () => [],
): Labelled {
  const script = scriptAt(SPECIMEN, cursor).flatMap((e) => edit(e as unknown as Loose));
  return labelled([...script, ...append(script)] as unknown as Unplaced[]);
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
  // After the targeted read has printed (t/3 ends at 1,935).
  buildReading: { beat: 5, t: 2_000 },
  testsRunning: { beat: 5, t: 12_500 },
  done: { beat: 5 },
} as const satisfies Record<string, Cursor>;
