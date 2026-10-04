/**
 * The one reader of a tool call's file (#372, ruled 5983588781, 5983595066):
 * the bytes come by digest from a source -- `serve`'s `GET /files/<sha256>`
 * on a live page (`HttpTransport.file`), a recording's published asset on the
 * replay page (`data/<name>/files/<sha256>.js`, read with `import()`: the
 * Pages table forbids a network call) -- and are hashed and refused on a
 * mismatch here, before anything draws them. A path in the log is never
 * followed.
 */

import type { FileRef } from './log.ts';

/** What a source has for a digest. */
export type FileAnswer =
  /** The bytes it holds under that digest: not yet checked. */
  | { readonly kind: 'bytes'; readonly bytes: Uint8Array }
  /** A published recording's asset with no `declared-clean` line: not published, and said so (#372 5977059903). */
  | { readonly kind: 'withheld' }
  /** Nothing under that digest: `serve`'s 404, or no asset in the recording. */
  | { readonly kind: 'not-found' }
  /** The source could not be asked: why, as it says. */
  | { readonly kind: 'unreachable'; readonly why: string };

/** Where a page gets a file's bytes by digest. */
export type FileSource = (sha256: string) => Promise<FileAnswer>;

/** A file, read and checked: what the surface draws. */
export type Checked =
  /** Its bytes hash to its digest. */
  | { readonly kind: 'shown'; readonly bytes: Uint8Array }
  /** Its bytes hash to something else: never drawn (`got` is what they hash to). */
  | { readonly kind: 'mismatch'; readonly got: string }
  | Exclude<FileAnswer, { readonly kind: 'bytes' }>;

/** The sha256 of BYTES, lowercase hex. */
export async function sha256(bytes: Uint8Array): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', bytes as Uint8Array<ArrayBuffer>);
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

/** REF's bytes from SOURCE, checked against its digest. */
export async function read(ref: FileRef, source: FileSource): Promise<Checked> {
  const answer = await source(ref.sha256);
  if (answer.kind !== 'bytes') return answer;
  const got = await sha256(answer.bytes);
  return got === ref.sha256.toLowerCase() ? { kind: 'shown', bytes: answer.bytes } : { kind: 'mismatch', got };
}

/**
 * A published asset's module text: `export default` and the bytes as base64 --
 * the payload's own form (`payload.ts`) -- or, for an asset its recording
 * does not declare clean, `export default null`: the reference is published,
 * the bytes are not (#372 5977059903).
 */
export function assetModule(bytes: Uint8Array | null): string {
  if (bytes === null) return 'export default null\n';
  let text = '';
  for (const byte of bytes) text += String.fromCharCode(byte);
  return `export default ${JSON.stringify(btoa(text))}\n`;
}

/** What an asset module's default says: its bytes, withheld, or (anything else) no asset at all. */
export function assetAnswer(value: unknown): FileAnswer {
  if (value === null) return { kind: 'withheld' };
  if (typeof value !== 'string') return { kind: 'not-found' };
  const text = atob(value);
  return { kind: 'bytes', bytes: Uint8Array.from(text, (c) => c.charCodeAt(0)) };
}

/**
 * A recording's published assets as a source: each read with `import()` from URL_OF its digest (the Pages table
 * forbids a network call, #32 N1). A digest that is not one, or an asset the build did not publish, is not found.
 */
export function importedFiles(urlOf: (sha256: string) => string): FileSource {
  return async (sha256) => {
    if (!/^[0-9a-f]{64}$/.test(sha256)) return { kind: 'not-found' };
    try {
      const module = (await import(/* @vite-ignore */ urlOf(sha256))) as { readonly default?: unknown };
      return assetAnswer(module.default);
    } catch {
      return { kind: 'not-found' };
    }
  };
}
