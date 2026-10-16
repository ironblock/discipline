import { describe, expect, it } from 'vitest';

import { MODEL_PROPOSAL_KEPT, MODEL_PROPOSED, OPERATOR_ACCEPTED, OPERATOR_ASKED, OPERATOR_DECLINED, PHASE_IDLE, PHASE_OPENED, PHASE_STARTED } from '../drive/phase-keys.ts';
import { fold } from './fold.ts';

describe('the two keys (#788), as the kitchen sink logged them, folded for the phase control (#789)', () => {
  it('knows every line of them', () => {
    for (const log of [MODEL_PROPOSAL_KEPT, OPERATOR_DECLINED, OPERATOR_ACCEPTED, PHASE_STARTED]) expect([...fold(log).unknown.keys()]).toEqual([]);
  });

  it('takes the model’s proposal from `phase.proposed`, with its evidence', () => {
    expect(fold(MODEL_PROPOSED).proposal).toEqual({
      call: 'call-p1',
      from: 'orient',
      to: 'plan',
      reason: 'the scope is settled',
      evidence: { steps: 35, tokens: 4948, sinceMeditation: 0, manualMeditation: false },
    });
  });

  it('moves the phase on a keep, and clears the proposal', () => {
    const session = fold(MODEL_PROPOSAL_KEPT);
    expect(session.proposal).toBeUndefined();
    expect(session.phase).toBe('build');
    expect(session.phaseDocument?.name).toBe('build');
  });

  it('holds the operator’s proposal until the model answers, then says the answer', () => {
    expect(fold(OPERATOR_ASKED).asked).toEqual({ to: 'test', meditate: false });
    const accepted = fold(OPERATOR_ACCEPTED);
    expect(accepted.asked).toBeUndefined();
    expect(accepted.answered).toEqual({ to: 'test', accepted: true, reason: 'the list is built' });
    expect(accepted.phase).toBe('test');
    const declined = fold(OPERATOR_DECLINED);
    expect(declined.answered).toEqual({ to: 'build', accepted: false, reason: 'the list is not tested yet' });
    expect(declined.phase).toBe('test');
  });

  it('offers the library to start from until the first ask, and starts where the operator chose', () => {
    expect(fold(PHASE_OPENED).startable).toEqual(['plan', 'build', 'orient', 'quick', 'test']);
    expect(fold(PHASE_IDLE).startable).toBeUndefined();
    const started = fold(PHASE_STARTED);
    expect(started.phase).toBe('orient');
    expect(started.phaseDocument?.name).toBe('orient');
  });

  it('idles in `build` with nothing proposed: the operator’s key', () => {
    const idle = fold(PHASE_IDLE);
    expect([idle.phase, idle.proposal, idle.asked]).toEqual(['build', undefined, undefined]);
  });
});
