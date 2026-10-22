// Dev server for one run: root = the run's worktree (VITE_ROOT), Babylon Lite's TypeScript source aliased in read-only.
// Started OUTSIDE the model's sandbox. Two flavours of the same config:
//   - the MODEL's server: no LITE_TRACE, serves Lite untouched;
//   - the GRADER's server: LITE_TRACE=1 adds the lifecycle-recording transform (vite/lite-trace-plugin.mjs). Nothing else differs.
// Nothing is written to the reference checkout (cacheDir is ours, per flavour).
import { liteTrace } from "./lite-trace-plugin.mjs";
const LITE = process.env.LITE_SRC;       // <reference>/packages/babylon-lite/src
const REF = process.env.LITE_REPO;       // <reference> (read access for /@fs/ module URLs)
const port = Number(process.env.VITE_PORT);

// No HMR client: a plain dev server that serves what is on disk, so a page never reloads itself under the model.
const noClient = { name: "no-vite-client", transformIndexHtml: { order: "post", handler: (html) => html.replace(/<script type="module" src="\/@vite\/client"><\/script>\s*/, "") } };

export default {
    plugins: [noClient, ...(process.env.LITE_TRACE ? [liteTrace(LITE)] : [])],
    root: process.env.VITE_ROOT,
    cacheDir: process.env.VITE_CACHE,
    clearScreen: false,
    logLevel: "info",
    optimizeDeps: { exclude: ["@recast-navigation/core", "@recast-navigation/generators", "@recast-navigation/wasm"] },
    resolve: { alias: { "babylon-lite": LITE, "@babylonjs/lite": LITE } },
    server: { host: "127.0.0.1", port, strictPort: true, fs: { allow: [process.env.VITE_ROOT, REF] }, hmr: false },
};
