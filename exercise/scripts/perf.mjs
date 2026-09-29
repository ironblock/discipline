#!/usr/bin/env node
/**
 * A performance trace of the surface: where frames drop, and what is heavy.
 *
 *     node scripts/perf.mjs [--sessions kitchen-sink,first-drive,voxel-stress] [--out report.json] [--ablate] [--css '<rules>']
 *
 * Builds the app for production -- React's production build, unminified so a
 * profile names functions -- serves it in-process, and drives headless
 * Chromium (Playwright) at a desktop's size, wide enough for the trunk, the
 * lanes, working memory and the lines between them. Per session, three
 * phases, each measured on its own:
 *
 *   replay  the session replayed at 10x: a stress of streaming updates
 *   scroll  the whole session top to bottom and back, by smooth scroll
 *           gestures, at full CPU and at 4x CPU throttling
 *   point   the pointer swept over trunk nodes, lighting each one's chain
 *
 * Measured: every frame's interval (a requestAnimationFrame loop; a frame
 * longer than 1.5x a 60 Hz frame is dropped), Chrome's long animation frames
 * (over 50 ms, with the scripts that ran in them), Chrome's own counts of
 * layout, style recalculation and script time (CDP Performance), and a CPU
 * profile, by function, of each scroll, and a trace of the unthrottled one:
 * paint, raster and compositing by thread. Headless: absolute numbers are this
 * machine's; compare runs, and phases, with each other.
 */

import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { chromium } from 'playwright';
import { build, preview } from 'vite';

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const arg = (name, fallback) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 ? process.argv[i + 1] : fallback;
};
const SESSIONS = arg('sessions', 'kitchen-sink,first-drive,voxel-stress').split(',');
const OUT = arg('out', undefined);
/** Extra CSS on every page: an experiment -- what does taking this away, or adding it, cost? */
const CSS = arg('css', undefined);
const VIEWPORT = { width: 1920, height: 1080 };
const REPLAY_SPEED = 10;
const REPLAY_MS = 20_000;
const SCROLL_PX_PER_S = 1600;
const THROTTLES = [1, 4];
const ABLATE = process.argv.includes('--ablate');
/**
 * With --ablate, the unthrottled scroll again with one thing the surface
 * paints taken away at a time, to say what the raster and the compositor
 * are spending on: the grain texture, glass (backdrop blur), the lit field
 * behind everything, glows and shadows, the lines overlay, all of them.
 */
const TEXTURE = '*, *::before, *::after { --texture: none !important; }';
const GLASS = '*, *::before, *::after { --glass-backdrop: none !important; backdrop-filter: none !important; -webkit-backdrop-filter: none !important; }';
const FIELD = '.ex-session::before { display: none !important; } .ex-session__composer::before { background: var(--page) !important; }';
const SHADOW = '*, *::before, *::after { box-shadow: none !important; text-shadow: none !important; }';
const LINES = '.ex-links { display: none !important; }';
const ABLATIONS = [
  ['none (baseline)', '/* nothing taken away */'],
  ['no grain texture', TEXTURE],
  ['no glass', GLASS],
  ['no lit field', FIELD],
  ['no glows or shadows', SHADOW],
  ['no lines overlay', LINES],
  ['none of these', [TEXTURE, GLASS, FIELD, SHADOW, LINES].join('\n')],
];
const rasterOf = (threads) => threads.filter((t) => t.what.endsWith('RasterTask')).reduce((a, t) => a + t.ms, 0);
const drawOf = (threads) => threads.filter((t) => t.what.includes('DrawAndSwap')).reduce((a, t) => a + t.ms, 0);
const FRAME = 1000 / 60;

// ------------------------------------------------------------------ page instrumentation

