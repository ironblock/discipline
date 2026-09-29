import type { Authority, ForkLane, PatchOp, Timings, Unplaced } from './script.ts';

/**
 * A session composed from a script, rather than recorded or written event by
 * event: the script says what was asked, what the model thought, said and
 * ran, and what the side calls asked and learned; `compose` places it on a
 * clock. Every time is derived -- a request's tokens from its text, its
 * prefill and decode from assumed rates, a tool's run from the script -- so
 * the numbers are plausible for one local model and are not measurements.
 */

/** Assumed rates: prefill of new tokens, and decode on the trunk and on a side slot. scripts/stitch-sides.py assumed the same, once. */
export const RATES = { prefill: 1400, trunkDecode: 36, sideDecode: 40 } as const;

/** Roughly how many tokens a text is. */
export function tokensOf(text: string): number {
  return Math.max(1, Math.ceil(text.length / 3.8));
}

export interface Step {
  readonly think?: string;
  readonly say: string;
  /** The tool call the step ends in; the last step of a turn has none. */
  readonly run?: { readonly command: string; readonly output: string; readonly ms: number; readonly exit?: number; readonly truncated?: boolean };
}

export interface Change {
  readonly op: PatchOp;
  readonly id: string;
  readonly category: string;
  readonly text: string;
  readonly supersedes?: string;
  readonly authority?: Authority;
}

export interface Side {
  readonly lane: ForkLane;
  readonly slot: number;
  /** When it forks: while step `n`'s tool runs, or once the turn has settled. */
  readonly when: number | 'settled';
  readonly why: string;
  readonly question: string;
  readonly answer: string;
  readonly changes: readonly Change[];
}

export interface Turn {
  readonly kind: 'turn';
  /** How long the person took to ask, from the turn before settling (or the session opening). */
  readonly after: number;
  readonly ask: string;
  readonly steps: readonly Step[];
  readonly sides: readonly Side[];
}

export interface Refill {
  readonly kind: 'refill';
  /** How long the person took to declare it, from the turn before settling. */
  readonly after: number;
  readonly phase: { readonly from: string; readonly to: string };
  readonly ratify: Omit<Side, 'lane' | 'when'>;
  /** The new system prompt: working memory rendered, with the phase's priming. */
  readonly render: string;
}

export interface Script {
  readonly model: string;
  readonly slots: number;
  readonly phase: string;
  readonly system: string;
  readonly parts: readonly (Turn | Refill)[];
}

const round = (ms: number) => Math.round(ms);
const timings = (prompt_n: number, cache_n: number, predicted_n: number, decode: number): Timings => ({
  prompt_n,
  cache_n,
  prompt_ms: round((prompt_n / RATES.prefill) * 1000),
  predicted_n,
  predicted_ms: round((predicted_n / decode) * 1000),
});
const took = (t: Timings) => (t.prompt_ms ?? 0) + (t.predicted_ms ?? 0);

