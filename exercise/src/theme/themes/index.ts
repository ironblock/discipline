/**
 * The themes a person chooses between (`../../ui/prefs.ts`): how things sit on the
 * page, each in a dark and a light mode. `tokens.css` is the dark palette and
 * every default; `light.css` the light palette; a theme's file sets only what
 * it treats differently, under `[data-theme]`, and by mode under
 * `[data-theme][data-mode]` -- so no value depends on the order of these
 * imports.
 */
import './bloom.css';
import './emboss.css';
import './light.css';
import './paper.css';

export const THEMES = ['bloom', 'paper', 'emboss'] as const;

export const MODES = ['dark', 'light'] as const;
export type Mode = (typeof MODES)[number];
