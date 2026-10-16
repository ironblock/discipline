import { describe, expect, it } from 'vitest';

import { CHUNKED_AUDITS, GATED, LOG, MERGE, QUIET_STEP, WHOLE_AUDITS, firedStep, latencyOf, meditationAfter } from '../drive/moments.ts';
import type { AssistantNode, BranchNode } from './fold.ts';
import { fold } from './fold.ts';

/**
 * The warrant gate's lines and a meditation's concurrent audit chunks (#862), folded from the kitchen sink's own log:
 * `diet/scenarios/kitchen-sink.jsonl` runs the classifier gate on a canned classifier and the audit two chunks at once.
 * Each moment is found by what it is (`moments.ts`, #875); the values asserted are the scenario's.
 */
const session = fold(LOG);
const nodes = session.eras.flatMap((era) => era.nodes);
const step = (seq: number) => nodes.find((n) => n.kind === 'assistant' && n.id === String(seq)) as AssistantNode | undefined;
const branch = (seq: number): BranchNode | undefined => [...session.branches.values()].flat().find((b) => b.id === String(seq));
const MODEL = 'canned-scenario-classifier@0000000000000000000000000000000000000000';
const MISSING_FILE = firedStep('constraint', 'decision', 'gotcha');

describe('the warrant gate, on the step it decided (#850, #862)', () => {
  it('folds every gate line onto its step: four probes on each', () => {
    const gated = nodes.filter((n) => n.kind === 'assistant' && (n as AssistantNode).gate !== undefined) as AssistantNode[];
    expect([gated.length, gated.flatMap((n) => n.gate!.probes).length]).toEqual([GATED.size, 4 * GATED.size]);
  });

  it('says which probes fired on the missing-file step, with their scores against the threshold', () => {
    expect(step(MISSING_FILE)?.gate).toEqual({
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
      latencyMs: latencyOf(MISSING_FILE),
    });
    expect(step(firedStep('open'))?.gate?.probes.filter((probe) => probe.fired)).toEqual([{ probe: 'open', score: 0.88, fired: true }]);
  });

  it('keeps a quiet step quiet: every probe decided, none fired', () => {
    expect(step(QUIET_STEP)?.gate?.probes.map((p) => [p.probe, p.score, p.fired])).toEqual([
      ['constraint', 0.04, false],
      ['decision', 0.04, false],
      ['gotcha', 0.04, false],
      ['open', 0.04, false],
    ]);
  });
});

describe('a gate that did not decide (#850)', () => {
  it('says so, and why, with no probe scored: the kitchen sink’s first step, as a classifier that fell back would log it', () => {
    const first = Math.min(...GATED.keys());
    const fellBack = LOG.flatMap((line) => {
      if (line.kind !== 'warrant.gated' || line.request !== first) return [line];
      if (line.probe !== 'constraint') return [];
      // The line as a fallback writes it: who would have scored, how long it waited, and why it did not -- no probe.
      const { seq, t, kind, request, model, revision, compute_units, latency_ms } = line;
      return [{ seq, t, kind, request, model, revision, compute_units, latency_ms, outcome: 'fallback', reason: 'the classifier did not answer' }];
    });
    const gate = (fold(fellBack).eras.flatMap((era) => era.nodes).find((n) => n.kind === 'assistant' && n.id === String(first)) as AssistantNode | undefined)?.gate;
    const waited = GATED.get(first)!.find((line) => line.probe === 'constraint')!.latency_ms;
    expect(gate).toEqual({ outcome: 'fallback', probes: [], reason: 'the classifier did not answer', model: MODEL, computeUnits: 'none', latencyMs: waited });
  });
});

describe('a note merged into a live one as similar (#872)', () => {
  it('is folded into the note it names, not drawn as a new entry: the kitchen sink’s second LEARNED', () => {
    const into = session.memory.find((m) => m.id === MERGE?.merged);
    expect(into?.merges).toEqual([{ id: MERGE?.entry.id, text: 'learned: notes.md reads back as written', cosine: '0.6857', by: String(MERGE?.seq) }]);
    expect(session.memory.some((m) => m.id === MERGE?.entry.id)).toBe(false);
  });
});

describe('a meditation’s audit in concurrent chunks (#854, #862)', () => {
  it('numbers each chunk among its audit’s, and says it overlapped the other', () => {
    expect(CHUNKED_AUDITS.map((chunks) => chunks.map((fork) => branch(fork.seq)?.chunk))).toEqual(
      CHUNKED_AUDITS.map(() => [
        { n: 1, of: 2, overlapped: true },
        { n: 2, of: 2, overlapped: true },
      ]),
    );
    expect(CHUNKED_AUDITS).toHaveLength(3);
    // The operator's reconcile audit is not chunked.
    expect(WHOLE_AUDITS.map((fork) => branch(fork.seq)?.chunk)).toEqual([undefined]);
  });

  it('reconciles from every chunk’s answer, in chunk order, not the last one alone', () => {
    const meditation = String(meditationAfter(CHUNKED_AUDITS[0]!.at(-1)!).seq);
    const answer = session.eras.find((era) => era.meditation?.id === meditation)?.meditation?.reconcile?.answer;
    expect(answer?.split('\n').filter((line) => line.startsWith('chunk'))).toEqual(['chunk 1 of 2', 'chunk 2 of 2']);
    expect(answer?.match(/KEEP/g)?.length).toBe(8 + 7);
  });
});