/** The script's events, in the order they happened. */
export function compose(script: Script): Unplaced[] {
  const out: Unplaced[] = [];
  const count = { q: 0, t: 0, p: 0, s: 0, interview: 0, extraction: 0, ratify: 0 } as Record<string, number>;
  const next = (key: string) => (count[key] = (count[key] ?? 0) + 1);
  // When each side slot is next free, and how much the trunk's context holds.
  const free = new Map<number, number>();
  let context = tokensOf(script.system);
  let now = 0;
  let turn = 0;
  let last = '';

  out.push({ kind: 'session.start', t: 0, arm: 'diet', model: script.model, slots: script.slots, trunk_slot: 0, phase: script.phase, system: { text: script.system, tokens: context } });

  const side = (s: Omit<Side, 'when'> & { readonly lane: ForkLane }, at: string, wanted: number, ofTurn: number) => {
    const n = next(s.lane);
    const id = `${s.lane.slice(0, 1)}/${n}`;
    const start = Math.max(wanted, free.get(s.slot) ?? 0);
    out.push({ kind: 'fork', t: start, id, lane: s.lane, slot: s.slot, of_turn: ofTurn, at, why: s.why, question: s.question, prefix_tokens: context });
    out.push({ kind: 'request', t: start + 10, id: `${id}/q`, lane: s.lane, slot: s.slot, turn: ofTurn, fork: id });
    const tm = timings(tokensOf(s.question), context, tokensOf(s.answer), RATES.sideDecode);
    const done = start + 10 + took(tm);
    out.push({ kind: 'response', t: done, id: `${id}/q#response`, to_request: `${id}/q`, text: s.answer, stop: 'stop', timings: tm });
    out.push({ kind: 'fork.settled', t: done + 10, id, outcome: 'value' });
    for (const c of s.changes) {
      out.push({
        kind: 'patch',
        t: done + 20,
        id: `p/${next('p')}`,
        from: id,
        op: c.op,
        entry: { id: c.id, category: c.category, text: c.text },
        ...(c.supersedes ? { supersedes: c.supersedes } : {}),
        ...(c.authority ? { authority: c.authority } : {}),
      });
    }
    free.set(s.slot, done + 30);
    return done + 30;
  };

  for (const part of script.parts) {
    if (part.kind === 'refill') {
      const r = side({ ...part.ratify, lane: 'ratify' }, last, now + part.after, turn);
      const warm = timings(tokensOf(part.render), 0, 1, RATES.trunkDecode);
      const seamAt = r + 1200;
      out.push({
        kind: 'seam',
        t: seamAt,
        id: `s/${next('s')}`,
        at_turn: turn,
        reason: 'operator',
        phase: part.phase,
        prefix_hash_before: hash(`${turn}:${context}`),
        prefix_hash_after: hash(part.render),
        render: { version: count['s'] ?? 1, text: part.render, tokens: tokensOf(part.render) },
        warm,
      });
      context = tokensOf(part.render);
      now = seamAt + took(warm);
      continue;
    }
    turn += 1;
    now += part.after;
    out.push({ kind: 'ask', t: now, turn, text: part.ask });
    let added = tokensOf(part.ask) + 8;
    for (const [i, step] of part.steps.entries()) {
      const q = `q/${next('q')}`;
      const request = now + 30;
      out.push({ kind: 'request', t: request, id: q, lane: 'trunk', slot: 0, turn });
      const said = `${step.think ?? ''}${step.say}${step.run?.command ?? ''}`;
      const tm = timings(added, context, tokensOf(said), RATES.trunkDecode);
      // A drive calling tools natively knows where each call began: the text's tokens, and the time they took.
      const text = tokensOf(`${step.think ?? ''}${step.say}`);
      const callsFrom = step.run ? { predicted_n: text, predicted_ms: round((text / RATES.trunkDecode) * 1000) } : undefined;
      now = request + took(tm);
      context += added + (tm.predicted_n ?? 0);
      last = `${q}#response`;
      out.push({
        kind: 'response',
        t: now,
        id: last,
        to_request: q,
        ...(step.think ? { reasoning: step.think } : {}),
        text: step.say,
        stop: step.run ? 'tool' : 'stop',
        timings: tm,
        ...(callsFrom ? { calls_from: callsFrom } : {}),
      });
      if (!step.run) continue;
      const tool = `t/${next('t')}`;
      const begin = now + 20;
      out.push({ kind: 'tool.begin', t: begin, id: tool, turn, after: last, tool: 'bash', args: { command: step.run.command } });
      for (const s of part.sides.filter((x) => x.when === i)) side(s, tool, begin + 60, turn);
      now = begin + step.run.ms;
      out.push({ kind: 'tool.end', t: now, id: tool, exit: step.run.exit ?? 0, output: step.run.output, ...(step.run.truncated ? { truncated: true } : {}) });
      last = tool;
      added = tokensOf(step.run.output) + 8;
    }
    now += 20;
    out.push({ kind: 'turn.settled', t: now, turn, reason: 'final' });
    for (const s of part.sides.filter((x) => x.when === 'settled')) side(s, last, now + 60, turn);
  }
  return out.sort((a, b) => a.t - b.t);
}

/** A stand-in prefix hash: eight hex digits from the text, stable run to run. */
function hash(text: string): string {
  let h = 0x811c9dc5;
  for (const ch of text) h = Math.imul(h ^ ch.charCodeAt(0), 0x01000193) >>> 0;
  return h.toString(16).padStart(8, '0');
}
