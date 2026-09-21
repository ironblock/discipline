import type { ReactNode } from 'react';

import type { Containment } from '../record/group.ts';
import type { Kind } from '../record/types.ts';
import './rows.css';

export interface RowProps {
  readonly kind: Kind | 'exchange';
  readonly containment: Containment;
  /** The header: chips, badges, links. Only what is anomalous. */
  readonly head: ReactNode;
  /** Prose or a size. Never inlined tool output. */
  readonly body?: ReactNode;
  /** Member rows absorbed into this group: retries, captures, corrections. */
  readonly children?: ReactNode;
  readonly className?: string;
}

/** The frame every row shares: a containment gutter and a header. */
export function Row({ kind, containment, head, body, children, className }: RowProps) {
  return (
    <article
      className={['ex-row', className].filter(Boolean).join(' ')}
      data-kind={kind}
      data-containment={containment.by}
      data-turn={containment.by === 'none' ? undefined : containment.turn}
    >
      <div className="ex-row__gutter" aria-hidden="true" />
      <header className="ex-row__head">{head}</header>
      {body ? <div className="ex-row__body">{body}</div> : null}
      {children}
    </article>
  );
}

/** A sub-row under the head: a retry, a capture, a correction. */
export function SubRow({ children, pending = false }: { readonly children: ReactNode; readonly pending?: boolean }) {
  return <div className={pending ? 'ex-row__sub ex-row__sub--pending' : 'ex-row__sub'}>{children}</div>;
}

export function Spacer() {
  return <span className="ex-spacer" />;
}
