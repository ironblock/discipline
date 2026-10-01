#!/usr/bin/env node
// The replay page as a visitor gets it (#32): the built site served under a
// path, as Pages serves a project (`/<repo>/`), opened in Chromium, a
// published recording loaded through the page's real load path. Fails --
// naming what -- if the recording does not load, if any request leaves this
// origin, or if any request is not found. The issue's own acceptance: "loads
// the fixture record in a browser with no network calls other than fetching
// static assets".
//
//   node scripts/replay-smoke.mjs SITE        SITE is the built site, ../_site
import { createReadStream, existsSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import path from 'node:path';

import { chromium } from 'playwright';

const site = path.resolve(process.argv[2] ?? '');
if (!process.argv[2] || !existsSync(path.join(site, 'replay', 'index.html'))) {
  console.error(`replay-smoke: ${process.argv[2] ?? '(no site named)'}: no replay/index.html`);
  process.exit(2);
}
const BASE = '/discipline/';
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.json': 'application/json' };
const server = createServer((req, res) => {
  const url = new URL(req.url ?? '/', 'http://localhost');
  const rel = url.pathname.startsWith(BASE) ? url.pathname.slice(BASE.length) : null;
  let file = rel === null ? null : path.join(site, decodeURIComponent(rel));
  if (file && existsSync(file) && statSync(file).isDirectory()) file = path.join(file, 'index.html');
  if (!file || !file.startsWith(site) || !existsSync(file)) {
    res.writeHead(404).end();
    return;
  }
  res.writeHead(200, { 'Content-Type': TYPES[path.extname(file)] ?? 'application/octet-stream' });
  createReadStream(file).pipe(res);
});
await new Promise((ready) => server.listen(0, '127.0.0.1', ready));
const origin = `http://127.0.0.1:${server.address().port}`;

const problems = [];
const browser = await chromium.launch();
try {
  const page = await browser.newPage();
  page.on('request', (r) => {
    if (!r.url().startsWith(origin)) problems.push(`a request leaves the site: ${r.url()}`);
  });
  page.on('response', (r) => {
    if (r.status() >= 400) problems.push(`not found (${r.status()}): ${r.url().replace(origin, '')}`);
  });
  page.on('pageerror', (err) => problems.push(`the page threw: ${err.message}`));

  // A published recording, through its real path.
  await page.goto(`${origin}${BASE}replay/?session=step-limit`);
  const loaded = await Promise.race([
    page.waitForSelector('.ex-trunk .ex-block', { timeout: 20_000 }).then(() => true),
    page.waitForSelector('#root[data-failed]', { timeout: 20_000 }).then(() => false),
  ]).catch(() => false);
  if (!loaded) problems.push(`step-limit did not load: ${(await page.textContent('#root'))?.slice(0, 300) ?? '(nothing on the page)'}`);
  const source = await page.textContent('.ex-replay__source').catch(() => null);
  if (loaded && !source?.includes('Scrubbed:')) problems.push('step-limit loaded without its source and scrub drawn above it');

  // The bare page: what is published, and what is not and why.
  await page.goto(`${origin}${BASE}replay/`);
  const listed = await page.$$eval('.ex-replay__list a', (as) => as.map((a) => a.textContent));
  if (listed.join(' ') !== 'first-drive cancelled-capture step-limit') problems.push(`the index lists ${JSON.stringify(listed)}`);
} finally {
  await browser.close();
  server.close();
}
if (problems.length > 0) {
  for (const p of problems) console.error(`replay-smoke: ${p}`);
  process.exit(1);
}
console.log('replay-smoke: step-limit replays from the built site, the index lists the three published, and nothing left the site');
