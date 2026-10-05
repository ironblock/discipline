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
 * - a tool call is v3's (#297), and a placed log is served-shaped (ruled on
 *   #300, 5974672622): a response that ends in calls is placed with the
 *   calls' fragments BEFORE it, as a server streams them -- one `delta` per
 *   call carrying a `tool_call` piece with the whole arguments text, at the
 *   response's time -- read from the script's `tool.begin`s on that response,
 *   which the placer is handed ahead (`place`'s `ahead`); the `tool.begin`
 *   itself then places nothing. Each `tool.end` is its call's `tool_call`
 *   line at the script's end time: `ran`, or `cancelled` with no exit or
 *   output. A script's recording did not separate the streams: it kept one
 *   output, the terminal's, and the placement puts all of it in `stdout`,
 *   with `stderr` empty by construction, each with its UTF-8 byte count as
 *   the format pairs them (ruled on #300, 5976370830). When each call began is not
 *   placed: v3 has no start line, and the fold infers it (ruling 3). A
 *   `bash` call's `argv` is `sh -c` and its command, as diet's own canned
 *   commands are spelled (`diet/src/drive/canned.rs`), and its isolation and
 *   network are `unrecorded`, with no `confined`: no script ever said what
 *   confined its commands, and the format lets a log without a substrate
 *   claim say so rather than guess (ruled on #300). A call of any other tool
 *   carries its arguments only (ruling 4). A script's `truncated` is not
 *   placed: v3 logs what ran printed, whole, and what the model was shown is
 *   the record's (#297 Q3, #302);
 * - log v4's keys (#388, AHEAD of the format on `develop`), only where a
 *   script says them: a bash call's `cwd` beside its `argv`; a `tool.end`
 *   `refused` is a `refused` line with its reason, its `argv` and `cwd`, and
 *   no policy words (5982002587); a `tool.end`'s `approval` rides on a `ran`
 *   or `cancelled` line. The specimen and the replayed sessions say none of
 *   them, so what `diet check-log` reads of those is unchanged;
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
  /** Each placed call, by its label: what its `tool_call` line repeats. */
  readonly #calls = new Map<string, { readonly request: number; readonly turn: number; readonly name: string; readonly arguments: string; readonly command?: string; readonly cwd?: string }>();
  /** The calls a `tool.end` has ended. */
  readonly #ended = new Set<string>();
  /** How many calls each request has made so far: the next call's `index`. */
  readonly #indexes = new Map<number, number>();
  #turnOpen = false;

  /** The `seq` a label was placed at, if it has been. */
  seqOf(label: string): number | undefined {
    return this.#labels.get(label);
  }

  /** The calls placed (their fragments streamed) that no `tool.end` has ended yet: what a cancel must end. */
  openCalls(): string[] {
    return [...this.#calls.keys()].filter((label) => !this.#ended.has(label));
  }

  /** Every label placed so far, and its `seq`. */
  labels(): ReadonlyMap<string, number> {
    return this.#labels;
  }

  /** A line already in the log's shape (the surface's own `idle.gap`), given the next `seq`. */
  line(line: Placed): LogLine {
    return { ...line, seq: this.#seq++ } as LogLine;
  }

  /**
   * One scripted event, as the log's line or lines. AHEAD is what the script
   * says next: a response ending in calls finds its calls there.
   */
  place(event: Unplaced | DriveEvent, ahead: readonly (Unplaced | DriveEvent)[] = []): LogLine[] {
    return this.#lines(event, ahead).map((line) => ({ ...line, seq: this.#seq++ }) as LogLine);
  }

  #ref(label: string): number {
    return this.#labels.get(label) ?? -1;
  }

  /** Label the line about to be placed (at `this.#seq + offset`). */
  #name(label: string, offset = 0): void {
    this.#labels.set(label, this.#seq + offset);
  }

  /** A call's fragment, placed at OFFSET among the lines about to be: its label names that line. */
  #fragment(e: Extract<Unplaced | DriveEvent, { kind: 'tool.begin' }>, t: number, offset: number): Placed {
    const request = this.#ref(e.after);
    const index = this.#indexes.get(request) ?? 0;
    this.#indexes.set(request, index + 1);
    const text = JSON.stringify(e.args);
    const command = e.tool === 'bash' && typeof e.args['command'] === 'string' ? e.args['command'] : undefined;
    this.#calls.set(e.id, { request, turn: e.turn, name: e.tool, arguments: text, ...(command !== undefined ? { command } : {}), ...(command !== undefined && e.cwd !== undefined ? { cwd: e.cwd } : {}) });
    this.#name(e.id, offset);
    return { kind: 'delta', t, request, tool_call: { index, id: e.id, name: e.tool, arguments: text } };
  }

  #lines(e: Unplaced | DriveEvent, ahead: readonly (Unplaced | DriveEvent)[]): Placed[] {
    switch (e.kind) {
      case 'session.start': {
        const line: Omit<LineOf<'session.start'>, 'seq'> = {
          kind: 'session.start',
          t: e.t,
          // v1's timings and progress, v2's reasoning, v3's tool calls; v4's cwd, approvals and files where the
          // script declares 4 (its `version`), since a v4 log carries `cwd` with every `argv`.
          version: e.version ?? 3,
          // These lines were placed from a script, not served as they ran: the log says so (#297, ruled 5976392264).
          provenance: 'placed',
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
        // The operator's scope mark (#453), as serve logs it: a v5 key, which only a live canned session carries.
        if (e.scoping) return [{ kind: 'ask', t: e.t, turn: e.turn, text: e.text, scoping: true }];
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
        // Its calls, streamed before it.
        const calls = e.stop === 'tool' ? ahead.filter((a): a is Extract<Unplaced | DriveEvent, { kind: 'tool.begin' }> => a.kind === 'tool.begin' && this.#ref(a.after) === request) : [];
        return [
          ...calls.map((call, offset) => this.#fragment(call, e.t, offset)),
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
        // Placed with its response; one the response was not handed is placed here, late, rather than lost.
        return this.#calls.has(e.id) ? [] : [this.#fragment(e, e.t, 0)];
      case 'tool.end': {
        const call = this.#calls.get(e.id);
        if (!call || this.#ended.has(e.id)) return [];
        this.#ended.add(e.id);
        // The command, where it is one; and where the script said, the directory it ran in (log v4's `cwd`, #388).
        const argv = call.command !== undefined ? { argv: ['sh', '-c', call.command], ...(call.cwd !== undefined ? { cwd: call.cwd } : {}) } : {};
        const approval = e.approval ? { approval: e.approval } : {};
        return [
          {
            kind: 'tool_call',
            t: e.t,
            request: call.request,
            turn: call.turn,
            id: e.id,
            name: call.name,
            arguments: call.arguments,
            ...(e.refused !== undefined
              ? // A parsed refusal carries the command it refused, and no policy words: it never ran (#388, 5982002587).
                { outcome: 'refused', reason: e.refused, ...argv }
              : e.cancelled
              ? { outcome: 'cancelled', ...(call.command !== undefined ? { ...argv, isolation: 'unrecorded', network: 'unrecorded' } : {}), ...approval }
              : {
                  outcome: 'ran',
                  ...(call.command !== undefined ? { ...argv, isolation: 'unrecorded', network: 'unrecorded' } : {}),
                  ...approval,
                  exit: e.exit,
                  stdout: e.output,
                  // A stream's text and its byte count come together (the format's rule): the count is of its UTF-8.
                  stdout_bytes: new TextEncoder().encode(e.output).length,
                  // The files its result is, by reference (log v4, #372): never their bytes.
                  ...(e.files && e.files.length > 0 ? { files: e.files.map((f) => ({ path: f.path, sha256: f.sha256, media_type: f.media_type, bytes: f.bytes.length })) } : {}),
                  // A script kept one output, the terminal's: it is placed whole as stdout, and stderr as empty.
                  stderr: '',
                  stderr_bytes: 0,
                }),
          },
        ];
      }
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
  const log = events.flatMap((e, i) => placer.place(e, events.slice(i + 1)));
  return { log, labels: placer.labels() };
}
