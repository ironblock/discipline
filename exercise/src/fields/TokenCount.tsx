import './fields.css';

export interface TokenCountProps {
  readonly tokens: number;
  /** Which count this is, when the row needs to say. */
  readonly of?: 'prefill' | 'output' | 'total';
}

const formatter = new Intl.NumberFormat('en-US');

/** A token count: the number the program is about. Tabular, thousands-separated, unit last. */
export function TokenCount({ tokens, of }: TokenCountProps) {
  return (
    <span className="ex-num" data-field="tokens">
      {formatter.format(tokens)}
      <span className="ex-unit">{of ? `${of} tok` : 'tok'}</span>
    </span>
  );
}
