import { describe, expect, it } from 'vitest';

import { DEFAULT_LOOK, readLook, themeOf } from './look.ts';
import { THEMES } from './themes/index.ts';

describe('the look', () => {
  it('is two settings: a material, and dark or light', () => {
    expect(themeOf({ material: 'colo', scheme: 'dark' }, false)).toBe('bloom');
    expect(themeOf({ material: 'colo', scheme: 'light' }, true)).toBe('bloom-light');
    expect(themeOf({ material: 'paper', scheme: 'light' }, true)).toBe('paper');
    expect(themeOf({ material: 'paper', scheme: 'dark' }, false)).toBe('paper-dark');
  });

  it('follows the system when asked to', () => {
    expect(themeOf({ material: 'colo', scheme: 'system' }, true)).toBe('bloom');
    expect(themeOf({ material: 'colo', scheme: 'system' }, false)).toBe('bloom-light');
    expect(themeOf({ material: 'paper', scheme: 'system' }, true)).toBe('paper-dark');
  });

  it('names only themes that exist', () => {
    const names = new Set<string>(THEMES.map((t) => t.name));
    for (const material of ['colo', 'paper'] as const)
      for (const dark of [true, false]) expect(names.has(themeOf({ material, scheme: 'system' }, dark))).toBe(true);
  });

  it('reads back what was stored, and the default from anything else', () => {
    expect(readLook('{"material":"paper","scheme":"light"}')).toEqual({ material: 'paper', scheme: 'light' });
    expect(readLook(null)).toEqual(DEFAULT_LOOK);
    expect(readLook('not json')).toEqual(DEFAULT_LOOK);
    expect(readLook('{"material":"velvet","scheme":"dark"}')).toEqual({ ...DEFAULT_LOOK, scheme: 'dark' });
    expect(readLook('{"material":"paper","scheme":"dusk"}')).toEqual({ ...DEFAULT_LOOK, material: 'paper' });
  });
});
