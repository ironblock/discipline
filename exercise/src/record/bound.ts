/**
 * `Bound<T>`: a value that knows which event and field it came from.
 *
 * A row may only render what the record carries, and the way that rule is
 * enforced is that a row's props are `Bound` values and nothing but this
 * module can make one. A literal handed to a row is a type error (#31's
 * fifth acceptance row); a field the SPA computes for itself has no event to
 * cite and cannot be bound (#92's "a row bound to nothing"). Provenance is
 * not decoration on the value -- it is what makes the value admissible.
 *
 * The brand is a `unique symbol` that is declared and never exported, so the
 * object shape cannot be spelled outside this file. `bind()` is the only
 * constructor and it takes the event, its position in the record and a key
 * of that event's own type, so the path is checked against the provisional
 * types rather than typed as a string.
 */

import type { Event, Kind } from './types.ts';

declare const brand: unique symbol;

/** Where in the record a bound value was read from. */
export interface Ref {
  /** The event's position in the record, from 0. Every event has one. */
  readonly index: number;
  /** The event's kind. */
  readonly kind: Kind;
  /** The event's own id, for the kinds that carry one. */
  readonly id?: string;
  /** The field, as the record spells it. Dotted for a nested member. */
  readonly path: string;
}

/** A value read from one field of one event. */
export interface Bound<T> {
  readonly value: T;
  readonly at: Ref;
  readonly [brand]: true;
}

/** An event with its position, which is the only identity every kind has. */
export interface Placed<E extends Event = Event> {
  readonly index: number;
  readonly event: E;
}

/** The keys of `E` whose value is present -- required or optional-and-set. */
type Field<E extends Event> = Exclude<keyof E, 'record'> & string;

function refOf(placed: Placed, path: string): Ref {
  const { event, index } = placed;
  const id = 'id' in event ? event.id : undefined;
  return id === undefined ? { index, kind: event.record, path } : { index, kind: event.record, id, path };
}

/**
 * Bind a required field of an event.
 *
 * The return type is the field's own type, so `bind(turn, 'prefill_tokens')`
 * is `Bound<number>` and `bind(fork, 'lane')` is `Bound<string>`, with no
 * cast at the call site. An optional field goes through [`bindOptional`],
 * whose absence is a distinct value rather than a `Bound<undefined>`.
 */
export function bind<E extends Event, K extends Field<E>>(placed: Placed<E>, key: K): Bound<E[K]> {
  return { value: placed.event[key], at: refOf(placed, key) } as Bound<E[K]>;
}

/**
 * Bind an optional field: absent stays absent, and a present field is bound.
 *
 * The distinction matters to every row: `tool_call.output` absent means the
 * record did not keep it; present and empty means the command printed
 * nothing. A row that received `Bound<string | undefined>` could not tell a
 * reader which it was showing.
 */
export function bindOptional<E extends Event, K extends Field<E>>(
  placed: Placed<E>,
  key: K,
): Bound<NonNullable<E[K]>> | undefined {
  const value = placed.event[key];
  if (value === undefined) return undefined;
  return { value, at: refOf(placed, key) } as Bound<NonNullable<E[K]>>;
}

/**
 * Bind a member of a nested field, naming the path.
 *
 * For the regime's substrates and the summary's totals: the event is the
 * `start` or `summary` row, and the path says which member. The value is
 * whatever the caller read; the path is a string because nested paths are
 * not enumerable through `keyof` without a second type system. The event
 * and index are still real, so the row still cites a row.
 */
export function bindAt<T>(placed: Placed, path: string, value: T): Bound<T> {
  return { value, at: refOf(placed, path) } as Bound<T>;
}

/** Map a bound value without losing where it came from. */
export function mapBound<T, U>(bound: Bound<T>, f: (value: T) => U): Bound<U> {
  return { value: f(bound.value), at: bound.at } as Bound<U>;
}
