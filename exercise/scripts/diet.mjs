// Where `diet` is, and how the scripts here reach it.
//
// One implementation, reached across a process boundary: every script in
// this directory that needs a verdict on a record asks the `diet` binary the
// workspace builds, never a reader of its own. The binary is built here if
// it is missing, the way `diet/tests/wasm_conformance.rs` builds its own
// artifact -- a script that assumed a prior CI step had built it would pass
// on a contributor's machine for a different reason than it passes in CI.
//
// `DIET_BIN` overrides the location, for a caller that already has one.

import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const PACKAGE_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const REPO_ROOT = path.resolve(PACKAGE_ROOT, '..');

/** The record corpus `diet` itself is tested against. One source. */
export const RECORD_FIXTURES = path.join(REPO_ROOT, 'diet', 'formats', 'record', 'fixtures');

function build(bin) {
  const built = spawnSync('cargo', ['build', '-p', 'discipline-diet', '--bin', bin], {
    cwd: REPO_ROOT,
    stdio: 'inherit',
  });
  if (built.status !== 0) {
    throw new Error(`cargo build --bin ${bin} exited ${built.status}`);
  }
}

/** The path to a built `diet` (or `diet-drive`) binary, building it if needed. */
export function dietBinary(bin = 'diet') {
  const override = process.env[bin === 'diet' ? 'DIET_BIN' : 'DIET_DRIVE_BIN'];
  if (override) return override;
  const candidate = path.join(REPO_ROOT, 'target', 'debug', bin);
  if (!existsSync(candidate)) build(bin);
  if (!existsSync(candidate)) throw new Error(`built ${bin} but ${candidate} is missing`);
  return candidate;
}

/**
 * `diet check-record <file>`, decoded.
 *
 * Returns the CLI's own envelope -- `{format, path, ok, value}` or
 * `{format, path, ok: false, error}` -- and its exit code. The envelope is
 * JSON in the record's value space; decoding it is reading `diet`'s answer,
 * not the record.
 */
export function checkRecord(file) {
  const ran = spawnSync(dietBinary(), ['check-record', file], { encoding: 'utf8' });
  if (ran.error) throw ran.error;
  let envelope;
  try {
    envelope = JSON.parse(ran.stdout);
  } catch {
    throw new Error(`diet check-record ${file} printed something that is not its envelope:\n${ran.stdout}\n${ran.stderr}`);
  }
  return { exit: ran.status, envelope };
}
