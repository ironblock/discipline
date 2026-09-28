import { describe, expect, it } from 'vitest';

import type { IdleGapBody } from './gap.ts';
import { GapMeter } from './gap.ts';

/** Q4's invariant: the five phases sum to the gap's wall clock -- here, exactly. */
const sum = (g: IdleGapBody) => g.notice + g.read + g.compose + g.away + g.blocked;

describe('the idle gap, measured (Q4)', () => {
  it('splits a gap into noticing, reading and composing, summing to its wall clock', () => {
    const m = new GapMeter(7, 1000);
    m.present(1400);
    m.composing(9400);
    const g = m.end(12_400, 'ask');
    expect(g).toEqual({ opened_by: 7, notice: 400, read: 8000, compose: 3000, away: 0, blocked: 0, ended_by: 'ask' });
    expect(sum(g)).toBe(11_400);
  });

  it('has no notice when the person was already interacting as the turn settled', () => {
    const m = new GapMeter(7, 1000, { interacting: true });
    expect(m.phase).toBe('read');
    m.composing(3000);
    expect(m.end(4000, 'ask')).toMatchObject({ notice: 0, read: 2000, compose: 1000 });
  });

  it('takes time hidden out of the phase it interrupted, as away', () => {
    const m = new GapMeter(7, 0);
    m.present(100);
    m.visibility(true, 1000);
    m.visibility(false, 6000);
    m.composing(7000);
    const g = m.end(8000, 'ask');
    expect(g).toMatchObject({ notice: 100, read: 1900, compose: 1000, away: 5000 });
    expect(sum(g)).toBe(8000);
  });

  it('splits time hidden across a phase boundary between the phases, all of it away', () => {
    const m = new GapMeter(7, 0);
    m.present(100);
    m.visibility(true, 1000);
    m.composing(2000);
    m.visibility(false, 3000);
    const g = m.end(4000, 'ask');
    expect(g).toMatchObject({ read: 900, compose: 1000, away: 2000 });
    expect(sum(g)).toBe(4000);
  });

  it('opens hidden in notice, and showing the page again is the first sign of presence', () => {
    const m = new GapMeter(7, 0, { hidden: true });
    m.present(500);
    expect(m.phase).toBe('notice');
    m.visibility(false, 30_000);
    expect(m.phase).toBe('read');
    const g = m.end(31_000, 'seam');
    expect(g).toMatchObject({ notice: 0, read: 1000, away: 30_000, ended_by: 'seam' });
  });

  it('counts blocked from the first send refused because work was in flight, not as composing', () => {
    const m = new GapMeter(7, 0);
    m.present(0);
    m.composing(1000);
    m.refused(4000);
    m.refused(6000);
    const g = m.end(10_000, 'ask');
    expect(g).toMatchObject({ read: 1000, compose: 3000, blocked: 6000 });
    expect(sum(g)).toBe(10_000);
  });

  it('keeps the sum exact on a clock that is not whole milliseconds', () => {
    const m = new GapMeter(7, 1000.4);
    m.present(1333.3);
    m.visibility(true, 2000.6);
    m.visibility(false, 2500.2);
    m.composing(3000.7);
    const g = m.end(4999.9, 'cancel');
    expect(sum(g)).toBe(m.wall(4999.9));
    expect(Object.values(g).filter((v) => typeof v === 'number').every(Number.isInteger)).toBe(true);
  });

  it('reads out the gap a command would carry without ending it: refused, it keeps running', () => {
    const m = new GapMeter(7, 0);
    m.present(100);
    m.composing(1000);
    expect(m.ending(2000, 'ask')).toMatchObject({ notice: 100, read: 900, compose: 1000 });
    m.refused(2000);
    expect(m.phase).toBe('blocked');
    const g = m.end(5000, 'ask');
    expect(g).toMatchObject({ notice: 100, read: 900, compose: 1000, blocked: 3000 });
    expect(sum(g)).toBe(5000);
  });
});
