#!/usr/bin/env node
/**
 * A stand-in for `diet-drive serve` (#117 I5) until it lands: `serve.rs`'s
 * contract (#128), serving a session whose log is `diet/formats/log` v0 and
 * nothing else, so `?drive` can be run end to end today. TEMPORARY: delete it
 * when I5 lands, and drive `diet` itself.
 *
 *     node scripts/stand-in.mjs [--port 7801] [--allow-origin http://localhost:5173]
 *     DIET_DRIVE=http://127.0.0.1:7801 pnpm dev   # then open /?drive
 *
 * What it keeps of the contract: `GET /events?from=<seq>`, replayed then
 * tailed, each event `id: <opened>-<seq>` and one `data:` line; a
 * `Last-Event-ID` from another process answered 410; `POST /commands` as
 * JSON only (415 otherwise), a refusal 409 `{"refused": <tag>}` and logged;
 * `Host` and `Origin` held to the allowed origins (403). What it is not: a
 * model. An ask is answered by a canned sentence, streamed a word at a time.
 * `SIGUSR2` restarts the session under a new `opened`, to exercise a 410.
 */

import { createServer } from 'node:http';

const arg = (name, fallback) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 ? process.argv[i + 1] : fallback;
};
const PORT = Number(arg('port', '7801'));
const ORIGINS = [arg('allow-origin', 'http://localhost:5173')];
const HOSTS = new Set([`127.0.0.1:${PORT}`, `localhost:${PORT}`, ...ORIGINS.map((o) => new URL(o).host)]);

let session;
function open() {
  session = { opened: Date.now(), log: [], readers: new Set(), state: 'awaiting', turn: 0, timers: new Set() };
  push({ kind: 'session.start', version: 0, opened: session.opened, model: 'stand-in', head: [{ role: 'system', content: 'You are a stand-in for diet-drive serve: no model, a canned answer.' }] });
}
function push(event) {
  const line = { seq: session.log.length, t: Date.now() - session.opened, ...event };
  session.log.push(line);
  for (const reader of session.readers) write(reader, line);
  return line;
}
const write = (res, line) => res.write(`id: ${session.opened}-${line.seq}\ndata: ${JSON.stringify(line)}\n\n`);
function settle(to) {
  push({ kind: 'settlement', from: session.state, to });
  session.state = to;
}
function later(ms, fn) {
  const timer = setTimeout(() => {
    session.timers.delete(timer);
    fn();
  }, ms);
  session.timers.add(timer);
}

/** An ask, answered: the log a turn makes, a word at a time. */
function ask(text) {
  session.turn += 1;
  const turn = session.turn;
  const admitted = push({ kind: 'ask', turn, text });
  settle('turn');
  const request = push({ kind: 'request', turn, lane: 'trunk' });
  const words = `You asked: "${text.slice(0, 80)}". This is the stand-in answering; diet-drive serve (#117 I5) will put a model here.`.split(/(?<= )/);
  let written = '';
  words.forEach((word, i) =>
    later(400 + i * 90, () => {
      written += word;
      push({ kind: 'delta', request: request.seq, text: word });
      if (i === words.length - 1) {
        push({ kind: 'response', to_request: request.seq, text: written, finish_reason: 'stop' });
        push({ kind: 'turn.settled', turn, reason: 'final' });
        settle('awaiting');
      }
    }),
  );
  return { seq: admitted.seq, turn };
}

function refuse(res, command, because) {
  push({ kind: 'refused', command, because, during: session.state });
  reply(res, 409, { refused: because });
}
const reply = (res, status, body) => {
  res.writeHead(status, { 'Content-Type': 'application/json' });
  res.end(JSON.stringify(body));
};

function command(res, body) {
  switch (body.kind) {
    case 'ask':
      if (typeof body.text !== 'string') return reply(res, 400, {});
      if (session.state !== 'awaiting') return refuse(res, 'ask', session.state === 'ended' ? 'ended' : 'in-flight');
      return reply(res, 200, ask(body.text));
    case 'cancel': {
      if (typeof body.turn !== 'number') return reply(res, 400, {});
      if (body.turn < session.turn) return refuse(res, 'cancel', 'stale');
      if (session.state !== 'turn') return refuse(res, 'cancel', 'nothing-in-flight');
      push({ kind: 'stop.asked', turn: body.turn });
      for (const timer of session.timers) clearTimeout(timer);
      session.timers.clear();
      const request = session.log.findLast((l) => l.kind === 'request');
      const partial = session.log.filter((l) => l.kind === 'delta' && l.request === request.seq).map((l) => l.text).join('');
      push({ kind: 'cancelled', request: request.seq, partial });
      push({ kind: 'turn.settled', turn: body.turn, reason: 'cancelled' });
      settle('awaiting');
      return reply(res, 200, {});
    }
    case 'declare-seam':
      return refuse(res, 'declare-seam', 'seam-not-built');
    case 'end':
      if (session.state === 'ended') return refuse(res, 'end', 'ended');
      settle('ended');
      return reply(res, 200, {});
    default:
      return reply(res, 400, {});
  }
}

const server = createServer((req, res) => {
  const origin = req.headers.origin;
  if (!HOSTS.has(req.headers.host ?? '') || (origin !== undefined && !ORIGINS.includes(origin))) {
    res.writeHead(403);
    return res.end();
  }
  const url = new URL(req.url ?? '/', 'http://stand-in');
  if (req.method === 'GET' && url.pathname === '/events') {
    const resume = req.headers['last-event-id'];
    let from = Number(url.searchParams.get('from') ?? '0');
    if (typeof resume === 'string') {
      const [opened, seq] = resume.split('-').map(Number);
      if (opened !== session.opened) {
        res.writeHead(410);
        return res.end();
      }
      from = seq + 1;
    }
    res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache' });
    for (const line of session.log.slice(from)) write(res, line);
    session.readers.add(res);
    const beat = setInterval(() => res.write(': heartbeat\n\n'), 15_000);
    req.on('close', () => {
      clearInterval(beat);
      session.readers.delete(res);
    });
    return;
  }
  if (req.method === 'POST' && url.pathname === '/commands') {
    if (!(req.headers['content-type'] ?? '').startsWith('application/json')) {
      res.writeHead(415);
      return res.end();
    }
    let text = '';
    req.on('data', (chunk) => (text += chunk));
    req.on('end', () => {
      let body;
      try {
        body = JSON.parse(text);
      } catch {
        return reply(res, 400, {});
      }
      command(res, body);
    });
    return;
  }
  res.writeHead(404);
  res.end();
});

open();
process.on('SIGUSR2', () => {
  // A restarted process: every open stream ends, and a resume naming the old session is refused 410.
  for (const reader of session.readers) reader.end();
  open();
  console.log(`restarted: opened ${session.opened}`);
});
server.listen(PORT, '127.0.0.1', () => console.log(`stand-in for diet-drive serve on 127.0.0.1:${PORT}, allowing ${ORIGINS.join(', ')}`));
