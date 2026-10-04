import { createHash } from 'node:crypto';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { publishedAssets } from '../replay/assets.ts';
import { fold } from '../session/fold.ts';
import { CannedTransport } from './canned.ts';
import { assetAnswer, assetModule, importedFiles, read } from './files.ts';
import type { FileSource } from './files.ts';
import type { FileRef, LogLine } from './log.ts';
import { SCENE, SCENE_PNG, SCENE_SHA256, SCREENSHOT } from './screenshot.ts';

/**
 * A tool call's file (#372): read by digest from a source and refused unless
 * its bytes hash to the digest -- one reader for `serve`'s `GET /files` and a
 * recording's published assets (ruled 5983588781, 5983595066).
 */
const ref: FileRef = { path: 'shots/scene.png', sha256: SCENE_SHA256, media_type: 'image/png', bytes: SCENE_PNG.length };
const answering = (bytes: Uint8Array): FileSource => () => Promise.resolve({ kind: 'bytes', bytes });
const moduleUrl = (text: string) => `data:text/javascript;base64,${Buffer.from(text).toString('base64')}`;

describe('reading a file by its digest (#372)', () => {
  it('pins the authored screenshot: its digest is its bytes’', () => {
    expect(createHash('sha256').update(SCENE_PNG).digest('hex')).toBe(SCENE_SHA256);
  });

  it('shows bytes that hash to the digest', async () => {
    await expect(read(ref, answering(SCENE_PNG))).resolves.toEqual({ kind: 'shown', bytes: SCENE_PNG });
  });

  it('refuses bytes that do not, saying what they hash to', async () => {
    const other = SCENE_PNG.slice(0, -1);
    const got = createHash('sha256').update(other).digest('hex');
    await expect(read(ref, answering(other))).resolves.toEqual({ kind: 'mismatch', got });
  });

  it('passes on what a source has instead of bytes: withheld, not found, unreachable', async () => {
    for (const answer of [{ kind: 'withheld' }, { kind: 'not-found' }, { kind: 'unreachable', why: 'the drive cannot be reached' }] as const) {
      await expect(read(ref, () => Promise.resolve(answer))).resolves.toEqual(answer);
    }
  });

  it('reads a published asset with import(): bytes, withheld, or (no module) not found', async () => {
    const urls: Record<string, string> = { [SCENE_SHA256]: moduleUrl(assetModule(SCENE_PNG)), ['0'.repeat(64)]: moduleUrl(assetModule(null)), ['1'.repeat(64)]: 'data:text/javascript,throw new Error()' };
    const source = importedFiles((sha256) => urls[sha256] ?? 'data:text/javascript,');
    await expect(read(ref, source)).resolves.toEqual({ kind: 'shown', bytes: SCENE_PNG });
    await expect(source('0'.repeat(64))).resolves.toEqual({ kind: 'withheld' });
    await expect(source('1'.repeat(64))).resolves.toEqual({ kind: 'not-found' });
    await expect(source('../first-drive')).resolves.toEqual({ kind: 'not-found' });
    expect(assetAnswer(42)).toEqual({ kind: 'not-found' });
  });
});

describe('a recording’s assets, as the replay build publishes them (#372)', () => {
  let dir: string | undefined;
  afterEach(() => {
    if (dir) rmSync(dir, { recursive: true, force: true });
    dir = undefined;
  });
  const recording = (files: Record<string, Uint8Array>) => {
    dir = mkdtempSync(path.join(tmpdir(), 'exercise-assets-'));
    mkdirSync(path.join(dir, 'shot', 'files'), { recursive: true });
    for (const [name, bytes] of Object.entries(files)) writeFileSync(path.join(dir, 'shot', 'files', name), bytes);
    return dir;
  };

  it('publishes a declared asset’s bytes, and withholds one the admission does not declare clean', () => {
    const other = new Uint8Array([1, 2, 3]);
    const otherSha = createHash('sha256').update(other).digest('hex');
    const at = recording({ [SCENE_SHA256]: SCENE_PNG, [otherSha]: other });
    const emitted = publishedAssets(at, 'shot', { files: [{ sha256: SCENE_SHA256, scrub: 'declared-clean' }, { sha256: otherSha, scrub: 'pending' }] });
    expect(emitted.map((e) => e.fileName).sort()).toEqual([`data/shot/files/${otherSha}.js`, `data/shot/files/${SCENE_SHA256}.js`].sort());
    expect(emitted.find((e) => e.fileName.includes(SCENE_SHA256))?.source).toBe(assetModule(SCENE_PNG));
    expect(emitted.find((e) => e.fileName.includes(otherSha))?.source).toBe('export default null\n');
  });

  it('refuses an asset not named by a digest, or whose bytes are not its name', () => {
    expect(() => publishedAssets(recording({ 'scene.png': SCENE_PNG }), 'shot', {})).toThrow(/named by its sha256/);
    expect(() => publishedAssets(recording({ ['a'.repeat(64)]: SCENE_PNG }), 'shot', {})).toThrow(/its bytes are/);
  });

  it('publishes nothing for a recording with no files', () => {
    expect(publishedAssets(recording({}), 'none', {})).toEqual([]);
  });
});

describe('a call whose result is a file, canned (#372)', () => {
  it('places the reference -- path, digest, media type, size -- never the bytes, and the fold carries it', async () => {
    const transport = new CannedTransport(SCREENSHOT, { speed: 1_000_000 });
    const lines: LogLine[] = [];
    transport.subscribe((line) => lines.push(line));
    await transport.dispatch({ kind: 'ask', text: 'Render the scene and show me a screenshot.' });
    await new Promise((resolve) => setTimeout(resolve, 100));
    transport.close();
    const call = lines.find((l) => l.kind === 'tool_call');
    expect(call).toMatchObject({ outcome: 'ran', files: [ref] });
    expect(JSON.stringify(call)).not.toContain(Buffer.from(SCENE_PNG).toString('base64').slice(0, 40));
    const node = fold(lines).eras.flatMap((e) => e.nodes).find((n) => n.kind === 'tool');
    expect(node?.kind === 'tool' && node.files).toEqual([ref]);
    await expect(read(ref, transport.file)).resolves.toEqual({ kind: 'shown', bytes: SCENE.bytes });
    await expect(transport.file('0'.repeat(64))).resolves.toEqual({ kind: 'not-found' });
  });
});
