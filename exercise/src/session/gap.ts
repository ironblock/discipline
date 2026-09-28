/**
 * The idle gap, as the surface measures it (Q4, ruled on #117): from a turn
 * settling to the person's next accepted command, split into what they were
 * doing -- noticing, reading, composing, away, blocked -- in integer ms on
 * the surface's own clock. `diet` logs it as `idle.gap` (log v0), sent as
 * `idle_gap` on the command that ends it; the receipt's sixth number is
 * side-call time inside notice + read + compose.
 *
 * Pure: time is given, never read, so the phases are exact -- they sum to
 * the gap's wall clock to the millisecond, as the ruling's invariant needs.
 *
 *   notice   settling → the first sign the person is present
 *   read     → their first keystroke in the composer, or first click on a
 *              declare-seam control
 *   compose  → the accepted send or declare
 *   blocked  from the first send refused because work was in flight → the
 *            accepted send (the time after it is not composing)
 *   away     every interval the page was hidden, taken out of whichever
 *            phase it interrupted
 */

import type { GapEnd } from '../drive/log.ts';

/** The phases a gap moves through, in order; `away` is not one -- it is taken out of them. */
type Phase = 'notice' | 'read' | 'compose' | 'blocked';

/** `idle_gap` as a command carries it: what `diet` logs as the `idle.gap` line. */
export interface IdleGapBody {
  readonly opened_by: number;
  readonly notice: number;
  readonly read: number;
  readonly compose: number;
  readonly away: number;
  readonly blocked: number;
  readonly ended_by: GapEnd;
}

export class GapMeter {
  readonly #openedBy: number;
  readonly #openedAt: number;
  #phase: Phase;
  #phaseStart: number;
  /** Hidden since, while the page is. */
  #hiddenAt: number | undefined;
  /** Hidden time inside the current phase so far. */
  #awayInPhase = 0;
  readonly #spent: Record<Phase, number> = { notice: 0, read: 0, compose: 0, blocked: 0 };
  #away = 0;

  /**
   * A gap opened by the `turn.settled` at `openedBy`, at `now`. Zero notice
   * when the person was already interacting as it settled.
   */
  constructor(openedBy: number, now: number, { hidden = false, interacting = false } = {}) {
    this.#openedBy = openedBy;
    this.#openedAt = ms(now);
    this.#phase = interacting && !hidden ? 'read' : 'notice';
    this.#phaseStart = this.#openedAt;
    if (hidden) this.#hiddenAt = this.#openedAt;
  }

  /** What the person is doing now. */
  get phase(): Phase {
    return this.#phase;
  }

  /** A sign they are here: an input event, the settled block coming into view. Ends `notice`. */
  present(now: number): void {
    if (this.#phase === 'notice' && this.#hiddenAt === undefined) this.#enter('read', now);
  }

  /** The page hid, or showed again. Showing again is a sign of presence. */
  visibility(hidden: boolean, now: number): void {
    const t = ms(now);
    if (hidden) {
      this.#hiddenAt ??= t;
      return;
    }
    if (this.#hiddenAt !== undefined) {
      this.#awayInPhase += t - this.#hiddenAt;
      this.#hiddenAt = undefined;
    }
    this.present(t);
  }

  /** A keystroke in the composer, or a click on a declare-seam control: reading is over. */
  composing(now: number): void {
    this.present(now);
    if (this.#phase === 'read') this.#enter('compose', now);
  }

  /** A send refused because work was in flight -- or not even offered, the composer holding it for that reason. */
  refused(now: number): void {
    this.composing(now);
    if (this.#phase !== 'blocked') this.#enter('blocked', now);
  }

  /** The command that ends the gap was sent: the gap, as `diet` logs it. */
  end(now: number, endedBy: GapEnd): IdleGapBody {
    const t = ms(now);
    this.#close(t);
    return {
      opened_by: this.#openedBy,
      notice: this.#spent.notice,
      read: this.#spent.read,
      compose: this.#spent.compose,
      away: this.#away,
      blocked: this.#spent.blocked,
      ended_by: endedBy,
    };
  }

  /** The gap's wall clock to `now`: what the five sum to. */
  wall(now: number): number {
    return ms(now) - this.#openedAt;
  }

  #enter(next: Phase, now: number): void {
    this.#close(ms(now));
    this.#phase = next;
  }

  /** Close the current phase at `t`: its attended time to its total, its hidden time to `away`. */
  #close(t: number): void {
    const hiddenNow = this.#hiddenAt !== undefined ? t - this.#hiddenAt : 0;
    const away = this.#awayInPhase + hiddenNow;
    this.#spent[this.#phase] += t - this.#phaseStart - away;
    this.#away += away;
    this.#awayInPhase = 0;
    if (this.#hiddenAt !== undefined) this.#hiddenAt = t;
    this.#phaseStart = t;
  }
}

/** The surface's clock, in whole milliseconds: integers, so the phases sum exactly. */
const ms = (t: number) => Math.round(t);
