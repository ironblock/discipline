import { describe, expect, it } from 'vitest';

import { beatLength, snapshot } from '../drive/canned.ts';
import { SPECIMEN } from '../drive/specimen.ts';
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
    expect(s.occupancy[0]).toBe('q/3');
    expect(s.occupancy[1]).toBeUndefined();
  });

  it('is in capture after settlement, with the interview in slot 1 hung off the tool call it was about', () => {
    const s = at(2, 28_000);
    expect(s.state).toBe('capture');
    expect(s.occupancy[1]).toBe('i/1');
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
    expect(at(4, 1000).state).toBe('ratify');
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
    expect(s.occupancy[1]).toBe('i/4');
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
