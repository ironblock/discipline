/**
 * The themes to try, one file per layer beside this one. A layer overrides
 * token VALUES under `.ex-root[data-theme~=<layer>]` and nothing else, and a
 * theme is a stack of layers, later ones winning: `glass` is fabric + glass.
 * `mockup` is the base in `tokens.css` and has no file; `DEFAULT_THEME` is
 * what a session opens in. This list is the
 * one source the Storybook toolbar and the app's `?theme=` read.
 */
import './paper.css';
import './fabric.css';
import './glass.css';
import './colo.css';
import './lantern.css';
import './emboss.css';
import './bloom.css';
import './rack.css';
import './band.css';

export const THEMES = [
  { name: 'mockup', layers: 'mockup', title: 'mockup — the author’s sketch' },
  { name: 'paper', layers: 'paper', title: 'paper — the sketch, light' },
  { name: 'fabric', layers: 'fabric', title: 'fabric — the prefix pressed into one material' },
  { name: 'glass', layers: 'fabric glass', title: 'glass — fabric, with glass beside the prefix' },
  { name: 'colo', layers: 'fabric glass colo', title: 'colo — glass, and things glow while they work' },
  { name: 'lantern', layers: 'fabric glass lantern', title: 'lantern — glass, and the glass is the light' },
  { name: 'emboss', layers: 'fabric emboss', title: 'emboss — the prefix raised, square, light sweeping across' },
  { name: 'hybrid', layers: 'fabric glass colo lantern', title: 'hybrid — colo’s emission, lantern’s lit glass' },
  { name: 'bloom', layers: 'fabric glass colo bloom rack', title: 'bloom — frosted slabs over pools of their own light, every colour an LED' },
  { name: 'band', layers: 'fabric glass colo bloom rack band', title: 'band — bloom, each footer a full-width shaded band' },
] as const;

export type ThemeName = (typeof THEMES)[number]['name'];

/** The theme a session opens in. */
export const DEFAULT_THEME: ThemeName = 'bloom';

export function isTheme(name: string | null): name is ThemeName {
  return THEMES.some((t) => t.name === name);
}

/** The `data-theme` value for a theme: its layers, space-separated. */
export function layersOf(name: string): string {
  return THEMES.find((t) => t.name === name)?.layers ?? layersOf(DEFAULT_THEME);
}
