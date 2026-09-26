import { createContext, useContext } from 'react';

/** How much of the machinery the person has asked to see. */
export interface Surface {
  /** Behind the curtain: the slot lanes, working memory, provenance. */
  readonly curtain: boolean;
  /** Behind the curtain, each side call condensed to a bar that keeps its place and grows as it writes. */
  readonly condensed?: boolean;
  /** Outline everything drawn from an event `diet` cannot emit yet, naming the step of #117. */
  readonly gaps: boolean;
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
