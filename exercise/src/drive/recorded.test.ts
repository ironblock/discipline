import { describe, expect, it, vi } from 'vitest';

import { fold } from '../session/fold.ts';
import type { LogLine } from './log.ts';
import { RECORDINGS, ReplayTransport, placed, recordedAt } from './recorded.ts';
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

/** The kinds a recording's `migration` header says it carries under their own name, with their counts. */
function declaredUnknown(migration: readonly string[]): [string, number][] {
  const line = migration.find((m) => m.includes('carried under their own name'));
  return line ? [...line.matchAll(/'([^']+)': (\d+)/g)].map(([, kind, n]) => [kind!, Number(n)]) : [];
}

describe('replaying a recording', () => {
  it.each(Object.keys(RECORDINGS) as (keyof typeof RECORDINGS)[])('%s replays whole, in order, and folds with nothing unknown its header does not declare', (name) => {
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
      expect([...fold(seen).unknown]).toEqual(declaredUnknown(recording.migration));
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
