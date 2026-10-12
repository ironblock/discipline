import { describe, expect, it } from 'vitest';

import { MODEL_PROPOSAL_KEPT, MODEL_PROPOSED, OPERATOR_ACCEPTED, OPERATOR_ASKED, OPERATOR_DECLINED, PHASE_IDLE, PHASE_OPENED, PHASE_STARTED } from '../drive/served/phase-keys.ts';
import { fold } from './fold.ts';

describe('the two keys (#788), folded for the phase control (#789)', () => {
  it('knows every line of them', () => {
    for (const log of [MODEL_PROPOSAL_KEPT, OPERATOR_DECLINED, OPERATOR_ACCEPTED, PHASE_STARTED]) expect([...fold(log).unknown.keys()]).toEqual([]);
  });

  it('takes the model’s proposal from `phase.proposed`, with its evidence', () => {
    expect(fold(MODEL_PROPOSED).proposal).toEqual({
      call: 'p1',
      from: 'plan',
      to: 'build',
      reason: 'the plan is settled',
      evidence: { steps: 6, tokens: 12_400, sinceMeditation: 12_400, manualMeditation: false },
    });
  });

  it('moves the phase on a keep, and clears the proposal', () => {
    const session = fold(MODEL_PROPOSAL_KEPT);
    expect(session.proposal).toBeUndefined();
    expect(session.phase).toBe('build');
    expect(session.phaseDocument?.name).toBe('build');
  });

  it('holds the operator’s proposal until the model answers, then says the answer', () => {
    expect(fold(OPERATOR_ASKED).asked).toEqual({ to: 'build', meditate: false });
    const declined = fold(OPERATOR_DECLINED);
    expect(declined.asked).toBeUndefined();
    expect(declined.answered).toEqual({ to: 'build', accepted: false, reason: 'the tables are not settled yet' });
    expect(declined.phase).toBe('plan');
    const accepted = fold(OPERATOR_ACCEPTED);
    expect(accepted.answered).toEqual({ to: 'build', accepted: true });
    expect(accepted.phase).toBe('build');
  });

  it('offers the library to start from until the first ask, and starts where the operator chose', () => {
    expect(fold(PHASE_OPENED).startable).toEqual(['plan', 'build', 'test']);
    expect(fold(PHASE_IDLE).startable).toBeUndefined();
    const started = fold(PHASE_STARTED);
    expect(started.phase).toBe('build');
    expect(started.phaseDocument?.name).toBe('build');
  });
});
