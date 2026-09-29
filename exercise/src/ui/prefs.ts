import { MODES, THEMES } from '../theme/themes/index.ts';
import type { Mode } from '../theme/themes/index.ts';
import type { Bend, Crossing } from './harness.ts';

/**
 * What a person may prefer, each a short list of choices, the first of
 * which is not always the default: how the page looks (a theme, in a mode),
 * how the lines between things are drawn -- as TRACES routed like a circuit
 * board (harness.ts), whose crossings hop or break and whose corners are
 * round, cut at 45 degrees or square, or as SWEEPS, curves -- whether
 * things move, whether side calls draw lines into working memory, and
 * whether the minimap is shown. Remembered in the browser; the address may
 * override any of them for one visit (`?theme=paper&connectors=sweep`).
 */
export const PREFS = {
  theme: THEMES,
  mode: ['system', ...MODES],
  connectors: ['trace', 'sweep'],
  crossings: ['hop', 'gap'],
  corners: ['round', 'chamfer', 'square'],
  motion: ['system', 'on', 'off'],
  memoryLines: ['on', 'off'],
  minimap: ['on', 'off'],
} as const satisfies Record<string, readonly string[]>;

export type PrefName = keyof typeof PREFS;
export type Prefs = { readonly [K in PrefName]: (typeof PREFS)[K][number] };

export const DEFAULT_PREFS: Prefs = {
  theme: 'bloom',
  mode: 'system',
  connectors: 'trace',
  crossings: 'hop',
  corners: 'round',
  motion: 'system',
  memoryLines: 'on',
  minimap: 'on',
};

/** How each is named where a person picks it. */
export const PREF_LABELS: Readonly<Record<PrefName, string>> = {
  theme: 'theme',
  mode: 'mode',
  connectors: 'connectors',
  crossings: 'crossings',
  corners: 'corners',
  motion: 'motion',
  memoryLines: 'lines into memory',
  minimap: 'minimap',
};

/** Choices that only mean something while another is made: a trace's crossings and corners. */
export function applies(name: PrefName, prefs: Prefs): boolean {
  return name === 'crossings' || name === 'corners' ? prefs.connectors === 'trace' : true;
}

/**
 * Preferences read back: from what was stored (JSON), then from `overrides`
 * (the address), each falling back to the default on its own. A look stored
 * before these preferences (`material`, `scheme`) is read as its theme and
 * mode.
 */
export function readPrefs(stored: string | null, overrides: Readonly<Record<string, string | null>> = {}): Prefs {
  let raw: unknown;
  try {
    raw = stored === null ? undefined : JSON.parse(stored);
  } catch {
    raw = undefined;
  }
  const record = typeof raw === 'object' && raw !== null ? { ...(raw as Record<string, unknown>) } : {};
  // The old `colo` is no theme now, so it falls back to bloom, which it was.
  if (record['theme'] === undefined && record['material'] !== undefined) record['theme'] = record['material'];
  if (record['mode'] === undefined && record['scheme'] !== undefined) record['mode'] = record['scheme'];
  const pick = <K extends PrefName>(name: K): Prefs[K] => {
    const choices: readonly string[] = PREFS[name];
    const over = overrides[name];
    if (over !== undefined && over !== null && choices.includes(over)) return over as Prefs[K];
    const kept = record[name];
    return typeof kept === 'string' && choices.includes(kept) ? (kept as Prefs[K]) : DEFAULT_PREFS[name];
  };
  return {
    theme: pick('theme'),
    mode: pick('mode'),
    connectors: pick('connectors'),
    crossings: pick('crossings'),
    corners: pick('corners'),
    motion: pick('motion'),
    memoryLines: pick('memoryLines'),
    minimap: pick('minimap'),
  };
}

/** The mode drawn: the one chosen, or the system's (`prefersDark`) when that is the choice. */
export function modeOf(prefs: Prefs, prefersDark: boolean): Mode {
  return prefs.mode === 'system' ? (prefersDark ? 'dark' : 'light') : prefs.mode;
}

/** Whether things hold still: chosen, or the system's reduced motion (`prefersReduced`) when that is the choice. */
export function stillOf(prefs: Prefs, prefersReduced: boolean): boolean {
  return prefs.motion === 'system' ? prefersReduced : prefs.motion === 'off';
}

/** Lines routed as a harness, and how its crossings and corners are drawn. */
export interface Wiring {
  readonly crossing: Crossing;
  readonly bend: Bend;
}

/** How lines are routed and drawn: as a harness, or (undefined) as sweeps. */
export function wiringOf(prefs: Pick<Prefs, 'connectors' | 'crossings' | 'corners'>): Wiring | undefined {
  return prefs.connectors === 'trace' ? { crossing: prefs.crossings, bend: prefs.corners } : undefined;
}
