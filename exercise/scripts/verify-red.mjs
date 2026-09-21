// Every gate here, seen red on its own seeded fault.
//
// The repository's rule: a gate that has never been seen red is not a gate,
// and each seeded fault must be proven to change the tree. This copies the
// package's sources into `.red/<case>/`, applies one mutation whose needle
// MUST be found (a mutation that matches nothing is itself a failure, not a
// pass), runs `tsc` over the copy, and requires the exit to be non-zero AND
// the diagnostic to be the one the case names. Then it does the same for the
// fixtures ledger with a temporary fixture.
//
// Cases, and what each proves:
//
//   ledger-directive-removed   removing a pending line -> TS2344: the gap is real
//   ledger-field-landed        the field lands in the provisional type -> TS2578: the flip works
//   guard-literal-to-a-row     a row handed a literal -> TS2344 (#31 row 5)
//   guard-forged-bound         a Bound spelled outside bound.ts -> TS2344 (#92 row 4)
//   provisional-field-misspelt a provisional field diet does not emit -> TS2353 on the fixtures
//   pending-fixture-accepted   a pending fixture diet accepts -> check-fixtures exit 1, "ACCEPTED"
//   pending-refusal-moved      a pinned message that no longer matches -> check-fixtures exit 1, "DIFFERENTLY"
//
// Exit 0 when every case went red for its own reason; 1 otherwise.

import { spawnSync } from 'node:child_process';
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync, copyFileSync, existsSync } from 'node:fs';
import path from 'node:path';

import { PACKAGE_ROOT } from './diet.mjs';

const RED = path.join(PACKAGE_ROOT, '.red');
const TSC = path.join(PACKAGE_ROOT, 'node_modules', '.bin', 'tsc');

/** Replace exactly one occurrence, refusing a needle that is not there. */
function mutate(file, needle, replacement) {
  const text = readFileSync(file, 'utf8');
  const at = text.indexOf(needle);
  if (at < 0) throw new Error(`${path.relative(PACKAGE_ROOT, file)}: the needle was not found, so this fault would change nothing:\n  ${needle}`);
  writeFileSync(file, text.slice(0, at) + replacement + text.slice(at + needle.length));
}

function typeCase(name, code, apply) {
  const dir = path.join(RED, name);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  cpSync(path.join(PACKAGE_ROOT, 'src'), path.join(dir, 'src'), { recursive: true });
  copyFileSync(path.join(PACKAGE_ROOT, 'tsconfig.json'), path.join(dir, 'tsconfig.json'));
  apply(dir);
  const ran = spawnSync(TSC, ['-p', path.join(dir, 'tsconfig.json'), '--noEmit', '--pretty', 'false'], { encoding: 'utf8' });
  const out = `${ran.stdout}\n${ran.stderr}`;
  const red = ran.status !== 0;
  const own = out.includes(`error ${code}`);
  return { name, red, own, detail: red ? (own ? `${code} as expected` : `red, but not ${code}:\n${out.trim().split('\n').slice(0, 3).join('\n')}`) : 'GREEN -- the gate did not fire' };
}

function fixtureCase(name, marker, apply) {
  const pending = path.join(PACKAGE_ROOT, 'fixtures', 'pending');
  const temp = [`zz-red-${name}.jsonl`, `zz-red-${name}.expect.json`].map((f) => path.join(pending, f));
  try {
    apply(temp[0], temp[1]);
    const ran = spawnSync(process.execPath, [path.join(PACKAGE_ROOT, 'scripts', 'check-fixtures.mjs')], { encoding: 'utf8' });
    const out = `${ran.stdout}\n${ran.stderr}`;
    const red = ran.status !== 0;
    const own = out.includes(marker);
    return { name, red, own, detail: red ? (own ? `"${marker}" as expected` : `red, but without "${marker}"`) : 'GREEN -- the gate did not fire' };
  } finally {
    for (const f of temp) if (existsSync(f)) rmSync(f);
  }
}

const results = [];

results.push(
  typeCase('ledger-directive-removed', 'TS2344', (dir) =>
    mutate(path.join(dir, 'src/record/pending.types.ts'), "// @ts-expect-error #92.4 fork.outcome is not on the fork event\n", ''),
  ),
);
results.push(
  typeCase('ledger-field-landed', 'TS2578', (dir) =>
    mutate(path.join(dir, 'src/record/types.ts'), '  readonly of_turn: number;\n}', '  readonly of_turn: number;\n  readonly outcome?: string;\n}'),
  ),
);
results.push(
  typeCase('guard-literal-to-a-row', 'TS2344', (dir) =>
    mutate(path.join(dir, 'src/record/guards.types.ts'), '// @ts-expect-error #31 a row refuses a bare literal where a Bound value is required\n', ''),
  ),
);
results.push(
  typeCase('guard-forged-bound', 'TS2344', (dir) =>
    mutate(path.join(dir, 'src/record/guards.types.ts'), '// @ts-expect-error #92 a Bound cannot be constructed outside the accessor module\n', ''),
  ),
);
results.push(
  typeCase('provisional-field-misspelt', 'TS2353', (dir) =>
    mutate(path.join(dir, 'src/record/types.ts'), 'readonly prefill_tokens: number;', 'readonly prefil_tokens: number;'),
  ),
);
results.push(
  fixtureCase('accepted', 'ACCEPTED', (jsonl, expect) => {
    const src = path.join(PACKAGE_ROOT, 'fixtures', 'pending', '92-4-fork-outcome-viewer-vocabulary');
    writeFileSync(jsonl, readFileSync(`${src}.jsonl`, 'utf8').replace(',"outcome":"timeout"', ''));
    copyFileSync(`${src}.expect.json`, expect);
  }),
);
results.push(
  fixtureCase('moved', 'DIFFERENTLY', (jsonl, expect) => {
    const src = path.join(PACKAGE_ROOT, 'fixtures', 'pending', '92-2-refused-phase-proposal');
    copyFileSync(`${src}.jsonl`, jsonl);
    writeFileSync(expect, readFileSync(`${src}.expect.json`, 'utf8').replace('names no event kind', 'names no event kind (an older spelling)'));
  }),
);

rmSync(RED, { recursive: true, force: true });

let failed = 0;
for (const r of results) {
  const ok = r.red && r.own;
  if (!ok) failed += 1;
  console.log(`${ok ? 'red ' : 'FAIL'}  ${r.name.padEnd(28)} ${r.detail}`);
}
if (failed > 0) {
  console.error(`verify-red: ${failed} case(s) did not go red for their own fault`);
  process.exit(1);
}
console.log(`verify-red: every gate was seen red on its own seeded fault (${results.length} cases)`);
