#!/usr/bin/env node
// The replay page as a visitor gets it (#32): the built site served under a
// path, as Pages serves a project (`/<repo>/`), opened in Chromium, and every
// published recording loaded through the page's real load path and played to
// its end. Fails -- naming what -- if a recording does not load, if any
// request leaves this origin or fails, or if any request is not found. The issue's own acceptance: "loads
// the fixture record in a browser with no network calls other than fetching
// static assets".
//
//   node scripts/replay-smoke.mjs SITE        SITE is the built site, ../_site
import { createReadStream, existsSync, readFileSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import path from 'node:path';

import { chromium } from 'playwright';

const site = path.resolve(process.argv[2] ?? '');
// The published list, from the one place it is written.
const PUBLISHED = [...(/export const PUBLISHED = \[([^\]]*)\] as const/.exec(readFileSync(new URL('../src/replay/published.ts', import.meta.url), 'utf8'))?.[1] ?? '').matchAll(/'([a-z0-9-]+)'/g)].map((m) => m[1]);
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
  if (!file || !(file === site || file.startsWith(site + path.sep)) || !existsSync(file)) {
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
  const own = (url) => {
    try {
      return new URL(url).origin === origin;
    } catch {
      return false;
    }
  };
  page.on('request', (r) => {
    if (!own(r.url())) problems.push(`a request leaves the site: ${r.url()}`);
  });
  page.on('requestfailed', (r) => problems.push(`a request failed (${r.failure()?.errorText ?? 'unknown'}): ${r.url().replace(origin, '')}`));
  page.on('response', (r) => {
    if (r.status() >= 400) problems.push(`not found (${r.status()}): ${r.url().replace(origin, '')}`);
  });
  page.on('pageerror', (err) => problems.push(`the page threw: ${err.message}`));

  if (PUBLISHED.length === 0) problems.push('src/replay/published.ts publishes nothing to load');
  // Each published recording, through its real path, played to its end -- fast -- and watched a moment after.
  for (const name of PUBLISHED) {
    await page.goto(`${origin}${BASE}replay/?session=${name}&speed=100000`);
    const loaded = await Promise.race([
      page.waitForSelector('.ex-trunk .ex-block', { timeout: 30_000 }).then(() => true),
      page.waitForSelector('#root[data-failed]', { timeout: 30_000 }).then(() => false),
    ]).catch(() => false);
    if (!loaded) {
      problems.push(`${name} did not load: ${(await page.textContent('#root'))?.slice(0, 300) ?? '(nothing on the page)'}`);
      continue;
    }
    const source = await page.textContent('.ex-replay__source').catch(() => null);
    if (!source?.includes('Scrubbed:')) problems.push(`${name} loaded without its source and scrub drawn above it`);
    await page.waitForTimeout(2_000);
  }

  // The bare page: what is published, and what is not and why.
  await page.goto(`${origin}${BASE}replay/`);
  const listed = await page.$$eval('.ex-replay__list a', (as) => as.map((a) => a.textContent));
  if (listed.join(' ') !== PUBLISHED.join(' ')) problems.push(`the index lists ${JSON.stringify(listed)}, not ${JSON.stringify(PUBLISHED)}`);
  // ?drive: this page replays, and names the landing page.
  await page.goto(`${origin}${BASE}replay/?drive`);
  const landing = await page.$eval('.ex-replay__note a', (a) => a.getAttribute('href')).catch(() => null);
  if (landing !== '../') problems.push(`?drive does not name the landing page (found ${JSON.stringify(landing)})`);
} finally {
  await browser.close();
  server.close();
}
if (problems.length > 0) {
  for (const p of problems) console.error(`replay-smoke: ${p}`);
  process.exit(1);
}
console.log(`replay-smoke: ${PUBLISHED.join(', ')} replay from the built site to their ends, the index lists them, ?drive names the landing page, and nothing left the site`);
