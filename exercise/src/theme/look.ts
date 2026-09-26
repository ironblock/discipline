import type { ThemeName } from './themes/index.ts';

/**
 * The look: the two settings a person chooses, where a theme is a stack of
 * layers only the lab needs to know. A MATERIAL is how things sit on the
 * page -- `colo`, frosted slabs lit by their own LEDs; `paper`, flat and
 * printed -- and a SCHEME is dark or light, or whatever the system says.
 * Every combination is one canonical theme.
 */
export const MATERIALS = ['colo', 'paper'] as const;
export const SCHEMES = ['system', 'dark', 'light'] as const;

export type Material = (typeof MATERIALS)[number];
export type Scheme = (typeof SCHEMES)[number];

export interface Look {
  readonly material: Material;
  readonly scheme: Scheme;
}

export const DEFAULT_LOOK: Look = { material: 'colo', scheme: 'system' };

const CANON: Readonly<Record<Material, Readonly<Record<'dark' | 'light', ThemeName>>>> = {
  colo: { dark: 'bloom', light: 'bloom-light' },
  paper: { dark: 'paper-dark', light: 'paper' },
};

/** The theme a look draws with; `prefersDark` is the system's answer, used when the scheme is `system`. */
export function themeOf(look: Look, prefersDark: boolean): ThemeName {
  const dark = look.scheme === 'system' ? prefersDark : look.scheme === 'dark';
  return CANON[look.material][dark ? 'dark' : 'light'];
}

/** A stored look, read back; each setting falls back to the default on its own. */
export function readLook(stored: string | null): Look {
  let raw: unknown;
  try {
    raw = stored === null ? undefined : JSON.parse(stored);
  } catch {
    raw = undefined;
  }
  const record = typeof raw === 'object' && raw !== null ? (raw as Record<string, unknown>) : {};
  const material = MATERIALS.find((m) => m === record['material']) ?? DEFAULT_LOOK.material;
  const scheme = SCHEMES.find((s) => s === record['scheme']) ?? DEFAULT_LOOK.scheme;
  return { material, scheme };
}
