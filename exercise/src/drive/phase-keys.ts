import { AUTHORED } from './examples.ts';
import type { LogLine } from './log.ts';
import { placed } from './recorded.ts';

/**
 * The two keys (#788), as the kitchen sink logged them (#798): each moment is its log cut at a line of
 * `diet/scenarios/kitchen-sink.jsonl`'s run, found by what the line is, never by its index. The session opens in
 * `plan`, starts in `orient`, and the model proposes `plan`; the model proposes `build` and the operator keeps it; the
 * operator asks the model for `test`, which accepts, then for `build`, which declines.
 */
const LOG = placed(AUTHORED['kitchen-sink']);

type Field = string | number | boolean;
const is =
  (kind: string, fields: Readonly<Record<string, Field>> = {}) =>
  (line: LogLine): boolean => {
    const record = line as unknown as Readonly<Record<string, unknown>>;
    return line.kind === kind && Object.entries(fields).every(([key, value]) => record[key] === value);
  };

/** The index of the first line `match` finds, at or after `from`; the scenario not logging it fails loudly. */
function at(match: (line: LogLine) => boolean, from = 0): number {
  const index = LOG.findIndex((line, i) => i >= from && match(line));
  if (index === -1) throw new Error('phase-keys: the kitchen sink no longer logs a line this moment is cut at');
  return index;
}
const through = (index: number): readonly LogLine[] => LOG.slice(0, index + 1);
const before = (index: number): readonly LogLine[] => LOG.slice(0, index);

const firstProposal = at(is('phase.proposed', { by: 'model' }));
const kept = at(is('phase.ruled', { choice: 'keep' }));
const asked = at(is('phase.proposed', { by: 'operator' }));
const declined = at(is('phase.answered', { accepted: false }));

/** The session opened, nothing asked: the starting phase may still be chosen. */
export const PHASE_OPENED = before(at(is('phase.started')));

/** The operator chose `orient` before the first ask. */
export const PHASE_STARTED = through(at(is('phase.started')));

/** Idle in `build`, nothing proposed: the operator's key. */
export const PHASE_IDLE = before(asked);

/** The model proposed `plan`, ending its turn: the operator rules, with the evidence. */
export const MODEL_PROPOSED = before(at(is('phase.ruled'), firstProposal));

/** Ruled keep: the phase moves to `build`, the trunk is kept. */
export const MODEL_PROPOSAL_KEPT = through(kept);

/** The operator asked the model to move to `test`, keeping the trunk; the model has not answered. */
export const OPERATOR_ASKED = through(asked);

/** The model accepted: the move follows the operator's choice, ruled by the model. */
export const OPERATOR_ACCEPTED = through(at(is('phase.ruled', { by: 'model' }), asked));

/** Asked again, for `build`, the model declined, with its reason. */
export const OPERATOR_DECLINED = through(declined);
