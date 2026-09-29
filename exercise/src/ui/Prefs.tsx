import { createContext, useContext, useEffect, useRef, useState, useSyncExternalStore } from 'react';
import type { ReactNode } from 'react';

import { DEFAULT_PREFS, PREFS, PREF_LABELS, applies, modeOf, readPrefs, stillOf } from './prefs.ts';
import type { PrefName, Prefs } from './prefs.ts';
import { Segments } from './Segments.tsx';
import './prefs.css';

const STORED = 'exercise.prefs';
/** Where a look was kept before there were preferences: read once, as a starting point. */
const STORED_BEFORE = 'exercise.look';

interface Preferring {
  readonly prefs: Prefs;
  readonly onPrefs?: (next: Prefs) => void;
}

const PrefsContext = createContext<Preferring>({ prefs: DEFAULT_PREFS });

/** The person's preferences, wherever they are needed. */
export function usePrefs(): Prefs {
  return useContext(PrefsContext).prefs;
}

/**
 * The page root, drawn as the person prefers: its theme and mode, and
 * whether things move (`data-motion`), each following the system where
 * that is the choice. Remembered in this browser unless `remember` is off
 * (Storybook, whose toolbar sets `initial`); `overrides` (the address) win
 * for this visit and are not remembered.
 */
export function Preferred({
  initial,
  overrides,
  remember = true,
  children,
}: {
  readonly initial?: Prefs;
  readonly overrides?: Readonly<Record<string, string | null>>;
  readonly remember?: boolean;
  readonly children: ReactNode;
}) {
  const [prefs, setPrefs] = useState<Prefs>(() => {
    if (initial) return initial;
    return readPrefs(stored(STORED) ?? stored(STORED_BEFORE), overrides);
  });
  const prefersDark = useMedia('(prefers-color-scheme: dark)');
  const prefersReduced = useMedia('(prefers-reduced-motion: reduce)');
  const onPrefs = (next: Prefs) => {
    setPrefs(next);
    if (!remember) return;
    try {
      localStorage.setItem(STORED, JSON.stringify(next));
    } catch {
      // Storage refused (a private window): the preferences last as long as the page.
    }
  };
  return (
    <PrefsContext.Provider value={{ prefs, onPrefs }}>
      <div className="ex-root" data-theme={prefs.theme} data-mode={modeOf(prefs, prefersDark)} data-motion={stillOf(prefs, prefersReduced) ? 'still' : 'moving'}>
        {children}
      </div>
    </PrefsContext.Provider>
  );
}

/**
 * The preferences, behind a button: a small panel of segmented switches,
 * one per preference, a trace's details only while connectors are traces.
 * Closed by Escape, by pressing outside it, or by the button again.
 */
export function Settings() {
  const { prefs, onPrefs } = useContext(PrefsContext);
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    if (!open) return;
    const away = (e: PointerEvent) => {
      if (!box.current?.contains(e.target as Node)) setOpen(false);
    };
    const escape = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false);
    };
    document.addEventListener('pointerdown', away);
    document.addEventListener('keydown', escape);
    return () => {
      document.removeEventListener('pointerdown', away);
      document.removeEventListener('keydown', escape);
    };
  }, [open]);
  if (!onPrefs) return null;
  const set = <K extends PrefName>(name: K, value: Prefs[K]) => onPrefs({ ...prefs, [name]: value });
  return (
    <span className="ex-settings" ref={box}>
      <button type="button" className="ex-settings__button" aria-expanded={open} aria-controls="ex-settings" onClick={() => setOpen(!open)}>
        settings
      </button>
      {open ? (
        <div className="ex-settings__panel" id="ex-settings" role="group" aria-label="settings">
          {(Object.keys(PREFS) as PrefName[]).map((name) =>
            applies(name, prefs) ? (
              <div key={name} className="ex-settings__row">
                <span className="ex-settings__name">{PREF_LABELS[name]}</span>
                <Segments name={name} label={PREF_LABELS[name]} options={PREFS[name]} value={prefs[name]} onPick={(value) => set(name, value)} />
              </div>
            ) : null,
          )}
        </div>
      ) : null}
    </span>
  );
}

function stored(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function useMedia(query: string): boolean {
  return useSyncExternalStore(
    (change) => {
      const list = window.matchMedia(query);
      list.addEventListener('change', change);
      return () => list.removeEventListener('change', change);
    },
    () => window.matchMedia(query).matches,
  );
}
