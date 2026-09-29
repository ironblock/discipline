/** Numbers as the stats footers print them: short, unit last. */

export function ms(value: number): string {
  if (value < 1000) return `${Math.round(value)} ms`;
  if (value < 60_000) return `${(value / 1000).toFixed(value < 10_000 ? 2 : 1)} s`;
  const m = Math.floor(value / 60_000);
  const s = Math.round((value % 60_000) / 1000);
  return `${m}m ${s}s`;
}

/** How long something took, or has taken so far: tenths of a second under a minute, so a live count moves. */
export function took(value: number): string {
  if (value < 1000) return `${Math.round(value)} ms`;
  if (value < 60_000) return `${(value / 1000).toFixed(1)} s`;
  const s = Math.floor(value / 1000);
  return `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, '0')}s`;
}

/** A running counter: whole seconds, so it ticks rather than flickers. */
export function counter(value: number): string {
  const s = Math.floor(value / 1000);
  return s < 60 ? `${s} s` : `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, '0')}s`;
}

export function tokens(value: number): string {
  if (value < 1000) return `${value}`;
  if (value < 10_000) return `${(value / 1000).toFixed(1)}k`;
  return `${Math.round(value / 100) / 10}k`;
}

/** One formatter, made once: `toLocaleString` builds one per call, and a replay calls it thousands of times (scripts/perf.mjs). */
const GROUPED = new Intl.NumberFormat('en-US');

/** A count, grouped: `1,860`. */
export function count(n: number): string {
  return GROUPED.format(n);
}

export function rate(n: number, ms: number): string {
  if (ms <= 0 || n <= 0) return '–';
  const perSecond = (n / ms) * 1000;
  return perSecond >= 100 ? count(Math.round(perSecond)) : perSecond.toFixed(1);
}

export function bytes(text: string): string {
  const n = new TextEncoder().encode(text).length;
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

export function lines(text: string): number {
  return text === '' ? 0 : text.split('\n').length;
}
