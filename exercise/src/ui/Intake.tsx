import type { Generation } from '../session/fold.ts';
import { counter, ms, rate, tokens } from './format.ts';

/**
 * What a generation read before it wrote -- its INTAKE -- whether it is
 * reading now or has read: the whole prompt, the warm part (reused), the new
 * part and how much of it is read, how fast, how long. From the progress
 * frames while reading, from the response's timings once it has answered.
 * Undefined before anything says (a request with no frame yet is `reading`
 * with nothing known).
 */
export interface Intake {
  readonly reading: boolean;
  /** Tokens: all of the prompt, the warm part, the new part, and how much of the new part is read. */
  readonly counts?: { readonly total: number; readonly warm: number; readonly fresh: number; readonly read: number };
  readonly ppRate?: number;
  /** How long the reading took, once it is done; how long is left, while it goes. */
  readonly ms?: number;
}

export function intakeOf(g: Partial<Pick<Generation, 'progress' | 'meter' | 'timings'>>): Intake | undefined {
  if (g.progress === 'prefill') {
    const m = g.meter;
    if (!m) return { reading: true };
    const fresh = m.total - m.cache;
    return {
      reading: true,
      counts: { total: m.total, warm: m.cache, fresh, read: Math.min(fresh, m.processed) },
      ...(m.ppRate !== undefined ? { ppRate: m.ppRate } : {}),
      ...(m.ppRate !== undefined && m.ppRate > 0 ? { ms: ((fresh - m.processed) / m.ppRate) * 1000 } : {}),
    };
  }
  const t = g.timings;
  if (t?.prompt_n === undefined) {
    // Writing, before the response's timings: the frames said what was read.
    const m = g.meter;
    if (!m) return undefined;
    return { reading: false, counts: { total: m.total, warm: m.cache, fresh: m.total - m.cache, read: m.total - m.cache }, ...(m.ppRate !== undefined ? { ppRate: m.ppRate } : {}) };
  }
  const warm = t.cache_n ?? 0;
  return {
    reading: false,
    counts: { total: t.prompt_n + warm, warm, fresh: t.prompt_n, read: t.prompt_n },
    ...(t.prompt_ms !== undefined && t.prompt_ms > 0 ? { ppRate: (t.prompt_n / t.prompt_ms) * 1000, ms: t.prompt_ms } : {}),
  };
}

/** The intake as the edge of a block draws it: how much of the new part is read. */
export function intakeEdge(intake: Intake): { readonly read: number } | undefined {
  const c = intake.counts;
  if (!c) return undefined;
  return { read: c.fresh === 0 ? 1 : c.read / c.fresh };
}

/** The intake in one line: what is being read, or what was. */
export function IntakeLine({ intake }: { readonly intake: Intake }) {
  const c = intake.counts;
  if (!c) return <>reading the prompt</>;
  return (
    <>
      {intake.reading ? `reading · ${tokens(c.read)} of ${tokens(c.fresh)} new` : `read ${tokens(c.fresh)} new`}
      {c.warm > 0 ? ` · ${tokens(c.warm)} warm` : ''}
      {intake.ppRate !== undefined ? ` · ${rate(intake.ppRate, 1000)} pp t/s` : ''}
      {intake.ms !== undefined ? ` · ${intake.reading ? `${counter(intake.ms)} left` : ms(intake.ms)}` : ''}
    </>
  );
}
