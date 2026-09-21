// The one fixture in this package that no hand wrote: a record the canned
// drive produced.
//
// `diet/formats/record/fixtures/valid/` is small and hand-authored, and
// nothing in it -- nor in `results/` -- carries a fork with its capture, or a
// seam with the ratification exchange that fired it. The canned drive does
// (`diet/src/drive/canned.rs`: three scripted turns, two forks, one seam at
// the operator's declared boundary), deterministically, on loopback, in
// milliseconds. So this script runs it and keeps the record, and `--check`
// runs it again and refuses a byte of difference.
//
// This is a FIXTURE, not a generator. A record generator for the viewer --
// N turns, the states the drive can reach -- belongs in `diet/src/drive/`
// and is requested on the PR, not built here.
//
//   node scripts/drive-fixture.mjs           write fixtures/records/canned-drive.jsonl
//   node scripts/drive-fixture.mjs --check   re-run and diff; exit 1 on drift

import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync, existsSync, mkdirSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { PACKAGE_ROOT, REPO_ROOT, dietBinary } from './diet.mjs';

const REGIMEN = path.join(REPO_ROOT, 'diet', 'drive', 'dev-loop.toml');
const OUT_DIR = path.join(PACKAGE_ROOT, 'fixtures', 'records');
const RECORD = path.join(OUT_DIR, 'canned-drive.jsonl');
const PRODUCT = path.join(OUT_DIR, 'canned-drive.product');

function drive() {
  const scratch = mkdtempSync(path.join(os.tmpdir(), 'exercise-drive-'));
  const worktree = path.join(scratch, 'worktree');
  const record = path.join(scratch, 'record.jsonl');
  mkdirSync(worktree);
  try {
    const ran = spawnSync(dietBinary('diet-drive'), [REGIMEN, worktree, record], { encoding: 'utf8' });
    if (ran.status !== 0) {
      throw new Error(`diet-drive exited ${ran.status}:\n${ran.stdout}\n${ran.stderr}`);
    }
    return { record: readFileSync(record, 'utf8'), product: readFileSync(`${record}.product`, 'utf8') };
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

const check = process.argv.includes('--check');
const fresh = drive();

if (check) {
  const failures = [];
  for (const [file, text] of [
    [RECORD, fresh.record],
    [PRODUCT, fresh.product],
  ]) {
    if (!existsSync(file)) failures.push(`${path.relative(PACKAGE_ROOT, file)}: missing; run \`pnpm gen:drive\``);
    else if (readFileSync(file, 'utf8') !== text) failures.push(`${path.relative(PACKAGE_ROOT, file)}: differs from a fresh drive`);
  }
  if (failures.length > 0) {
    console.error(failures.join('\n'));
    process.exit(1);
  }
  console.log('drive-fixture: canned-drive.jsonl and .product match a fresh run byte for byte');
} else {
  mkdirSync(OUT_DIR, { recursive: true });
  writeFileSync(RECORD, fresh.record);
  writeFileSync(PRODUCT, fresh.product);
  console.log(`drive-fixture: wrote ${path.relative(PACKAGE_ROOT, RECORD)} and .product`);
}
