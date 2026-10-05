/**
 * The shell gate on the page (#389, ruled 5982832752): `diet`'s own
 * `judge_argv` (#402), compiled to wasm by `scripts/build-gate.mjs`, so a
 * replay re-derives a logged call's segments from its `argv` with the reader
 * the drive used -- the segments are not logged (#388 5982826236). Loaded on
 * first use, as a chunk of its own, and compiled from bytes the page already
 * has: the Pages table forbids a network call (#32 N1).
 */

import type { Open } from '../drive/log.ts';
import type { Segment } from '../drive/transport.ts';
import type * as GlueModule from './wasm/diet_wasm.js';

/**
 * The denylist a call ran under. The log carries none (the receipt carries its digest), and `judge_argv` takes it
 * as an input, so the replay judges under the drive's standard list: `DENYLIST` in
 * `diet/src/drive/shell_gate.rs`, which `judge.test.ts` holds this to, entry for entry.
 */
export const DENYLIST: readonly string[] = ['sudo', 'su', 'doas', 'dd', 'shred', 'docker', 'kubectl', 'systemctl', 'crontab', 'at', 'git push', 'git reset --hard'];

/** What the gate makes of one command line: the gate module's value space. */
export interface Judgement {
  readonly outcome: Open<'refused' | 'prompt' | 'run'>;
  readonly segments: readonly Segment[];
}

type Glue = typeof GlueModule;

let loaded: Promise<Glue> | undefined;

/** The gate, compiled and bound once. */
function gate(): Promise<Glue> {
  loaded ??= (async () => {
    const [glue, bytes] = await Promise.all([import('./wasm/diet_wasm.js'), import('./wasm/bytes.js')]);
    const text = atob(bytes.default);
    const module = await WebAssembly.compile(Uint8Array.from(text, (c) => c.charCodeAt(0)));
    glue.initSync({ module });
    return glue;
  })();
  return loaded;
}

export type Judged = { readonly ok: true; readonly judgement: Judgement } | { readonly ok: false; readonly error: string };

/** ARGV judged under DENYLIST, with no approvals in force: the segments, or why the gate would not say. */
export function judge(argv: readonly string[], denylist: readonly string[] = DENYLIST): Promise<Judged> {
  return judgeRequest(JSON.stringify({ argv, denylist }));
}

/** A judgement request as `judge_argv` takes it, whole: the conformance corpus's own form. */
export async function judgeRequest(request: string): Promise<Judged> {
  const glue = await gate();
  const said = JSON.parse(glue.judge_argv(request)) as { readonly ok: boolean; readonly value?: Judgement; readonly error?: string };
  return said.ok && said.value ? { ok: true, judgement: said.value } : { ok: false, error: said.error ?? 'the gate said nothing' };
}
