import { describe, expect, it } from 'vitest';

import { DEFAULT_PREFS, applies, modeOf, readPrefs, stillOf, wiringOf } from './prefs.ts';

describe('preferences', () => {
  it('read back what was stored, each falling back to the default on its own', () => {
    expect(readPrefs('{"theme":"paper","mode":"light","connectors":"sweep"}')).toEqual({ ...DEFAULT_PREFS, theme: 'paper', mode: 'light', connectors: 'sweep' });
    expect(readPrefs(null)).toEqual(DEFAULT_PREFS);
    expect(readPrefs('not json')).toEqual(DEFAULT_PREFS);
    expect(readPrefs('{"theme":"velvet","minimap":"off"}')).toEqual({ ...DEFAULT_PREFS, minimap: 'off' });
  });

  it('read a look stored before them as its theme and mode', () => {
    expect(readPrefs('{"material":"colo","scheme":"light"}')).toMatchObject({ theme: 'bloom', mode: 'light' });
    expect(readPrefs('{"material":"paper","scheme":"dark"}')).toMatchObject({ theme: 'paper', mode: 'dark' });
  });

  it('let the address override any of them, when it names a real choice', () => {
    const got = readPrefs('{"theme":"paper"}', { theme: 'emboss', connectors: 'sweep', corners: 'wobbly', mode: null });
    expect(got).toMatchObject({ theme: 'emboss', connectors: 'sweep', corners: 'round', mode: 'system' });
  });

  it('follow the system’s dark or light, and its reduced motion, when asked to', () => {
    expect(modeOf({ ...DEFAULT_PREFS, mode: 'system' }, true)).toBe('dark');
    expect(modeOf({ ...DEFAULT_PREFS, mode: 'system' }, false)).toBe('light');
    expect(modeOf({ ...DEFAULT_PREFS, mode: 'light' }, true)).toBe('light');
    expect(stillOf({ ...DEFAULT_PREFS, motion: 'system' }, true)).toBe(true);
    expect(stillOf({ ...DEFAULT_PREFS, motion: 'on' }, true)).toBe(false);
    expect(stillOf({ ...DEFAULT_PREFS, motion: 'off' }, false)).toBe(true);
  });

  it('draw lines as traces by default, sweeps when asked; a trace’s crossings and corners apply only to traces', () => {
    expect(wiringOf(DEFAULT_PREFS)).toEqual({ crossing: 'hop', bend: 'round' });
    expect(wiringOf({ ...DEFAULT_PREFS, connectors: 'sweep' })).toBeUndefined();
    expect(applies('corners', { ...DEFAULT_PREFS, connectors: 'sweep' })).toBe(false);
    expect(applies('minimap', { ...DEFAULT_PREFS, connectors: 'sweep' })).toBe(true);
  });
});
