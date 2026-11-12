#!/usr/bin/env node
// The kitchen sink (#798): the published example is the log `diet-drive scenario` writes for
// diet/scenarios/kitchen-sink.jsonl, scrubbed of the run's own paths and placed on the compose clock.
//
// `write` composes it with src/drive/kitchen-sink.ts: each gate's `latency_ms` -- wall time on the machine that ran,
// which `check` never compares (scenario-log.ts's CLOCK) -- is carried from the example it replaces, matched by lane
// and place as `check` compares lines, so a rewrite changes only what the session changed and writing the same log
// twice gives the same bytes. The migration says so, with how many were carried and how many are this run's own.
// Nothing pins the values: the tests and stories read them from the example (src/drive/moments.ts, #875).
//
//   node exercise/scripts/kitchen-sink.mjs write             run the scenario, scrub, retime, and write src/drive/examples/kitchen-sink.json
//   node exercise/scripts/kitchen-sink.mjs check             run the scenario and compare each lane's lines, in order, with the published ones
//   node exercise/scripts/kitchen-sink.mjs check --current   the same, and fail unless they are the published ones
//
// The sink lags (the maintainer's ruling, 2026-11-11): a PR that changes what the scenario writes merges without
// writing the example, and exercise/scripts/regenerate-kitchen-sink.sh catches it up on develop in one small PR.
// `check` fails only when diet/scenarios/kitchen-sink.jsonl is not the file the example was written from (its
// `scenario_sha256`): the PR that changes the scenario writes the example too. Lane drift alone is STALE: printed with
// its lane diff, as a warning annotation on a CI run and in the job's summary, and not failed. `--current` fails on it:
// the regeneration runs it after writing.
//
// After `write`, admit it again: python3 exercise/scripts/admission.py admit kitchen-sink. `check` runs in verify's
// `exercise` check. Both build diet-drive and diet (the run's assembly checks its log with diet) unless DIET_DRIVE names a drive. Both refuse a scrub that left a home or temp
// path; `write` refuses this machine's user or host name too.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { appendFileSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { hostname, tmpdir, userInfo } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { KITCHEN_SINK_SCENARIO, fails, kitchenSinkExample, sinkVerdict } from '../src/drive/kitchen-sink.ts';
import { imagesOf, residue, scrub } from '../src/drive/scenario-log.ts';
import { serialize } from '../src/replay/payload.ts';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const SCENARIO = KITCHEN_SINK_SCENARIO;
const EXAMPLE = 'exercise/src/drive/examples/kitchen-sink.json';
/** The images the example publishes beside it, by digest (#372's layout): `<dir>/<name>/files/<sha256>`. */
const FILES = 'exercise/src/drive/examples/kitchen-sink/files';
const FIX = 'node exercise/scripts/kitchen-sink.mjs write, then python3 exercise/scripts/admission.py admit kitchen-sink';

const REGENERATE = 'exercise/scripts/regenerate-kitchen-sink.sh, from develop\'s head';

const [mode, ...flags] = process.argv.slice(2);
const current = flags.includes('--current');
if ((mode !== 'write' && mode !== 'check') || flags.some((flag) => flag !== '--current') || (mode === 'write' && current)) {
  console.error('usage: kitchen-sink.mjs write | check [--current]');
  process.exit(2);
}
/** The scenario file's digest: what the example records it was written from. */
const scenarioSha256 = createHash('sha256').update(readFileSync(path.join(ROOT, SCENARIO))).digest('hex');

function drive() {
  if (process.env.DIET_DRIVE) return process.env.DIET_DRIVE;
  execFileSync('cargo', ['build', '--locked', '--quiet', '-p', 'discipline-diet', '--bin', 'diet-drive', '--bin', 'diet'], { cwd: ROOT, stdio: 'inherit' });
  return path.join(process.env.CARGO_TARGET_DIR ?? path.join(ROOT, 'target'), 'debug', 'diet-drive');
}

/** The scenario's log, as written today, scrubbed; and the bytes of each image it names, read from the run's files and
 * checked against their digests. */
