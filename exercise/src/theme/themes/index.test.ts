import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import { MODES, THEMES } from './index.ts';

const here = path.dirname(fileURLToPath(import.meta.url));
const read = (file: string) => readFileSync(path.join(here, file), 'utf8');

describe('the themes a person chooses between', () => {
  it('has, for every theme it names, a stylesheet that styles that theme and is imported here', () => {
    const index = read('index.ts');
    for (const theme of THEMES) {
      expect(read(`${theme}.css`), theme).toMatch(new RegExp(`\\[data-theme=(['"])${theme}\\1\\]`));
      expect(index, theme).toContain(`import './${theme}.css';`);
    }
  });

  it('has a palette for every mode: dark is the default (tokens.css), light its own file, and they differ', () => {
    expect(MODES).toEqual(['dark', 'light']);
    const page = (css: string) => /--page:\s*([^;]+);/.exec(css)?.[1];
    const light = read('light.css');
    expect(light).toMatch(/\[data-mode=(['"])light\1\]/);
    expect(page(read('../tokens.css'))).toBeDefined();
    expect(page(light)).toBeDefined();
    expect(page(light)).not.toBe(page(read('../tokens.css')));
  });
});
