import { createContext, useContext, useState, useSyncExternalStore } from 'react';
import type { ReactNode } from 'react';

import { MATERIALS, SCHEMES, readLook, themeOf } from '../theme/look.ts';
import type { Look } from '../theme/look.ts';
import { layersOf } from '../theme/themes/index.ts';
import type { ThemeName } from '../theme/themes/index.ts';

const STORED = 'exercise.look';

interface LookSettings {
  readonly look: Look;
  readonly onLook: (next: Look) => void;
}

/** The person's look and how to change it; absent where something else chooses the theme (Storybook's toolbar). */
const LookContext = createContext<LookSettings | undefined>(undefined);

/**
 * The page root in a chosen look: remembered in this browser, following the
 * system's dark or light when asked to. `pinned` (the app's `?theme=`) draws
 * a lab theme instead, until the person picks a look.
 */
export function Looked({ pinned, children }: { readonly pinned?: ThemeName | undefined; readonly children: ReactNode }) {
  const [look, setLook] = useState(() => readLook(stored()));
  const [pin, setPin] = useState(pinned);
  const prefersDark = useSyncExternalStore(watchScheme, () => window.matchMedia('(prefers-color-scheme: dark)').matches);
  const onLook = (next: Look) => {
    setLook(next);
    setPin(undefined);
    try {
      localStorage.setItem(STORED, JSON.stringify(next));
    } catch {
      // Storage refused (a private window): the look lasts as long as the page.
    }
  };
  return (
    <LookContext.Provider value={{ look, onLook }}>
      <div className="ex-root" data-theme={layersOf(pin ?? themeOf(look, prefersDark))}>
        {children}
      </div>
    </LookContext.Provider>
  );
}

/** The look's two settings, as two small segmented switches. Nothing where no one chooses a look. */
export function LookSetting() {
  const settings = useContext(LookContext);
  if (!settings) return null;
  const { look, onLook } = settings;
  return (
    <span className="ex-look" role="group" aria-label="look">
      <Segments name="material" options={MATERIALS} value={look.material} onPick={(material) => onLook({ ...look, material })} />
      <Segments name="scheme" options={SCHEMES} value={look.scheme} onPick={(scheme) => onLook({ ...look, scheme })} />
    </span>
  );
}

function Segments<T extends string>({ name, options, value, onPick }: { readonly name: string; readonly options: readonly T[]; readonly value: T; readonly onPick: (next: T) => void }) {
  return (
    <span className="ex-segments" role="radiogroup" aria-label={name}>
      {options.map((option) => (
        <label key={option} className="ex-segment" data-on={option === value ? '' : undefined}>
          <input type="radio" name={`ex-look-${name}`} value={option} checked={option === value} onChange={() => onPick(option)} />
          {option}
        </label>
      ))}
    </span>
  );
}

function stored(): string | null {
  try {
    return localStorage.getItem(STORED);
  } catch {
    return null;
  }
}

function watchScheme(change: () => void): () => void {
  const query = window.matchMedia('(prefers-color-scheme: dark)');
  query.addEventListener('change', change);
  return () => query.removeEventListener('change', change);
}
