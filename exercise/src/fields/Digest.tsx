import './fields.css';

export interface DigestProps {
  /** 64 lowercase hex characters, as the structure checker requires. */
  readonly sha256: string;
  /** How many leading characters to show. The rest is in the title. */
  readonly shown?: number;
}

/** A sha256: the first eight characters in ink, the rest muted, all present. */
export function Digest({ sha256, shown = 8 }: DigestProps) {
  return (
    <span className="ex-digest" data-field="sha256" title={sha256}>
      {sha256.slice(0, shown)}
      <span className="ex-digest__rest">{sha256.slice(shown, shown + 4)}…</span>
    </span>
  );
}
