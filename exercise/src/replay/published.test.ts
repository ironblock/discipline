import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import { AUTHORED } from '../drive/examples.ts';
import { examplePath, load } from '../drive/recorded.ts';
import { RECORDINGS } from '../drive/recordings.ts';
import { PREFIX, serialize, unwrap, wrap } from './payload.ts';
import { EXAMPLE_LABEL, EXAMPLES, NOT_PUBLISHED, PUBLISHED } from './published.ts';

const recorded = path.join(path.dirname(fileURLToPath(import.meta.url)), '../drive/recorded');
const examples = path.join(path.dirname(fileURLToPath(import.meta.url)), '../drive/examples');

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

describe('the authored examples it publishes apart from the recordings (#272)', () => {
  it('publishes the kitchen sink as an example, never as a recording, and still not voxel-stress', () => {
    expect([...EXAMPLES]).toEqual(['kitchen-sink']);
    expect(EXAMPLES.filter((name) => (PUBLISHED as readonly string[]).includes(name))).toEqual([]);
    expect(EXAMPLES.filter((name) => Object.hasOwn(NOT_PUBLISHED, name))).toEqual([]);
    expect(EXAMPLES).not.toContain('voxel-stress');
    expect(NOT_PUBLISHED['voxel-stress']).toContain('written by a model after the session');
  });

  it("labels each with the maintainer's sentence, verbatim", () => {
    expect(EXAMPLE_LABEL).toBe('this is an example of everything working the way we think it should, not a real session');
  });

  it.each(EXAMPLES)('%s: says it was authored, in one line carrying the label, and says nothing was scrubbed', (name) => {
    const { migration } = AUTHORED[name];
    expect(migration.filter((line) => line.startsWith('Authored:'))).toEqual([`Authored: ${EXAMPLE_LABEL}`]);
    expect(migration.filter((line) => line.startsWith('Scrubbed:'))).toEqual([]);
  });

  it.each(EXAMPLES)('%s: the committed file is its source, serialized (node scripts/write-examples.mjs)', (name) => {
    const committed = readFileSync(path.join(examples, `${name}.json`), 'utf8');
    expect(committed).toBe(serialize(AUTHORED[name]));
    expect(load(name, committed, examplePath(name))).toEqual(AUTHORED[name]);
  });

  it('publishes no example that was not admitted: each has its admission beside it', () => {
    expect(EXAMPLES.filter((name) => !existsSync(path.join(examples, `${name}.admission.json`)))).toEqual([]);
  });

  it.each(EXAMPLES)('%s: wrap() puts the committed file, byte for byte, behind one prefix', (name) => {
    const source = readFileSync(path.join(examples, `${name}.json`), 'utf8');
    expect(unwrap(wrap(source))).toBe(source);
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
