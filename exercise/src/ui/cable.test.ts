import { describe, expect, it } from 'vitest';

import { cable } from './cable.ts';

describe('the cable from a trunk node to its side call', () => {
  it('runs straight across when the slot was free', () => {
    expect(cable(40, 0)).toBe('M0 0H40');
  });

  it('bends down through true quarter circles when the side call stacked below', () => {
    // Out to the middle, a quarter turn down, straight down, a quarter turn back out.
    expect(cable(40, 100, 6)).toBe('M0 0H14A6 6 0 0 1 20 6V94A6 6 0 0 0 26 100H40');
  });

  it('tightens the bend when the drop or the gap is too short for it', () => {
    expect(cable(40, 4, 6)).toBe('M0 0H18A2 2 0 0 1 20 2V2A2 2 0 0 0 22 4H40');
    expect(cable(8, 100, 6)).toBe('M0 0H2A2 2 0 0 1 4 2V98A2 2 0 0 0 6 100H8');
  });
});
