window.__errs = [];
window.addEventListener("error", (e) => {
  if (e.target && e.target !== window && e.target.src) window.__errs.push("uncaught: failed to load script " + e.target.src.replace(location.origin, ""));
  else window.__errs.push("uncaught: " + String((e.error && e.error.message) || e.message).slice(0, 300));
}, true);
window.addEventListener("unhandledrejection", (e) => window.__errs.push("unhandled rejection: " + String((e.reason && e.reason.message) || e.reason).slice(0, 300)));
const __ce = console.error.bind(console);
console.error = (...a) => { try { window.__errs.push("console.error: " + a.map((x) => (x && x.message) || String(x)).join(" ").slice(0, 300)); } catch (e) {} __ce(...a); };
const __cw = console.warn.bind(console);
console.warn = (...a) => { try { (window.__warns = window.__warns || []).push(a.map((x) => (x && x.message) || String(x)).join(" ").slice(0, 300)); } catch (e) {} __cw(...a); };
// The GRADER-only lifecycle recorder. The vite plugin's wrappers call into this; it holds no references the page can observe except `window.__LT`.
window.__LT = {
  ev: [], t0: performance.now(), meshes: [], scenes: [], mats: [],
  push(ev, data) { this.ev.push(Object.assign({ ev, t: Math.round(performance.now() - this.t0) }, data)); },
  sid(s) { let i = this.scenes.indexOf(s); if (i < 0) { this.scenes.push(s); i = this.scenes.length - 1; } return i + 1; },
  matid(m) { let i = this.mats.indexOf(m); if (i < 0) { this.mats.push(m); i = this.mats.length - 1; } return i; },
  mid(m) { let i = this.meshes.indexOf(m); if (i < 0) { this.meshes.push(m); i = this.meshes.length - 1; } return i; },
};
