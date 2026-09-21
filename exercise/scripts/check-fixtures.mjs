// The fixtures ledger, both halves.
//
// SEQUENCES: every record a Sequences story renders must be one `diet
// check-record` accepts -- the corpus's `valid/` and this package's
// `fixtures/records/`. Exit 0 on each, or this exits 1.
//
// PENDING: every `fixtures/pending/<name>.jsonl` is a record the schema is
// EXPECTED to refuse, and its `<name>.expect.json` pins how: the exit code,
// the refusal's class, and `diet`'s message verbatim. Three outcomes:
//
//   refused as pinned    green -- the gap is still open, exactly as recorded
//   refused differently  red   -- the schema moved under the fixture; the
//                                 message names what changed (a field landed
//                                 and another did not, a vocabulary was
//                                 refused), so re-examine rather than re-pin
//   accepted             red   -- the gap CLOSED. Move the fixture to
//                                 fixtures/records/, delete its ledger line
//                                 in src/record/pending.types.ts, and give
//                                 it a Sequences story
//
// `diet` prints the refusal as prose with no class field (disclosed on the
// PR and requested), so `refusal` in the sidecar is this package's reading
// of the message -- `schema.unknown_field` for "carries `x`, which this
// schema does not define", `schema.unknown_kind` for "`x` names no event
// kind" -- and `message` is the byte-exact string that reading was made
// from. A change to either is a change to the fact, and is red.

import { existsSync, readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';

import { PACKAGE_ROOT, RECORD_FIXTURES, checkRecord } from './diet.mjs';

const PENDING = path.join(PACKAGE_ROOT, 'fixtures', 'pending');

/** The refusal class this package reads out of `diet`'s message. */
export function classify(message) {
  if (/names no event kind$/.test(message)) return 'schema.unknown_kind';
  if (/which this schema does not define$/.test(message)) return 'schema.unknown_field';
  if (/is outside its vocabulary$/.test(message)) return 'schema.bad_value';
  if (/is missing its required/.test(message)) return 'schema.missing_field';
  return 'unclassified';
}

const failures = [];
let sequences = 0;
let pending = 0;

for (const dir of [path.join(RECORD_FIXTURES, 'valid'), path.join(PACKAGE_ROOT, 'fixtures', 'records')]) {
  if (!existsSync(dir)) continue;
  for (const file of readdirSync(dir).sort()) {
    if (!file.endsWith('.jsonl')) continue;
    const { exit, envelope } = checkRecord(path.join(dir, file));
    sequences += 1;
    if (exit !== 0 || !envelope.ok) failures.push(`sequences: ${file}: diet refused it (exit ${exit}): ${envelope.error}`);
  }
}

for (const file of readdirSync(PENDING).sort()) {
  if (!file.endsWith('.jsonl')) continue;
  const name = file.slice(0, -'.jsonl'.length);
  const sidecar = path.join(PENDING, `${name}.expect.json`);
  if (!existsSync(sidecar)) {
    failures.push(`pending: ${file}: has no ${name}.expect.json pinning its refusal`);
    continue;
  }
  const expect = JSON.parse(readFileSync(sidecar, 'utf8'));
  const { exit, envelope } = checkRecord(path.join(PENDING, file));
  pending += 1;
  if (exit === 0 && envelope.ok) {
    failures.push(
      `pending: ${file}: ACCEPTED -- the gap ${expect.issue} (${expect.atom}) has closed. ` +
        `Move it to fixtures/records/, delete its line in src/record/pending.types.ts, give it a Sequences story.`,
    );
    continue;
  }
  const message = envelope.error;
  if (exit !== expect.exit) failures.push(`pending: ${file}: exit ${exit}, pinned ${expect.exit}`);
  if (message !== expect.message) {
    failures.push(`pending: ${file}: refused DIFFERENTLY.\n    pinned: ${expect.message}\n    now:    ${message}`);
  }
  const cls = classify(message);
  if (cls !== expect.refusal) failures.push(`pending: ${file}: refusal reads as ${cls}, pinned ${expect.refusal}`);
}

if (failures.length > 0) {
  console.error(failures.join('\n'));
  console.error(`check-fixtures: ${failures.length} failure(s)`);
  process.exit(1);
}
console.log(`check-fixtures: ${sequences} sequence fixture(s) accepted, ${pending} pending fixture(s) refused exactly as pinned`);
