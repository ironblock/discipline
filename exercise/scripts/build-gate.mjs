// The shell gate, built for the page (#389, ruled 5982832752): `diet/wasm`'s
// `judge_argv` (#402) compiled to wasm32 and bound with the `wasm-bindgen`
// CLI at the exact version `diet/wasm/Cargo.toml` pins -- the bindgen schema
// is shared between the two and a mismatch is refused outright (#78). The
// replay re-derives a logged call's segments through it, so the page and the
// drive have one reader.
//
// Writes src/gate/wasm/ (gitignored, built every run, never committed, so it
// is never older than the gate it was built from):
//   diet_wasm.js, diet_wasm.d.ts   the glue, `--target web`, with no default
//                                  module path: the page never fetches it
//   bytes.js                       `export default` and the wasm as base64,
//                                  compiled with `WebAssembly.compile` -- the
//                                  Pages table forbids a network call (#32 N1)
//
// Needs `rustup target add wasm32-unknown-unknown` and
// `cargo install wasm-bindgen-cli --version <the pin> --locked`.

import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const exercise = path.join(here, '..');
const root = path.join(exercise, '..');
const out = path.join(exercise, 'src/gate/wasm');

const fail = (why) => {
  process.stderr.write(`build-gate: ${why}\n`);
  process.exit(1);
};

// The one pin: the crate's exact `wasm-bindgen` version.
const manifest = readFileSync(path.join(root, 'diet/wasm/Cargo.toml'), 'utf8');
const pin = manifest.match(/^wasm-bindgen = \{ version = "=([0-9.]+)"/m)?.[1];
if (!pin) fail('diet/wasm/Cargo.toml pins no exact wasm-bindgen version');

let cli;
try {
  cli = execFileSync('wasm-bindgen', ['--version'], { encoding: 'utf8' }).trim();
} catch {
  fail(`no wasm-bindgen CLI on PATH: cargo install wasm-bindgen-cli --version ${pin} --locked`);
}
if (cli !== `wasm-bindgen ${pin}`) fail(`${cli} is not the pinned ${pin}: cargo install wasm-bindgen-cli --version ${pin} --locked`);

execFileSync('cargo', ['build', '--locked', '--quiet', '--release', '--target', 'wasm32-unknown-unknown', '-p', 'discipline-diet-wasm', '--no-default-features', '--features', 'wasm'], {
  cwd: root,
  stdio: 'inherit',
});
const target = process.env['CARGO_TARGET_DIR'] ?? path.join(root, 'target');
const wasm = path.join(target, 'wasm32-unknown-unknown/release/diet_wasm.wasm');

rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
execFileSync('wasm-bindgen', ['--target', 'web', '--omit-default-module-path', '--out-dir', out, wasm], { stdio: 'inherit' });
// The glue's two loaders fetch a module path, and the page never calls them: it compiles bytes it already has and
// binds them with `initSync`. They go, with the default export that names one -- the Pages table refuses a network
// call in what Pages serves (#32 N1), and the bundler keeps them however little is imported (measured: a dynamic
// import of a two-name re-export still carried them). What is left must not fetch at all.
const glue = path.join(out, 'diet_wasm.js');
let text = readFileSync(glue, 'utf8');
for (const name of ['__wbg_load', '__wbg_init']) {
  const start = text.indexOf(`async function ${name}(`);
  if (start < 0) fail(`the glue has no ${name}: wasm-bindgen ${pin}'s glue is not the shape this script removes from`);
  let depth = 0;
  let end = text.indexOf('{', start);
  for (; end < text.length; end += 1) {
    if (text[end] === '{') depth += 1;
    if (text[end] === '}' && --depth === 0) break;
  }
  text = text.slice(0, start) + text.slice(end + 1);
}
text = text.replace(/^export default __wbg_init;\n/m, '').replace(/^ *__wbg_init\.__wbindgen_wasm_module = module;\n/m, '');
if (/\b__wbg_init\b/.test(text)) fail('the glue still names __wbg_init, which it no longer has');
if (/\b(fetch|XMLHttpRequest|WebSocket|EventSource|importScripts)\s*\(/.test(text)) fail('the glue still makes a network call');
writeFileSync(glue, text);
const typings = path.join(out, 'diet_wasm.d.ts');
writeFileSync(typings, readFileSync(typings, 'utf8').replace(/^export default function __wbg_init\b.*\n/m, ''));

const bound = path.join(out, 'diet_wasm_bg.wasm');
writeFileSync(path.join(out, 'bytes.js'), `export default ${JSON.stringify(readFileSync(bound).toString('base64'))};\n`);
writeFileSync(path.join(out, 'bytes.d.ts'), 'declare const bytes: string;\nexport default bytes;\n');
rmSync(bound);
rmSync(path.join(out, 'diet_wasm_bg.wasm.d.ts'), { force: true });
process.stdout.write(`build-gate: ${path.relative(exercise, out)} from diet/wasm, ${cli}\n`);
