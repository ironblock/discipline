import { describe, expect, it, vi } from 'vitest';

import { fold } from '../session/fold.ts';
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
    for (const log of [recording.events, SPECIMEN.flatMap((beat) => beat.events)] as const) {
      const events = log as readonly Record<string, unknown>[];
      const outcomes = new Set(events.filter((e) => e['kind'] === 'fork.settled').map((e) => e['outcome']));
      expect([...outcomes].filter((o) => !RULED_OUTCOMES.includes(o as string))).toEqual([]);
      const patches = events.filter((e) => e['kind'] === 'patch');
      expect([...new Set(patches.map((p) => p['op']))].filter((op) => !RULED_OPS.includes(op as string))).toEqual([]);
      expect(patches.filter((p) => 'provenance' in p)).toEqual([]);
    }
  });

  it('carries no home-directory path and no internal ticket id', () => {
    const text = JSON.stringify(recording);
    // Report the match, not the 600 KB it was found in.
    expect(/(\/Users\/|\/home\/)[A-Za-z0-9._-]+/.exec(text)?.[0]).toBeUndefined();
    expect(/(^|[^A-Za-z0-9])DIE-?[0-9]+/i.exec(text)?.[0]).toBeUndefined();
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

describe('replaying a recording', () => {
  it('survives a subscribe, close and subscribe again -- a StrictMode remount -- and carries on from where it stopped', async () => {
    vi.useFakeTimers();
    try {
      const transport = new ReplayTransport(recording, { speed: 1 });
      const first: string[] = [];
      transport.subscribe((e) => first.push(e.kind)).call(undefined);
      transport.close();
      const seen: string[] = [];
      transport.subscribe((e) => seen.push(e.kind));
      await vi.advanceTimersByTimeAsync(40_000);
      expect(seen).toContain('ask');
      expect(seen.filter((k) => k === 'ask').length).toBe(2);
      expect(await transport.dispatch()).toEqual({ ok: false, refused: 'recording' });
      transport.close();
    } finally {
      vi.useRealTimers();
    }
  });
});
