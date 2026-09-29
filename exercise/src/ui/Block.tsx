import type { CSSProperties, ReactNode } from 'react';

import type { ForkLane } from '../drive/log.ts';
import { laneStyle } from './sets.ts';
import { useSurface, useTarget } from './surface.tsx';
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
  /** The role or lane, in the harness's words: the header's chip. */
  readonly label: string;
  /** What went in, in a line: after the chip -- tokens read, or an ask's new tokens. The header's, as `output` is the footer's. */
  readonly input?: ReactNode;
  /** What came out, in a line: the footer's first -- tokens written, or a tool's lines and bytes, and how long. */
  readonly output?: ReactNode;
  /** What came out: the footer's numbers, after the line. No footer without one or the other. */
  readonly stats?: readonly (Stat | false | undefined)[];
  /** Where this block came from, and what it waits on. From a folded node. */
  readonly provenance: { readonly from: readonly number[]; readonly needs: readonly string[] };
  /** Thin: one row -- chip, numbers -- and no body, for a harness step. */
  readonly thin?: boolean;
  readonly live?: boolean;
  /**
   * What the block read before it wrote, along its top edge: the new part of
   * the prompt as a bar filling as it is read (absent while nothing is
   * known: light sweeps instead). While it reads, the bottom edge rests.
   */
  readonly intake?: { readonly reading: boolean; readonly edge?: { readonly read: number } | undefined };
  /** Something here went wrong, and how badly: what the minimap marks. */
  readonly alarm?: 'warn' | 'bad' | undefined;
  /** The node's id: its DOM id too, so `#<id>` links to it. */
  readonly id?: string;
  /** Per-block actions -- copy today; retry, annotate, link later -- at the header's right end, on hover or focus. */
  readonly actions?: ReactNode;
  readonly children?: ReactNode;
}

/**
 * The session event: one block, filled by role. Its header says who and
 * what went in -- the role's chip, and what it read -- and its footer what
 * came out, in low-contrast mono. Every message, tool call and lane step on
 * the surface is one of these, refined.
 */
export function Block({ tone, lane, label, input, output, stats = [], provenance, thin = false, live = false, intake, alarm, id, actions, children }: BlockProps) {
  const { curtain, gaps } = useSurface();
  const target = useTarget();
  const outputs = stats.filter((s): s is Stat => Boolean(s));
  const chip = <span className="ex-block__label">{label}</span>;
  const line =
    input !== undefined ? (
      <span className="ex-block__flow" data-at="head" role={intake?.reading ? 'status' : undefined}>
        {input}
      </span>
    ) : null;
  const out = output !== undefined ? <span className="ex-block__flow" data-at="foot">{output}</span> : null;
  // The header's right end: what you can do with the block and where it came from, on hover or focus; and,
  // with the gaps overlay on, what `diet` cannot emit yet that it was drawn from.
  const needs = gaps && provenance.needs.length > 0 ? <span className="ex-block__needs">needs {provenance.needs.join(' ')}</span> : null;
  const corner =
    actions !== undefined || curtain ? (
      <span className="ex-block__corner" ref={cornered}>
        {actions}
        {curtain ? (
          <span className="ex-cite" title="log positions this block was folded from">
            #{provenance.from.join(' #')}
          </span>
        ) : null}
      </span>
    ) : null;
  return (
    <div
      className={`ex-block ex-block--${tone}${thin ? ' ex-block--thin' : ''}${live ? ' ex-block--live' : ''}`}
      data-tone={tone}
      data-lane={lane}
      data-alarm={alarm}
      style={laneStyle(lane)}
      data-reading={intake?.reading ? '' : undefined}
      id={id}
      data-id={id}
      data-target={id !== undefined && id === target ? '' : undefined}
      data-from={provenance.from.join(' ')}
      data-needs={provenance.needs.join(' ')}
    >
      {intake ? (
        <span
          className="ex-block__edge"
          aria-hidden="true"
          data-reading={intake.reading ? '' : undefined}
          data-unknown={intake.edge ? undefined : ''}
          style={intake.edge ? ({ '--read': intake.edge.read } as CSSProperties) : undefined}
        />
      ) : null}
      {thin ? (
        <footer className="ex-block__foot">
          {chip}
          {line}
          {children !== undefined ? <span className="ex-block__inline">{children}</span> : null}
          {out}
          <span className="ex-block__spacer" />
          {outputs.map(stat)}
          {needs}
        </footer>
      ) : (
        <>
          <header className="ex-block__head">
            {chip}
            {line}
            {corner}
            {needs}
          </header>
          {children !== undefined ? <div className="ex-block__body">{children}</div> : null}
          {out || outputs.length > 0 ? (
            <footer className="ex-block__foot">
              {out}
              {out ? <span className="ex-block__spacer" /> : null}
              {outputs.map(stat)}
            </footer>
          ) : null}
        </>
      )}
    </div>
  );
}

/**
 * How wide each block's corner is, on its header as `--corner-w`: the header's
 * line fades out under it (block.css) rather than the corner covering it
 * with a box. One observer for every corner on the page.
 */
const corners =
  typeof ResizeObserver === 'undefined'
    ? undefined
    : new ResizeObserver((entries) => {
        for (const entry of entries) (entry.target.parentElement as HTMLElement | null)?.style.setProperty('--corner-w', `${Math.ceil(entry.borderBoxSize[0]?.inlineSize ?? 0)}px`);
      });

function cornered(el: HTMLElement | null) {
  if (!el || !corners) return;
  corners.observe(el);
  return () => corners.unobserve(el);
}

function stat(s: Stat, i: number) {
  return (
    <span className="ex-stat" key={i} title={s.title}>
      {s.value}
      {s.unit ? <span className="ex-stat__unit">{s.unit}</span> : null}
    </span>
  );
}
