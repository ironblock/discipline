import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { existsSync, readFileSync } from 'node:fs';

import { storybookTest } from '@storybook/addon-vitest/vitest-plugin';
import react from '@vitejs/plugin-react';
import { playwright } from '@vitest/browser-playwright';
import type { Plugin, ProxyOptions } from 'vite';
import { defineConfig } from 'vitest/config';

import { EXAMPLES, PUBLISHED } from './src/replay/published.ts';
import { wrap } from './src/replay/payload.ts';

const dirname = path.dirname(fileURLToPath(import.meta.url));

// `diet`'s served session (`diet-drive serve`, #117 I5), for `?drive`: the page
// reaches it same-origin through this proxy. `DIET_DRIVE=http://127.0.0.1:<port>
// pnpm dev`, and `diet` started with `--allow-origin http://localhost:5173`:
// it accepts that origin's host as the `Host` a proxy forwards, so the proxy
// leaves `Host` alone (no `changeOrigin`, Q5 on #117). No CORS is involved.
// A proxy keeps the page's side of a stream open when `diet`'s side dies,
// so the page would wait on a dead stream, live, forever (measured: a
// stream through it outlived its upstream until the client's own timeout).
// Here it ends with its upstream, and a connection `diet` refuses is dropped:
// the browser retries, and a `diet` that comes back answers the resume 410.
const drive = process.env['DIET_DRIVE'];
const configure: NonNullable<ProxyOptions['configure']> = (proxy) => {
  proxy.on('proxyRes', (upstream, _req, res) => upstream.on('close', () => res.destroy()));
  proxy.on('error', (_error, _req, res) => res.destroy());
};
const proxied = drive ? Object.fromEntries(['/events', '/commands'].map((route) => [route, { target: drive, changeOrigin: false, configure }])) : undefined;

// The replay page (#32): `pnpm build:replay` builds `replay.html` alone into
// ../_site/replay/, its index, with `base: './'` so it works under whatever
// path Pages serves it from. Each published recording is written beside it as
// `data/<name>.js` (src/replay/payload.ts), with its admission -- the table it
// was scanned under and its digests (scripts/admission.py) -- beside that: never
// imported, so never bundled. Each authored example (#272) the same way, from
// src/drive/examples/. A published recording or example with no admission
// does not build.
const published = [
  ...PUBLISHED.map((name) => ({ name, dir: 'src/drive/recorded' })),
  ...EXAMPLES.map((name) => ({ name, dir: 'src/drive/examples' })),
];
const replayPayload: Plugin = {
  name: 'exercise:replay-payload',
  apply: 'build',
  // After Vite's own HTML plugin has emitted the page, so the page can be renamed.
  enforce: 'post',
  generateBundle(_options, bundle) {
    // The page is the directory's index: `replay/`, not `replay/replay.html`.
    const html = bundle['replay.html'];
    if (html?.type === 'asset') {
      delete bundle['replay.html'];
      this.emitFile({ type: 'asset', fileName: 'index.html', source: html.source });
    }
    for (const { name, dir } of published) {
      const admission = path.join(dirname, dir, `${name}.admission.json`);
      if (!existsSync(admission)) this.error(`exercise/${dir}/${name}.json: published but never admitted (no ${name}.admission.json beside it; scripts/admission.py admit ${name})`);
      this.emitFile({ type: 'asset', fileName: `data/${name}.js`, source: wrap(readFileSync(path.join(dirname, dir, `${name}.json`), 'utf8')) });
      this.emitFile({ type: 'asset', fileName: `data/${name}.admission.json`, source: readFileSync(admission, 'utf8') });
    }
  },
};

// One config for the app (`pnpm dev`), Storybook and Vitest. Two test
// projects: every story that is not tagged `!test` is a browser test
// (`storybook`), and every `*.test.ts` is a plain unit test in Node (`unit`)
// -- the fold, the transports, the layout maths: cases that are sequences,
// not pictures.
export default defineConfig(({ mode }) => ({
  plugins: [react(), ...(mode === 'replay' ? [replayPayload] : [])],
  ...(mode === 'replay' ? { base: './' } : {}),
  build: {
    // Off: Vite 8 hands the modulepreload polyfill to Rolldown, whose injected
    // text calls `fetch(`, which the Pages table forbids (#32, N1).
    modulePreload: { polyfill: false },
    ...(mode === 'replay' ? { outDir: path.join(dirname, '../_site/replay'), emptyOutDir: true, rollupOptions: { input: path.join(dirname, 'replay.html') } } : {}),
  },
  ...(proxied ? { server: { proxy: proxied } } : {}),
  test: {
    projects: [
      {
        extends: true,
        test: { name: 'unit', include: ['src/**/*.test.ts'], environment: 'node' },
      },
      {
        extends: true,
        plugins: [storybookTest({ configDir: path.join(dirname, '.storybook') })],
        test: {
          name: 'storybook',
          browser: {
            enabled: true,
            headless: true,
            provider: playwright({}),
            instances: [{ browser: 'chromium' }],
          },
        },
      },
    ],
  },
}));
