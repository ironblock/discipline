/**
 * Rows are event GROUPS, formed only through relations the record validates.
 *
 * `formats::record::validate` checks exactly four links -- `request.retry_of
 * → request`, `response.to_request → request`, `capture.from_fork → fork`,
 * `claim.supersedes → claim` -- and turn existence for every `at_turn` and
 * `of_turn`. Those are the only joins this module makes. A request, its
 * retry chain and its response are one row because the record says they
 * are; a fork and its captures are one row for the same reason.
 *
 * What this module deliberately does NOT join, because nothing validates it:
 * a fork's own request and response (adjacent by row order and lane only),
 * and a canonical request to its turn (row order only). Both render as
 * separate rows placed after the turn they follow, and the placement is
 * marked positional. The ledger lines `unfiled/fork-request-link` and
 * `unfiled/request-turn-link` are the two joins this file is waiting to
 * make.
 */

import type { Placed } from './bound.ts';
import type {
  Capture,
  Claim,
  Fork,
  Rejected,
  Request,
  Response,
  Seam,
  Start,
  Summary,
  ToolCall,
  Turn,
  Unknown,
} from './types.ts';

/** Where a group sits relative to the turns, and on whose word. */
export type Containment =
  /** A validated link: the head's `at_turn` or `of_turn`. */
  | { readonly by: 'link'; readonly turn: number }
  /** Row order only: the most recent turn row above it. */
  | { readonly by: 'position'; readonly turn: number }
  /** Before turn 1, or a record with no turns. */
  | { readonly by: 'none' };

export type Group =
  | { readonly kind: 'start'; readonly head: Placed<Start>; readonly containment: Containment }
  | { readonly kind: 'turn'; readonly head: Placed<Turn>; readonly containment: Containment }
  | {
      readonly kind: 'exchange';
      /** The request and every retry of it, in row order. Never empty. */
      readonly requests: readonly Placed<Request>[];
      /** The response naming any request in the chain, if one came back. */
      readonly response?: Placed<Response>;
      readonly containment: Containment;
    }
  | {
      readonly kind: 'fork';
      readonly head: Placed<Fork>;
      readonly captures: readonly Placed<Capture>[];
      readonly containment: Containment;
    }
  | { readonly kind: 'seam'; readonly head: Placed<Seam>; readonly containment: Containment }
  | { readonly kind: 'tool_call'; readonly head: Placed<ToolCall>; readonly containment: Containment }
  | { readonly kind: 'rejected'; readonly head: Placed<Rejected>; readonly containment: Containment }
  | {
      readonly kind: 'claim';
      /** The claim and each correction of it, oldest first. Never empty. */
      readonly chain: readonly Placed<Claim>[];
      readonly containment: Containment;
    }
  | { readonly kind: 'summary'; readonly head: Placed<Summary>; readonly containment: Containment }
  | { readonly kind: 'unknown'; readonly head: Placed<Unknown>; readonly containment: Containment };

/** The row-order position a group sorts by: its first event's. */
export function firstIndex(group: Group): number {
  switch (group.kind) {
    case 'exchange':
      return group.requests[0]?.index ?? Number.MAX_SAFE_INTEGER;
    case 'claim':
      return group.chain[0]?.index ?? Number.MAX_SAFE_INTEGER;
    default:
      return group.head.index;
  }
}

/** Every event in the group, in row order. */
export function members(group: Group): readonly Placed[] {
  switch (group.kind) {
    case 'exchange':
      return group.response ? [...group.requests, group.response].sort((a, b) => a.index - b.index) : group.requests;
    case 'fork':
      return [group.head, ...group.captures];
    case 'claim':
      return group.chain;
    default:
      return [group.head];
  }
}

