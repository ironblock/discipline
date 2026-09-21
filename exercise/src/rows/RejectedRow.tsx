import { bind } from '../record/bound.ts';
import type { Bound } from '../record/bound.ts';
import type { Group } from '../record/group.ts';
import { IdChip } from '../fields/IdChip.tsx';
import { LaneBadge } from '../fields/LaneBadge.tsx';
import { Ratio } from '../fields/Ratio.tsx';
import { Row, Spacer } from './Row.tsx';

export interface RejectedRowProps {
  readonly id: Bound<string>;
  readonly lane: Bound<string>;
  readonly atTurn: Bound<number>;
  readonly grounded: Bound<number>;
  readonly of: Bound<number>;
}

/**
 * A lane's output rejected whole by the groundedness floor, with the score
 * so the rejection can be audited. No substrate on the row: a lane is a
 * role on a substrate, and the record inherits it through the lane
 * (`Record::substrate_of`), ruled 2026-09-08.
 */
export function RejectedRow({ id, lane, atTurn, grounded, of }: RejectedRowProps) {
  return (
    <Row
      kind="rejected"
      containment={{ by: 'link', turn: atTurn.value }}
      head={
        <>
          <IdChip id={id.value} kind="rejected" />
          <LaneBadge lane={lane.value} />
          <span className="ex-chip ex-outcome ex-badge--red">rejected whole</span>
          <Spacer />
          <Ratio numerator={grounded.value} denominator={of.value} of="grounded" />
        </>
      }
    />
  );
}

export function bindRejected(group: Extract<Group, { kind: 'rejected' }>): RejectedRowProps {
  return {
    id: bind(group.head, 'id'),
    lane: bind(group.head, 'lane'),
    atTurn: bind(group.head, 'at_turn'),
    grounded: bind(group.head, 'grounded'),
    of: bind(group.head, 'of'),
  };
}
