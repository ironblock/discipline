import type { Kind } from '../record/types.ts';
import './fields.css';

export interface LinkProps {
  /** The linking field, as the record spells it: `to_request`, `retry_of`, ... */
  readonly field: string;
  /** The id it names. */
  readonly to: string;
  /** The kind the link must name, per the structure checker. */
  readonly wants: Kind;
}

/**
 * A validated link: one of the four relations `validate()` checks, drawn as
 * field → id. Only these four and turn existence ever render as a link; a
 * join by row order is not a link and does not get this glyph.
 */
export function Link({ field, to, wants }: LinkProps) {
  return (
    <span className="ex-link" data-field={field} data-wants={wants}>
      <span className="ex-link__field">{field}</span>
      <span className="ex-link__arrow">→</span>
      <span className="ex-link__to">{to}</span>
    </span>
  );
}
