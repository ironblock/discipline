import { describe, expect, it } from 'vitest';

import { FORK_ASKS } from '../../../diet/dogma/fork-asks.ts';
import { pickerAsks } from './forkAsks.ts';

describe('the fork picker\u2019s pinned asks (#1021)', () => {
  it('offers the five first, in their order, and every other one under more', () => {
    const { first, more } = pickerAsks();
    expect(first.map((ask) => ask.name)).toEqual(['GOTCHAS', 'DECISIONS', 'CONSTRAINTS', 'OPEN_NEXT', 'RATIONALE']);
    expect([...first, ...more].map((ask) => ask.name).sort()).toEqual(FORK_ASKS.map((ask) => ask.name).sort());
    expect(more.some((ask) => first.includes(ask))).toBe(false);
  });

  it('shows each ask with the text it sends', () => {
    for (const ask of FORK_ASKS) expect(ask.text.trim()).not.toBe('');
  });
});
