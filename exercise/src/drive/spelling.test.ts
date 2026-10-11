import { describe, expect, it } from 'vitest';

import { respelled } from './spelling.ts';

describe('the boundary respelled (#583)', () => {
  it('reads each old word at its tagged position as the new one, and leaves free text and new words alone', () => {
    expect(respelled({ kind: 'seam', at_turn: 1, render: 'the seam' })).toEqual({ kind: 'meditation', at_turn: 1, render: 'the seam' });
    expect(respelled({ kind: 'refused', command: 'declare-seam', because: 'nothing-to-seam' })).toEqual({
      kind: 'refused',
      command: 'declare-meditation',
      because: 'nothing-to-meditate',
    });
    expect(respelled({ kind: 'idle.gap', ended_by: 'seam' })).toEqual({ kind: 'idle.gap', ended_by: 'meditation' });
    expect(respelled({ kind: 'fork', why: 'seam' }).why).toBe('meditation');
    expect(respelled({ kind: 'session.start', fork_delivery: 'seam' }).fork_delivery).toBe('meditation');
    expect(respelled({ kind: 'phase.ruled', choice: 'seam' }).choice).toBe('meditation');
    expect(respelled({ kind: 'seam.held', at_turn: 1 }).kind).toBe('meditation.held');
    expect(respelled({ kind: 'refused', because: 'seam-held' }).because).toBe('meditation-held');
    // On another kind, the same key is free text.
    expect(respelled({ kind: 'captured', why: 'seam' }).why).toBe('seam');
    const current = { kind: 'meditation', at_turn: 2 };
    expect(respelled(current)).toBe(current);
  });
});
