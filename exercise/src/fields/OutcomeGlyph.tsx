import './fields.css';

/**
 * THREE vocabularies for what became of a fork, and the record carries none.
 *
 * - `viewer`: what #92.4 asks for on the fork row and what v3 drew:
 *   value | decline | mimicry | truncated | unparseable | timeout.
 * - `drive`: what `diet::drive::ForkOutcome` computes today:
 *   complete | truncated | empty | thinking_exhausted.
 * - `interview`: what the interview grammar types per answer:
 *   `Completion` (complete | empty | truncated) and per field `Outcome`
 *   (value | decline | unparseable).
 *
 * This atom renders all three so the disagreement is visible in one place.
 * It is a Field with plain props because no row can bind it: `fork.outcome`
 * is a ledger line (`#92.4`) and two pending fixtures, one per vocabulary.
 */
export type ViewerOutcome = 'value' | 'decline' | 'mimicry' | 'truncated' | 'unparseable' | 'timeout';
export type DriveOutcome = 'complete' | 'truncated' | 'empty' | 'thinking_exhausted';
export type InterviewOutcome = 'complete' | 'empty' | 'truncated' | 'value' | 'decline' | 'unparseable';

export type OutcomeGlyphProps =
  | { readonly vocabulary: 'viewer'; readonly outcome: ViewerOutcome }
  | { readonly vocabulary: 'drive'; readonly outcome: DriveOutcome }
  | { readonly vocabulary: 'interview'; readonly outcome: InterviewOutcome };

type Hue = 'ok' | 'amber' | 'orange' | 'red' | 'blue' | 'magenta' | 'muted';

const GLYPH: Readonly<Record<string, readonly [string, Hue]>> = {
  // viewer (v3's glyph set, kept)
  value: ['●', 'ok'],
  decline: ['○', 'blue'],
  mimicry: ['◈', 'magenta'],
  truncated: ['◐', 'amber'],
  timeout: ['◌', 'orange'],
  unparseable: ['✕', 'red'],
  // drive
  complete: ['●', 'ok'],
  empty: ['○', 'muted'],
  thinking_exhausted: ['◔', 'orange'],
};

/** A fork outcome as a glyph and its word, in the vocabulary it came from. */
export function OutcomeGlyph({ vocabulary, outcome }: OutcomeGlyphProps) {
  const [glyph, hue] = GLYPH[outcome] ?? ['?', 'muted'];
  return (
    <span className={`ex-chip ex-outcome ex-badge--${hue}`} data-field="outcome" data-vocabulary={vocabulary}>
      <span className="ex-glyph" aria-hidden="true">
        {glyph}
      </span>
      {outcome}
    </span>
  );
}
