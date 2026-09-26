import { describe, expect, it } from 'vitest';

import { beatLength, snapshot } from '../drive/canned.ts';
import { SPECIMEN } from '../drive/specimen.ts';
import type { DriveEvent } from '../drive/events.ts';
import { fold } from './fold.ts';

const at = (beat: number, t?: number) => fold(snapshot(SPECIMEN, t === undefined ? { beat } : { beat, t }));
const beat = (n: number) => SPECIMEN[n]!;

describe('fold over the specimen', () => {
  it('opens awaiting the first ask, with one era and no nodes', () => {
    const s = at(1);
    expect(s.state).toBe('awaiting');
    expect(s.eras).toHaveLength(1);
    expect(s.eras[0]?.nodes).toHaveLength(0);
    expect(s.phase).toBe('spec');
  });

  it('is mid-turn while the trunk streams, and the slot says who holds it', () => {
    const s = at(2, 20_000);
    expect(s.state).toBe('turn');
    const last = s.eras[0]?.nodes.at(-1);
    expect(last?.kind).toBe('assistant');
    expect(last?.kind === 'assistant' && last.progress).toBe('streaming');
    expect(s.occupancy[0]).toEqual({ id: 'q/3', lane: 'trunk' });
    expect(s.occupancy[1]).toBeUndefined();
  });

  it('tracks a generation\'s last sign of life, so silence -- not length -- is what worries', () => {
    const s = at(2, 22_000);
    const node = s.eras[0]?.nodes.at(-1);
    expect(node?.kind).toBe('assistant');
    if (node?.kind !== 'assistant') return;
    expect(node.timings).toBeUndefined();
    expect(node.lastActivityAt).toBeGreaterThan(node.startedAt);
    expect(s.now - node.lastActivityAt).toBeLessThan(1_000);
  });

  it('is in capture after settlement, with the interview in slot 1 hung off the tool call it was about', () => {
    const s = at(2, 28_000);
    expect(s.state).toBe('capture');
    expect(s.occupancy[1]).toEqual({ id: 'i/1', lane: 'interview' });
    const branch = s.branches.get('t/2')?.[0];
    expect(branch?.lane).toBe('interview');
    expect(branch?.slot).toBe(1);
  });

  it('anchors a branch named by a response id to its trunk request', () => {
    const s = at(2);
    expect(s.branches.get('q/3')?.[0]?.id).toBe('i/2');
  });

  it('lands patches in working memory, fresh until the next ask', () => {
    const settled = at(2);
    expect(settled.state).toBe('awaiting');
    expect(settled.memory.map((m) => m.id)).toEqual(['f1', 'f2', 'f3', 'd1', 'd2', 'o1']);
    expect(settled.memory.every((m) => m.fresh)).toBe(true);
    const next = at(3, 100);
    expect(next.memory.every((m) => !m.fresh)).toBe(true);
  });

  it('supersedes an open question and retires at ratify, keeping both visible', () => {
    const s = at(4);
    const byId = new Map(s.memory.map((m) => [m.id, m]));
    expect(byId.get('o1')?.state).toBe('superseded');
    expect(byId.get('d3')?.state).toBe('live');
    expect(byId.get('f3')?.state).toBe('retired');
  });

  it('is ratifying while the seam-time fork runs', () => {
    const s = at(4, 1000);
    expect(s.state).toBe('ratify');
    expect(s.occupancy[1]).toEqual({ id: 'r/1', lane: 'ratify' });
  });

  it('opens a second era at the seam, whose system prompt is the render', () => {
    const s = at(4);
    expect(s.eras).toHaveLength(2);
    expect(s.phase).toBe('build');
    const era = s.eras[1]!;
    expect(era.system.render).toBe(1);
    expect(era.seam?.phase).toEqual({ from: 'spec', to: 'build' });
    expect(era.seam?.prefixAfter).toBeLessThan(era.seam?.prefixBefore ?? 0);
  });

  it('prices the build turn ask against the refilled prefix, not the transcript', () => {
    const s = at(5);
    const ask = s.eras[1]?.nodes[0];
    expect(ask?.kind === 'user' && ask.prefill).toEqual({ fresh: 12, cached: 1512 });
  });

  it('runs an interview in the idle gap of a running tool call', () => {
    const s = at(5, 12_000);
    expect(s.state).toBe('turn');
    const tool = s.eras[1]?.nodes.find((n) => n.kind === 'tool' && n.id === 't/5');
    expect(tool?.kind === 'tool' && tool.running).toBe(true);
    expect(s.occupancy[1]).toEqual({ id: 'i/4', lane: 'interview' });
  });

  it('ends the specimen settled, every node carrying where it came from and what it needs', () => {
    const s = at(SPECIMEN.length);
    expect(s.state).toBe('awaiting');
    for (const era of s.eras) {
      for (const node of era.nodes) {
        expect(node.from.length).toBeGreaterThan(0);
        expect(node.needs.length).toBeGreaterThan(0);
      }
    }
    expect(beatLength(beat(4))).toBeGreaterThan(30_000);
  });
});

