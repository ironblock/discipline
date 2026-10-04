/**
 * A published recording's files (#372, ruled 5983588781, 5983595066), as the
 * replay build writes them: each asset committed in the recording's directory
 * under its digest, `<dir>/<name>/files/<sha256>`, becomes
 * `data/<name>/files/<sha256>.js` beside the payload -- its bytes as base64
 * when the recording's admission declares it clean, and `export default null`
 * when it does not, so the page can say it was withheld rather than lost.
 *
 * The admission's asset rows are the record half's (Track one): read here as
 * `files: [{ sha256, scrub }]`, one place to change if that half spells them
 * otherwise. Node only: the Vite config and a test call it, never the page.
 */

import { createHash } from 'node:crypto';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';

import { assetModule } from '../drive/files.ts';

/** The admission's word for an asset its author has looked at and found nothing in (#372 5977059903). */
export const DECLARED_CLEAN = 'declared-clean';

/** The digests ADMISSION declares clean. */
export function declared(admission: unknown): ReadonlySet<string> {
  const rows = (admission as { readonly files?: unknown } | null)?.files;
  if (!Array.isArray(rows)) return new Set();
  return new Set(rows.flatMap((row: unknown) => (typeof row === 'object' && row !== null && (row as { scrub?: unknown }).scrub === DECLARED_CLEAN && typeof (row as { sha256?: unknown }).sha256 === 'string' ? [(row as { sha256: string }).sha256] : [])));
}

/**
 * Every asset of recording NAME in DIR, as the files the build emits. A file not named by a digest, or whose bytes
 * are not its name, is an error: the build refuses it rather than publish bytes the log does not pin.
 */
export function publishedAssets(dir: string, name: string, admission: unknown): { readonly fileName: string; readonly source: string }[] {
  const files = path.join(dir, name, 'files');
  if (!existsSync(files)) return [];
  const clean = declared(admission);
  return readdirSync(files)
    .sort()
    .map((sha256) => {
      if (!/^[0-9a-f]{64}$/.test(sha256)) throw new Error(`${path.join(files, sha256)}: an asset is named by its sha256`);
      const bytes = readFileSync(path.join(files, sha256));
      const got = createHash('sha256').update(bytes).digest('hex');
      if (got !== sha256) throw new Error(`${path.join(files, sha256)}: its bytes are ${got}, not its name`);
      return { fileName: `data/${name}/files/${sha256}.js`, source: assetModule(clean.has(sha256) ? new Uint8Array(bytes) : null) };
    });
}
