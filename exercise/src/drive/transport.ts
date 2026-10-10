/**
 * The drive interface: everything the surface can ask of a session.
 *
 * Narrow on purpose. The surface subscribes to the session's log and
 * dispatches commands; everything it draws is folded from the log. When
 * `diet`'s loop is served over HTTP + SSE (#117), an `HttpTransport`
 * implements this against it -- `subscribe` is the SSE stream (history, then
 * tail), and each command is one POST -- and nothing above this file changes.
 */

import type { IdleGapBody } from '../session/gap.ts';
import type { FileSource } from './files.ts';
import type { LogLine, Open } from './log.ts';

/**
 * What a person can ask of the drive. Closed: the surface owns what it
 * sends, and a new command should break the build where it must be offered.
 * Named so far and not yet here: edit memory, retry, annotate, a choice at
 * ratify.
 */
export type Command =
  /** A person's ask, for the trunk. */
  | {
      readonly kind: 'ask';
      readonly text: string;
      /**
       * The operator marked it the scope answer (#453): the ask whose settled turn warrants T1's interview fork
       * (#374). Sent as `"scoping": true` on that ask only; absent, it is unmarked.
       */
      readonly scoping?: true;
      /** The operator's attachments, by digest, each uploaded first (`DriveTransport.upload`): sent as image parts before the text. */
      readonly files?: readonly string[];
    }
  /** Stop whatever is in flight, the trunk's call or a fork's. */
  | { readonly kind: 'cancel' }
  /** Declare a seam: ratify, render, refill -- to the phase named, where the drive declares phases; `diet`'s does not yet. */
  | { readonly kind: 'seam'; readonly to?: string }
  /** End the session: nothing more is asked of it (#289). */
  | { readonly kind: 'end' }
  /** Move the running foreground command to the background (#614): it keeps running, and the turn goes on. */
  | { readonly kind: 'background' }
  /** The operator's answer to the call waiting on them (#389): `call` is its id as the model streamed it. */
  | { readonly kind: 'approve'; readonly call: string; readonly scope: Decision };

/** What the operator may answer a prompt with (#298 5981578399 point 8): a scope, or decline. Closed: the surface offers each. */
export type Decision = 'once' | 'session' | 'workspace' | 'decline';

/**
 * One segment of a command as the gate read it: the gate module's own value space (`judge_argv`, #402;
 * `diet/src/drive/shell_gate.rs`), which the waiting event carries (#389 5982826097) and the replay re-derives
 * (`gate/judge.ts`). `shape` is what an approval of it would cover, absent when it is dynamic or unreadable;
 * `why` and `reason` are on a prompting segment, `entry` on a refused one.
 */
export interface Segment {
  readonly shape?: string;
  readonly verdict: Open<'refused' | 'prompt' | 'free' | 'approved'>;
  readonly why?: Open<'not_approved' | 'dynamic' | 'unparsable'>;
  readonly reason?: string;
  readonly entry?: string;
}

/**
 * A call waiting on the operator (#389, ruled 5982826097): `serve`'s `waiting` event, which is not a log line --
 * the log has the decision, on the call's `tool_call` line, once it is taken. Found by its request and id, as
 * that line is.
 */
export interface Prompt {
  readonly request: number;
  readonly id: string;
  readonly command: string;
  readonly cwd: string;
  /** Why it prompted: an open set, drawn under its own name. */
  readonly reason: string;
  readonly segments: readonly Segment[];
}

/** Why a command was not taken. Open: the drive may refuse for reasons the surface has not heard of. */
export type Refusal = Open<
  /** A turn or a capture round is in flight. */
  | 'busy'
  /** The session has ended. */
  | 'ended'
  /** There is nothing to seam: no turn has settled. */
  | 'nothing-to-seam'
  /** Nothing is in flight. */
  | 'nothing-to-cancel'
  /** Canned transport only: the script expects a different command next. */
  | 'off-script'
  /** A recording plays; it takes no commands. */
  | 'recording'
  /** `diet`'s own (log v0 `Refusal`): a turn or a capture is in flight; the turn named has nothing in flight; seams are not built yet (read in an older log; v6 writes `nothing-to-seam` instead); a stop named an older turn. */
  | 'in-flight'
  | 'nothing-in-flight'
  | 'seam-not-built'
  | 'stale'
  /** An answer to a prompt when none waits (#389); one naming another call than the one waiting is `stale`. */
  | 'nothing-waiting'
  /** The HTTP transport's: the drive did not answer. */
  | 'unreachable'
  /** An upload whose bytes do not begin with the PNG signature (`POST /files`'s 400). */
  | 'not-a-png'
  /** An upload over the drive's size cap (`POST /files`'s 413; `MAX_BYTES` in `diet/src/drive/attach.rs`). */
  | 'too-large'
  /** An ask naming a digest this session was never sent. */
  | 'not-uploaded'
  /** A move to the background with no command running in the foreground (#614). */
  | 'nothing-running'
>;

export type Ack = { readonly ok: true } | { readonly ok: false; readonly refused: Refusal };

/** What an upload came to: the digest an ask names it by, and its size; or why it was not kept. */
export type Uploaded = { readonly ok: true; readonly sha256: string; readonly bytes: number } | { readonly ok: false; readonly refused: Refusal };

/**
 * The connection to the drive, which the log cannot carry: while it is down,
 * nothing reaches the log at all. `live` is the only state a transport that
 * never drops (canned, a replay) is ever in.
 */
export type Link = 'live' | 'reconnecting' | 'lost';

export interface DriveTransport {
  /** Every event so far, in order, then each new one. Returns an unsubscribe. */
  subscribe(listener: (line: LogLine) => void): () => void;
  /**
   * One command. `idle_gap`: the idle gap it ends, as the surface measured it
   * (Q4), which the drive logs as `idle.gap` just before the command's first
   * line if it is admitted, and drops if it is refused (#146). A gap the
   * drive cannot log never costs the command: it goes without it.
   */
  dispatch(command: Command, extras?: { readonly idle_gap?: IdleGapBody }): Promise<Ack>;
  /**
   * A tool call's file, by digest (#372): `serve`'s `GET /files/<sha256>`, a recording's published asset, a
   * script's own. Unchecked: `files.ts`'s `read` hashes what it answers. Absent: this session has no files.
   */
  readonly file?: FileSource;
  /**
   * The operator's PNG, its bytes sent ahead of the ask that names it: `serve`'s `POST /files`. Absent: this
   * transport takes no attachments (a replay).
   */
  upload?(bytes: Uint8Array): Promise<Uploaded>;
  /** The connection's state now, then each change, and why when it is not live. Absent: always `live`. */
  watchLink?(listener: (link: Link, why?: string) => void): () => void;
  /** The call waiting on the operator now, then each change: `undefined` when none waits. Absent: none ever does (a replay). */
  watchPrompt?(listener: (prompt: Prompt | undefined) => void): () => void;
}
