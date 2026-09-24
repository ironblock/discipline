/**
 * The drive interface: everything the surface can ask of a session.
 *
 * Narrow on purpose. The surface subscribes to the session's log and sends
 * three commands; everything it draws is folded from the log. When `diet`'s
 * loop is served over HTTP + SSE (#117), an `HttpTransport` implements this
 * against it -- `subscribe` is the SSE stream (history, then tail), and the
 * commands are POSTs -- and nothing above this file changes.
 */

import type { DriveEvent } from './events.ts';

export type Refusal =
  /** A turn or a capture round is in flight; the ask was not taken. */
  | 'busy'
  /** The session has ended. */
  | 'ended'
  /** There is nothing to seam: the session has no turns yet. */
  | 'nothing-to-seam'
  /** Nothing is in flight. */
  | 'nothing-to-cancel'
  /** Canned transport only: the script expects a different command next. */
  | 'off-script';

export type Ack = { readonly ok: true } | { readonly ok: false; readonly refused: Refusal };

export interface DriveTransport {
  /** Every event so far, in order, then each new one. Returns an unsubscribe. */
  subscribe(listener: (event: DriveEvent) => void): () => void;
  /** Send a person's ask to the trunk. */
  send(ask: string): Promise<Ack>;
  /** Stop whatever is in flight, the trunk's call or a fork's. */
  cancel(): Promise<Ack>;
  /** Declare a phase transition: ratify, render, refill. */
  declareSeam(to: string): Promise<Ack>;
}
