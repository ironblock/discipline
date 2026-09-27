import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { CSSProperties } from 'react';

import { draw, HARNESS, route } from './harness.ts';
import type { Net } from './harness.ts';
import { link } from './links.ts';
import type { Box } from './links.ts';
import { ENTER } from './placement.ts';
import { laneStyle, opOf } from './sets.ts';
import type { Surface } from './surface.tsx';
import './links.css';

/** One patch: the side call that landed it, and the working-memory entry it touched. */
export interface Wire {
  readonly branch: string;
  readonly entry: string;
  readonly lane: string;
  readonly op: string;
}

interface Drawn {
  readonly key: string;
  readonly wire: Wire;
  readonly d: string;
  readonly clipped: boolean;
}

/** A side call's lines as one harness net: its runs, its stubs off the panel's edge, where it forks. */
interface DrawnNet {
  readonly branch: string;
  readonly lane: string;
  readonly d: string;
  readonly clipped: string;
  readonly dots: readonly { readonly x: number; readonly y: number }[];
}

/**
 * What each side call wrote, as a line from it into working memory: one per
 * patch, for the side calls on screen, drawn over the page in viewport
 * coordinates and redrawn as the page or memory's panel scrolls. Faint at
 * rest; lit when either end is `hot` (pointed at, or the address's target).
 * An entry scrolled out of memory's panel is reached at the panel's edge,
 * dashed. Nothing is drawn into a shut drawer.
 *
 * With `wiring`, the lines are routed as a harness instead (harness.ts):
 * each side call's lines share a track in the gutter before memory, faint
 * as one net, and the lines of whatever is lit are drawn over it whole.
 */
export function Links({
  wires,
  hot,
  wiring,
  revision,
}: {
  readonly wires: readonly Wire[];
  readonly hot: { readonly branches: ReadonlySet<string>; readonly entries: ReadonlySet<string> };
  readonly wiring?: Surface['wiring'];
  readonly revision: readonly unknown[];
}) {
  const [drawn, setDrawn] = useState<readonly Drawn[]>([]);
  const [nets, setNets] = useState<readonly DrawnNet[]>([]);
  const frame = useRef(0);

  const measure = useCallback(() => {
    cancelAnimationFrame(frame.current);
    frame.current = requestAnimationFrame(() => {
      const next: Drawn[] = [];
      const cells = new Map<string, DOMRect | null>();
      const routed = new Map<string, { from: Box; wires: Wire[]; to: Net['to'][number][] }>();
      let memory: Box | undefined;
      for (const wire of wires) {
        let from = cells.get(wire.branch);
        if (from === undefined) {
          const cell = document.querySelector(`[data-branch="${CSS.escape(wire.branch)}"]`);
          const bar = cell?.querySelector('.ex-block, .ex-bar');
          const r = bar?.getBoundingClientRect();
          const shown = cell && r && getComputedStyle(cell).visibility !== 'hidden' && r.bottom > 0 && r.top < window.innerHeight;
          from = shown ? r : null;
          cells.set(wire.branch, from);
        }
        if (!from) continue;
        const entry = document.getElementById(`memory/${wire.entry}`);
        const panel = entry?.closest('.ex-memory');
        if (!entry || !panel || entry.closest('[inert]')) continue;
        if (wiring) {
          memory ??= panel.getBoundingClientRect();
          const net = routed.get(wire.branch) ?? { from, wires: [], to: [] };
          net.wires.push(wire);
          net.to.push({ entry: wire.entry, box: entry.getBoundingClientRect() });
          routed.set(wire.branch, net);
          continue;
        }
        const line = link(from, entry.getBoundingClientRect(), panel.getBoundingClientRect(), ENTER);
        next.push({ key: `${wire.branch}>${wire.entry}>${wire.op}`, wire, d: line.d, clipped: line.clipped });
      }
      if (wiring && memory) {
        // The gutter between the lanes and memory; beside an open drawer, a strip just left of it.
        const lanes = document.querySelector('.ex-session__columns')?.getBoundingClientRect().right ?? memory.left;
        const gutter = memory.left - lanes >= 24 ? { left: lanes, right: memory.left } : { left: memory.left - 48, right: memory.left };
        const options = { ...HARNESS, ...wiring };
        const laid = draw(
          route(
            [...routed].map(([key, net]) => ({ key, from: net.from, to: net.to })),
            gutter,
            memory,
            ENTER,
            options,
          ),
          options,
        );
        const lit: DrawnNet[] = [];
        for (const net of laid) {
          const own = routed.get(net.key)?.wires ?? [];
          const lane = own[0]?.lane ?? '';
          lit.push({ branch: net.key, lane, d: net.d, clipped: net.clipped, dots: net.dots });
          for (const w of own) {
            const line = net.wires.find((l) => l.entry === w.entry);
            if (line) next.push({ key: `${w.branch}>${w.entry}>${w.op}`, wire: w, d: line.d, clipped: line.clipped });
          }
        }
        setNets(lit);
      } else setNets([]);
      setDrawn(next);
    });
  }, [wires, wiring]);

  useLayoutEffect(measure, [measure, ...revision]);

  useEffect(() => {
    const again = () => measure();
    window.addEventListener('scroll', again, { passive: true });
    window.addEventListener('resize', again);
    // Memory's panel scrolls inside itself; scroll does not bubble, so listen in the capture phase.
    document.addEventListener('scroll', again, { passive: true, capture: true });
    return () => {
      window.removeEventListener('scroll', again);
      window.removeEventListener('resize', again);
      document.removeEventListener('scroll', again, { capture: true });
      cancelAnimationFrame(frame.current);
    };
  }, [measure]);

  return (
    <svg className="ex-links" aria-hidden="true" data-route={wiring ? 'harness' : undefined}>
      {nets.map((net) => (
        <g key={net.branch} className="ex-net" data-branch={net.branch} style={laneStyle(net.lane) as CSSProperties}>
          <path className="ex-net__run" d={net.d} />
          {net.clipped ? <path className="ex-net__run" data-clipped="" d={net.clipped} /> : null}
          {net.dots.map((p) => (
            <circle key={`${p.x} ${p.y}`} className="ex-net__dot" cx={p.x} cy={p.y} r={2} />
          ))}
        </g>
      ))}
      {drawn.map(({ key, wire, d, clipped }) => (
        <path
          key={key}
          className="ex-link"
          d={d}
          data-branch={wire.branch}
          data-entry={wire.entry}
          data-level={opOf(wire.op).level}
          data-clipped={clipped ? '' : undefined}
          data-hot={hot.branches.has(wire.branch) || hot.entries.has(wire.entry) ? '' : undefined}
          style={laneStyle(wire.lane) as CSSProperties}
        />
      ))}
    </svg>
  );
}