/** Runs in the page before the app: a frame clock and a long-animation-frame log, both off until a phase starts. */
function instrument() {
  const perf = (window.__perf = { recording: false, frames: [], loafs: [] });
  const tick = (t) => {
    if (perf.recording) perf.frames.push(t);
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
  new PerformanceObserver((list) => {
    if (!perf.recording) return;
    for (const e of list.getEntries()) {
      const end = e.startTime + e.duration;
      perf.loafs.push({
        duration: e.duration,
        blocking: e.blockingDuration,
        styleAndLayout: e.styleAndLayoutStart ? end - e.styleAndLayoutStart : 0,
        scripts: e.scripts.map((s) => ({
          what: `${s.invoker}${s.sourceFunctionName ? ` → ${s.sourceFunctionName}` : ''}`,
          duration: s.duration,
          forcedLayout: s.forcedStyleAndLayoutDuration,
        })),
      });
    }
  }).observe({ type: 'long-animation-frame', buffered: false });
}

// ------------------------------------------------------------------ measuring a phase

const METRICS = ['LayoutCount', 'LayoutDuration', 'RecalcStyleCount', 'RecalcStyleDuration', 'ScriptDuration', 'TaskDuration'];

async function metrics(cdp) {
  const { metrics: all } = await cdp.send('Performance.getMetrics');
  return Object.fromEntries(all.filter((m) => METRICS.includes(m.name)).map((m) => [m.name, m.value]));
}

/** Runs `act` as one measured phase; with `profile`, a CPU profile of it too; with `trace`, where frames went, by thread. */
async function phase(page, cdp, act, { profile = false, trace = false } = {}) {
  const events = [];
  const collect = ({ value }) => events.push(...value);
  if (trace) {
    cdp.on('Tracing.dataCollected', collect);
    await cdp.send('Tracing.start', { categories: TRACE, transferMode: 'ReportEvents' });
  }
  await page.evaluate(() => Object.assign(window.__perf, { recording: true, frames: [], loafs: [] }));
  const before = await metrics(cdp);
  if (profile) {
    await cdp.send('Profiler.enable');
    await cdp.send('Profiler.setSamplingInterval', { interval: 250 });
    await cdp.send('Profiler.start');
  }
  const started = Date.now();
  await act();
  const wall = Date.now() - started;
  const cpu = profile ? (await cdp.send('Profiler.stop')).profile : undefined;
  if (trace) {
    const done = new Promise((resolve) => cdp.once('Tracing.tracingComplete', resolve));
    await cdp.send('Tracing.end');
    await done;
    cdp.off('Tracing.dataCollected', collect);
  }
  const after = await metrics(cdp);
  const { frames, loafs } = await page.evaluate(() => {
    window.__perf.recording = false;
    return { frames: window.__perf.frames, loafs: window.__perf.loafs };
  });
  const dom = await page.evaluate(() => ({
    elements: document.getElementsByTagName('*').length,
    paths: document.querySelectorAll('svg path').length,
    blocks: document.querySelectorAll('.ex-block').length,
  }));
  const delta = Object.fromEntries(METRICS.map((m) => [m, after[m] - before[m]]));
  return { wall, frames: frameStats(frames), loafs: loafStats(loafs), chrome: delta, dom, ...(cpu ? { hot: hottest(cpu) } : {}), ...(trace ? { threads: byThread(events) } : {}) };
}

const TRACE = ['devtools.timeline', 'disabled-by-default-devtools.timeline', 'cc', 'viz', 'gpu', 'blink'].join(',');
/** The trace's work, by thread and event, the heaviest first: complete events' own durations, children not subtracted. */
const WATCHED = /^(Paint|PaintImage|RasterTask|Layerize|UpdateLayerTree|Layout|UpdateLayoutTree|PrePaint|Commit|CompositeLayers|ActivateLayerTree|DrawFrame|Display::DrawAndSwap|SkiaOutputSurfaceImpl::.*|GPUTask|FunctionCall|EvaluateScript|FireAnimationFrame|EventDispatch|HitTest|ScrollLayer|ProxyMain::BeginMainFrame|DecodeImage|Filter.*|.*Backdrop.*|.*Blur.*)$/;
function byThread(events) {
  const names = new Map();
  for (const e of events) if (e.ph === 'M' && e.name === 'thread_name') names.set(`${e.pid}:${e.tid}`, e.args.name);
  const sums = new Map();
  for (const e of events) {
    if (e.ph !== 'X' || !e.dur || !WATCHED.test(e.name)) continue;
    const thread = (names.get(`${e.pid}:${e.tid}`) ?? '?').replace(/\d+$/, '');
    const key = `${thread} · ${e.name}`;
    const s = sums.get(key) ?? { n: 0, ms: 0 };
    s.n += 1;
    s.ms += e.dur / 1000;
    sums.set(key, s);
  }
  return [...sums].sort((a, b) => b[1].ms - a[1].ms).slice(0, 14).map(([what, s]) => ({ what, n: s.n, ms: Math.round(s.ms) }));
}

function frameStats(times) {
  const gaps = times.slice(1).map((t, i) => t - times[i]);
  if (gaps.length === 0) return { n: 0 };
  const sorted = [...gaps].sort((a, b) => a - b);
  const at = (q) => sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))];
  // Against the display's budget (60 Hz), not the phase's own median: a phase that runs every frame at 30 fps drops half.
  const dropped = gaps.filter((g) => g > 1.5 * FRAME).length;
  return { n: gaps.length, p50: at(0.5), p95: at(0.95), p99: at(0.99), max: sorted.at(-1), dropped, droppedShare: dropped / gaps.length };
}

