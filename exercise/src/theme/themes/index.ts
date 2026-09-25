/**
 * The themes to try, one file each beside this one. A theme overrides token
 * values under `.ex-root[data-theme=<name>]` and nothing else; `mockup` is the
 * default in `tokens.css` and has no file. This list is the one source the
 * Storybook toolbar and the app's `?theme=` read.
 */
import './paper.css';

export const THEMES = [
  { name: 'mockup', title: 'mockup — the author’s sketch, dark' },
  { name: 'paper', title: 'paper — the same meanings, light' },
] as const;

export type ThemeName = (typeof THEMES)[number]['name'];

export function isTheme(name: string | null): name is ThemeName {
  return THEMES.some((t) => t.name === name);
}
