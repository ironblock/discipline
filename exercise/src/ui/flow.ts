import type { Generation } from '../session/fold.ts';
import { rate, tokens, took } from './format.ts';

/**
 * Tokens moving through the model, and how long they have taken: READ (the
 * new part of the prompt, prefill, `pp`) or WRITTEN (generation, `tg`). One
 * shape for both, said one way -- `+100 tok in 5.0 s (20 t/s pp)` -- so a
 * block's header (what went in) and footer (what came out) read alike. The
 * header counts up while it reads, from the log's `progress` lines; the footer
 * says only how long until the response's timings say how much, since nothing
 * in the log counts tokens generated before then (#288). The rate is derived
 * from the two numbers shown, never reported beside them.
 */
export interface Flow {
  readonly phase: 'pp' | 'tg';
  /** Tokens so far; absent while nothing has said. */
  readonly n?: number;
  /** While reading: how many new tokens there are to read. */
  readonly of?: number;
  readonly ms: number;
  readonly running: boolean;
  /** A stop cut it short (#294): while it was reading the prompt, or while it was writing. */
  readonly stopped?: 'reading' | 'writing';
}

type Generating = Partial<Pick<Generation, 'progress' | 'meter' | 'timings' | 'startedAt' | 'writingSince' | 'endedAt' | 'lastActivityAt' | 'callsFrom'>>;

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
      // `processed` counts the warm part in: the new tokens read are what is past it.
      ...(m && fresh !== undefined ? { n: Math.min(fresh, Math.max(0, m.processed - m.cache)), of: fresh } : {}),
      ms: Math.max(0, now - started),
      running: true,
    };
  }
  const t = g.timings;
  if (t?.prompt_n !== undefined) return { phase: 'pp', n: t.prompt_n, ms: t.prompt_ms ?? 0, running: false };
  if (!m) return undefined;
  // No timings yet, or none to come: the frames said what was read, and the first token -- or the stop -- when.
  const fresh = m.total - m.cache;
  const read = Math.min(fresh, Math.max(0, m.processed - m.cache));
  // Stopped before the last of its prompt was read: how far it got, of how much, and that a stop cut it (#294).
  const cut = g.progress === 'cancelled' && read < fresh;
  const until = g.writingSince ?? g.endedAt ?? now;
  return { phase: 'pp', n: read, ...(cut ? { of: fresh, stopped: 'reading' as const } : {}), ms: Math.max(0, until - started), running: false };
}

/** What a generation has written, and how long that took (or is taking, at `now`). Nothing while it reads. */
export function writingOf(g: Generating, at: number): Flow | undefined {
  if (g.progress === 'prefill') return undefined;
  const now = clock(g, at);
  const t = g.timings;
  if (t?.predicted_n !== undefined) return { phase: 'tg', n: t.predicted_n, ms: t.predicted_ms ?? 0, running: false };
  // Stopped: for how long it had written when the stop came, or that it never began (#294).
  if (g.progress === 'cancelled') {
    return g.writingSince !== undefined
      ? { phase: 'tg', ms: Math.max(0, (g.endedAt ?? now) - g.writingSince), running: false, stopped: 'writing' }
      : { phase: 'tg', ms: 0, running: false, stopped: 'reading' };
  }
  if (g.progress !== 'streaming') return undefined;
  const since = g.writingSince ?? g.startedAt ?? now;
  // Only how long: nothing in the log counts tokens generated before the response's timings do (#288).
  return { phase: 'tg', ms: Math.max(0, now - since), running: true };
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
  if (g.timings) return { read: 1 };
  // A stopped read stays where the stop left it (#294); any other past prefill was read through.
  if (g.progress !== 'prefill' && g.progress !== 'cancelled') return g.meter ? { read: 1 } : undefined;
  const m = g.meter;
  if (!m) return undefined;
  const fresh = m.total - m.cache;
  return { read: fresh === 0 ? 1 : Math.min(1, Math.max(0, m.processed - m.cache) / fresh) };
}

/** The warm part of a generation's prompt: tokens reused, not read. */
export function warmOf(g: Generating): number | undefined {
  return g.timings?.cache_n ?? g.meter?.cache;
}

/** A flow in one line: `+7.9k of 16.4k tok in 5.7 s (1,380 t/s pp)`. */
export function flowText(f: Flow): string {
  if (f.phase === 'tg' && f.stopped === 'reading') return 'stopped before writing';
  if (f.phase === 'tg' && f.stopped === 'writing' && f.n === undefined) return `stopped after ${took(f.ms)} of writing`;
  if (f.n === undefined) return f.running ? `${f.phase === 'pp' ? 'reading' : 'writing'} · ${took(f.ms)}` : `+? tok in ${took(f.ms)}`;
  const of = f.of !== undefined ? ` of ${tokens(f.of)}` : '';
  return `+${tokens(f.n)}${of} tok in ${took(f.ms)} (${rate(f.n, f.ms)} t/s ${f.phase})${f.stopped ? ' · stopped' : ''}`;
}
