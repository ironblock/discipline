import { bind } from '../record/bound.ts';
import type { Bound } from '../record/bound.ts';
import type { Group } from '../record/group.ts';
import { TokenCount } from '../fields/TokenCount.tsx';
import { Row } from './Row.tsx';

export interface TurnRowProps {
  readonly index: Bound<number>;
  readonly prefill: Bound<number>;
}

/**
 * A turn: the section header of the reading spine. `prefill_tokens` is the
 * one number on it, because it is the number the program is about.
 */
export function TurnRow({ index, prefill }: TurnRowProps) {
  return (
    <Row
      kind="turn"
      className="ex-row--turn"
      containment={{ by: 'link', turn: index.value }}
      head={
        <>
          <span className="ex-kind">turn</span>
          <strong style={{ fontWeight: 'normal' }}>{index.value}</strong>
          <span className="ex-spacer" />
          <TokenCount tokens={prefill.value} of="prefill" />
        </>
      }
    />
  );
}

export function bindTurn(group: Extract<Group, { kind: 'turn' }>): TurnRowProps {
  return { index: bind(group.head, 'index'), prefill: bind(group.head, 'prefill_tokens') };
}
