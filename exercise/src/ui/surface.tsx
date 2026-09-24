import { createContext, useContext } from 'react';

/** How much of the machinery the person has asked to see. */
export interface Surface {
  /** Behind the curtain: the slot lanes, working memory, provenance. */
  readonly curtain: boolean;
  /** Outline everything drawn from an event `diet` cannot emit yet, naming the step of #117. */
  readonly gaps: boolean;
}

export const SurfaceContext = createContext<Surface>({ curtain: true, gaps: false });

export function useSurface(): Surface {
  return useContext(SurfaceContext);
}
