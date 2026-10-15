import type { LogLine } from './log.ts';
import { lanesOf } from './scenario-log.ts';

/**
 * The kitchen sink placed on a working drive's cadence (#798). A scenario's log is on the canned substrate's clock:
 * each reply after its `delay_ms`, the operator instant. This keeps every line and its order and gives each a `t`
 * from assumed rates -- a person's time to ask, prefill and decode from a text's tokens, a tool's run as logged with
 * a floor -- so the published example reads at a drive's pace. Its `t` is ours, not measured; the example says so
 * (`timing: 'retimed'`).
 *
 * Lanes overlap as logged: each lane keeps its own clock, and a line is placed when its lane is due, never before the
 * line logged ahead of it.
 */

/** Assumed rates, tokens a second: prefill of new tokens, and decode on the trunk and on a side slot. */
export const RATES = { prefill: 1400, trunkDecode: 36, sideDecode: 40 } as const;

/** Roughly how many tokens a text is. */
export function tokensOf(text: string): number {
  return Math.max(1, Math.ceil(text.length / 3.8));
}

/** A person's time to ask: read what came back, then type. */
const askTime = (text: string) => Math.min(12_000, 1800 + text.length * 45);
/** A person's time to rule, flag, open or close: a decision, not a sentence. */
const COMMAND_MS = 1500;
/** Before a request's first token: its prompt read, past the cache. */
const FIRST_TOKEN_MS = { trunk: 350, side: 150 } as const;
/** A tool's run, at least: the canned substrate runs one in a millisecond or two. */
const TOOL_FLOOR_MS = 150;

type Line = LogLine & Readonly<Record<string, unknown>>;

/** Lines the operator causes: placed after the operator's time, from the last line before them. */
function operatorTime(line: Line): number | undefined {
  if (line.kind === 'ask') return askTime(typeof line['text'] === 'string' ? line['text'] : '');
  if (line.kind === 'phase.started' || line.kind === 'flag' || line.kind === 'tangent.open' || line.kind === 'tangent.close') return COMMAND_MS;
  if ((line.kind === 'phase.ruled' || line.kind === 'phase.proposed') && line['by'] === 'operator') return COMMAND_MS;
  if (line.kind === 'meditation' && line['reason'] === 'operator') return COMMAND_MS;
  return undefined;
}

/** What a delta carries, as text, for its decode time. */
function deltaText(line: Line): string {
  const call = line['tool_call'] as { arguments?: string } | undefined;
  return [line['text'], line['reasoning'], call?.arguments].filter((part): part is string => typeof part === 'string').join('');
}

export function retime(lines: readonly LogLine[]): LogLine[] {
  const laneOf = lanesOf(lines);
  const requests = new Map<number, { readonly lane: string; streamed: boolean }>();
  const clock = new Map<string, number>();
  const logged = new Map<string, number>();
  let last = 0;
  return lines.map((original, index) => {
    const line = original as Line;
    const lane = laneOf.get(line.seq) ?? 'session';
    const side = lane !== 'trunk' && lane !== 'session';
    const decode = side ? RATES.sideDecode : RATES.trunkDecode;
    const loggedGap = Math.max(0, line.t - (logged.get(lane) ?? line.t));
    logged.set(lane, line.t);
    const own = clock.get(lane) ?? last;
    let due: number;
    const asked = operatorTime(line);
    if (index === 0) due = 0;
    else if (asked !== undefined) due = last + asked;
    else if (line.kind === 'request') {
      requests.set(line.seq, { lane, streamed: false });
      due = Math.max(own, last) + 60;
    } else if (line.kind === 'delta') {
      const request = requests.get(line['request'] as number);
      const first = request !== undefined && !request.streamed;
      if (request) request.streamed = true;
      due = own + (first ? FIRST_TOKEN_MS[side ? 'side' : 'trunk'] : 0) + (tokensOf(deltaText(line)) / decode) * 1000;
    } else if (line.kind === 'response') {
      const request = requests.get(line['to_request'] as number);
      const text = typeof line['text'] === 'string' ? line['text'] : '';
      due = request?.streamed ? own + 5 : own + FIRST_TOKEN_MS[side ? 'side' : 'trunk'] + (tokensOf(text) / decode) * 1000;
    } else if (line.kind === 'tool_call') due = own + Math.max(TOOL_FLOOR_MS, loggedGap);
    else if (line.kind === 'fork') due = last + 5;
    else due = own + 5;
    const t = Math.round(Math.max(last, due));
    clock.set(lane, t);
    last = t;
    return { ...original, t } as LogLine;
  });
}
