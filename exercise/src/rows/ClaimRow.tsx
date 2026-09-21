import { bind, bindOptional } from '../record/bound.ts';
import type { Bound } from '../record/bound.ts';
import type { Containment, Group } from '../record/group.ts';
import type { Artifact, Verdict } from '../record/types.ts';
import { VerdictBadge } from '../fields/Badge.tsx';
import { Digest } from '../fields/Digest.tsx';
import { IdChip } from '../fields/IdChip.tsx';
import { Link } from '../fields/Link.tsx';
import { Row, Spacer, SubRow } from './Row.tsx';

export interface BoundClaim {
  readonly id: Bound<string>;
  readonly hypothesis: Bound<string>;
  readonly result: Bound<Verdict>;
  readonly consumes: Bound<readonly Artifact[]>;
  readonly supersedes?: Bound<string>;
}

export interface ClaimRowProps {
  /** The claim and each correction of it, oldest first. Never empty. */
  readonly chain: readonly BoundClaim[];
  readonly containment: Containment;
}

/**
 * One hypothesis, one result, and a correction chain: `supersedes` is a
 * validated link, so a corrected claim and its correction are one row. The
 * superseded claim stays visible and struck -- evict to history, never
 * delete -- and the current one carries the verdict.
 */
export function ClaimRow({ chain, containment }: ClaimRowProps) {
  const current = chain[chain.length - 1];
  if (!current) throw new Error('a claim row with no claim');
  const history = chain.slice(0, -1);
  return (
    <Row
      kind="claim"
      containment={containment}
      head={
        <>
          <IdChip id={current.id.value} kind="claim" />
          <VerdictBadge verdict={current.result.value} />
          {current.supersedes ? <Link field="supersedes" to={current.supersedes.value} wants="claim" /> : null}
          <Spacer />
          <span className="ex-label">
            consumes {current.consumes.value.length} artifact{current.consumes.value.length === 1 ? '' : 's'}
          </span>
        </>
      }
      body={<p className="ex-prose ex-prose--ask">{current.hypothesis.value}</p>}
    >
      <SubRow>
        {current.consumes.value.map((artifact) => (
          <span className="ex-kv" key={artifact.path}>
            <span className="ex-kv__k">{artifact.path}</span>
            <Digest sha256={artifact.sha256} />
          </span>
        ))}
      </SubRow>
      {history.map((claim) => (
        <SubRow key={claim.id.value}>
          <IdChip id={claim.id.value} kind="claim" />
          <span className="ex-superseded">{claim.hypothesis.value}</span>
          <span className="ex-superseded">{claim.result.value}</span>
          <span className="ex-label">superseded</span>
        </SubRow>
      ))}
    </Row>
  );
}

export function bindClaim(group: Extract<Group, { kind: 'claim' }>): ClaimRowProps {
  return {
    chain: group.chain.map((placed) => {
      const supersedes = bindOptional(placed, 'supersedes');
      return {
        id: bind(placed, 'id'),
        hypothesis: bind(placed, 'hypothesis'),
        result: bind(placed, 'result'),
        consumes: bind(placed, 'consumes'),
        ...(supersedes ? { supersedes } : {}),
      };
    }),
    containment: group.containment,
  };
}
