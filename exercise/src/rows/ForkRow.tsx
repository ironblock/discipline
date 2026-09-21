import { bind } from '../record/bound.ts';
import type { Bound } from '../record/bound.ts';
import type { Group } from '../record/group.ts';
import { IdChip } from '../fields/IdChip.tsx';
import { LaneBadge } from '../fields/LaneBadge.tsx';
import { Link } from '../fields/Link.tsx';
import { Pending } from '../fields/Pending.tsx';
import { SubstrateBadge } from '../fields/SubstrateBadge.tsx';
import { Row, Spacer, SubRow } from './Row.tsx';

export interface BoundCapture {
  readonly id: Bound<string>;
  readonly fromFork: Bound<string>;
  /** `entries` in a v0 record means entries_touched. */
  readonly entries: Bound<number>;
}

export interface ForkRowProps {
  readonly id: Bound<string>;
  readonly lane: Bound<string>;
  readonly substrate: Bound<string>;
  readonly ofTurn: Bound<number>;
  readonly captures: readonly BoundCapture[];
}

/**
 * A fork and its captures: one row, because `capture.from_fork` is a link
 * the record validates. Depth-1 is an invariant, so the fork is a property
 * of its turn -- containment, not an edge.
 *
 * What this row cannot show, drawn dotted: the fork's outcome (`#92.4`,
 * two vocabularies), the fork's own request and response (joined by row
 * order and lane only -- they render as the next exchange row), and
 * `entries_created` beside the touched count the record has.
 */
export function ForkRow({ id, lane, substrate, ofTurn, captures }: ForkRowProps) {
  return (
    <Row
      kind="fork"
      containment={{ by: 'link', turn: ofTurn.value }}
      head={
        <>
          <IdChip id={id.value} kind="fork" />
          <LaneBadge lane={lane.value} />
          <SubstrateBadge id={substrate.value} />
          <span className="ex-label">of turn {ofTurn.value}</span>
          <Pending issue="#92.4" atom="fork.outcome" why="the typed outcome is recoverable only from the answer; two vocabularies are candidates" />
          <Spacer />
          {captures.length === 0 ? <span className="ex-label">no capture</span> : null}
        </>
      }
    >
      <SubRow pending>
        <Pending issue="unfiled/fork-request-link" atom="request.of_fork" why="the fork's own request and response follow it by row order and lane; nothing validates the join" />
        <span className="ex-label">its exchange is the next row</span>
      </SubRow>
      {captures.map((capture) => (
        <SubRow key={capture.id.value}>
          <IdChip id={capture.id.value} kind="capture" />
          <Link field="from_fork" to={capture.fromFork.value} wants="fork" />
          <span className="ex-num">
            {capture.entries.value}
            <span className="ex-unit">{capture.entries.value === 1 ? 'entry touched' : 'entries touched'}</span>
          </span>
          <Pending issue="unfiled/entries-created" atom="capture.entries_created" why="the drive computes both counts; the record holds one" />
        </SubRow>
      ))}
    </Row>
  );
}

export function bindFork(group: Extract<Group, { kind: 'fork' }>): ForkRowProps {
  return {
    id: bind(group.head, 'id'),
    lane: bind(group.head, 'lane'),
    substrate: bind(group.head, 'substrate'),
    ofTurn: bind(group.head, 'of_turn'),
    captures: group.captures.map((c) => ({ id: bind(c, 'id'), fromFork: bind(c, 'from_fork'), entries: bind(c, 'entries') })),
  };
}
