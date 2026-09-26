import type { ReactNode } from 'react';

import type { ForkLane } from '../drive/events.ts';
import { laneStyle } from './sets.ts';
import { useSurface } from './surface.tsx';
import './block.css';

/** A block's fill: a role on the trunk (closed), or a lane beside it -- which lane is `lane`, an open set. */
export type Tone = 'system' | 'user' | 'assistant' | 'tool' | 'lane';

export interface Stat {
  readonly value: ReactNode;
  /** A short label after the value: `tok`, `t/s`. */
  readonly unit?: string;
  /** What the number is, on hover. */
  readonly title?: string;
}

export interface BlockProps {
  readonly tone: Tone;
  /** For tone `lane`: which. Coloured from the lane registry; an unknown lane is neutral. */
  readonly lane?: ForkLane;
  /** The footer's first chip: the role or lane, in the harness's words. */
  readonly label: string;
  readonly stats?: readonly (Stat | false | undefined)[];
  /** Where this block came from, and what it waits on. From a folded node. */
  readonly provenance: { readonly from: readonly number[]; readonly needs: readonly string[] };
  /** Thin: a bar with a footer and no body, for a harness step. */
  readonly thin?: boolean;
  readonly live?: boolean;
  readonly id?: string;
  /** Per-block actions -- copy today; retry, annotate, link later -- shown in the corner on hover or focus. */
  readonly actions?: ReactNode;
  readonly children?: ReactNode;
}

/**
 * The session event: one block, filled by role, with a low-contrast mono
 * footer of what the harness measured. Every message, tool call and lane
 * step on the surface is one of these, refined.
 */
export function Block({ tone, lane, label, stats = [], provenance, thin = false, live = false, id, actions, children }: BlockProps) {
  const { curtain } = useSurface();
  const shown = stats.filter((s): s is Stat => Boolean(s));
  return (
    <div
      className={`ex-block ex-block--${tone}${thin ? ' ex-block--thin' : ''}${live ? ' ex-block--live' : ''}`}
      data-tone={tone}
      data-lane={lane}
      style={laneStyle(lane)}
      data-id={id}
      data-from={provenance.from.join(' ')}
      data-needs={provenance.needs.join(' ')}
    >
      {children !== undefined && !thin ? <div className="ex-block__body">{children}</div> : null}
      <footer className="ex-block__foot">
        <span className="ex-block__label">{label}</span>
        {thin && children !== undefined ? <span className="ex-block__inline">{children}</span> : null}
        <span className="ex-block__spacer" />
        {shown.map((s, i) => (
          <span className="ex-stat" key={i} title={s.title}>
            {s.value}
            {s.unit ? <span className="ex-stat__unit">{s.unit}</span> : null}
          </span>
        ))}
      </footer>
      {actions !== undefined || curtain ? (
        <div className="ex-block__corner">
          {actions}
          {curtain ? (
            <span className="ex-cite" title="log positions this block was folded from">
              #{provenance.from.join(' #')}
            </span>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
