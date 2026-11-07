import { createContext } from 'react';

/**
 * The macOS shell's overlay title bar: the window draws to its top edge, its traffic lights over the header's left
 * end. The shell measures their right edge, in points, and tells the page as `?titlebar=<edge>`
 * (`src-tauri/src/main.rs`'s `titlebar_inset`); a browser is never told, and the header there is unchanged.
 */
export function titlebarInset(search: string): number | undefined {
  const said = new URLSearchParams(search).get('titlebar');
  const edge = said === null || said === '' ? Number.NaN : Number(said);
  return Number.isFinite(edge) && edge > 0 ? edge : undefined;
}

/** The traffic lights' edge, for the header: none in a browser. */
export const Titlebar = createContext<number | undefined>(undefined);
