import { AUTHORED } from './examples.ts';
import type { LogLine } from './log.ts';
import { placed } from './recorded.ts';

/**
 * The kitchen sink's moments (#875), found in its log by what they are -- a line's kind and content -- never by its
 * `seq`, a wall-clock latency, or an id diet built from one. Regenerating the example with a line more moves every seq
 * after it; a test or story that names its moment through here moves with it.
 */
export const LOG: readonly LogLine[] = placed(AUTHORED['kitchen-sink']);

type Line<K extends LogLine['kind']> = Extract<LogLine, { kind: K }>;

/** Every line of KIND whose fields hold MATCH's values. */
export function all<K extends LogLine['kind']>(kind: K, match: Readonly<Record<string, unknown>> = {}): Line<K>[] {
  return LOG.filter((line): line is Line<K> => {
    const record = line as unknown as Readonly<Record<string, unknown>>;
    return line.kind === kind && Object.entries(match).every(([key, value]) => record[key] === value);
  });
}

/** The first such line; the kitchen sink no longer logging it fails loudly. */
export function the<K extends LogLine['kind']>(kind: K, match: Readonly<Record<string, unknown>> = {}): Line<K> {
  const line = all(kind, match)[0];
  if (line === undefined) throw new Error(`moments: the kitchen sink logs no ${kind} with ${JSON.stringify(match)}`);
  return line;
}

/** The warrant gate's lines (#850), by the request of the step they decided. */
export const GATED: ReadonlyMap<number, readonly Line<'warrant.gated'>[]> = all('warrant.gated').reduce(
  (steps, line) => steps.set(line.request, [...(steps.get(line.request) ?? []), line]),
  new Map<number, Line<'warrant.gated'>[]>(),
);

/** The step whose gate fired exactly PROBES, by its request's seq. */
export function firedStep(...probes: string[]): number {
  const want = [...probes].sort().join(' ');
  const step = [...GATED].find(([, lines]) => lines.filter((l) => l.fired === true).map((l) => l.probe).sort().join(' ') === want);
  if (step === undefined) throw new Error(`moments: no step's gate fired exactly ${want}`);
  return step[0];
}

/** The first step the gate decided with nothing fired. */
export const QUIET_STEP: number = [...GATED].find(([, lines]) => lines.every((l) => l.outcome === 'decided' && l.fired !== true))![0];

/** A step's gate latency as logged: its slowest probe's. */
export const latencyOf = (step: number): number => Math.max(...(GATED.get(step) ?? []).map((l) => l.latency_ms));

/** A meditation's audit run in chunks (#854): each audit's forks, chunk order, grouped by the trunk line they cut at. */
export const CHUNKED_AUDITS: readonly (readonly Line<'fork'>[])[] = [
  ...all('fork', { lane: 'audit' })
    .filter((fork) => fork.chunk !== undefined)
    .reduce((audits, fork) => audits.set(fork.at, [...(audits.get(fork.at) ?? []), fork]), new Map<number, Line<'fork'>[]>())
    .values(),
].map((forks) => forks.sort((a, b) => a.chunk! - b.chunk!));

/** The audits that ran whole. */
export const WHOLE_AUDITS: readonly Line<'fork'>[] = all('fork', { lane: 'audit' }).filter((fork) => fork.chunk === undefined);

/** The meditation a fork's audit was for: the first after the fork settled. */
export function meditationAfter(fork: Line<'fork'>): Line<'meditation'> {
  const settled = the('fork.settled', { fork: fork.seq });
  const meditation = LOG.find((line): line is Line<'meditation'> => line.kind === 'meditation' && line.seq > settled.seq);
  if (meditation === undefined) throw new Error(`moments: no meditation after fork ${fork.seq} settled`);
  return meditation;
}

/** The note merged into a live one as similar (#872): the merged `add`. */
export const MERGE = LOG.find((line): line is Line<'patch'> & { merged: string; cosine: string } => line.kind === 'patch' && 'merged' in line && line.merged !== undefined);
