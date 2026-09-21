import { Pending } from '../fields/Pending.tsx';
import '../rows/rows.css';

/** The sidecar `fixtures/pending/<name>.expect.json` pins. */
export interface Expectation {
  readonly issue: string;
  readonly atom: string;
  readonly why: string;
  readonly refusal: string;
  readonly message: string;
  readonly exit: number;
}

export interface PendingSequenceProps {
  readonly name: string;
  /** The proposed record change, as the JSONL `diet check-record` refuses. */
  readonly proposal: string;
  readonly expect: Expectation;
}

/**
 * A sequence waiting on the record: the proposal in executable form, the
 * refusal `diet` gives it today, and the dotted placeholder that stands
 * where its row will be. When `scripts/check-fixtures.mjs` reports the
 * fixture ACCEPTED, this story is deleted and a Sequences story replaces it.
 */
export function PendingSequence({ name, proposal, expect }: PendingSequenceProps) {
  const lines = proposal.split('\n').filter((l) => l !== '');
  return (
    <section className="ex-timeline" data-pending={expect.issue} data-fixture={name}>
      <div className="ex-timeline__rail">
        <span>
          fixture <strong>{name}.jsonl</strong>
        </span>
        <span>
          diet check-record exit <strong>{expect.exit}</strong>
        </span>
        <span>
          read as <strong>{expect.refusal}</strong>
        </span>
      </div>
      <div style={{ padding: '0.75rem' }}>
        <Pending as="block" issue={expect.issue} atom={expect.atom} why={expect.why} />
        <p className="ex-prose ex-prose--record" style={{ marginTop: '0.75rem' }}>
          <span className="ex-label">diet says: </span>
          {expect.message}
        </p>
        <details style={{ marginTop: '0.5rem' }}>
          <summary className="ex-label" style={{ cursor: 'pointer' }}>
            the proposal, as {lines.length} record line{lines.length === 1 ? '' : 's'}
          </summary>
          <pre className="ex-prose ex-prose--record" style={{ maxWidth: 'none' }}>
            {lines.map((l, i) => (
              <div key={i}>{l}</div>
            ))}
          </pre>
        </details>
      </div>
    </section>
  );
}
