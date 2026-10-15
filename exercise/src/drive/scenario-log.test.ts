import { describe, expect, it } from 'vitest';

import type { LogLine } from './log.ts';
import { byLane, declarationsKept, imagesOf, laneDiff, residue, scrub } from './scenario-log.ts';

/**
 * The kitchen sink's log as a scenario writes it (#798): scrubbed of the run's own paths before it is published, and
 * compared with what the scenario writes today lane by lane -- each lane's lines in order, how the lanes interleave,
 * `t`, `opened` and `seq` itself left out, and every reference between lines read as its target's lane and place in it.
 */
const START = { seq: 0, t: 0, kind: 'session.start', version: 8, opened: 1, model: 'm', head: [{ role: 'system', content: 's' }] };
const TRUNK_ASK = [
  START,
  { seq: 1, t: 1, kind: 'ask', turn: 1, text: 'go' },
  { seq: 2, t: 2, kind: 'request', lane: 'trunk', turn: 1 },
  { seq: 3, t: 3, kind: 'fork', lane: 'interview', of_turn: 1, why: 'cadence', question: 'q', at: 2 },
  { seq: 4, t: 4, kind: 'request', lane: 'interview', fork: 3, turn: 1 },
  { seq: 5, t: 5, kind: 'response', to_request: 2, text: 'trunk answer' },
  { seq: 6, t: 6, kind: 'response', to_request: 4, text: 'fork answer' },
  { seq: 7, t: 7, kind: 'fork.settled', fork: 3, outcome: 'value' },
] as unknown as LogLine[];

/** The same session, its lanes interleaved otherwise, renumbered and on another clock. */
const INTERLEAVED = [
  { ...START, opened: 99 },
  { seq: 10, t: 50, kind: 'ask', turn: 1, text: 'go' },
  { seq: 11, t: 51, kind: 'request', lane: 'trunk', turn: 1 },
  { seq: 12, t: 52, kind: 'fork', lane: 'interview', of_turn: 1, why: 'cadence', question: 'q', at: 11 },
  { seq: 13, t: 53, kind: 'request', lane: 'interview', fork: 12, turn: 1 },
  { seq: 14, t: 54, kind: 'response', to_request: 13, text: 'fork answer' },
  { seq: 15, t: 55, kind: 'fork.settled', fork: 12, outcome: 'value' },
  { seq: 16, t: 56, kind: 'response', to_request: 11, text: 'trunk answer' },
] as unknown as LogLine[];

describe('the kitchen sink, compared lane by lane', () => {
  it('sorts lines into lanes: the session’s, the trunk’s, and one per fork', () => {
    expect(Object.keys(byLane(TRUNK_ASK)).sort()).toEqual(['fork:interview|1|cadence|q#0', 'session', 'trunk']);
  });

  it('holds two runs equal that differ only in interleaving, numbering and clock', () => {
    expect(laneDiff(TRUNK_ASK, INTERLEAVED)).toEqual([]);
  });

  it('compares a digest: a scenario shows its model the same bytes on any machine (#859)', () => {
    const digest = (hex: string) => INTERLEAVED.map((line) => (line.seq === 16 ? { ...line, text: `saved at /scenario/recording/files/${hex.repeat(64)}` } : line)) as LogLine[];
    expect(laneDiff(digest('a'), digest('a'))).toEqual([]);
    expect(laneDiff(digest('a'), digest('b')).join('\n')).toMatch(/trunk.*response/);
  });

  it('compares sizes, and leaves the build out: a scenario’s prompts and outputs are path-free (#859)', () => {
    const sized = (n: number, build: string) =>
      INTERLEAVED.map((line) =>
        line.seq === 12 ? { ...line, reserved: 900 + n, carried_tokens: 372 + n, text_bytes: 534 + n } : line.seq === 16 ? { ...line, text: `${761 + n} bytes, saved` } : line.kind === 'session.start' ? { ...line, build_commit: build, build_dirty: n > 0 } : line,
      ) as LogLine[];
    expect(laneDiff(sized(0, 'aaa'), sized(0, 'bbb'))).toEqual([]);
    expect(laneDiff(sized(0, 'aaa'), sized(2, 'aaa')).join('\n')).toMatch(/reserved/);
    // A turn is structure, not a size: it is compared.
    const moved = INTERLEAVED.map((line) => (line.seq === 12 ? { ...line, of_turn: 2 } : line)) as LogLine[];
    expect(laneDiff(INTERLEAVED, moved)).not.toEqual([]);
  });

  it('reads a trigger that names a line by its seq as that line’s lane and place', () => {
    const triggered = (log: readonly LogLine[], at: number) => log.map((line) => (line.kind === 'fork' ? { ...line, trigger: `open:${at}` } : line)) as LogLine[];
    expect(laneDiff(triggered(TRUNK_ASK, 2), triggered(INTERLEAVED, 11))).toEqual([]);
    expect(laneDiff(triggered(TRUNK_ASK, 2), triggered(INTERLEAVED, 13)).join('\n')).toMatch(/trigger/);
  });

  it('gives a background job its own lane: when it ends against the session is a race, not a lever', () => {
    const ended = { kind: 'background.ended', job: 'bg_1', status: 'exited' };
    const settled = { kind: 'turn.settled', turn: 1, reason: 'final' };
    const before = [...TRUNK_ASK, { seq: 8, t: 8, ...ended }, { seq: 9, t: 9, ...settled }] as LogLine[];
    const after = [...TRUNK_ASK, { seq: 8, t: 8, ...settled }, { seq: 9, t: 9, ...ended }] as LogLine[];
    expect(laneDiff(before, after)).toEqual([]);
    expect(Object.keys(byLane(before))).toContain('background:bg_1');
  });

  it('fails on a reference to the wrong line, naming the lane', () => {
    const misdirected = INTERLEAVED.map((line) => (line.seq === 16 ? { ...line, to_request: 13 } : line)) as LogLine[];
    expect(laneDiff(TRUNK_ASK, misdirected).join('\n')).toMatch(/trunk/);
  });

  it('fails on a lever’s line changed or gone, naming the lane and kind', () => {
    expect(laneDiff(TRUNK_ASK, INTERLEAVED.filter((line) => line.kind !== 'fork.settled')).join('\n')).toMatch(/fork:interview.*fork\.settled/);
    const said = INTERLEAVED.map((line) => (line.seq === 16 ? { ...line, text: 'another answer' } : line)) as LogLine[];
    expect(laneDiff(TRUNK_ASK, said).join('\n')).toMatch(/trunk.*response/);
  });
});

