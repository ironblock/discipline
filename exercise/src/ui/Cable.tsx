import type { CSSProperties } from 'react';

import { cable } from './cable.ts';
import './cable.css';

/** Room around the path, so a round cap, a port or a glow is not clipped at the box's edge. */
const PAD = 6;

/**
 * The cable from a trunk node into a side call off it: `reach` across the
 * gutter and `drop` down, placed so that (0, 0) is where it leaves the
 * trunk. How it looks -- its weight, its colour at rest, whether it ends in
 * an arrow or in two ports, how it glows -- is the theme's (`--cable-*`);
 * while the side call runs, light travels down it, from the trunk it read
 * to the slot doing the work.
 */
export function Cable({
  reach,
  drop,
  live,
  pending = false,
  top,
  style,
}: {
  readonly reach: number;
  readonly drop: number;
  readonly live: boolean;
  /** Its side call is waiting for its slot: the cable is drawn, dashed, and nothing travels it yet. */
  readonly pending?: boolean;
  readonly top: number;
  readonly style?: CSSProperties;
}) {
  if (reach <= 0) return null;
  const d = cable(reach, drop);
  return (
    <svg
      className="ex-cable"
      aria-hidden="true"
      width={reach}
      height={drop + 2 * PAD}
      viewBox={`0 ${-PAD} ${reach} ${drop + 2 * PAD}`}
      style={{ left: -reach, top: top - PAD, ...style }}
      data-live={live ? '' : undefined}
      data-pending={pending ? '' : undefined}
    >
      <path className="ex-cable__line" d={d} />
      <path className="ex-cable__pulse" d={d} pathLength={100} />
      <path className="ex-cable__arrow" d={`M${reach - 5} ${drop - 3.5}L${reach} ${drop}L${reach - 5} ${drop + 3.5}`} />
      <circle className="ex-cable__port" cx={0} cy={0} r={2.25} />
      <circle className="ex-cable__port" cx={reach} cy={drop} r={2.25} />
    </svg>
  );
}
