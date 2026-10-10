import { describe, expect, it } from 'vitest';

import { driveAuthHeaders } from './proxy-auth.ts';

describe('the dev proxy’s credential for serve', () => {
  it('presents serve’s auth file as a Basic credential, its trailing newline dropped', () => {
    expect(driveAuthHeaders('operator:s3cret\n')).toEqual({ Authorization: `Basic ${btoa('operator:s3cret')}` });
  });

  it('presents nothing for a blank file, and refuses one that is not user:password', () => {
    expect(driveAuthHeaders('  \n')).toBeUndefined();
    expect(() => driveAuthHeaders('just-a-password')).toThrow(/user:password/);
  });
});
