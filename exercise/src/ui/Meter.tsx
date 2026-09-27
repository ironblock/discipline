import type { CSSProperties } from 'react';

import type { Generation, Meter as Progress } from '../session/fold.ts';
import { counter, ms, rate, tokens } from './format.ts';
import './meter.css';

/** How much of a prompt is in the model: the warm part and the new part read, as a fraction of all of it. */
export function readFraction(m: Progress): number {
  return m.total === 0 ? 0 : Math.min(1, (m.cache + m.processed) / m.total);
}

/**
 * Prefill, measured: a bar of the whole prompt with the warm part there from
 * the start and the new part filling, and a line saying how many of how many,
 * how fast, and how long is left. What "reading the prompt" was for twelve
 * seconds, with the numbers in it.
 */
export function Meter({ meter: m }: { readonly meter: Progress }) {
  const fresh = m.total - m.cache;
  const left = m.ppRate !== undefined && m.ppRate > 0 ? ((fresh - m.processed) / m.ppRate) * 1000 : undefined;
  const style = {
    '--warm': m.total === 0 ? 0 : m.cache / m.total,
    '--read': fresh === 0 ? 1 : m.processed / fresh,
  } as CSSProperties;
  return (
    <div className="ex-meter" role="status" style={style}>
      <div className="ex-meter__bar" aria-hidden="true">
        <span className="ex-meter__warm" />
        <span className="ex-meter__read" />
      </div>
      <p className="ex-meter__line">
        reading the prompt · {tokens(m.processed)} of {tokens(fresh)} new
        {m.cache > 0 ? ` · ${tokens(m.cache)} warm` : ''}
        {m.ppRate !== undefined ? ` · ${rate(m.ppRate, 1000)} pp t/s` : ''}
        {left !== undefined ? ` · ${counter(left)} left` : ''}
      </p>
    </div>
  );
}

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

/** The intake as the edge of a block draws it: the warm part and the read part, as fractions of the prompt. */
export function intakeEdge(intake: Intake): { readonly warm: number; readonly read: number } | undefined {
  const c = intake.counts;
  if (!c || c.total === 0) return undefined;
  return { warm: c.warm / c.total, read: c.fresh === 0 ? 1 : c.read / c.fresh };
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
