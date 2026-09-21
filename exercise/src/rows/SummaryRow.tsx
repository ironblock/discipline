import { bind, bindAt } from '../record/bound.ts';
import type { Bound } from '../record/bound.ts';
import type { Containment, Group } from '../record/group.ts';
import { Digest } from '../fields/Digest.tsx';
import { Ratio } from '../fields/Ratio.tsx';
import { TokenCount } from '../fields/TokenCount.tsx';
import { Row, Spacer, SubRow } from './Row.tsx';

export type SummaryRowProps =
  | {
      readonly kind: 'drive';
      readonly productSha256: Bound<string>;
      readonly turns: Bound<number>;
      readonly prefillTotal: Bound<number>;
      readonly containment: Containment;
    }
  | {
      readonly kind: 'recompute';
      readonly productSha256: Bound<string>;
      readonly targetsChecked: Bound<number>;
      readonly targetsMatched: Bound<number>;
      readonly digests: Bound<readonly string[]>;
      readonly containment: Containment;
    };

/** The run's totals, in the terms of the kind of run it was. */
export function SummaryRow(props: SummaryRowProps) {
  return (
    <Row
      kind="summary"
      containment={props.containment}
      head={
        <>
          <span className="ex-kind">summary</span>
          <span className="ex-chip">{props.kind}</span>
          <Spacer />
          {props.kind === 'drive' ? (
            <>
              <span className="ex-label">{props.turns.value} turns</span>
              <TokenCount tokens={props.prefillTotal.value} of="prefill" />
            </>
          ) : (
            <Ratio numerator={props.targetsMatched.value} denominator={props.targetsChecked.value} of="matched" />
          )}
          <span className="ex-label">product</span>
          <Digest sha256={props.productSha256.value} />
        </>
      }
    >
      {props.kind === 'recompute' ? (
        <SubRow>
          {props.digests.value.map((d) => (
            <Digest key={d} sha256={d} />
          ))}
        </SubRow>
      ) : null}
    </Row>
  );
}

export function bindSummary(group: Extract<Group, { kind: 'summary' }>): SummaryRowProps {
  const { head, containment } = group;
  const productSha256 = bind(head, 'product_sha256');
  const summary = head.event;
  if (summary.kind === 'drive') {
    return {
      kind: 'drive',
      productSha256,
      turns: bindAt(head, 'turns', summary.turns),
      prefillTotal: bindAt(head, 'prefill_tokens_total', summary.prefill_tokens_total),
      containment,
    };
  }
  return {
    kind: 'recompute',
    productSha256,
    targetsChecked: bindAt(head, 'targets_checked', summary.targets_checked),
    targetsMatched: bindAt(head, 'targets_matched', summary.targets_matched),
    digests: bindAt(head, 'digests', summary.digests),
    containment,
  };
}
