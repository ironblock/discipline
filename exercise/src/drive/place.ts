/**
 * A script placed in the log: each authored event (`script.ts`) becomes the
 * line or lines `diet`'s log would carry (`log.ts`), in order, with its
 * `seq` issued here and every label resolved to the `seq` it names.
 *
 * What the script says that v0 spells differently is spelled as v0 does:
 *
 * - a request's label becomes its line's `seq`, and whatever named it (a
 *   delta, its response, a failure, a tool call, a fork) names that `seq`;
 *   a response's own label (`q/2#response`) names its request's `seq` too,
 *   since v0 gives a response no identity of its own;
 * - a response stopped as `cancelled` is a `cancelled` line; any other stop
 *   is its `finish_reason`, in llama.cpp's spelling (`tool` → `tool_calls`);
 * - a delta carrying both reasoning and text is two lines, reasoning first;
 * - a failure `disconnected` is `transport`;
 * - `session.end` is a `settlement` into `ended`;
 * - the system prompt is the head's `system` message.
 *
 * A label nothing placed yet stays unresolved (-1): a script that names what
 * it has not said is a script bug, and the fold draws nothing for it.
 */

import type { LineOf, LogLine } from './log.ts';
import type { DriveEvent, Unplaced } from './script.ts';

/** When the scripted sessions opened: a fixed instant, so a placed log is the same every time. */
export const SCRIPTED_OPENING = 1_790_000_000_000;

type Placed = LogLine extends infer L ? (L extends LogLine ? Omit<L, 'seq'> : never) : never;

export class Placer {
  #seq = 0;
  readonly #labels = new Map<string, number>();
  #turnOpen = false;

  /** The `seq` a label was placed at, if it has been. */
  seqOf(label: string): number | undefined {
    return this.#labels.get(label);
  }

  /** Every label placed so far, and its `seq`. */
  labels(): ReadonlyMap<string, number> {
    return this.#labels;
  }

  /** A line already in the log's shape (the surface's own `idle.gap`), given the next `seq`. */
  line(line: Placed): LogLine {
    return { ...line, seq: this.#seq++ } as LogLine;
  }

  /** One scripted event, as the log's line or lines. */
  place(event: Unplaced | DriveEvent): LogLine[] {
    return this.#lines(event).map((line) => ({ ...line, seq: this.#seq++ }) as LogLine);
  }

  #ref(label: string): number {
    return this.#labels.get(label) ?? -1;
  }

  /** Label the line about to be placed (at `this.#seq + offset`). */
  #name(label: string, offset = 0): void {
    this.#labels.set(label, this.#seq + offset);
  }

  #lines(e: Unplaced | DriveEvent): Placed[] {
    switch (e.kind) {
      case 'session.start': {
        const line: Omit<LineOf<'session.start'>, 'seq'> = {
          kind: 'session.start',
          t: e.t,
          version: 0,
          opened: SCRIPTED_OPENING,
          model: e.model,
          head: [{ role: 'system', content: e.system.text }],
          arm: e.arm,
          slots: e.slots,
          trunk_slot: e.trunk_slot,
          phase: e.phase,
          ...(e.system.tokens !== undefined ? { system_tokens: e.system.tokens } : {}),
        };
        return [line];
      }
      case 'ask':
        this.#turnOpen = true;
        return [{ kind: 'ask', t: e.t, turn: e.turn, text: e.text }];
      case 'request': {
        this.#name(e.id);
        this.#name(`${e.id}#response`);
        return [
          {
            kind: 'request',
            t: e.t,
            turn: e.turn,
            lane: e.lane,
            slot: e.slot,
            ...(e.fork !== undefined ? { fork: this.#ref(e.fork) } : {}),
          },
        ];
      }
      case 'delta': {
        const request = this.#ref(e.request);
        const lines: Placed[] = [];
        if (e.reasoning) lines.push({ kind: 'delta', t: e.t, request, reasoning: e.reasoning });
        if (e.text) lines.push({ kind: 'delta', t: e.t, request, text: e.text });
        return lines;
      }
      case 'progress':
        return [{ kind: 'progress', t: e.t, request: this.#ref(e.request), total: e.total, cache: e.cache, processed: e.processed, time_ms: e.time_ms }];
      case 'response': {
        const request = this.#ref(e.to_request);
        // Its own label names its request, as a v0 reference to an answer would.
        this.#labels.set(e.id, request);
        if (e.stop === 'cancelled') return [{ kind: 'cancelled', t: e.t, request, partial: e.text }];
        return [
          {
            kind: 'response',
            t: e.t,
            to_request: request,
            text: e.text,
            finish_reason: e.stop === 'tool' ? 'tool_calls' : e.stop,
            timings: e.timings,
            ...(e.calls_from ? { calls_from: e.calls_from } : {}),
            ...(e.reasoning ? { reasoning: e.reasoning } : {}),
          },
        ];
      }
      case 'request.failed':
        return [{ kind: 'request.failed', t: e.t, request: this.#ref(e.request), reason: e.reason === 'disconnected' ? 'transport' : e.reason, message: e.message }];
      case 'tool.begin':
        this.#name(e.id);
        return [{ kind: 'tool.begin', t: e.t, turn: e.turn, request: this.#ref(e.after), tool: e.tool, args: e.args }];
      case 'tool.end':
        return [{ kind: 'tool.end', t: e.t, begin: this.#ref(e.id), exit: e.exit, output: e.output, ...(e.truncated ? { truncated: true } : {}) }];
      case 'turn.settled':
        this.#turnOpen = false;
        return [{ kind: 'turn.settled', t: e.t, turn: e.turn, reason: e.reason }];
      case 'fork':
        this.#name(e.id);
        return [
          {
            kind: 'fork',
            t: e.t,
            lane: e.lane,
            slot: e.slot,
            of_turn: e.of_turn,
            at: this.#ref(e.at),
            why: e.why,
            question: e.question,
            prefix_tokens: e.prefix_tokens,
          },
        ];
      case 'fork.settled':
        return [{ kind: 'fork.settled', t: e.t, fork: this.#ref(e.id), outcome: e.outcome }];
      case 'patch':
        this.#name(e.id);
        return [
          {
            kind: 'patch',
            t: e.t,
            fork: this.#ref(e.from),
            op: e.op,
            entry: e.entry,
            ...(e.supersedes !== undefined ? { supersedes: e.supersedes } : {}),
            ...(e.authority !== undefined ? { authority: e.authority } : {}),
          },
        ];
      case 'seam':
        this.#name(e.id);
        return [
          {
            kind: 'seam',
            t: e.t,
            at_turn: e.at_turn,
            reason: e.reason,
            ...(e.phase ? { phase: e.phase } : {}),
            prefix_hash_before: e.prefix_hash_before,
            prefix_hash_after: e.prefix_hash_after,
            render: e.render,
            ...(e.warm ? { warm: e.warm } : {}),
          },
        ];
      case 'session.end':
        return [{ kind: 'settlement', t: e.t, from: this.#turnOpen ? 'turn' : 'awaiting', to: 'ended' }];
      default: {
        // A kind the script language does not have: carried as it is, for the fold to count.
        const other = e as unknown as Placed;
        return [other];
      }
    }
  }
}

/** A whole script, placed: the log, and where each label landed. */
export function place(events: readonly (Unplaced | DriveEvent)[]): { readonly log: readonly LogLine[]; readonly labels: ReadonlyMap<string, number> } {
  const placer = new Placer();
  const log = events.flatMap((e) => placer.place(e));
  return { log, labels: placer.labels() };
}
