import { describe, expect, it } from 'vitest';

import { AUTHORED } from '../drive/examples.ts';
import { placed } from '../drive/recorded.ts';
import type { AssistantNode, BranchNode } from './fold.ts';
import { fold } from './fold.ts';

/**
 * The warrant gate's lines and a meditation's concurrent audit chunks (#862), folded from the kitchen sink's own log:
 * `diet/scenarios/kitchen-sink.jsonl` runs the classifier gate on a canned classifier and the audit two chunks at once.
 */
const session = fold(placed(AUTHORED['kitchen-sink']));
const nodes = session.eras.flatMap((era) => era.nodes);
const step = (id: string) => nodes.find((n) => n.kind === 'assistant' && n.id === id) as AssistantNode | undefined;
const branch = (id: string): BranchNode | undefined => [...session.branches.values()].flat().find((b) => b.id === id);
const MODEL = 'canned-scenario-classifier@0000000000000000000000000000000000000000';

describe('the warrant gate, on the step it decided (#850, #862)', () => {
  it('folds every gate line onto its step: four probes on each of the 45', () => {
    const gated = nodes.filter((n) => n.kind === 'assistant' && (n as AssistantNode).gate !== undefined) as AssistantNode[];
    expect([gated.length, gated.flatMap((n) => n.gate!.probes).length]).toEqual([45, 180]);
  });

  it('says which probes fired on the missing-file step, with their scores against the threshold', () => {
    expect(step('185')?.gate).toEqual({
      outcome: 'decided',
      probes: [
        { probe: 'constraint', score: 0.64, fired: true },
        { probe: 'decision', score: 0.71, fired: true },
        { probe: 'gotcha', score: 0.93, fired: true },
        { probe: 'open', score: 0.04, fired: false },
      ],
      threshold: 0.5,
      model: MODEL,
      computeUnits: 'none',
      latencyMs: 2,
    });
    expect(step('228')?.gate?.probes.filter((probe) => probe.fired)).toEqual([{ probe: 'open', score: 0.88, fired: true }]);
  });

  it('keeps a quiet step quiet: every probe decided, none fired', () => {
    expect(step('5')?.gate?.probes.map((p) => [p.probe, p.score, p.fired])).toEqual([
      ['constraint', 0.04, false],
      ['decision', 0.04, false],
      ['gotcha', 0.04, false],
      ['open', 0.04, false],
    ]);
  });
});

describe('a gate that did not decide (#850)', () => {
  it('says so, and why, with no probe scored: the kitchen sink’s first step, as a classifier that fell back would log it', () => {
    const fellBack = placed(AUTHORED['kitchen-sink']).flatMap((line) => {
      if (line.kind !== 'warrant.gated' || line.request !== 5) return [line];
      if (line.probe !== 'constraint') return [];
      // The line as a fallback writes it: who would have scored, how long it waited, and why it did not -- no probe.
      const { seq, t, kind, request, model, revision, compute_units, latency_ms } = line;
      return [{ seq, t, kind, request, model, revision, compute_units, latency_ms, outcome: 'fallback', reason: 'the classifier did not answer' }];
    });
    const gate = (fold(fellBack).eras.flatMap((era) => era.nodes).find((n) => n.kind === 'assistant' && n.id === '5') as AssistantNode | undefined)?.gate;
    expect(gate).toEqual({ outcome: 'fallback', probes: [], reason: 'the classifier did not answer', model: MODEL, computeUnits: 'none', latencyMs: 3 });
  });
});

describe('a note merged into a live one as similar (#872)', () => {
  it('is folded into the note it names, not drawn as a new entry: the kitchen sink’s second LEARNED', () => {
    const into = session.memory.find((m) => m.id === 'interview-t1-0');
    expect(into?.merges).toEqual([{ id: 'interview-t2-f83-0', text: 'learned: notes.md reads back as written', cosine: '0.6857', by: '89' }]);
    expect(session.memory.some((m) => m.id === 'interview-t2-f83-0')).toBe(false);
  });
});

describe('a meditation’s audit in concurrent chunks (#854, #862)', () => {
  it('numbers each chunk among its audit’s, and says it overlapped the other', () => {
    expect([branch('497')?.chunk, branch('499')?.chunk]).toEqual([
      { n: 1, of: 2, overlapped: true },
      { n: 2, of: 2, overlapped: true },
    ]);
    expect(['640', '642', '684', '686'].map((id) => branch(id)?.chunk?.n)).toEqual([1, 2, 1, 2]);
    // The operator's reconcile audit is not chunked.
    expect(branch('694')?.chunk).toBeUndefined();
  });

  it('reconciles from every chunk’s answer, in chunk order, not the last one alone', () => {
    const answer = session.eras.find((era) => era.meditation?.id === '508')?.meditation?.reconcile?.answer;
    expect(answer?.split('\n').filter((line) => line.startsWith('chunk'))).toEqual(['chunk 1 of 2', 'chunk 2 of 2']);
    expect(answer?.match(/KEEP/g)?.length).toBe(8 + 7);
  });
});
