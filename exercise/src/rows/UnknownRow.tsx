import { bind } from '../record/bound.ts';
import type { Bound } from '../record/bound.ts';
import type { Containment, Group } from '../record/group.ts';
import { Row, Spacer } from './Row.tsx';

export interface UnknownRowProps {
  readonly sourceKind: Bound<string>;
  readonly raw: Bound<string>;
  readonly containment: Containment;
}

/**
 * A row an adapter kept without having a kind for it: the source's own word
 * and the row verbatim. Evidence about a format this library does not know,
 * shown as the record spells it and not normalised.
 */
export function UnknownRow({ sourceKind, raw, containment }: UnknownRowProps) {
  return (
    <Row
      kind="unknown"
      containment={containment}
      head={
        <>
          <span className="ex-kind">unknown</span>
          <span className="ex-chip ex-chip--record">{sourceKind.value}</span>
          <span className="ex-label">the source's own word for it</span>
          <Spacer />
        </>
      }
      body={<pre className="ex-prose ex-prose--record">{raw.value}</pre>}
    />
  );
}

export function bindUnknown(group: Extract<Group, { kind: 'unknown' }>): UnknownRowProps {
  return { sourceKind: bind(group.head, 'source_kind'), raw: bind(group.head, 'raw'), containment: group.containment };
}
