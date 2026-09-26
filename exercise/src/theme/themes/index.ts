/**
 * The themes to try, one file per layer beside this one. A layer overrides
 * token VALUES under `.ex-root[data-theme~=<layer>]` and nothing else, and a
 * theme is a stack of layers. Every layer's selector is equally specific, so
 * which one wins is the order its FILE is imported below, not the order a
 * theme lists it in: import a layer after every layer it must override.
 * `mockup` is the base in `tokens.css` and has no file; `DEFAULT_THEME` is
 * what a session opens in. This list is the one source the Storybook toolbar
 * and the app's `?theme=` read; a person chooses a look instead (`../look.ts`),
 * which names one of four of these.
 */
import './fabric.css';
import './glass.css';
import './colo.css';
import './lantern.css';
import './emboss.css';
import './bloom.css';
import './rack.css';
import './band.css';
import './paper.css';
import './paper-dark.css';
import './day.css';

export const THEMES = [
  { name: 'mockup', layers: 'mockup', title: 'mockup — the author’s sketch' },
  { name: 'paper', layers: 'paper', title: 'paper — the sketch, light: the same meanings on a white page' },
  { name: 'paper-dark', layers: 'rack paper-dark', title: 'paper-dark — paper’s dark twin: flat surfaces in the LED palette, nothing lit' },
  { name: 'fabric', layers: 'fabric', title: 'fabric — the prefix pressed into one material' },
  { name: 'glass', layers: 'fabric glass', title: 'glass — fabric, with glass beside the prefix' },
  { name: 'colo', layers: 'fabric glass colo', title: 'colo — glass, and things glow while they work' },
  { name: 'lantern', layers: 'fabric glass lantern', title: 'lantern — glass, and the glass is the light' },
  { name: 'emboss', layers: 'fabric emboss', title: 'emboss — the prefix raised, square, light sweeping across' },
  { name: 'hybrid', layers: 'fabric glass colo lantern', title: 'hybrid — colo’s emission, lantern’s lit glass' },
  { name: 'bloom', layers: 'fabric glass colo bloom rack band', title: 'bloom — frosted slabs over pools of their own light, every colour an LED, each footer a shaded band' },
  { name: 'bloom-inline', layers: 'fabric glass colo bloom rack', title: 'bloom-inline — bloom with the footer inline under the text' },
  { name: 'bloom-light', layers: 'fabric glass colo bloom rack band paper day', title: 'bloom-light — bloom in daylight: white frosted slabs over pools of colour' },
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
