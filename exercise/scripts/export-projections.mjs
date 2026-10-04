#!/usr/bin/env node
// Every placed session's projection (src/drive/projection.ts), one log file
// each, into DIR: what the repository's `exercise` check hands to
// `diet check-log` (ruled on #300, 5974717646). Loaded through Vite, as the
// recordings are `?raw` imports.
//
//   node scripts/export-projections.mjs DIR
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';

import { createServer } from 'vite';

const out = process.argv[2];
if (!out) {
  console.error('usage: export-projections.mjs DIR');
  process.exit(2);
}
mkdirSync(out, { recursive: true });
const vite = await createServer({ server: { middlewareMode: true }, appType: 'custom', logLevel: 'error' });
try {
  const { projections } = await vite.ssrLoadModule('/src/drive/projections.ts');
  for (const { name, lines } of projections()) {
    const file = path.join(out, `${name}.log`);
    writeFileSync(file, lines.map((line) => JSON.stringify(line)).join('\n') + '\n');
    console.log(`export-projections: ${file} (${lines.length} lines)`);
  }
} finally {
  await vite.close();
}
