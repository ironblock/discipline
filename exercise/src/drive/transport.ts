/**
 * The drive interface: everything the surface can ask of a session.
 *
 * Narrow on purpose. The surface subscribes to the session's log and
 * dispatches commands; everything it draws is folded from the log. When
 * `diet`'s loop is served over HTTP + SSE (#117), an `HttpTransport`
 * implements this against it -- `subscribe` is the SSE stream (history, then
 * tail), and each command is one POST -- and nothing above this file changes.
 */

import type { DriveEvent, Open } from './events.ts';

/**
 * What a person can ask of the drive. Closed: the surface owns what it
 * sends, and a new command should break the build where it must be offered.
 * Named so far and not yet here: approve, deny, end, edit memory, retry,
 * annotate, a choice at ratify.
 */
export type Command =
  /** A person's ask, for the trunk. */
  | { readonly kind: 'ask'; readonly text: string }
  /** Stop whatever is in flight, the trunk's call or a fork's. */
  | { readonly kind: 'cancel' }
  /** Declare a phase transition: ratify, render, refill. */
  | { readonly kind: 'seam'; readonly to: string };

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
>;

export type Ack = { readonly ok: true } | { readonly ok: false; readonly refused: Refusal };

export interface DriveTransport {
  /** Every event so far, in order, then each new one. Returns an unsubscribe. */
  subscribe(listener: (event: DriveEvent) => void): () => void;
  dispatch(command: Command): Promise<Ack>;
}