function written(withImages) {
  const scratch = mkdtempSync(path.join(tmpdir(), 'kitchen-sink-'));
  // The worktree at a path of one length on every machine (`/tmp/ks-` and six characters, as given: diet does not
  // resolve it), so every prompt and output that names it is one size, and the check compares sizes (#851).
  const tree = mkdtempSync('/tmp/ks-');
  try {
    const out = path.join(scratch, 'run');
    execFileSync(drive(), ['scenario', SCENARIO, '--worktree', tree, '--out', out], { cwd: ROOT, stdio: ['ignore', 'ignore', 'inherit'] });
    const lines = readFileSync(path.join(out, 'session.log'), 'utf8').trimEnd().split('\n').map((line) => JSON.parse(line));
    const log = scrub(lines, { [tree]: '/work/scenario-worktree' });
    const images = new Map();
    for (const sha256 of withImages ? imagesOf(log) : []) {
      const file = path.join(out, 'files', sha256);
      if (!existsSync(file)) throw new Error(`the run names image ${sha256} and its --out has no files/${sha256}: diet-drive scenario must keep the recording's files/`);
      const bytes = readFileSync(file);
      const got = createHash('sha256').update(bytes).digest('hex');
      if (got !== sha256) throw new Error(`files/${sha256} hashes to ${got}, not its name`);
      images.set(sha256, bytes);
    }
    return { log, images };
  } finally {
    if (scratch.startsWith(tmpdir()) && existsSync(scratch)) rmSync(scratch, { recursive: true, force: true });
    if (/^\/tmp\/ks-[A-Za-z0-9]{6}$/.test(tree) && existsSync(tree)) rmSync(tree, { recursive: true, force: true });
  }
}

let log;
let images;
try {
  ({ log, images } = written(mode === 'write'));
} catch (err) {
  console.error(`kitchen-sink: ${err.message}`);
  process.exit(1);
}
const left = residue(JSON.stringify(log), mode === 'write' ? [userInfo().username, hostname()].filter((word) => word.length > 2) : []);
if (left.length > 0) {
  console.error(`kitchen-sink: the scrub left ${left.length} private string(s); nothing is ${mode === 'write' ? 'written' : 'compared'}:`);
  for (const found of left) console.error(`  ${found.replace(userInfo().username, '<user>').replace(hostname(), '<host>')}`);
  process.exit(1);
}

if (mode === 'write') {
  // The example it replaces: a person's Declared-clean lines kept for the images still named (the script never writes
  // one), and its clocks carried.
  const before = existsSync(path.join(ROOT, EXAMPLE)) ? JSON.parse(readFileSync(path.join(ROOT, EXAMPLE), 'utf8')) : undefined;
  const example = kitchenSinkExample(log, before, [...images.keys()], scenarioSha256);
  writeFileSync(path.join(ROOT, EXAMPLE), serialize(example));
  // Exactly the images it names: a file left from an earlier write is removed, as admission would refuse it.
  const files = path.join(ROOT, FILES);
  mkdirSync(files, { recursive: true });
  for (const stale of readdirSync(files)) if (!images.has(stale)) rmSync(path.join(files, stale));
  for (const [sha256, bytes] of images) writeFileSync(path.join(files, sha256), bytes);
  console.log(`kitchen-sink: wrote ${EXAMPLE} (${log.length} lines); now admit it: python3 exercise/scripts/admission.py admit kitchen-sink`);
} else {
  const published = JSON.parse(readFileSync(path.join(ROOT, EXAMPLE), 'utf8'));
  const verdict = sinkVerdict(published, log, scenarioSha256);
  const built = published.log?.[0]?.build_commit ?? 'an unrecorded commit';
  const shown = (lane) => (lane.length > 600 ? `${lane.slice(0, 600)}…` : lane);
  if (verdict.verdict === 'unwritten') {
    console.error(
      `kitchen-sink: ${SCENARIO} is not the file ${EXAMPLE} was written from (sha256 ${verdict.recorded ?? 'unrecorded'}, today ${verdict.today}): a PR that changes the scenario writes the example too`,
    );
    console.error(`kitchen-sink: ${FIX}`);
    process.exit(1);
  }
  if (verdict.verdict === 'stale') {
    const said = `${EXAMPLE} is STALE: built at ${built}, and the code writes something else in ${verdict.lanes.length} lane(s)`;
    console.error(`kitchen-sink: ${said}:`);
    for (const lane of verdict.lanes) console.error(`  ${shown(lane)}`);
    console.error(`kitchen-sink: ${current ? 'asked for the current example: ' : 'not failed (the sink lags); '}it is caught up by ${REGENERATE}`);
    // A CI run shows it where a reviewer looks: an annotation, and the job's summary with the lane diff.
    if (process.env.GITHUB_ACTIONS === 'true') {
      console.log(`::warning title=Kitchen sink stale::${said}. Caught up by ${REGENERATE}.`);
      if (process.env.GITHUB_STEP_SUMMARY) {
        appendFileSync(process.env.GITHUB_STEP_SUMMARY, `### Kitchen sink stale\n\n${said}:\n\n${verdict.lanes.map((lane) => `- \`${shown(lane).replaceAll('`', "'")}\``).join('\n')}\n\nCaught up by ${REGENERATE}.\n`);
      }
    }
    process.exit(fails(verdict, { current }) ? 1 : 0);
  }
  console.log(`kitchen-sink: ${EXAMPLE} is what ${SCENARIO} writes today (${log.length} lines)`);
}
