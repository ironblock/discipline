/**
 * The record's events, as TypeScript -- PROVISIONAL, and a second description.
 *
 * `diet/src/formats/record/mod.rs` is the one authorized description of the
 * record. This file is hand-written from its `event_value()` -- the wire
 * form: a `record` tag, canonical sorted keys, an absent key for an absent
 * optional, `weights.kind` and `budget_tokens.kind` as tagged unions -- and
 * it is a second description of the same thing, which this repository does
 * not keep on purpose. It is disclosed as temporary. The permanent generator
 * belongs in `diet` (#31: "TS bindings generated from diet's record and
 * object types"; #78 row 3 typed the wasm call boundary, not the record).
 *
 * What keeps it honest in the meantime: `scripts/gen-fixtures.mjs` writes
 * every fixture `diet check-record` accepts as a module
 * `export default [...] as const satisfies readonly Event[]`, so a field
 * this file spells differently from the record fails `pnpm typecheck` on the
 * fixture that carries it. A kind or field the corpus never exercises is
 * unchecked here, and `src/record/pending.types.ts` is the list of fields
 * this file deliberately does NOT have.
 *
 * Numbers are `number`. The record's value space is `i64` and exact
 * decimals; JavaScript's is a double. `Count::MAX` is `i64::MAX`, which
 * `JSON.parse` cannot hold, and `0.7000` reads back as `0.7`. Both are named
 * in `pending.types.ts` and in the generator's `lossy` output rather than
 * papered over with a string type the record does not have.
 */

/** Every event kind, in the record's own spelling. */
export type Kind =
  | 'start'
  | 'turn'
  | 'request'
  | 'response'
  | 'fork'
  | 'capture'
  | 'seam'
  | 'tool_call'
  | 'rejected'
  | 'claim'
  | 'summary'
  | 'unknown';

/** The record's value space: the JSON subset `formats::record::json` admits. */
export type Value = string | number | boolean | readonly Value[] | { readonly [key: string]: Value };

export type WeightsKind = 'digest' | 'hosted' | 'canned';

export type Weights =
  | { readonly kind: 'digest'; readonly sha256: string }
  | {
      readonly kind: 'hosted';
      readonly provider: string;
      readonly model_id: string;
      readonly version_or_date_observed: string;
    }
  | { readonly kind: 'canned'; readonly acts_sha256: string };

export interface Engine {
  readonly name: string;
  readonly version_or_digest: string;
}

export type Reasoning = 'off' | 'on' | 'suppressed' | 'undeclared';

export type Budget = { readonly kind: 'tokens'; readonly tokens: number } | { readonly kind: 'none' };

export interface ReasoningControl {
  readonly effort: string;
  readonly budget_tokens: Budget;
}

export interface Substrate {
  readonly id: string;
  readonly engine: Engine;
  readonly weights: Weights;
  readonly hardware_fingerprint: string;
  readonly sampler_card: { readonly [setting: string]: Value };
  readonly reasoning: Reasoning;
  readonly reasoning_control?: ReasoningControl;
  readonly chat_template_sha256?: string;
}

export interface Regime {
  readonly arm: string;
  readonly substrates: readonly Substrate[];
  readonly dogma_version: number;
}

export type Availability = 'committed' | 'pinned_only';

export type Source =
  | { readonly kind: 'live' }
  | {
      readonly kind: 'adapted';
      readonly adapter: string;
      readonly source_digest: string;
      readonly source_available: Availability;
    };

export type Verdict = 'supported' | 'refuted' | 'inconclusive' | 'unadjudicated';

export interface Artifact {
  readonly path: string;
  readonly sha256: string;
}

export interface Start {
  readonly record: 'start';
  readonly regime: Regime;
  readonly source: Source;
}

export interface Turn {
  readonly record: 'turn';
  readonly index: number;
  readonly prefill_tokens: number;
}

export interface Request {
  readonly record: 'request';
  readonly id: string;
  readonly lane: string;
  readonly substrate: string;
  readonly retry_of?: string;
  /** The whole wire body when the record is an archive; absent in a ledger. */
  readonly text?: string;
}

export interface Response {
  readonly record: 'response';
  readonly id: string;
  readonly to_request: string;
  readonly output_tokens: number;
  /** May be present and empty: an answer of nothing is a typed outcome. */
  readonly text?: string;
}

export interface Fork {
  readonly record: 'fork';
  readonly id: string;
  readonly lane: string;
  readonly substrate: string;
  readonly of_turn: number;
}

export interface Capture {
  readonly record: 'capture';
  readonly id: string;
  readonly from_fork: string;
  /** In a v0 record this is `entries_touched`; see the drive's ruling 6. */
  readonly entries: number;
}

export interface Seam {
  readonly record: 'seam';
  readonly id: string;
  readonly at_turn: number;
  readonly rendered_bytes: number;
}

export interface ToolCall {
  readonly record: 'tool_call';
  readonly id: string;
  readonly at_turn: number;
  readonly tool: string;
  readonly args?: { readonly [name: string]: Value };
  readonly exit?: number;
  /** Absent: not kept. Present and empty: the command printed nothing. */
  readonly output?: string;
}

export interface Rejected {
  readonly record: 'rejected';
  readonly id: string;
  readonly lane: string;
  readonly at_turn: number;
  readonly grounded: number;
  readonly of: number;
}

export interface Claim {
  readonly record: 'claim';
  readonly id: string;
  readonly hypothesis: string;
  readonly result: Verdict;
  readonly consumes: readonly Artifact[];
  readonly supersedes?: string;
}

export type SummaryKind = 'drive' | 'recompute';

export type Summary =
  | {
      readonly record: 'summary';
      readonly kind: 'drive';
      readonly product_sha256: string;
      readonly turns: number;
      readonly prefill_tokens_total: number;
    }
  | {
      readonly record: 'summary';
      readonly kind: 'recompute';
      readonly product_sha256: string;
      readonly targets_checked: number;
      readonly targets_matched: number;
      readonly digests: readonly string[];
    };

export interface Unknown {
  readonly record: 'unknown';
  readonly source_kind: string;
  readonly raw: string;
}

export type Event =
  | Start
  | Turn
  | Request
  | Response
  | Fork
  | Capture
  | Seam
  | ToolCall
  | Rejected
  | Claim
  | Summary
  | Unknown;

/** The event of one kind. `never` for a kind the record does not have. */
export type EventOf<K extends Kind> = Extract<Event, { readonly record: K }>;

/** What `diet check-record` returns for a record it accepts: `project()`. */
export interface Projection {
  readonly regime: {
    readonly arm: string;
    readonly substrates: readonly string[];
    readonly hosted_substrates: readonly string[];
    readonly dogma_version: number;
  };
  readonly kinds: readonly Kind[];
  readonly source: Source;
  /** The record rendered back to its own spelling, one event per line. */
  readonly canonical: string;
}

/** The envelope the CLI prints around a projection, or around its refusal. */
export type CliVerdict =
  | { readonly format: 'record'; readonly path: string; readonly ok: true; readonly value: Projection }
  | { readonly format: 'record'; readonly path: string; readonly ok: false; readonly error: string };
