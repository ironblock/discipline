import type { Kind } from '../record/types.ts';
import './fields.css';

export interface IdChipProps {
  readonly id: string;
  readonly kind: Kind;
}

/** An event's own id, in the record's face, tagged with its kind. */
export function IdChip({ id, kind }: IdChipProps) {
  return (
    <span className="ex-chip ex-chip--record" data-field="id" data-kind={kind}>
      <span className="ex-kind">{kind}</span>
      {id}
    </span>
  );
}
