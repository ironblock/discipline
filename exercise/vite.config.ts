import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { storybookTest } from '@storybook/addon-vitest/vitest-plugin';
import react from '@vitejs/plugin-react';
import { playwright } from '@vitest/browser-playwright';
import type { ProxyOptions } from 'vite';
import { defineConfig } from 'vitest/config';

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

// One config for the app (`pnpm dev`), Storybook and Vitest. Two test
// projects: every story is a browser test (`storybook`), and the fold -- the
// one place events become what the surface draws -- has plain unit tests
// (`unit`), because its cases are sequences, not pictures.
export default defineConfig({
  plugins: [react()],
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
});
