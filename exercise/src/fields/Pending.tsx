import type { ReactNode } from 'react';
import './fields.css';

export interface PendingProps {
  /** `#92.4`, or `unfiled/<slug>` while the issue is only drafted. */
  readonly issue: string;
  /** The atom, as the record would spell it: `fork.outcome`. */
  readonly atom: string;
  /** One line on why the viewer needs it, for the block form. */
  readonly why?: string;
  /** Inline in a row header, or a block standing where the row would be. */
  readonly as?: 'inline' | 'block';
  readonly children?: ReactNode;
}

/**
 * The shape an atom would take, dotted, naming the issue it waits on.
 *
 * v3's disclosure habit, kept as a component: a field the record does not
 * carry is drawn as a placeholder and filed, never faked. Every `Pending`
 * in the catalog has a twin line in `src/record/pending.types.ts` and a
 * twin fixture in `fixtures/pending/`; the issue string is the join.
 */
export function Pending({ issue, atom, why, as = 'inline', children }: PendingProps) {
  if (as === 'block') {
    return (
      <div className="ex-pending ex-pending--block" data-pending={issue} role="note">
        <span className="ex-pending__issue">pending {issue}</span>
        <span className="ex-pending__atom">{atom}</span>
        {why ? <span className="ex-pending__why">{why}</span> : null}
        {children}
      </div>
    );
  }
  return (
    <span className="ex-pending" data-pending={issue} title={why}>
      <span className="ex-pending__issue">{issue}</span>
      <span className="ex-pending__atom">{atom}</span>
      {children}
    </span>
  );
}
