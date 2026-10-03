import { describe, expect, it, vi } from 'vitest';

import { fold } from '../session/fold.ts';
import type { LogLine } from './log.ts';
import { type Recording, ReplayTransport, load, placed, recordedAt } from './recorded.ts';
import { RECORDINGS } from './recordings.ts';
import { SPECIMEN } from './specimen.ts';

const recording = RECORDINGS['first-drive'];

/** The rulings, restated here on purpose: the vocabulary's own types are open sets and accept anything. */
const RULED_OUTCOMES = ['value', 'decline', 'mimicry', 'unparseable', 'thinking_exhausted', 'rejected', 'timeout', 'truncated', 'output_too_large'];
const RULED_OPS = ['add', 'supersede', 'resolve', 'retire', 'park', 'edit'];

describe('the first drive, recorded and migrated', () => {
  it('says what the migration decided rather than the record', () => {
    expect(recording.migration.length).toBeGreaterThan(4);
    expect(recording.migration.join(' ')).toMatch(/mimicry/);
  });

  it('speaks the ruled vocabulary (#117, 2026-09-26): the one outcome enum, diet\'s ops, authority for how an entry was known', () => {
    for (const log of [...Object.values(RECORDINGS).map((rec) => rec.events), SPECIMEN.flatMap((beat) => beat.events)]) {
      const events = log as readonly Record<string, unknown>[];
      const outcomes = new Set(events.filter((e) => e['kind'] === 'fork.settled').map((e) => e['outcome']));
      expect([...outcomes].filter((o) => !RULED_OUTCOMES.includes(o as string))).toEqual([]);
      const patches = events.filter((e) => e['kind'] === 'patch');
      expect([...new Set(patches.map((p) => p['op']))].filter((op) => !RULED_OPS.includes(op as string))).toEqual([]);
      expect(patches.filter((p) => 'provenance' in p)).toEqual([]);
    }
  });

  it('carries nothing the migrations refuse on, in any recording: home paths, ticket ids, e-mail addresses, private addresses', () => {
    // The patterns are `leaks()` in scripts/migrate-recorded.py, checked again here on what was committed.
    const leaks = [
      /(\/Users\/|\/home\/)[A-Za-z0-9._-]+/,
      /(^|[^A-Za-z0-9])DIE-?[0-9]+/i,
      /[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/,
      /\b(?:10|192\.168|172\.(?:1[6-9]|2\d|3[01]))(?:\.\d{1,3}){2,3}\b/,
    ];
    for (const [name, rec] of Object.entries(RECORDINGS)) {
      const text = JSON.stringify(rec);
      // Report the match, not the 600 KB it was found in.
      for (const leak of leaks) expect(leak.exec(text)?.[0], name).toBeUndefined();
    }
  });

  it('folds whole with nothing unknown: three eras, every side call hung off a trunk node', () => {
    const s = fold(placed(recording));
    expect(s.unknown.size).toBe(0);
    expect(s.eras).toHaveLength(3);
    expect(s.eras.every((era) => era.nodes.length > 0)).toBe(true);
    const trunk = new Set(s.eras.flatMap((era) => era.nodes.map((n) => n.id)));
    const branches = [...s.branches.values()].flat();
    expect(branches.length).toBe(94);
    expect(branches.filter((b) => !trunk.has(b.at))).toEqual([]);
    expect(new Set(branches.map((b) => b.lane))).toEqual(new Set(['interview', 'extraction', 'ratify']));
  });

  it('keeps its failures: side calls that answered as the agent, and commands that failed', () => {
    const s = fold(placed(recording));
    const branches = [...s.branches.values()].flat();
    expect(branches.filter((b) => b.outcome === 'mimicry').length).toBe(7);
    const tools = s.eras.flatMap((era) => era.nodes).filter((n) => n.kind === 'tool');
    expect(tools.some((t) => t.kind === 'tool' && t.exit === 128)).toBe(true);
  });

  it('is in order, and stops cleanly at any moment', () => {
    const log = placed(recording);
    expect(log.every((e, i) => i === 0 || log[i - 1]!.t <= e.t)).toBe(true);
    const ratifying = fold(recordedAt(recording, 530_000));
    expect(ratifying.state).toBe('ratify');
    expect(ratifying.occupancy.some((h) => h?.lane === 'ratify')).toBe(true);
  });
});

describe('reading a recording', () => {
  it('names the recording’s file when it fails: an event with no kind, broken JSON, a missing header (#32, ruling 9)', () => {
    const good = JSON.stringify({ title: 't', migration: [], carried: {}, events: [{ kind: 'ask', t: 0 }] });
    expect(load('x', good).title).toBe('t');
    expect(() => load('x', JSON.stringify({ title: 't', migration: [], carried: {}, events: [{ t: 0 }] }))).toThrow('exercise/src/drive/recorded/x.json: not a recording: event 0 has no kind or time');
    expect(() => load('x', '{"title":')).toThrow(/^exercise\/src\/drive\/recorded\/x\.json: not a recording: not JSON/);
    expect(() => load('x', '{}')).toThrow('exercise/src/drive/recorded/x.json: not a recording: expected title, migration and events');
    // `carried` is the count a fold is checked against (#173): absent, or not {kind: count}, is no recording.
    const carrying = (carried: unknown) => JSON.stringify({ title: 't', migration: [], carried, events: [{ kind: 'ask', t: 0 }] });
    const noCarried = 'exercise/src/drive/recorded/x.json: not a recording: expected carried: {kind: count}, each kind named and each count a whole number above 0';
    expect(() => load('x', JSON.stringify({ title: 't', migration: [], events: [{ kind: 'ask', t: 0 }] }))).toThrow(noCarried);
    for (const bad of [null, [], 'compaction', { compaction: 0 }, { compaction: 1.5 }, { compaction: '1' }, { '': 1 }]) {
      expect(() => load('x', carrying(bad)), JSON.stringify(bad)).toThrow(noCarried);
    }
    expect(load('x', carrying({ compaction: 2 })).carried).toEqual({ compaction: 2 });
  });
});

describe('the other recordings: where the first drive never went', () => {
  it('a capture round the person cancelled: carried under its own name, which this vocabulary does not have yet', () => {
    const s = fold(placed(RECORDINGS['cancelled-capture']));
    expect(s.state).toBe('ended');
    expect([...s.unknown]).toEqual([['capture.cancelled', 1]]);
    expect(RECORDINGS['cancelled-capture'].migration.join(' ')).toMatch(/capture\.cancelled/);
  });

  it('a turn stopped at the step limit: the turn end says so', () => {
    const s = fold(placed(RECORDINGS['step-limit']));
    const ends = s.eras.flatMap((era) => era.nodes).filter((n) => n.kind === 'settled');
    expect(ends.map((n) => n.kind === 'settled' && n.reason)).toEqual(['max_steps']);
    expect(s.unknown.size).toBe(0);
  });
});

/**
 * A recording's `carried` against what a fold of its events leaves unknown
 * (#173): as objects, so kinds and counts are compared, not the order a
 * migration happened to count them in.
 */
function expectCarried(name: string, recording: Recording, seen: readonly LogLine[]) {
  expect(Object.fromEntries(fold(seen).unknown), `${name}: its carried field disagrees with its events`).toEqual(recording.carried);
}

describe('a recording\'s carried field', () => {
  it('is compared by kind and count, not by the order the migration counted them in', () => {
    // A real capture with a second unknown kind first seen BEFORE its own:
    // the fold meets them in one order, the field lists them in the other.
    const base = RECORDINGS['cancelled-capture'];
    const at = base.events.findIndex((e) => e.kind === 'session.start') + 1;
    const events = [...base.events.slice(0, at), { kind: 'zz.newer', t: base.events[at - 1]!.t }, ...base.events.slice(at)];
    const recording: Recording = { ...base, carried: { 'capture.cancelled': 1, 'zz.newer': 1 }, events: events as Recording['events'] };
    expect([...fold(placed(recording)).unknown.keys()]).toEqual(['zz.newer', 'capture.cancelled']);
    expectCarried('two kinds', recording, placed(recording));
    expect(() => expectCarried('two kinds', { ...recording, carried: { 'capture.cancelled': 1, 'zz.newer': 2 } }, placed(recording))).toThrow(
      'two kinds: its carried field disagrees with its events',
    );
  });
});

describe('replaying a recording', () => {
  it.each(Object.keys(RECORDINGS) as (keyof typeof RECORDINGS)[])('%s replays whole, in order, and folds to exactly what its carried field declares unknown', (name) => {
    vi.useFakeTimers();
    try {
      const recording = RECORDINGS[name];
      const log = placed(recording);
      const transport = new ReplayTransport(recording, { speed: 1 });
      const seen: LogLine[] = [];
      transport.subscribe((line) => seen.push(line));
      vi.advanceTimersByTime(log.at(-1)!.t);
      transport.close();
      // Where and when each line landed, not the 600 KB of what it says: placement is tested in `place`, delivery here.
      const at = (lines: readonly LogLine[]) => lines.map((line) => `${line.seq} ${line.kind} ${line.t}`).join('\n');
      expect(at(seen)).toBe(at(log));
      expect(seen.filter((line, i) => line.seq !== i)).toEqual([]);
      expectCarried(name, recording, seen);
    } finally {
      vi.useRealTimers();
    }
  });

  it('survives a subscribe, close and subscribe again -- a StrictMode remount -- and carries on from where it stopped', async () => {
    vi.useFakeTimers();
    try {
      const transport = new ReplayTransport(recording, { speed: 1 });
      const first: LogLine[] = [];
      const unsubscribe = transport.subscribe((line) => first.push(line));
      await vi.advanceTimersByTimeAsync(20_000);
      unsubscribe();
      transport.close();
      expect(first.length).toBeGreaterThan(0);
      // Closed, nothing plays.
      await vi.advanceTimersByTimeAsync(20_000);
      const seen: LogLine[] = [];
      transport.subscribe((line) => seen.push(line));
      // Again: what was played, at once, and then on from there -- not from the start, and nothing twice.
      expect(seen.map((l) => l.seq)).toEqual(first.map((l) => l.seq));
      // Its clock resumes at the last line played: the wait to the next is the recording's, in full.
      await vi.advanceTimersByTimeAsync(40_000);
      expect(seen.length).toBeGreaterThan(first.length);
      expect(seen.every((line, i) => line.seq === i)).toBe(true);
      expect(seen.filter((l) => l.kind === 'ask').length).toBe(2);
      expect(await transport.dispatch()).toEqual({ ok: false, refused: 'recording' });
      transport.close();
    } finally {
      vi.useRealTimers();
    }
  });
});
