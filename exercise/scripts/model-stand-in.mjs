#!/usr/bin/env node
/**
 * A model for `diet-drive serve` to call when there is no GPU at hand: every
 * request to it is answered with a real llama-server reply, replayed byte for
 * byte -- `diet`'s own capture (`diet/client/fixtures/llama-server-4df29be-
 * stream.http`, R2b; a small random-weight model, so the words are noise),
 * the same one `diet/tests/drive_serve_cli.rs` serves -- paced so the page
 * sees it stream. Nothing here is a second reader of anything: the bytes go
 * out as they were captured, and `diet` parses them.
 *
 *     node scripts/model-stand-in.mjs [--port 7901] [--pace-ms 15]
 *     diet-drive serve --endpoint http://127.0.0.1:7901/v1/chat/completions \
 *         --model tiny --head <file> --port 7801 --allow-origin http://localhost:5173
 *     DIET_DRIVE=http://127.0.0.1:7801 pnpm dev        # then open /?drive
 */

import { readFileSync } from 'node:fs';
import { createServer } from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const arg = (name, fallback) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 ? process.argv[i + 1] : fallback;
};
const PORT = Number(arg('port', '7901'));
const PACE_MS = Number(arg('pace-ms', '15'));
const CAPTURED = readFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)), '../../diet/client/fixtures/llama-server-4df29be-stream.http'));

const server = createServer((socket) => {
  let head = '';
  socket.on('data', (chunk) => {
    head += chunk.toString('latin1');
    // Answer once the request is in: its head, and a body as long as it says.
    const end = head.indexOf('\r\n\r\n');
    if (end < 0) return;
    const length = Number(/content-length:\s*(\d+)/i.exec(head.slice(0, end))?.[1] ?? 0);
    if (head.length - end - 4 < length) return;
    socket.removeAllListeners('data');
    let at = 0;
    const next = () => {
      if (socket.destroyed) return;
      if (at >= CAPTURED.length) return void socket.end();
      socket.write(CAPTURED.subarray(at, at + 96));
      at += 96;
      setTimeout(next, PACE_MS);
    };
    next();
  });
  socket.on('error', () => {});
});
server.listen(PORT, '127.0.0.1', () => console.log(`a captured llama-server reply, replayed on 127.0.0.1:${PORT}`));