/** Group a record's events into rows. */
export function groupEvents(events: readonly Placed[]): readonly Group[] {
  // Mutable builders, keyed by the id a later row's link will name.
  const exchangeOf = new Map<string, { requests: Placed<Request>[]; response?: Placed<Response>; first: number }>();
  const forkOf = new Map<string, { head: Placed<Fork>; captures: Placed<Capture>[] }>();
  const chainOf = new Map<string, { chain: Placed<Claim>[] }>();
  // The turn each row-order position falls under, for positional containment.
  let currentTurn: number | undefined;
  const positional = (): Containment => (currentTurn === undefined ? { by: 'none' } : { by: 'position', turn: currentTurn });

  type Pending = { at: number; build: () => Group };
  const out: Pending[] = [];

  for (const placed of events) {
    const { event } = placed;
    switch (event.record) {
      case 'turn': {
        currentTurn = event.index;
        const head = placed as Placed<Turn>;
        out.push({ at: placed.index, build: () => ({ kind: 'turn', head, containment: { by: 'link', turn: event.index } }) });
        break;
      }
      case 'request': {
        const head = placed as Placed<Request>;
        const chain = event.retry_of === undefined ? undefined : exchangeOf.get(event.retry_of);
        if (chain) {
          chain.requests.push(head);
          exchangeOf.set(event.id, chain);
        } else {
          const fresh: { requests: Placed<Request>[]; response?: Placed<Response>; first: number } = {
            requests: [head],
            first: placed.index,
          };
          exchangeOf.set(event.id, fresh);
          const containment = positional();
          out.push({
            at: placed.index,
            build: () =>
              fresh.response
                ? { kind: 'exchange', requests: fresh.requests, response: fresh.response, containment }
                : { kind: 'exchange', requests: fresh.requests, containment },
          });
        }
        break;
      }
      case 'response': {
        const chain = exchangeOf.get(event.to_request);
        // `validate` refuses a dangling `to_request`, so `chain` is set for
        // every record diet accepted; the branch exists for the type.
        if (chain) chain.response = placed as Placed<Response>;
        break;
      }
      case 'fork': {
        const head = placed as Placed<Fork>;
        const fresh = { head, captures: [] as Placed<Capture>[] };
        forkOf.set(event.id, fresh);
        out.push({ at: placed.index, build: () => ({ kind: 'fork', head, captures: fresh.captures, containment: { by: 'link', turn: event.of_turn } }) });
        break;
      }
      case 'capture': {
        const fork = forkOf.get(event.from_fork);
        if (fork) fork.captures.push(placed as Placed<Capture>);
        break;
      }
      case 'claim': {
        const head = placed as Placed<Claim>;
        const earlier = event.supersedes === undefined ? undefined : chainOf.get(event.supersedes);
        if (earlier) {
          earlier.chain.push(head);
          chainOf.set(event.id, earlier);
        } else {
          const fresh = { chain: [head] };
          chainOf.set(event.id, fresh);
          const containment = positional();
          out.push({ at: placed.index, build: () => ({ kind: 'claim', chain: fresh.chain, containment }) });
        }
        break;
      }
      case 'seam': {
        const head = placed as Placed<Seam>;
        out.push({ at: placed.index, build: () => ({ kind: 'seam', head, containment: { by: 'link', turn: event.at_turn } }) });
        break;
      }
      case 'tool_call': {
        const head = placed as Placed<ToolCall>;
        out.push({ at: placed.index, build: () => ({ kind: 'tool_call', head, containment: { by: 'link', turn: event.at_turn } }) });
        break;
      }
      case 'rejected': {
        const head = placed as Placed<Rejected>;
        out.push({ at: placed.index, build: () => ({ kind: 'rejected', head, containment: { by: 'link', turn: event.at_turn } }) });
        break;
      }
      case 'start': {
        const head = placed as Placed<Start>;
        out.push({ at: placed.index, build: () => ({ kind: 'start', head, containment: { by: 'none' } }) });
        break;
      }
      case 'summary': {
        const head = placed as Placed<Summary>;
        const containment = positional();
        out.push({ at: placed.index, build: () => ({ kind: 'summary', head, containment }) });
        break;
      }
      case 'unknown': {
        const head = placed as Placed<Unknown>;
        const containment = positional();
        out.push({ at: placed.index, build: () => ({ kind: 'unknown', head, containment }) });
        break;
      }
    }
  }
  return out.map((p) => p.build());
}

