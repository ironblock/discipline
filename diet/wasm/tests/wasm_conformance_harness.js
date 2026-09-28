'use strict';
// #78's conformance harness. Called by `tests/wasm_conformance.rs`'s own
// subprocess, never by a person.
//
// Reads a JSON manifest of `{id, func, path}` entries, calls
// `func(fs.readFileSync(path, "utf8"))` on the wasm-bindgen Node glue named
// by argv[2], and prints one NDJSON line per entry -- so a call that throws
// (the shape a trap takes at this boundary) still lets every other fixture
// in the batch report, rather than losing the whole run to whichever fixture
// came first.
const fs = require('fs');

const [, , gluePath, manifestPath] = process.argv;
const wasmModule = require(gluePath);
const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));

for (const entry of manifest) {
  const { id, func, path } = entry;
  let line;
  try {
    const source = fs.readFileSync(path, 'utf8');
    const result = wasmModule[func](source);
    line = { id, ok: true, result };
  } catch (err) {
    line = { id, ok: false, error: String((err && err.message) || err) };
  }
  process.stdout.write(`${JSON.stringify(line)}\n`);
}
