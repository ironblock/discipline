#!/usr/bin/env node
// The replay page as a visitor gets it (#32): the built site served under a
// path, as Pages serves a project (`/<repo>/`), opened in Chromium, and every
// published recording loaded through the page's real load path and played to
// its end, and each authored example (#272) the same way, under its label from
// first to last. Fails -- naming what -- if a recording does not load, if an
// example is ever on the page without its label, if any
// request leaves this origin or fails, or if any request is not found. The issue's own acceptance: "loads
// the fixture record in a browser with no network calls other than fetching
// static assets".
//
//   node scripts/replay-smoke.mjs SITE        SITE is the built site, ../_site
import { createReadStream, existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import path from 'node:path';

import { chromium } from 'playwright';

const site = path.resolve(process.argv[2] ?? '');
// The published lists and the examples' label, from the one place each is written.
const LISTS = readFileSync(new URL('../src/replay/published.ts', import.meta.url), 'utf8');
const listed = (which) => [...(new RegExp(`export const ${which} = \\[([^\\]]*)\\] as const`).exec(LISTS)?.[1] ?? '').matchAll(/'([a-z0-9-]+)'/g)].map((m) => m[1]);
const PUBLISHED = listed('PUBLISHED');
const EXAMPLES = listed('EXAMPLES');
const LABEL = /export const EXAMPLE_LABEL = '([^'\\]*)';/.exec(LISTS)?.[1];
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
// Each published file is a file of its own beside the page, never bundled: no long string of one (40 characters or
// more, as written or JSON-escaped) is in anything else the page ships -- every file under replay/ but data/. A
// bundler may drop a title it finds unused and keep the text, so the check reads every string, not one; a file
// that gives it no string to compare is a check of nothing, and says so.
const shipped = (dir) =>
  readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return full === path.join(site, 'replay', 'data') ? [] : shipped(full);
    return [[path.relative(path.join(site, 'replay'), full), readFileSync(full, 'utf8')]];
  });
const code = shipped(path.join(site, 'replay'));
const strings = (value, into = new Set()) => {
  if (typeof value === 'string' && value.length >= 40) into.add(value);
  else if (value && typeof value === 'object') for (const v of Object.values(value)) strings(v, into);
  return into;
};
for (const name of [...PUBLISHED, ...EXAMPLES]) {
  const payload = path.join(site, 'replay', 'data', `${name}.js`);
  if (!existsSync(payload)) continue; // named below, when it does not load
  const forms = [...strings(JSON.parse(readFileSync(payload, 'utf8').replace(/^export default /, '')))].flatMap((t) => [t, JSON.stringify(t).slice(1, -1)]);
  if (forms.length === 0) problems.push(`${name} has no string of 40 characters or more, so nothing shows whether it is bundled into the page's code`);
  const [file] = code.find(([, text]) => forms.some((form) => text.includes(form))) ?? [];
  if (file) problems.push(`${name} is bundled into the page's code (replay/${file}), not only beside it`);
}
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
  if (EXAMPLES.length > 0 && !LABEL) problems.push('src/replay/published.ts publishes examples and no EXAMPLE_LABEL to put over them');
  // The label on the page, and in view: at the top of it while the page is scrolled to the end.
  const labelled = async () =>
    page.$eval('.ex-replay__label', (el) => {
      window.scrollTo(0, document.documentElement.scrollHeight);
      const box = el.getBoundingClientRect();
      return { text: el.textContent, inView: box.height > 0 && box.top >= 0 && box.bottom <= window.innerHeight };
    }).catch(() => null);
  // Each published recording and example, through its real path, played to its end -- fast -- and watched a moment after.
  for (const name of [...PUBLISHED, ...EXAMPLES]) {
    const example = EXAMPLES.includes(name);
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
    if (!example && !source?.includes('Scrubbed:')) problems.push(`${name} loaded without its source and scrub drawn above it`);
    if (example && !source?.includes(`Authored: ${LABEL}`)) problems.push(`${name} loaded without its header saying it was authored`);
    if (!example && (await page.$('.ex-replay__label'))) problems.push(`${name} is a recording, replayed under the examples' label`);
    const atStart = example ? await labelled() : null;
    await page.waitForTimeout(2_000);
    if (example) {
      // At its end, scrolled to the bottom: the label is the page's for the whole replay, not the opening moment's.
      const atEnd = await labelled();
      for (const [when, seen] of [['as it loaded', atStart], ['at its end', atEnd]]) {
        if (seen?.text !== LABEL) problems.push(`${name} replayed without its label on the page ${when} (found ${JSON.stringify(seen?.text ?? null)})`);
        else if (!seen.inView) problems.push(`${name}'s label is on the page but out of view ${when}, scrolled to the end`);
      }
    }
  }

  // The bare page: what is published, the examples apart under their label, and what is not and why.
  await page.goto(`${origin}${BASE}replay/`);
  const recordings = await page.$$eval('.ex-replay__recordings a', (as) => as.map((a) => a.textContent));
  if (recordings.join(' ') !== PUBLISHED.join(' ')) problems.push(`the index lists ${JSON.stringify(recordings)} as recordings, not ${JSON.stringify(PUBLISHED)}`);
  const examples = await page.$$eval('.ex-replay__examples a', (as) => as.map((a) => a.textContent));
  if (examples.join(' ') !== EXAMPLES.join(' ')) problems.push(`the index lists ${JSON.stringify(examples)} as examples, not ${JSON.stringify(EXAMPLES)}`);
  const indexLabel = await page.textContent('.ex-replay__examples .ex-replay__label').catch(() => null);
  if (EXAMPLES.length > 0 && indexLabel !== LABEL) problems.push(`the index lists the examples without their label (found ${JSON.stringify(indexLabel)})`);
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
console.log(
  `replay-smoke: ${PUBLISHED.join(', ')} replay from the built site to their ends, and ${EXAMPLES.join(', ')} under the examples' label throughout; the index lists each in its section, ?drive names the landing page, and nothing left the site`,
);
