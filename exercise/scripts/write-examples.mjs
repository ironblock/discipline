#!/usr/bin/env node
// Each published example (src/replay/published.ts, EXAMPLES), written from its
// source (src/drive/examples.ts) to src/drive/examples/<name>.json: the file
// the replay page publishes and `scripts/admission.py admit <name>` admits
// (#272). Run it after editing an example's source, then admit it again;
// published.test.ts fails while the two differ.
//
//   node scripts/write-examples.mjs
import { writeFileSync } from 'node:fs';

import { AUTHORED } from '../src/drive/examples.ts';
import { serialize } from '../src/replay/payload.ts';
import { EXAMPLES } from '../src/replay/published.ts';

for (const name of EXAMPLES) {
  const file = new URL(`../src/drive/examples/${name}.json`, import.meta.url);
  writeFileSync(file, serialize(AUTHORED[name]));
  console.log(`write-examples: src/drive/examples/${name}.json`);
}
