import type { CSSProperties } from 'react';

import type { Pt } from './harness.ts';
import { laneStyle } from './sets.ts';
import './cable.css';

/** One trunk node's cables into one slot, routed and drawn (harness.ts `cabling`). */
export interface Cabled {
  readonly key: string;
  readonly lane: string;
  readonly pending: boolean;
  readonly d: string;
  readonly dots: readonly Pt[];
  readonly source: Pt;
  readonly pins: readonly Pt[];
  /** Each side call's own cable, and whether it is running. */
  readonly wires: readonly { readonly id: string; readonly lane: string; readonly d: string; readonly live: boolean }[];
}

/**
 * The trunk's cables as a harness, over the stage, in its coordinates: each
 * net drawn at rest as the theme draws a cable (`--cable-*`, its ports or
 * its arrows), and the cable of each side call still running drawn over it
 * whole, lit, with light travelling it from the trunk.
 */
export function Wiring({ nets }: { readonly nets: readonly Cabled[] }) {
  return (
    <svg className="ex-wiring" aria-hidden="true">
      {nets.map((net) => (
        <g key={net.key} className="ex-cable" data-net={net.key} data-pending={net.pending ? '' : undefined} style={laneStyle(net.lane) as CSSProperties}>
          <path className="ex-cable__line" d={net.d} />
          {net.dots.map((p) => (
            <circle key={`${p.x} ${p.y}`} className="ex-cable__dot" cx={p.x} cy={p.y} r={2} />
          ))}
          <circle className="ex-cable__port" cx={net.source.x} cy={net.source.y} r={2.25} />
          {net.pins.map((p) => (
            <g key={`${p.x} ${p.y}`}>
              <path className="ex-cable__arrow" d={`M${p.x - 5} ${p.y - 3.5}L${p.x} ${p.y}L${p.x - 5} ${p.y + 3.5}`} />
              <circle className="ex-cable__port" cx={p.x} cy={p.y} r={2.25} />
            </g>
          ))}
        </g>
      ))}
      {nets.flatMap((net) =>
        net.wires
          .filter((w) => w.live)
          .map((w) => (
            <g key={w.id} className="ex-cable" data-live="" data-from={w.id} style={laneStyle(w.lane) as CSSProperties}>
              <path className="ex-cable__line" d={w.d} />
              <path className="ex-cable__pulse" d={w.d} pathLength={100} />
            </g>
          )),
      )}
    </svg>
  );
}