describe('the scrub', () => {
  // Built, not written: a literal home or temp path here is what the tree's hygiene gate forbids.
  const home = ['', 'Users', 'someone', 'notes.md'].join('/');
  const temp = ['', 'var', 'folders', 'ab', 'cd1234', 'T'].join('/');
  const planted = [
    START,
    { seq: 1, t: 1, kind: 'tool_call', request: 0, turn: 1, id: 'c', name: 'read', arguments: `{"path": "${home}"}`, outcome: 'ran', cwd: `${temp}/diet-scenario-worktree-74285-1792008371311169000` },
    { seq: 2, t: 2, kind: 'notice', turn: 1, text: `<pid>91758</pid> job bg_8afc88f7 (pid: 91704) wrote /private${temp}/diet-scenario-work-74285-1792008371309696000/background/shell-bg_8afc88f7.output` },
    { seq: 3, t: 3, kind: 'tool_call', request: 0, turn: 1, id: 'd', name: 'bash', arguments: '{}', outcome: 'ran', cwd: ['', 'tmp', 'diet-scenario-worktree-1-2'].join('/') },
  ] as unknown as LogLine[];

  it('finds a home path and a temp path before it, and none after', () => {
    const before = JSON.stringify(planted);
    expect(residue(before).length).toBeGreaterThanOrEqual(3);
    const after = JSON.stringify(scrub(planted));
    expect(residue(after)).toEqual([]);
    expect(after).toContain('/work/scenario-worktree');
    expect(after).toContain('/work/scenario-work/background/shell-bg_1.output');
    expect(after).toContain('/work/user/notes.md');
    expect(after).toContain('<pid>1</pid> job bg_1 (pid: 2) wrote');
  });

  it('writes a path the caller names as what it was: the worktree the run was given', () => {
    const tree = ['', 'tmp', 'ks-AbC123'].join('/');
    const given = [START, { seq: 1, t: 1, kind: 'tool_call', request: 0, turn: 1, id: 'c', name: 'bash', arguments: '{}', outcome: 'ran', cwd: tree, stdout: `${tree}/notes.md` }] as unknown as LogLine[];
    const after = JSON.stringify(scrub(given, { [tree]: '/work/scenario-worktree' }));
    expect(after).toContain('"cwd":"/work/scenario-worktree"');
    expect(after).toContain('/work/scenario-worktree/notes.md');
    expect(residue(after)).toEqual([]);
  });

  it('finds what the caller names, such as this machine’s user and host', () => {
    expect(residue('ran as a-user on a-host', ['a-user', 'a-host'])).toEqual(['a-user', 'a-host']);
  });
});

describe('the files an example publishes', () => {
  const png = (hex: string) => ({ sha256: hex.repeat(64), media_type: 'image/png', bytes: 12, path: `files/${hex.repeat(64)}` });
  const text = { sha256: 'f'.repeat(64), media_type: 'text/plain', bytes: 9, path: `files/${'f'.repeat(64)}` };
  const log = [
    START,
    { seq: 1, t: 1, kind: 'ask', turn: 1, text: 'see', files: [png('a')] },
    { seq: 2, t: 2, kind: 'tool_call', request: 0, turn: 1, id: 'r', name: 'read', outcome: 'ran', files: [png('a'), text] },
    { seq: 3, t: 3, kind: 'background.ended', job: 'bg_1', files: [png('b'), text] },
  ] as unknown as LogLine[];

  it('are the images the log names, each once, in order: never the outputs saved by digest, which hold the run’s paths', () => {
    expect(imagesOf(log)).toEqual(['a'.repeat(64), 'b'.repeat(64)]);
  });
});

describe('a regenerated example keeps what a person declared', () => {
  const a = 'a'.repeat(64);
  const b = 'b'.repeat(64);
  it('keeps each Declared-clean line for an image the log still names, and drops the rest', () => {
    const before = ['Authored: x', `Declared-clean: ${a} by Someone on 2026-10-14`, `Declared-clean: ${b} by Someone on 2026-10-14`, 'retimed: y'];
    expect(declarationsKept(before, [a])).toEqual([`Declared-clean: ${a} by Someone on 2026-10-14`]);
    expect(declarationsKept([], [a])).toEqual([]);
  });
});
