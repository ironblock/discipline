import './fields.css';

export interface RatioProps {
  readonly numerator: number;
  readonly denominator: number;
  /** What is being counted: `grounded`, `matched`, ... */
  readonly of: string;
}

/** `grounded 3 / 30`: a count over a count, both real, the unit named. */
export function Ratio({ numerator, denominator, of }: RatioProps) {
  return (
    <span className="ex-num" data-field={of}>
      <span className="ex-unit" style={{ marginLeft: 0, marginRight: '0.3em' }}>
        {of}
      </span>
      {numerator}
      <span className="ex-muted"> / </span>
      {denominator}
    </span>
  );
}
