import { describe, expect, it } from 'vitest';

import { bytes, count, lines, ms, rate, took, tokens } from './format.ts';

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

describe('the other numbers the surface prints', () => {
  it('took: milliseconds, then tenths of a second, then minutes with padded seconds', () => {
    expect([took(999), took(1_000), took(59_950), took(60_000), took(125_000)]).toEqual(['999 ms', '1.0 s', '60.0 s', '1m 00s', '2m 05s']);
  });

  it('tokens: whole under a thousand, one decimal under ten thousand, then tenths of a thousand', () => {
    expect([tokens(999), tokens(1_512), tokens(18_009), tokens(123_456)]).toEqual(['999', '1.5k', '18k', '123.5k']);
  });

  it('count: grouped by thousands', () => {
    expect([count(7), count(1_860), count(1_234_567)]).toEqual(['7', '1,860', '1,234,567']);
  });

  it('rate: per second, one decimal under a hundred, grouped above; a dash when there is nothing to divide', () => {
    expect([rate(36, 1_000), rate(1_417, 1_000), rate(10, 0), rate(0, 500)]).toEqual(['36.0', '1,417', '–', '–']);
  });

  it('bytes: of the UTF-8 encoding, not the string length', () => {
    expect([bytes(''), bytes('é'), bytes('x'.repeat(2_048)), bytes('x'.repeat(3 * 1024 * 1024))]).toEqual(['0 B', '2 B', '2.0 KB', '3.0 MB']);
  });

  it('lines: none in nothing, one more than there are newlines otherwise', () => {
    expect([lines(''), lines('a'), lines('a\nb'), lines('a\n')]).toEqual([0, 1, 2, 2]);
  });
});
