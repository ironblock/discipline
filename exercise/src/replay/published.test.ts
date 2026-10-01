import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import { RECORDINGS } from '../drive/recordings.ts';
import { PREFIX, unwrap, wrap } from './payload.ts';
import { NOT_PUBLISHED, PUBLISHED } from './published.ts';

const recorded = path.join(path.dirname(fileURLToPath(import.meta.url)), '../drive/recorded');

describe('what the replay page publishes (#32)', () => {
  it('publishes the three recorded whole, and never an authored session', () => {
    expect([...PUBLISHED]).toEqual(['first-drive', 'cancelled-capture', 'step-limit']);
    expect(PUBLISHED).not.toContain('kitchen-sink');
    expect(PUBLISHED).not.toContain('voxel-stress');
  });

  it('says why of every recording it does not publish', () => {
    const unpublished = Object.keys(RECORDINGS).filter((name) => !(PUBLISHED as readonly string[]).includes(name));
    expect(unpublished.filter((name) => !NOT_PUBLISHED[name])).toEqual([]);
  });

  it('publishes nothing that was not admitted: each has its admission beside it', () => {
    expect(PUBLISHED.filter((name) => !existsSync(path.join(recorded, `${name}.admission.json`)))).toEqual([]);
  });
});

describe('a published recording as the page loads it', () => {
  // What wrap() writes. The built files themselves are held to it by `admission.py verify` (check_site).
  it.each(PUBLISHED)('%s: wrap() puts the committed recording, byte for byte, behind one prefix', (name) => {
    const source = readFileSync(path.join(recorded, `${name}.json`), 'utf8');
    const payload = wrap(source);
    expect(payload.startsWith(PREFIX)).toBe(true);
    expect(payload.length).toBe(PREFIX.length + source.length);
    expect(unwrap(payload)).toBe(source);
  });

  it('is not a payload unless it starts with the prefix', () => {
    expect(unwrap('{"title":"x"}')).toBeUndefined();
  });
});
