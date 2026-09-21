import { bind, bindOptional } from '../record/bound.ts';
import type { Bound } from '../record/bound.ts';
import type { Containment, Group } from '../record/group.ts';
import { ByteSize } from '../fields/ByteSize.tsx';
import { IdChip } from '../fields/IdChip.tsx';
import { LaneBadge } from '../fields/LaneBadge.tsx';
import { Link } from '../fields/Link.tsx';
import { Pending } from '../fields/Pending.tsx';
import { SubstrateBadge } from '../fields/SubstrateBadge.tsx';
import { TokenCount } from '../fields/TokenCount.tsx';
import { Row, Spacer, SubRow } from './Row.tsx';

export interface BoundRequest {
  readonly id: Bound<string>;
  readonly lane: Bound<string>;
  readonly substrate: Bound<string>;
  readonly retryOf?: Bound<string>;
  /** The whole wire body, when the record is an archive. Opaque here. */
  readonly text?: Bound<string>;
}

export interface BoundResponse {
  readonly id: Bound<string>;
  readonly toRequest: Bound<string>;
  readonly outputTokens: Bound<number>;
  /** Present and empty is a typed outcome: an answer of nothing. */
  readonly text?: Bound<string>;
}

export interface ExchangeRowProps {
  /** The request and its retries, in row order. Never empty. */
  readonly requests: readonly BoundRequest[];
  readonly response?: BoundResponse;
  readonly containment: Containment;
}

/**
 * A request, its retry chain and its response: one row, because
 * `retry_of` and `to_request` are links the record validates.
 *
 * Three things this row does on purpose. It does NOT parse `request.text`:
 * the wire body is shown as a size and a raw view, and the operator's ask
 * inside it is a pending atom (`unfiled/operator-ask`) -- reading the
 * messages array out of it would be a second parser. It renders an empty
 * `response.text` as the typed outcome it is. And a chain with no response
 * is a lane still running, which is the state the drive input derives from.
 */
export function ExchangeRow({ requests, response, containment }: ExchangeRowProps) {
  const first = requests[0];
  if (!first) throw new Error('an exchange row with no request is not an exchange');
  const retries = requests.slice(1);
  const answered = response !== undefined;
  const canonical = first.lane.value === 'main';
  return (
    <Row
      kind="exchange"
      containment={containment}
      head={
        <>
          <IdChip id={first.id.value} kind="request" />
          <LaneBadge lane={first.lane.value} />
          <SubstrateBadge id={first.substrate.value} />
          {first.text ? (
            <span className="ex-label" title="request.text is the whole wire body; the ask inside it is not a field">
              wire body <ByteSize bytes={new TextEncoder().encode(first.text.value).length} />
            </span>
          ) : (
            <span className="ex-label">ledger · text not kept</span>
          )}
          {canonical ? <Pending issue="unfiled/operator-ask" atom="request.ask" why="the ask is inside request.text; a diet read-side projection would surface it" /> : null}
          {containment.by === 'position' ? (
            <Pending issue="unfiled/request-turn-link" atom="request.at_turn" why="a canonical request is joined to its turn by row order only" />
          ) : null}
          <Spacer />
          {answered ? (
            <TokenCount tokens={response.outputTokens.value} of="output" />
          ) : (
            <span className="ex-chip ex-outcome ex-badge--amber" data-field="lane-state">
              lane running · no response yet
            </span>
          )}
        </>
      }
      body={answered ? <ResponseBody response={response} canonical={canonical} /> : undefined}
    >
      {retries.map((retry) => (
        <SubRow key={retry.id.value}>
          <IdChip id={retry.id.value} kind="request" />
          {retry.retryOf ? <Link field="retry_of" to={retry.retryOf.value} wants="request" /> : null}
          <span className="ex-label">retry</span>
        </SubRow>
      ))}
      {answered ? (
        <SubRow>
          <IdChip id={response.id.value} kind="response" />
          <Link field="to_request" to={response.toRequest.value} wants="request" />
        </SubRow>
      ) : null}
    </Row>
  );
}

function ResponseBody({ response, canonical }: { readonly response: BoundResponse; readonly canonical: boolean }) {
  if (!response.text) return <span className="ex-label">ledger · text not kept</span>;
  if (response.text.value === '') return <span className="ex-empty">empty answer — a typed outcome, not a missing one</span>;
  // The canonical lane's answer is the model's prose for the turn; an
  // interview lane's is a tagged answer. Both are the model speaking, so both
  // take the answer face; the interview grammar's tags are left as written.
  return <pre className={canonical ? 'ex-prose ex-prose--answer' : 'ex-prose ex-prose--answer'}>{response.text.value}</pre>;
}

export function bindExchange(group: Extract<Group, { kind: 'exchange' }>): ExchangeRowProps {
  const requests = group.requests.map((placed): BoundRequest => {
    const retryOf = bindOptional(placed, 'retry_of');
    const text = bindOptional(placed, 'text');
    return {
      id: bind(placed, 'id'),
      lane: bind(placed, 'lane'),
      substrate: bind(placed, 'substrate'),
      ...(retryOf ? { retryOf } : {}),
      ...(text ? { text } : {}),
    };
  });
  const response = group.response;
  if (!response) return { requests, containment: group.containment };
  const text = bindOptional(response, 'text');
  return {
    requests,
    response: {
      id: bind(response, 'id'),
      toRequest: bind(response, 'to_request'),
      outputTokens: bind(response, 'output_tokens'),
      ...(text ? { text } : {}),
    },
    containment: group.containment,
  };
}
