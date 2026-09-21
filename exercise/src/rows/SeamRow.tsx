import { bind } from '../record/bound.ts';
import type { Bound } from '../record/bound.ts';
import type { Group } from '../record/group.ts';
import { ByteSize } from '../fields/ByteSize.tsx';
import { IdChip } from '../fields/IdChip.tsx';
import { Pending } from '../fields/Pending.tsx';
import { Row, Spacer, SubRow } from './Row.tsx';

export interface SeamRowProps {
  readonly id: Bound<string>;
  readonly atTurn: Bound<number>;
  readonly renderedBytes: Bound<number>;
}

/**
 * A seam: the working object rendered into a fresh prompt. The event is
 * exactly `id`, `at_turn`, `rendered_bytes`, and this row shows exactly
 * that. Its reason, the prefix hashes either side, the ratification answer
 * and the object diff are the most-cited gaps in the record (`#92.1`,
 * `#92.3`, and the replay question on #78), so the row is mostly dotted.
 */
export function SeamRow({ id, atTurn, renderedBytes }: SeamRowProps) {
  return (
    <Row
      kind="seam"
      containment={{ by: 'link', turn: atTurn.value }}
      head={
        <>
          <IdChip id={id.value} kind="seam" />
          <span className="ex-label">at turn {atTurn.value}</span>
          <Pending issue="#92.1" atom="seam.reason" why="cadence | phase | budget | operator -- ruled on #27, never landed" />
          <Spacer />
          <span className="ex-label">rendered</span>
          <ByteSize bytes={renderedBytes.value} />
        </>
      }
    >
      <SubRow pending>
        <Pending issue="#92.1" atom="seam.prefix_hash_before → prefix_hash_after" why="the head fingerprint either side" />
        <Pending issue="#92.3" atom="seam.answer" why="the ratification answer the drive already keeps" />
        <Pending issue="unfiled/object-dumps" atom="object dump diff" why="no path from a record to a working object; the diff is computed in diet once there is one" />
      </SubRow>
    </Row>
  );
}

export function bindSeam(group: Extract<Group, { kind: 'seam' }>): SeamRowProps {
  return { id: bind(group.head, 'id'), atTurn: bind(group.head, 'at_turn'), renderedBytes: bind(group.head, 'rendered_bytes') };
}
