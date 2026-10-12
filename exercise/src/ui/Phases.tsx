/**
 * Phases on the page (#789): the starting phase, the current one with its moves, the two-key control with its
 * evidence, and the ground rules in force. Driven by props; wired to the log once #787 and #788 land.
 */
import { useId, useState } from 'react';

import type { PhaseChoice } from '../drive/transport.ts';
import type { PhaseEvidence } from '../session/fold.ts';
import { tokens } from './format.ts';
import './phases.css';

/** A move from the current phase (#787): offered, or one the model may take unprompted. */
export interface PhaseMove {
  readonly to: string;
  readonly kind: 'suggested' | 'allowed';
}

/** What an operator's choice is made with (#788), as the fold reads it. */
export type { PhaseEvidence };

/** The operator's ruling on the model's proposal (#788): `keep` moves without a meditation. One word set, the wire's. */
export type PhaseRuling = PhaseChoice;

export interface PhasesProps {
  readonly phase: string;
  readonly moves: readonly PhaseMove[];
  readonly groundRules?: string;
  readonly evidence?: PhaseEvidence;
  /** The model's proposal waiting on the operator. */
  readonly proposal?: { readonly from?: string; readonly to: string; readonly reason?: string };
  /** The operator's proposal waiting on the model (#788): no second is offered meanwhile. */
  readonly asked?: { readonly to: string; readonly meditate: boolean };
  /** The model's answer to the operator's last proposal (#788). */
  readonly answered?: { readonly to: string; readonly accepted: boolean; readonly reason?: string };
  /** Why there is nothing to meditate on (#685), as serve would refuse it: meditate-and-move is not offered. */
  readonly noMeditation?: string;
  /** `operator_unilateral` (#788): the operator may move alone. */
  readonly unilateral: boolean;
  readonly idle: boolean;
  /** The library to start from: present only before the first ask. */
  readonly start?: { readonly library: readonly string[] };
  readonly onStart?: (phase: string) => void;
  readonly onRule?: (ruling: PhaseRuling) => void;
  readonly onPropose?: (to: string) => void;
  readonly onMove?: (to: string, how: Exclude<PhaseRuling, 'stay'>) => void;
}

export function Phases({ phase, moves, groundRules, evidence, proposal, asked, answered, noMeditation, unilateral, idle, start, onStart, onRule, onPropose, onMove }: PhasesProps) {
  const [rules, setRules] = useState(false);
  const rulesId = useId();
  return (
    <section className="ex-phases" aria-label="phase">
      <div className="ex-phases__now">
        {start ? (
          <label className="ex-phases__start">
            <span className="ex-phases__label">start in</span>{' '}
            <select aria-label="starting phase" value={phase} disabled={!idle} onChange={(e) => onStart?.(e.target.value)}>
              {start.library.map((name) => (
                <option key={name} value={name}>
                  {name}
                </option>
              ))}
            </select>
          </label>
        ) : (
          <span className="ex-phases__current">
            <span className="ex-phases__label">phase</span> {phase}
          </span>
        )}
        {groundRules !== undefined ? (
          <button type="button" aria-label={`Ground rules of ${phase}`} aria-expanded={rules} aria-controls={rulesId} onClick={() => setRules(!rules)}>
            ground rules
          </button>
        ) : null}
      </div>
      {rules && groundRules !== undefined ? (
        <p id={rulesId} className="ex-phases__rules">
          {groundRules}
        </p>
      ) : null}
      {moves.length > 0 ? (
        <ul className="ex-phases__moves" aria-label={`moves from ${phase}`}>
          {moves.map((move) => (
            <li key={move.to} data-kind={move.kind}>{`${move.to} · ${move.kind}`}</li>
          ))}
        </ul>
      ) : null}
      {asked ? (
        <p className="ex-phases__asked" role="status" aria-label="asked the model">
          {`asked the model: ${phase} → ${asked.to}, ${asked.meditate ? 'through a meditation' : 'keeping the trunk'}`}
        </p>
      ) : null}
      {answered ? (
        <p className="ex-phases__answered" role="status" aria-label="the model answered">
          {`the model ${answered.accepted ? 'accepted' : 'declined'} ${answered.to}${answered.reason !== undefined ? `: ${answered.reason}` : ''}`}
        </p>
      ) : null}
      {proposal ? (
        // The model's key (#788): its proposal ended its turn; the operator turns the other -- meditate and move, as
        // suggested, keep, or stay -- with the facts that make the choice.
        <section className="ex-phases__proposal" aria-label={`the model proposes ${proposal.from ?? 'no phase'} → ${proposal.to}`}>
          <span className="ex-phases__label">the model proposes</span> {`${proposal.from ?? 'no phase'} → ${proposal.to}`}
          {proposal.reason !== undefined ? <p className="ex-phases__reason">{proposal.reason}</p> : null}
          {evidence ? <Evidence evidence={evidence} /> : null}
          <span className="ex-phases__actions">
            <button
              type="button"
              data-suggested=""
              aria-label={`Meditate and move to ${proposal.to}`}
              disabled={!idle || noMeditation !== undefined}
              onClick={() => onRule?.('meditation')}
            >
              meditate and move
            </button>
            <button type="button" aria-label={`Keep, move to ${proposal.to}`} disabled={!idle} onClick={() => onRule?.('keep')}>
              keep
            </button>
            <button type="button" aria-label={`Stay in ${phase}`} disabled={!idle} onClick={() => onRule?.('stay')}>
              stay
            </button>
          </span>
          {noMeditation !== undefined ? <span className="ex-phases__why">no meditation: {noMeditation}</span> : null}
        </section>
      ) : moves.length > 0 && !asked && (onPropose || onMove) ? (
        // The operator's key (#788): propose a move to the model, or, where `operator_unilateral` allows, take it.
        <section className="ex-phases__operator" aria-label={`move from ${phase}`}>
          {evidence ? <Evidence evidence={evidence} /> : null}
          {moves.map((move) => (
            <span key={move.to} className="ex-phases__actions">
              <span className="ex-phases__to">{move.to}</span>
              {unilateral && onMove ? (
                <>
                  <button type="button" aria-label={`Meditate and move to ${move.to}`} disabled={!idle || noMeditation !== undefined} onClick={() => onMove?.(move.to, 'meditation')}>
                    meditate and move
                  </button>
                  <button type="button" aria-label={`Keep, move to ${move.to}`} disabled={!idle} onClick={() => onMove?.(move.to, 'keep')}>
                    keep
                  </button>
                </>
              ) : null}
              {onPropose ? (
                <button type="button" aria-label={`Propose ${move.to} to the model`} disabled={!idle} onClick={() => onPropose(move.to)}>
                  propose
                </button>
              ) : null}
            </span>
          ))}
        </section>
      ) : null}
    </section>
  );
}

/** The facts an operator's choice is made with (#788): the phase's length, the tokens since the last meditation, and a
 * manual meditation just taken. */
function Evidence({ evidence }: { readonly evidence: PhaseEvidence }) {
  const id = useId();
  const row = (key: string, term: string, value: string) => (
    <div key={key}>
      <dt id={`${id}-${key}`}>{term}</dt>
      <dd aria-labelledby={`${id}-${key}`}>{value}</dd>
    </div>
  );
  return (
    <dl className="ex-phases__evidence">
      {row('length', 'phase length', `${evidence.steps} steps · ${tokens(evidence.tokens)} tokens`)}
      {row('since', 'since the last meditation', `${tokens(evidence.sinceMeditation)} tokens`)}
      {evidence.manualMeditation ? row('manual', 'manual meditation', 'just now') : null}
    </dl>
  );
}
