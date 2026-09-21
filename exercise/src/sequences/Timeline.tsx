import { groupEvents } from '../record/group.ts';
import type { Group } from '../record/group.ts';
import type { Loaded } from '../record/load.ts';
import { ClaimRow, bindClaim } from '../rows/ClaimRow.tsx';
import { ExchangeRow, bindExchange } from '../rows/ExchangeRow.tsx';
import { ForkRow, bindFork } from '../rows/ForkRow.tsx';
import { RejectedRow, bindRejected } from '../rows/RejectedRow.tsx';
import { SeamRow, bindSeam } from '../rows/SeamRow.tsx';
import { StartRow, bindStart } from '../rows/StartRow.tsx';
import { SummaryRow, bindSummary } from '../rows/SummaryRow.tsx';
import { ToolCallRow, bindToolCall } from '../rows/ToolCallRow.tsx';
import { TurnRow, bindTurn } from '../rows/TurnRow.tsx';
import { UnknownRow, bindUnknown } from '../rows/UnknownRow.tsx';
import '../rows/rows.css';

export interface TimelineProps {
  readonly loaded: Loaded;
  /**
   * The event count above which the list virtualizes. DECLARED, never
   * adaptive, and stated on the rail. Virtualization itself is not built in
   * this catalog: no record in reach crosses any sensible threshold (the
   * largest is 24 events), and #31's 5,000-event row waits on a generator
   * that belongs in diet/src/drive/. The rail says so rather than claiming
   * an engaged window it never had.
   */
  readonly virtualizeAbove?: number;
}

/** One group, as its row. Exhaustive over `Group['kind']`. */
export function GroupRow({ group }: { readonly group: Group }) {
  switch (group.kind) {
    case 'start':
      return <StartRow {...bindStart(group)} />;
    case 'turn':
      return <TurnRow {...bindTurn(group)} />;
    case 'exchange':
      return <ExchangeRow {...bindExchange(group)} />;
    case 'fork':
      return <ForkRow {...bindFork(group)} />;
    case 'seam':
      return <SeamRow {...bindSeam(group)} />;
    case 'tool_call':
      return <ToolCallRow {...bindToolCall(group)} />;
    case 'rejected':
      return <RejectedRow {...bindRejected(group)} />;
    case 'claim':
      return <ClaimRow {...bindClaim(group)} />;
    case 'summary':
      return <SummaryRow {...bindSummary(group)} />;
    case 'unknown':
      return <UnknownRow {...bindUnknown(group)} />;
  }
}

/**
 * The reading spine: a loaded record as rows, in canonical order, with turn
 * rows as section headers. The same loader the app will use feeds it;
 * nothing here is hand-assembled.
 */
export function Timeline({ loaded, virtualizeAbove = 2000 }: TimelineProps) {
  const groups = groupEvents(loaded.events);
  const events = loaded.events.length;
  const turns = loaded.events.filter((p) => p.event.record === 'turn').length;
  return (
    <section className="ex-timeline" data-events={events}>
      <div className="ex-timeline__rail" role="status">
        <span>
          <strong>{events}</strong> events
        </span>
        <span>
          <strong>{groups.length}</strong> rows
        </span>
        <span>
          <strong>{turns}</strong> turns
        </span>
        <span>
          arm <strong>{loaded.regime.arm}</strong>
        </span>
        <span>
          source <strong>{loaded.source.kind}</strong>
        </span>
        <span>
          virtualizes above <strong>{virtualizeAbove}</strong> · {events > virtualizeAbove ? 'would engage — not built' : 'not engaged'}
        </span>
      </div>
      {groups.map((group) => (
        <GroupRow key={groupKey(group)} group={group} />
      ))}
    </section>
  );
}

function groupKey(group: Group): string {
  switch (group.kind) {
    case 'exchange':
      return `x:${group.requests[0]?.index ?? -1}`;
    case 'claim':
      return `c:${group.chain[0]?.index ?? -1}`;
    default:
      return `${group.kind}:${group.head.index}`;
  }
}
