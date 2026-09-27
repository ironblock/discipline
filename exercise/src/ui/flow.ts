import type { Generation } from '../session/fold.ts';
import { rate, tokens, took } from './format.ts';

/**
 * Tokens moving through the model, and how long they have taken: READ (the
 * new part of the prompt, prefill, `pp`) or WRITTEN (generation, `tg`). One
 * shape for both, said one way -- `+100 tok in 5.0 s (20 t/s pp)` -- so a
 * block's header (what went in) and footer (what came out) read alike, and
 * both count up while they run. The rate is derived from the two numbers
 * shown, never reported beside them.
 */
export interface Flow {
  readonly phase: 'pp' | 'tg';
  /** Tokens so far; absent while nothing has said. */
  readonly n?: number;
  /** While reading: how many new tokens there are to read. */
  readonly of?: number;
  readonly ms: number;
  readonly running: boolean;
}

type Generating = Partial<Pick<Generation, 'progress' | 'meter' | 'timings' | 'startedAt' | 'writingSince' | 'lastActivityAt' | 'callsFrom'>>;

/** Now, as a running count reads it: never before the generation's last sign of life, where no clock runs (a part drawn on its own). */
function clock(g: Generating, now: number): number {
  return Math.max(now, g.lastActivityAt ?? 0, g.meter?.at ?? 0);
}

/** What a generation has read of the new part of its prompt, and how long that took (or is taking, at `now`). */
export function readingOf(g: Generating, at: number): Flow | undefined {
  const now = clock(g, at);
  const started = g.startedAt ?? now;
  const m = g.meter;
  if (g.progress === 'prefill') {
    const fresh = m ? m.total - m.cache : undefined;
    return {
      phase: 'pp',
      ...(m && fresh !== undefined ? { n: Math.min(fresh, m.processed), of: fresh } : {}),
      ms: Math.max(0, now - started),
      running: true,
    };
  }
  const t = g.timings;
  if (t?.prompt_n !== undefined) return { phase: 'pp', n: t.prompt_n, ms: t.prompt_ms ?? 0, running: false };
  // Writing, before the response's timings: the frames said what was read, and the first token when.
  if (m) return { phase: 'pp', n: m.total - m.cache, ms: Math.max(0, (g.writingSince ?? now) - started), running: false };
  return undefined;
}

/** What a generation has written, and how long that took (or is taking, at `now`). Nothing while it reads. */
export function writingOf(g: Generating, at: number): Flow | undefined {
  if (g.progress === 'prefill') return undefined;
  const now = clock(g, at);
  const t = g.timings;
  if (t?.predicted_n !== undefined) return { phase: 'tg', n: t.predicted_n, ms: t.predicted_ms ?? 0, running: false };
  if (g.progress !== 'streaming') return undefined;
  const since = g.writingSince ?? g.startedAt ?? now;
  // A count only from a frame since writing began: an earlier one's zero is stale, not measured.
  const counted = g.meter && g.meter.at >= since ? { n: g.meter.decoded } : {};
  return { phase: 'tg', ...counted, ms: Math.max(0, now - since), running: true };
}

/**
 * What a generation that ended in tool calls wrote, apart: its text's share
 * and its calls' share, where the drive said where the calls began
 * (`callsFrom`). Undefined where it did not: then only the whole is known.
 */
export function writtenApart(g: Generating): { readonly text: Flow; readonly calls: Flow } | undefined {
  const t = g.timings;
  const from = g.callsFrom;
  if (!t || !from) return undefined;
  // Only the time may have been kept: then how many tokens each share took is not known.
  const n = from.predicted_n;
  return {
    text: { phase: 'tg', ...(n !== undefined ? { n } : {}), ms: from.predicted_ms, running: false },
    calls: { phase: 'tg', ...(n !== undefined ? { n: Math.max(0, t.predicted_n - n) } : {}), ms: Math.max(0, t.predicted_ms - from.predicted_ms), running: false },
  };
}

/** How far through the new part of its prompt a generation is, for its top edge; absent while nothing has said. */
export function edgeOf(g: Generating): { readonly read: number } | undefined {
  if (g.progress !== 'prefill') return g.timings || g.meter ? { read: 1 } : undefined;
  const m = g.meter;
  if (!m) return undefined;
  const fresh = m.total - m.cache;
  return { read: fresh === 0 ? 1 : Math.min(1, m.processed / fresh) };
}

/** The warm part of a generation's prompt: tokens reused, not read. */
export function warmOf(g: Generating): number | undefined {
  return g.timings?.cache_n ?? g.meter?.cache;
}

/** A flow in one line: `+7.9k of 16.4k tok in 5.7 s (1,380 t/s pp)`. */
export function flowText(f: Flow): string {
  if (f.n === undefined) return f.running ? `${f.phase === 'pp' ? 'reading' : 'writing'} · ${took(f.ms)}` : `+? tok in ${took(f.ms)}`;
  const of = f.of !== undefined ? ` of ${tokens(f.of)}` : '';
  return `+${tokens(f.n)}${of} tok in ${took(f.ms)} (${rate(f.n, f.ms)} t/s ${f.phase})`;
}
