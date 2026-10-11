import { describe, expect, it } from 'vitest';

import { driveBase } from './base.ts';

/** A page as `driveBase` reads it: its base URL and, when serve or Tauri injected one, `<meta name="diet-drive">`. */
const page = (baseURI: string, meta?: string) => ({
  baseURI,
  querySelector: (selector: string) => (selector === 'meta[name="diet-drive"]' && meta !== undefined ? { content: meta } : null),
});

describe('where the page reaches the drive (#716)', () => {
  it('is same-origin when nothing names it: relative routes, resolved against the page’s own base', () => {
    expect(driveBase(page('http://localhost:5173/'), '?drive')).toEqual({ drive: true, base: '' });
    expect(driveBase(page('http://localhost:5173/'), '')).toEqual({ drive: false, base: '' });
  });

  it('takes the drive serve or Tauri named in <meta name="diet-drive">, and that alone makes the page drive', () => {
    expect(driveBase(page('tauri://localhost/', 'http://127.0.0.1:7801'), '')).toEqual({ drive: true, base: 'http://127.0.0.1:7801/' });
  });

  it('resolves a relative meta against the page’s base: a drive at a subpath of the server serving the page', () => {
    expect(driveBase(page('https://box.example/app/index.html', 'api/'), '')).toEqual({ drive: true, base: 'https://box.example/app/api/' });
  });

  it('lets ?drive=<url> override the meta, by hand', () => {
    expect(driveBase(page('tauri://localhost/', 'http://127.0.0.1:7801'), '?drive=http://127.0.0.1:7802')).toEqual({ drive: true, base: 'http://127.0.0.1:7802/' });
  });

  it('reads a ?drive that is no http(s) URL as the flag it always was', () => {
    expect(driveBase(page('http://localhost:5173/', 'http://127.0.0.1:7801'), '?drive=1')).toEqual({ drive: true, base: 'http://127.0.0.1:7801/' });
    expect(driveBase(page('http://localhost:5173/'), '?drive=javascript:alert(1)')).toEqual({ drive: true, base: '' });
  });
});
