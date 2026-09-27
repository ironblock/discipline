import { describe, expect, it } from 'vitest';

import { chain } from './chain.ts';

// Node q/1 asked side calls i/1 and e/1; q/2 asked i/2. i/1 wrote m1 and m2, e/1 wrote m2, i/2 wrote m3.
const written = [
  { branch: 'i/1', entry: 'm1' },
  { branch: 'i/1', entry: 'm2' },
  { branch: 'e/1', entry: 'm2' },
  { branch: 'i/2', entry: 'm3' },
];
const sides: Record<string, string[]> = { 'q/1': ['i/1', 'e/1'], 'q/2': ['i/2'] };
const at: Record<string, string> = { 'i/1': 'q/1', 'e/1': 'q/1', 'i/2': 'q/2' };
const lit = (pointed: Parameters<typeof chain>[0]) => {
  const c = chain(pointed, written, (n) => sides[n] ?? [], (b) => at[b]);
  return { nodes: [...c.nodes].sort(), branches: [...c.branches].sort(), entries: [...c.entries].sort(), lines: [...c.lines].sort() };
};

describe('the chain through what is pointed at', () => {
  it('from a trunk node: its side calls, and everything they wrote', () => {
    expect(lit({ node: 'q/1' })).toEqual({ nodes: ['q/1'], branches: ['e/1', 'i/1'], entries: ['m1', 'm2'], lines: ['e/1>m2', 'i/1>m1', 'i/1>m2'] });
  });

  it('from a side call: the node it came from, and what it wrote', () => {
    expect(lit({ branches: ['i/1'] })).toEqual({ nodes: ['q/1'], branches: ['i/1'], entries: ['m1', 'm2'], lines: ['i/1>m1', 'i/1>m2'] });
  });

  it('from an entry: the side calls that wrote it and their nodes -- not what else they wrote', () => {
    expect(lit({ entry: 'm2' })).toEqual({ nodes: ['q/1'], branches: ['e/1', 'i/1'], entries: ['m2'], lines: ['e/1>m2', 'i/1>m2'] });
  });

  it('from a line into memory: its two ends, and the node above', () => {
    expect(lit({ branches: ['i/1'], entry: 'm1' })).toEqual({ nodes: ['q/1'], branches: ['i/1'], entries: ['m1'], lines: ['i/1>m1'] });
  });

  it('from a trunk net: its node and only the side calls on it', () => {
    expect(lit({ node: 'q/1', branches: ['e/1'] })).toEqual({ nodes: ['q/1'], branches: ['e/1'], entries: ['m2'], lines: ['e/1>m2'] });
  });

  it('from nothing: nothing', () => {
    expect(lit({})).toEqual({ nodes: [], branches: [], entries: [], lines: [] });
  });
});
