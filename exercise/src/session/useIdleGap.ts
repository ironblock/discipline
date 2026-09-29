import { useCallback, useEffect, useRef } from 'react';

import type { GapEnd } from '../drive/log.ts';
import type { Session } from './fold.ts';
import { GapMeter } from './gap.ts';
import type { IdleGapBody } from './gap.ts';

/**
 * How recently an input event must have come for the person to count as
 * already interacting when a turn settles -- notice is then zero. Q4 (a),
 * ruled on #117: `presence_window_ms`, a constant log v1 declares on
 * `idle.gap`'s definition
 * (https://github.com/ironblock/discipline/issues/117#issuecomment-5883557987).
 */
const PRESENCE_WINDOW_MS = 2_000;

const MODIFIERS: ReadonlySet<string> = new Set(['Shift', 'Control', 'Alt', 'Meta', 'CapsLock', 'Tab', 'Escape']);

/**
 * The idle gap each settled turn opens, measured on the page (Q4, `gap.ts`):
 * opened when a `turn.settled` arrives, fed by what the person does, and
 * taken -- ended -- by the command that closes it, which carries it to the
 * drive. The signals:
 *
 *   presence   the page becoming visible; any pointer, key, wheel or touch
 *              -- a scroll by hand, not the page following its bottom; the
 *              settled block coming into view (a change, after the gap
 *              opened: already in view when it settled is not a sign)
 *   composing  a keystroke in the composer; a click on its refill controls
 *   refused    Enter on a draft while work is in flight -- the composer holds
 *              the send for that reason, and a held send emits nothing
 *   away       the page hidden
 */
export interface IdleGapCarrier {
  /** The gap a command would end now, to carry; the meter keeps running until `admitted`. */
  readonly carry: (endedBy: GapEnd) => IdleGapBody | undefined;
  /** The carrying command was admitted: the gap is over. */
  readonly admitted: () => void;
  /** A send was held or refused because work was in flight: the person is blocked from here. It carries no gap (Q4 (d)). */
  readonly refused: () => void;
}

export function useIdleGap(session: Session): IdleGapCarrier {
  const meter = useRef<GapMeter | undefined>(undefined);
  const opened = useRef<number | undefined>(undefined);
  const lastInput = useRef(Number.NEGATIVE_INFINITY);
  const busy = useRef(false);
  busy.current = session.state === 'turn' || session.state === 'capture' || session.state === 'ratify';

  // A new settling opens a gap.
  useEffect(() => {
    const seq = session.lastSettled;
    if (seq === undefined || seq === opened.current) return;
    opened.current = seq;
    const now = performance.now();
    meter.current = new GapMeter(seq, now, { hidden: document.visibilityState === 'hidden', interacting: now - lastInput.current < PRESENCE_WINDOW_MS });
    // The settled block coming into view: the last node of the trunk, once drawn.
    const block = document.querySelector('.ex-trunk .ex-trunk__node:last-child');
    if (!block || typeof IntersectionObserver === 'undefined') return;
    let first = true;
    const watch = new IntersectionObserver(([entry]) => {
      // The first report is where it is now, not a change.
      if (first) return void (first = false);
      if (entry?.isIntersecting) meter.current?.present(performance.now());
    });
    watch.observe(block);
    return () => watch.disconnect();
  }, [session.lastSettled]);

  // What the person does, page-wide.
  useEffect(() => {
    const present = () => {
      lastInput.current = performance.now();
      meter.current?.present(lastInput.current);
    };
    const onKey = (e: KeyboardEvent) => {
      present();
      const input = e.target instanceof HTMLElement ? e.target.closest<HTMLTextAreaElement>('.ex-composer__input') : null;
      if (!input || MODIFIERS.has(e.key)) return;
      const now = performance.now();
      if (e.key === 'Enter' && !e.shiftKey && busy.current && input.value.trim() !== '') meter.current?.refused(now);
      else meter.current?.composing(now);
    };
    const onClick = (e: MouseEvent) => {
      present();
      if (e.target instanceof HTMLElement && e.target.closest('.ex-composer__seam')) meter.current?.composing(performance.now());
    };
    const onVisibility = () => meter.current?.visibility(document.visibilityState === 'hidden', performance.now());
    window.addEventListener('keydown', onKey, true);
    window.addEventListener('pointerdown', onClick, true);
    window.addEventListener('wheel', present, { capture: true, passive: true });
    window.addEventListener('touchmove', present, { capture: true, passive: true });
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      window.removeEventListener('keydown', onKey, true);
      window.removeEventListener('pointerdown', onClick, true);
      window.removeEventListener('wheel', present, { capture: true });
      window.removeEventListener('touchmove', present, { capture: true });
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, []);

  const carry = useCallback((endedBy: GapEnd) => meter.current?.ending(performance.now(), endedBy), []);
  const admitted = useCallback(() => {
    meter.current = undefined;
  }, []);
  const refused = useCallback(() => meter.current?.refused(performance.now()), []);
  return { carry, admitted, refused };
}
