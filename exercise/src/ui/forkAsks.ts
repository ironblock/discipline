import { FORK_ASKS } from '../../../diet/dogma/fork-asks.ts';
import type { ForkAsk } from '../../../diet/dogma/fork-asks.ts';

/** The pinned asks the picker shows first (the ruling of 2026-11-03); the rest are under "more". */
const FIRST = ['GOTCHAS', 'DECISIONS', 'CONSTRAINTS', 'OPEN_NEXT', 'RATIONALE'];

/** The pinned asks, split: those first, in that order, then the rest in the dogma's. */
export function pickerAsks(asks: readonly ForkAsk[] = FORK_ASKS): { readonly first: readonly ForkAsk[]; readonly more: readonly ForkAsk[] } {
  const first = FIRST.flatMap((name) => asks.filter((ask) => ask.name === name));
  return { first, more: asks.filter((ask) => !FIRST.includes(ask.name)) };
}
