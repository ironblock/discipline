import { describe, expect, it } from 'vitest';

import { ms } from './format.ts';

describe('a duration, as the stats print it', () => {
  it('picks its unit by what it rounds to, so it never prints the upper bound of a unit', () => {
    expect(ms(999.4)).toBe('999 ms');
    expect(ms(999.6)).toBe('1.00 s');
    expect(ms(9_999)).toBe('10.0 s');
    expect(ms(59_990)).toBe('1m 0s');
    expect(ms(119_700)).toBe('2m 0s');
  });

  it('keeps hundredths under ten seconds, tenths under a minute, whole seconds past it', () => {
    expect(ms(1_234)).toBe('1.23 s');
    expect(ms(12_345)).toBe('12.3 s');
    expect(ms(125_000)).toBe('2m 5s');
  });
});
