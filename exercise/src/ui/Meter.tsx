import type { CSSProperties } from 'react';

import type { Meter as Progress } from '../session/fold.ts';
import { counter, rate, tokens } from './format.ts';
import './meter.css';

/** How much of a prompt is in the model: the warm part and the new part read, as a fraction of all of it. */
export function readFraction(m: Progress): number {
  return m.total === 0 ? 0 : Math.min(1, (m.cache + m.processed) / m.total);
}

/**
 * Prefill, measured: a bar of the whole prompt with the warm part there from
 * the start and the new part filling, and a line saying how many of how many,
 * how fast, and how long is left. What "reading the prompt" was for twelve
 * seconds, with the numbers in it.
 */
export function Meter({ meter: m }: { readonly meter: Progress }) {
  const fresh = m.total - m.cache;
  const left = m.ppRate !== undefined && m.ppRate > 0 ? ((fresh - m.processed) / m.ppRate) * 1000 : undefined;
  const style = {
    '--warm': m.total === 0 ? 0 : m.cache / m.total,
    '--read': fresh === 0 ? 1 : m.processed / fresh,
  } as CSSProperties;
  return (
    <div className="ex-meter" role="status" style={style}>
      <div className="ex-meter__bar" aria-hidden="true">
        <span className="ex-meter__warm" />
        <span className="ex-meter__read" />
      </div>
      <p className="ex-meter__line">
        reading the prompt · {tokens(m.processed)} of {tokens(fresh)} new
        {m.cache > 0 ? ` · ${tokens(m.cache)} warm` : ''}
        {m.ppRate !== undefined ? ` · ${rate(m.ppRate, 1000)} pp t/s` : ''}
        {left !== undefined ? ` · ${counter(left)} left` : ''}
      </p>
    </div>
  );
}
