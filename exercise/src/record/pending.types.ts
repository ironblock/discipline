/**
 * The types ledger: one line per atom the viewer needs and the record lacks.
 *
 * Each line is GREEN while the gap exists and RED the moment it closes. A
 * `@ts-expect-error` over `Has<Fork, 'outcome'>` is satisfied today because
 * `outcome` is not a key of `Fork` (TS2344); the day the provisional type
 * gains the field -- because `diet` started emitting it and a generated
 * fixture demanded it -- the expression compiles, the directive is unused,
 * and `pnpm typecheck` fails with TS2578 on exactly this line. That failure
 * is the instruction: delete the line, and move its fixture from
 * `fixtures/pending/` to `fixtures/records/`.
 *
 * HINGE ON PRESENCE, NEVER ON A LITERAL. A suppressed `outcome: 'mimicry'`
 * would stay suppressed if the field landed with the drive's vocabulary
 * instead of #92.4's, and that disagreement is exactly what this ledger
 * exists to announce. Every line asks only "is the field there" or "is the
 * kind there".
 *
 * Every directive cites an issue (`#92.4`) or says it waits on nothing filed
 * (`unfiled/<slug>`) -- `eslint.config.js` refuses one that does neither.
 * The `unfiled/` lines are drafted as issues in the PR's disconnect report;
 * when one is filed, its citation here changes to the number.
 *
 * `fixtures/pending/<same name>.jsonl` is each line's executable twin: the
 * proposed change to the record as JSONL that `diet check-record` refuses
 * today, pinned by `scripts/check-fixtures.mjs`.
 */

import type { Capture, EventOf, Fork, Regime, Request, Response, Seam, Substrate, ToolCall } from './types.ts';

/** The type of field `K` on `T` -- and a compile error if `T` has no `K`. */
type Has<T, K extends keyof T> = T[K];

// ---------------------------------------------------------------------------
// #82 -- record v1.1
// ---------------------------------------------------------------------------

// @ts-expect-error #82.1 response.started_at is not on the response event
export type _82_1_started_at = Has<Response, 'started_at'>;
// @ts-expect-error #82.1 response.ended_at is not on the response event
export type _82_1_ended_at = Has<Response, 'ended_at'>;
// @ts-expect-error #82.1 response.ttft_ms is not on the response event
export type _82_1_ttft_ms = Has<Response, 'ttft_ms'>;
// @ts-expect-error #82.1 response.prefill_tokens_new is not on the response event
export type _82_1_prefill_tokens_new = Has<Response, 'prefill_tokens_new'>;
// @ts-expect-error #82.1 response.prefill_tokens_cached is not on the response event
export type _82_1_prefill_tokens_cached = Has<Response, 'prefill_tokens_cached'>;
// @ts-expect-error #82.1 response.decode_ms is not on the response event
export type _82_1_decode_ms = Has<Response, 'decode_ms'>;
// @ts-expect-error #82.2 substrate.clock_offset_ms is not on the substrate table
export type _82_2 = Has<Substrate, 'clock_offset_ms'>;
// @ts-expect-error #82.3 fork.kind (canonical | fork | child_session) is not on the fork event
export type _82_3 = Has<Fork, 'kind'>;
// @ts-expect-error #82.4 tool_call.idempotent is not on the tool_call event
export type _82_4 = Has<ToolCall, 'idempotent'>;
// @ts-expect-error #82 prefix.cold_fork is not an event kind (acceptance row 2)
export type _82_cold_fork = EventOf<'prefix.cold_fork'>;

// ---------------------------------------------------------------------------
// #92 -- record v1.2
// ---------------------------------------------------------------------------

// @ts-expect-error #92.1 seam.reason is not on the seam event (ruled on #27, never landed)
export type _92_1_reason = Has<Seam, 'reason'>;
// @ts-expect-error #92.1 seam.prefix_hash_before is not on the seam event
export type _92_1_prefix_hash_before = Has<Seam, 'prefix_hash_before'>;
// @ts-expect-error #92.1 seam.prefix_hash_after is not on the seam event
export type _92_1_prefix_hash_after = Has<Seam, 'prefix_hash_after'>;
// @ts-expect-error #92.2 phase_proposal is not an event kind
export type _92_2 = EventOf<'phase_proposal'>;
// @ts-expect-error #92.3 seam.answer (the ratification answer) is not on the seam event
export type _92_3 = Has<Seam, 'answer'>;
// @ts-expect-error #92.4 fork.outcome is not on the fork event
export type _92_4 = Has<Fork, 'outcome'>;
// @ts-expect-error #92.5 composition (turn 0) is not an event kind
export type _92_5 = EventOf<'composition'>;
// @ts-expect-error #92.6 capture.targets (typed patch targets) is not on the capture event
export type _92_6 = Has<Capture, 'targets'>;

// ---------------------------------------------------------------------------
// unfiled -- drafted in the PR's disconnect report
// ---------------------------------------------------------------------------

// @ts-expect-error unfiled/sampler-echo response.echo is not on the response event; #82 wrongly lists it satisfied
export type _unfiled_sampler_echo = Has<Response, 'echo'>;
// @ts-expect-error unfiled/serving-slots regime.serving (slots) is not on the regime; ruled on #82, never landed
export type _unfiled_serving_slots = Has<Regime, 'serving'>;
// @ts-expect-error unfiled/request-slot request.slot is not on the request event
export type _unfiled_request_slot = Has<Request, 'slot'>;
// @ts-expect-error unfiled/entries-created capture.entries_created is not on the capture event; the drive computes it
export type _unfiled_entries_created = Has<Capture, 'entries_created'>;
// @ts-expect-error unfiled/fork-request-link request.of_fork is not on the request event; the join is row order and lane
export type _unfiled_fork_request_link = Has<Request, 'of_fork'>;
// @ts-expect-error unfiled/request-turn-link request.at_turn is not on the request event; the join is row order
export type _unfiled_request_turn_link = Has<Request, 'at_turn'>;
// @ts-expect-error unfiled/tangent tangent is not an event kind; object/tangent.rs holds it
export type _unfiled_tangent = EventOf<'tangent'>;
// @ts-expect-error unfiled/operator-ask request.ask is not on the request event; request.text is the whole wire body
export type _unfiled_operator_ask = Has<Request, 'ask'>;