describe('fold over what it does not know', () => {
  const withEvent = (extra: Record<string, unknown>) => {
    const log = [...snapshot(SPECIMEN, { beat: 2 })];
    const placed = { t: log.at(-1)!.t, seq: log.length, ...extra } as unknown as DriveEvent;
    return fold([...log, placed]);
  };

  it('keeps and counts an event kind it does not know, and folds the rest as before', () => {
    const plain = fold(snapshot(SPECIMEN, { beat: 2 }));
    const s = withEvent({ kind: 'gate.verdict', verdict: 'pass' });
    expect(s.unknown.get('gate.verdict')).toBe(1);
    expect(s.eras[0]?.nodes.map((n) => n.id)).toEqual(plain.eras[0]?.nodes.map((n) => n.id));
    expect(plain.unknown.size).toBe(0);
  });

  it('carries a patch op it does not know onto the entry, by name, without changing its state', () => {
    const log = [...snapshot(SPECIMEN, { beat: 2 })];
    const add = log.find((e) => e.kind === 'patch');
    expect(add?.kind).toBe('patch');
    if (add?.kind !== 'patch') return;
    const s = withEvent({ kind: 'patch', id: 'p/park', from: add.from, op: 'park', entry: { ...add.entry, text: 'parked for later' } });
    const entry = s.memory.find((m) => m.id === add.entry.id);
    expect(entry?.state).toBe('live');
    expect(entry?.op).toBe('park');
    expect(entry?.text).toBe('parked for later');
  });
});

describe('fold over what went wrong', () => {
  /** The specimen at a cursor, edited: the same helper stories use, without a DOM. */
  const variant = (beatNo: number, t: number | undefined, edit: (e: DriveEvent) => DriveEvent | readonly DriveEvent[]) => {
    const log = snapshot(SPECIMEN, t === undefined ? { beat: beatNo } : { beat: beatNo, t }).flatMap((e) => edit(e));
    return fold(log.map((e, seq) => ({ ...e, seq }) as DriveEvent));
  };

  it('fails a request the server gave up on: the answer so far stays, the reason and message are carried, the slot is free', () => {
    const s = variant(2, 22_000, (e) => e);
    const streaming = s.eras[0]?.nodes.at(-1);
    expect(streaming?.kind).toBe('assistant');
    if (streaming?.kind !== 'assistant') return;
    const log = [...snapshot(SPECIMEN, { beat: 2, t: 22_000 })];
    const failed = { kind: 'request.failed', t: 22_100, seq: log.length, request: streaming.id, reason: 'context_overflow', message: 'the prompt no longer fits in the context' } as DriveEvent;
    const f = fold([...log, failed]);
    const node = f.eras[0]?.nodes.find((n) => n.id === streaming.id);
    expect(node?.kind === 'assistant' && node.progress).toBe('failed');
    expect(node?.kind === 'assistant' && node.failure).toEqual({ reason: 'context_overflow', message: 'the prompt no longer fits in the context' });
    expect(node?.kind === 'assistant' && node.text).toBe(streaming.text);
    expect(f.occupancy[0]).toBeUndefined();
  });

  it('marks the end of a turn that did not end on its own: the step limit, a timeout', () => {
    const s = variant(2, undefined, (e) => (e.kind === 'turn.settled' ? { ...e, reason: 'max_steps' } : e));
    const last = s.eras[0]?.nodes.at(-1);
    expect(last?.kind).toBe('settled');
    expect(last?.kind === 'settled' && last.reason).toBe('max_steps');
    const plain = variant(2, undefined, (e) => e);
    expect(plain.eras[0]?.nodes.some((n) => n.kind === 'settled')).toBe(false);
  });
});