function loafStats(loafs) {
  const by = new Map();
  for (const l of loafs) for (const s of l.scripts) by.set(s.what, (by.get(s.what) ?? 0) + s.duration);
  return {
    n: loafs.length,
    blocking: loafs.reduce((a, l) => a + l.blocking, 0),
    longest: Math.max(0, ...loafs.map((l) => l.duration)),
    styleAndLayout: loafs.reduce((a, l) => a + l.styleAndLayout, 0),
    forcedLayout: loafs.reduce((a, l) => a + l.scripts.reduce((b, s) => b + s.forcedLayout, 0), 0),
    scripts: [...by].sort((a, b) => b[1] - a[1]).slice(0, 6),
  };
}

/** A CPU profile's self time by function, the hottest first: ours, React's, and the browser's own (GC, layout). */
function hottest(profile) {
  const self = new Map();
  const nodes = new Map(profile.nodes.map((n) => [n.id, n]));
  const parent = new Map(profile.nodes.flatMap((n) => (n.children ?? []).map((c) => [c, n])));
  const name = (f) => `${f.functionName || '(anonymous)'}${f.url ? ` ${f.url.split('/').pop()}:${f.lineNumber + 1}` : ''}`;
  profile.samples.forEach((id, i) => {
    const node = nodes.get(id);
    const f = node.callFrame;
    // A browser function (no source) is named with the script that called it: whose querySelector it is.
    const caller = !f.url && parent.get(id)?.callFrame.url ? ` ← ${name(parent.get(id).callFrame)}` : '';
    const key = `${name(f)}${caller}`;
    self.set(key, (self.get(key) ?? 0) + (profile.timeDeltas[i] ?? 0) / 1000);
  });
  const total = [...self.values()].reduce((a, b) => a + b, 0);
  return [...self]
    .filter(([k]) => k !== '(idle)' && k !== '(program)')
    .sort((a, b) => b[1] - a[1])
    .slice(0, 12)
    .map(([what, ms]) => ({ what, ms: Math.round(ms), share: ms / total }));
}

// ------------------------------------------------------------------ the phases

async function settle(page) {
  // Settled: the count of blocks has held for 1.5 s.
  let last = -1;
  let since = Date.now();
  for (;;) {
    const n = await page.evaluate(() => document.querySelectorAll('.ex-block').length);
    if (n !== last) [last, since] = [n, Date.now()];
    if (Date.now() - since > 1500) return n;
    await page.waitForTimeout(250);
  }
}

async function scrollThrough(page, cdp) {
  const height = await page.evaluate(() => document.documentElement.scrollHeight - innerHeight);
  await page.evaluate(() => window.scrollTo(0, 0));
  const x = Math.round(VIEWPORT.width * 0.3);
  const y = Math.round(VIEWPORT.height / 2);
  // Down the whole session, then back up at twice the speed: a read, then a fling.
  await cdp.send('Input.synthesizeScrollGesture', { x, y, yDistance: -height, speed: SCROLL_PX_PER_S, gestureSourceType: 'mouse', preventFling: true });
  await cdp.send('Input.synthesizeScrollGesture', { x, y, yDistance: height, speed: SCROLL_PX_PER_S * 2, gestureSourceType: 'mouse', preventFling: true });
}

async function pointAcross(page) {
  await page.evaluate(() => window.scrollTo(0, (document.documentElement.scrollHeight - innerHeight) / 2));
  await page.waitForTimeout(300);
  const targets = await page.evaluate(() =>
    [...document.querySelectorAll('.ex-trunk__node')]
      .map((n) => n.getBoundingClientRect())
      .filter((r) => r.bottom > 80 && r.top < innerHeight - 200)
      .map((r) => ({ x: r.left + r.width / 2, y: r.top + Math.min(20, r.height / 2) })),
  );
  for (const t of targets) {
    await page.mouse.move(t.x, t.y, { steps: 6 });
    await page.waitForTimeout(120);
  }
  return targets.length;
}

async function run(browser, url, name) {
  const context = await browser.newContext({ viewport: VIEWPORT });
  const page = await context.newPage();
  await page.addInitScript(instrument);
  if (CSS) await page.addInitScript((css) => document.addEventListener('DOMContentLoaded', () => document.head.append(Object.assign(document.createElement('style'), { textContent: css }))), CSS);
  const cdp = await context.newCDPSession(page);
  await cdp.send('Performance.enable');
  const report = { session: name };

  await page.goto(`${url}?session=${name}&speed=${REPLAY_SPEED}`);
  await page.waitForSelector('.ex-block');
  report.replay = await phase(page, cdp, () => page.waitForTimeout(REPLAY_MS), { profile: true });

  // The whole session, all at once, to scroll and point at.
  await page.goto(`${url}?session=${name}&speed=100000`);
  report.blocks = await settle(page);
  report.room = await page.evaluate(() => ({ room: document.querySelector('.ex-session')?.getAttribute('data-room'), drawer: document.querySelector('.ex-session')?.hasAttribute('data-drawer') }));
  for (const rate of THROTTLES) {
    await cdp.send('Emulation.setCPUThrottlingRate', { rate });
    report[`scroll×${rate}`] = await phase(page, cdp, () => scrollThrough(page, cdp), { profile: true, trace: rate === 1 });
  }
  await cdp.send('Emulation.setCPUThrottlingRate', { rate: 1 });
  if (ABLATE) {
    report.ablations = [];
    for (const [what, css] of ABLATIONS) {
      const tag = await page.addStyleTag({ content: css });
      await page.waitForTimeout(300);
      const p = await phase(page, cdp, () => scrollThrough(page, cdp), { trace: true });
      await tag.evaluate((el) => el.remove());
      report.ablations.push({ what, frames: p.frames, raster: rasterOf(p.threads), draw: drawOf(p.threads) });
    }
  }
  let pointed = 0;
  report.point = await phase(page, cdp, async () => {
    pointed = await pointAcross(page);
  });
  report.point.targets = pointed;
  await context.close();
  return report;
}

