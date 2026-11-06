import { describe, expect, it } from 'vitest';

import { AUTHORED } from '../drive/examples.ts';
import { placed } from '../drive/recorded.ts';
import { fold } from './fold.ts';

/**
 * The phase control's facts, folded from the kitchen sink's own log at every line (#1050's page audit, F6 and F16):
 * the phase, its moves and ground rules, and the model's last answer agree with each other at every moment.
 */
const LOG = placed(AUTHORED['kitchen-sink']);

describe('the phase and its document, at every line of the kitchen sink', () => {
  it('are the same phase: a ruling to meditate and move takes effect at its meditation, not before (F6)', () => {
    const disagree: number[] = [];
    for (let i = 1; i <= LOG.length; i += 1) {
      const session = fold(LOG.slice(0, i));
      if (session.phaseDocument !== undefined && session.phase !== '' && session.phaseDocument.name !== session.phase) disagree.push(LOG[i - 1]!.seq);
    }
    expect(disagree, 'lines after which the composer would show one phase with another phase’s moves').toEqual([]);
  });

  it('say no answer of the model’s about a move the phase has since made or left (F16)', () => {
    // The model declined build, then the operator moved to build alone: its answer is past.
    expect(fold(LOG).answered).toBeUndefined();
  });
});
