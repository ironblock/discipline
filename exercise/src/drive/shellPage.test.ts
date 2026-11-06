import { describe, expect, it } from 'vitest';

import { shellPage } from './shellPage.ts';

/**
 * The native shell's own page before and after its drive (the app audit, finding 6): the window opens at once on
 * "starting serve…", and a start that fails -- or a sidecar that exits -- is said there, not on a stderr nobody sees.
 */
describe('shellPage: what the shell says while it has no drive', () => {
  it('is nothing on a page with a drive, or in a browser', () => {
    expect(shellPage('?drive=http%3A%2F%2F127.0.0.1%3A7801%2F')).toBeUndefined();
    expect(shellPage('')).toBeUndefined();
  });

  it('is the start, then a failure with its reason', () => {
    expect(shellPage('?shell=starting')).toEqual({ kind: 'starting' });
    expect(shellPage('?shell=failed&why=serve+exited+%28code+1%29')).toEqual({ kind: 'failed', why: 'serve exited (code 1)' });
    expect(shellPage('?shell=failed')).toEqual({ kind: 'failed', why: 'the shell could not start its drive' });
  });
});
