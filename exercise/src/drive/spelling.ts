/**
 * The boundary's two spellings (#583): `seam` through log v7, `meditation` from v8.
 *
 * `diet` reads a log in its declared version's spelling and projects it back in that spelling, so a committed v7 log
 * and its projection still say `seam`; so do the recordings. The surface folds one vocabulary, the current one: a line
 * is respelled where v7 put the old word -- each tagged key on the one kind of line that carries it, never free text.
 */

/** A respelled position: the line's kind as v8 spells it (none for `kind` itself), the key, the old and new words. */
const POSITIONS: readonly (readonly [on: string | undefined, key: string, old: string, now: string])[] = [
  [undefined, 'kind', 'seam', 'meditation'],
  [undefined, 'kind', 'seam.held', 'meditation.held'],
  ['refused', 'command', 'declare-seam', 'declare-meditation'],
  ['refused', 'because', 'seam-not-built', 'meditation-not-built'],
  ['refused', 'because', 'nothing-to-seam', 'nothing-to-meditate'],
  ['refused', 'because', 'seam-held', 'meditation-held'],
  ['idle.gap', 'ended_by', 'seam', 'meditation'],
  ['fork', 'why', 'seam', 'meditation'],
  ['session.start', 'fork_delivery', 'seam', 'meditation'],
  ['phase.ruled', 'choice', 'seam', 'meditation'],
];

/** `line` in the current spelling: itself when it holds no old word. */
export function respelled<T extends { readonly kind: string }>(line: T): T {
  const fields = line as unknown as Readonly<Record<string, unknown>>;
  const kind = POSITIONS.find(([on, key, old]) => on === undefined && key === 'kind' && old === line.kind)?.[3] ?? line.kind;
  let out: Record<string, unknown> | undefined;
  for (const [on, key, old, now] of POSITIONS) {
    if ((on === undefined || on === kind) && fields[key] === old) {
      out ??= { ...fields };
      out[key] = now;
    }
  }
  return (out ?? line) as T;
}
