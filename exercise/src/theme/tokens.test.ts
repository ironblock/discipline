import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

/**
 * Every custom property a stylesheet reads is one something declares (the app audit's page walk, F15): a stylesheet's own,
 * a theme's, or one the page sets inline. A `var(--x)` no one declares falls back to the browser's defaults -- the
 * gesture dialogs drew on the system canvas, in every theme -- and nothing else would say so.
 */
const SRC = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');

function files(dir: string, ext: RegExp): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const at = path.join(dir, entry.name);
    if (entry.isDirectory()) return files(at, ext);
    return ext.test(entry.name) ? [at] : [];
  });
}

const all = (pattern: RegExp, text: string) => [...text.matchAll(pattern)].map((m) => m[1]!);

describe('custom properties', () => {
  it('are each declared somewhere: no stylesheet reads one that nothing sets', () => {
    const declared = new Set<string>();
    const read = new Map<string, Set<string>>();
    for (const file of files(SRC, /\.css$/)) {
      const text = readFileSync(file, 'utf8');
      for (const name of all(/(--[\w-]+)\s*:/g, text)) declared.add(name);
      for (const name of all(/var\(\s*(--[\w-]+)/g, text)) read.set(name, (read.get(name) ?? new Set()).add(path.relative(SRC, file)));
    }
    // Set inline by the page: `{ ['--lane-fill' as string]: … }`, and `style.setProperty('--x', …)`.
    for (const file of files(SRC, /\.tsx?$/)) {
      const text = readFileSync(file, 'utf8');
      for (const name of [...all(/\[\s*['"`](--[\w-]+)['"`]/g, text), ...all(/setProperty\(\s*['"`](--[\w-]+)/g, text), ...all(/['"](--[\w-]+)['"]\s*:/g, text)]) declared.add(name);
    }
    const undeclared = [...read].filter(([name]) => !declared.has(name)).map(([name, where]) => `${name} (${[...where].join(', ')})`);
    expect(undeclared).toEqual([]);
  });
});
