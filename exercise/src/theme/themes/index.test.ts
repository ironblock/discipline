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
      expect(read(`${theme}.css`), theme).toContain(`[data-theme='${theme}']`);
      expect(index, theme).toContain(`import './${theme}.css';`);
    }
  });

  it('has a palette for every mode: dark is the default (tokens.css), light its own file', () => {
    expect(MODES).toEqual(['dark', 'light']);
    expect(read('light.css')).toContain(`[data-mode='light']`);
    expect(read('../tokens.css')).toMatch(/:root\s*\{/);
  });
});
