import type { WeightsKind } from '../record/types.ts';
import './fields.css';

export interface SubstrateBadgeProps {
  readonly id: string;
  /** How the substrate's weights are identified, when the row knows. */
  readonly weights?: WeightsKind;
}

const WEIGHTS_GLYPH: Readonly<Record<WeightsKind, string>> = {
  digest: '#',
  hosted: '~',
  canned: '⏵',
};

/**
 * Which declared substrate served a row. The glyph is the weights kind --
 * `#` weights on disk by digest, `~` hosted and unreproducible, `⏵` a canned
 * server replaying acts -- because that kind decides what a result may claim.
 */
export function SubstrateBadge({ id, weights }: SubstrateBadgeProps) {
  return (
    <span className="ex-chip" data-field="substrate" data-weights={weights} title={weights ? `weights: ${weights}` : undefined}>
      {weights ? <span className="ex-muted">{WEIGHTS_GLYPH[weights]}</span> : null}
      {id}
    </span>
  );
}
