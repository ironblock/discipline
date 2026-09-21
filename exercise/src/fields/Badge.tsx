import type { Reasoning, Verdict } from '../record/types.ts';
import './fields.css';

type Hue = 'ok' | 'amber' | 'orange' | 'red' | 'blue' | 'magenta' | 'muted';

const REASONING_HUE: Readonly<Record<Reasoning, Hue>> = {
  on: 'ok',
  off: 'muted',
  suppressed: 'orange',
  undeclared: 'amber',
};

const VERDICT_HUE: Readonly<Record<Verdict, Hue>> = {
  supported: 'ok',
  refuted: 'red',
  inconclusive: 'amber',
  unadjudicated: 'muted',
};

/** The substrate's reasoning state: an outcome, not a control. */
export function ReasoningBadge({ reasoning }: { readonly reasoning: Reasoning }) {
  return (
    <span className={`ex-chip ex-outcome ex-badge--${REASONING_HUE[reasoning]}`} data-field="reasoning">
      <span className="ex-kind">reasoning</span>
      {reasoning}
    </span>
  );
}

/** A claim's verdict. `unadjudicated` is muted because it is not a verdict. */
export function VerdictBadge({ verdict }: { readonly verdict: Verdict }) {
  return (
    <span className={`ex-chip ex-outcome ex-badge--${VERDICT_HUE[verdict]}`} data-field="result">
      {verdict}
    </span>
  );
}
