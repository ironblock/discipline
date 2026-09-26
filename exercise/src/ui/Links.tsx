import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { CSSProperties } from 'react';

import { link } from './links.ts';
import { ENTER } from './placement.ts';
import { laneStyle, opOf } from './sets.ts';
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

/**
 * What each side call wrote, as a line from it into working memory: one per
 * patch, for the side calls on screen, drawn over the page in viewport
 * coordinates and redrawn as the page or memory's panel scrolls. Faint at
 * rest; lit when either end is `hot` (pointed at, or the address's target).
 * An entry scrolled out of memory's panel is reached at the panel's edge,
 * dashed. Nothing is drawn into a shut drawer.
 */
export function Links({ wires, hot, revision }: { readonly wires: readonly Wire[]; readonly hot: { readonly branches: ReadonlySet<string>; readonly entries: ReadonlySet<string> }; readonly revision: readonly unknown[] }) {
  const [drawn, setDrawn] = useState<readonly Drawn[]>([]);
  const frame = useRef(0);

  const measure = useCallback(() => {
    cancelAnimationFrame(frame.current);
    frame.current = requestAnimationFrame(() => {
      const next: Drawn[] = [];
      const cells = new Map<string, DOMRect | null>();
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
        const line = link(from, entry.getBoundingClientRect(), panel.getBoundingClientRect(), ENTER);
        next.push({ key: `${wire.branch}>${wire.entry}>${wire.op}`, wire, d: line.d, clipped: line.clipped });
      }
      setDrawn(next);
    });
  }, [wires]);

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
    <svg className="ex-links" aria-hidden="true">
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
