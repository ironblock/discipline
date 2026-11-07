import { describe, expect, it } from 'vitest';

import { titlebarInset } from './titlebar.ts';

describe('the overlay title bar (macOS shell)', () => {
  it('reads the traffic lights’ edge the shell measured, in points', () => {
    expect(titlebarInset('?drive=http%3A%2F%2F127.0.0.1%3A7801&titlebar=78')).toBe(78);
  });

  it('is none in a browser, and for an edge that is not a positive number', () => {
    for (const search of ['', '?drive=x', '?titlebar=0', '?titlebar=-4', '?titlebar=wide', '?titlebar='])
      expect(titlebarInset(search), search).toBeUndefined();
  });
});
