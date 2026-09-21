/**
 * The guards ledger: what the types REFUSE, kept as directives so a loosened
 * guard is a red typecheck rather than a quiet regression.
 *
 * The inverse of `pending.types.ts`. There, a directive is green while a
 * gap in the RECORD exists; here, a directive is green while a guard in the
 * TYPES holds. If someone exports the brand, or lets a row take a plain
 * number, the expression below starts to compile, the directive is unused,
 * and `pnpm typecheck` fails with TS2578 on the line that names the guard.
 * `scripts/verify-red.mjs` removes each directive in a copy and proves the
 * guard is real.
 */

import type { TurnRowProps } from '../rows/TurnRow.tsx';
import type { Bound } from './bound.ts';

/** `V` if it is a `P`; a compile error otherwise. */
type Accepts<P, V extends P> = V;

// #31, acceptance row 5: a row handed a literal instead of a bound value.
// @ts-expect-error #31 a row refuses a bare literal where a Bound value is required
export type _guard_literal_to_a_row = Accepts<TurnRowProps, { index: 1; prefill: 1024 }>;

// #92, acceptance row 4: a row bound to nothing. A `Bound` cannot be spelled
// outside `bound.ts` because its brand is a `unique symbol` that is not
// exported -- the shape below has the value and the provenance and is still
// refused.
// @ts-expect-error #92 a Bound cannot be constructed outside the accessor module
export type _guard_forged_bound = Accepts<Bound<number>, { value: 1024; at: { index: 0; kind: 'turn'; path: 'prefill_tokens' } }>;
