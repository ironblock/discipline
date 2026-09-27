import { createContext, useContext } from 'react';

import type { Bend, Crossing } from './harness.ts';

/** How much of the machinery the person has asked to see. */
export interface Surface {
  /** Behind the curtain: the slot lanes, working memory, provenance. */
  readonly curtain: boolean;
  /** Behind the curtain, each side call condensed to a bar that keeps its place and grows as it writes. */
  readonly condensed?: boolean;
  /** Outline everything drawn from an event `diet` cannot emit yet, naming the step of #117. */
  readonly gaps: boolean;
  /**
   * The lines -- the trunk's cables, and those into working memory -- routed
   * as a wiring harness (harness.ts), with crossings and corners drawn so;
   * curves when absent. The app's default is a harness whose crossings hop.
   */
  readonly wiring?: { readonly crossing: Crossing; readonly bend: Bend };
}

export const SurfaceContext = createContext<Surface>({ curtain: true, gaps: false });

export function useSurface(): Surface {
  return useContext(SurfaceContext);
}

/**
 * Session time now, in ms. In the live app it advances between events so a
 * running call can say how long it has run; in a story it is the moment the
 * story stopped at, and does not move.
 */
export const ClockContext = createContext<number>(0);

export function useNow(): number {
  return useContext(ClockContext);
}

/** How long something has been running, and whether that has become worrying. */
export function elapsed(now: number, since: number): { readonly ms: number; readonly level: 'ok' | 'slow' | 'stalled' } {
  const ms = Math.max(0, now - since);
  return { ms, level: ms >= 45_000 ? 'stalled' : ms >= 12_000 ? 'slow' : 'ok' };
}

/**
 * The node the address points at (`#<id>`), if any. Kept as state rather
 * than left to `:target`, which the browser resolves once, when the address
 * changes -- a node drawn after that never matches it.
 */
export const TargetContext = createContext<string | undefined>(undefined);

export function useTarget(): string | undefined {
  return useContext(TargetContext);
}

/** Working-memory entries lit because something they are linked to is pointed at (Links.tsx). */
export const HotEntriesContext = createContext<ReadonlySet<string>>(new Set());

export function useHotEntries(): ReadonlySet<string> {
  return useContext(HotEntriesContext);
}