// ------------------------------------------------------------------ report

const ms = (v) => (v === undefined ? '–' : `${v.toFixed(1)}`);

function print(report) {
  console.log(`\n== ${report.session}: ${report.blocks} blocks, room ${report.room.room}${report.room.drawer ? ', memory a drawer' : ''}`);
  const rows = ['replay', ...THROTTLES.map((r) => `scroll×${r}`), 'point'];
  console.log('phase        frames  p50   p95   p99    max   dropped   LoAF  blocking  layout(n/ms)   style(n/ms)   script ms  elements');
  for (const row of rows) {
    const p = report[row];
    const f = p.frames;
    console.log(
      [
        row.padEnd(11),
        String(f.n).padStart(6),
        ms(f.p50).padStart(5),
        ms(f.p95).padStart(5),
        ms(f.p99).padStart(5),
        ms(f.max).padStart(6),
        `${f.dropped} (${((f.droppedShare ?? 0) * 100).toFixed(1)}%)`.padStart(11),
        String(p.loafs.n).padStart(5),
        ms(p.loafs.blocking).padStart(9),
        `${p.chrome.LayoutCount}/${Math.round(p.chrome.LayoutDuration * 1000)}`.padStart(13),
        `${p.chrome.RecalcStyleCount}/${Math.round(p.chrome.RecalcStyleDuration * 1000)}`.padStart(13),
        String(Math.round(p.chrome.ScriptDuration * 1000)).padStart(10),
        String(p.dom.elements).padStart(9),
      ].join(' '),
    );
  }
  for (const row of rows) {
    const p = report[row];
    if (p.loafs.scripts.length) console.log(`  ${row} long-frame scripts: ${p.loafs.scripts.map(([w, d]) => `${w} ${Math.round(d)}ms`).join('; ')}`);
    if (p.loafs.forcedLayout > 1) console.log(`  ${row} forced layout in long frames: ${Math.round(p.loafs.forcedLayout)}ms`);
  }
  for (const a of report.ablations ?? []) {
    console.log(`  ablate ${a.what.padEnd(20)} p50 ${ms(a.frames.p50)}  dropped ${((a.frames.droppedShare ?? 0) * 100).toFixed(1)}%  raster ${a.raster}ms  draw ${a.draw}ms`);
  }
  if (report['scroll×1'].threads) console.log(`  scroll×1 by thread (trace): ${report['scroll×1'].threads.map((t) => `${t.what} ${t.ms}ms/${t.n}`).join('; ')}`);
  console.log(`  replay hottest (self time): ${report.replay.hot.slice(0, 8).map((h) => `${h.what} ${h.ms}ms`).join('; ')}`);
  for (const rate of THROTTLES) {
    console.log(`  scroll×${rate} hottest (self time): ${report[`scroll×${rate}`].hot.slice(0, 8).map((h) => `${h.what} ${h.ms}ms`).join('; ')}`);
  }
}

const outDir = mkdtempSync(path.join(tmpdir(), 'exercise-perf-'));
await build({ root, logLevel: 'warn', mode: 'production', build: { outDir, emptyOutDir: true, minify: false, sourcemap: false } });
const server = await preview({ root, logLevel: 'warn', preview: { port: 4317, strictPort: false }, build: { outDir } });
const url = `http://localhost:${server.httpServer.address().port}/`;
const browser = await chromium.launch();
const reports = [];
try {
  for (const name of SESSIONS) {
    const report = await run(browser, url, name);
    reports.push(report);
    print(report);
  }
} finally {
  await browser.close();
  await new Promise((done) => server.httpServer.close(done));
}
if (OUT) writeFileSync(OUT, JSON.stringify({ viewport: VIEWPORT, replaySpeed: REPLAY_SPEED, reports }, null, 1));
