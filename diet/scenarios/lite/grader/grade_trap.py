"""Grading machinery for the Babylon Lite trap tasks (python3 -I; standard library only). Loaded with runpy by verify_trap.py (and by the future driver).
  Session   one grader page: navigate with the lifecycle recorder, dump scene state, take screenshots, send real clicks/keys; keeps an `obs` dict
  helpers   colour classes, centroids, analytic camera projection, trace queries (`evs`, `late_add_checks`, ...)
  grade     run a task's scenario, then its feature (tier a) and constraint (tier b) check functions -> {a, b, obs}
A check is (label, ok, expected, got). A tier is right when every check in it is ok. The page throwing an uncaught error fails tier a.
Expected values are written by hand from the task text (never copied from engine output); pixel positions come from `project()`, a hand-written LH look-at +
perspective (vertical fov) camera model, not from the engine."""
import json, math, os, time

TOL = 1e-3


# ------------------------------------------------------------------ small pure helpers (unit-tested in selftest_trap.py)
def near(a, b, tol=TOL):
    return a is not None and b is not None and len(a) == len(b) and all(isinstance(x, (int, float)) and abs(x - y) <= tol for x, y in zip(a, b))


def fmt(v):
    if v is None:
        return "none"
    if isinstance(v, (list, tuple)):
        return "(" + ", ".join(("%.4g" % x) if isinstance(x, (int, float)) and not isinstance(x, bool) else ("none" if x is None else str(x)) for x in v) + ")"
    if isinstance(v, float):
        return "%.4g" % v
    return str(v)


def cls(px):
    """Colour class of an (r, g, b) pixel on the default dark-blue clear colour (51, 51, 76). A lit side face of a pure colour is half as bright as its top face, so the
    thresholds are low (110) but need a clear dominance gap: one channel (or two, for yellow/cyan/magenta) well above the rest."""
    r, g, b = px[0], px[1], px[2]
    if r > 110 and 45 < g < r - 45 and b < 80 and g > b + 25:
        return "orange"
    if r > 110 and g < r - 60 and b < r - 60:
        return "red"
    if g > 110 and r < g - 60 and b < g - 60:
        return "green"
    if b > 110 and r < b - 60 and g < b - 40:
        return "blue"
    if r > 110 and g > 110 and b < min(r, g) - 60:
        return "yellow"
    if r > 110 and b > 110 and g < min(r, b) - 60:
        return "magenta"
    if g > 110 and b > 110 and r < min(g, b) - 60:
        return "cyan"
    if r > 200 and g > 200 and b > 200:
        return "white"
    return None


def class_count(img, name, x0=0, y0=0, x1=None, y1=None, step=2):
    x1, y1 = x1 or img.w, y1 or img.h
    return sum(1 for y in range(int(y0), int(y1), step) for x in range(int(x0), int(x1), step) if cls(img.px(x, y)) == name) * step * step


def class_centroid(img, name, x0=0, y0=0, x1=None, y1=None, step=2):
    x1, y1 = x1 or img.w, y1 or img.h
    xs = ys = n = 0
    for y in range(int(y0), int(y1), step):
        for x in range(int(x0), int(x1), step):
            if cls(img.px(x, y)) == name:
                xs += x
                ys += y
                n += 1
    return (xs / n, ys / n, n * step * step) if n else None


def luma(px):
    return 0.299 * px[0] + 0.587 * px[1] + 0.114 * px[2]


def region_luma(img, cx, cy, half=6):
    p = img.region(cx - half, cy - half, cx + half + 1, cy + half + 1)
    return sum(luma(q) for q in p) / max(1, len(p))


def project(cam, p, w, h):
    """World point -> CSS pixel for a Lite free/arc camera dump {pos, target, fov} (left-handed look-at, vertical fov). cam['pos'] for an arc camera is derived by the caller."""
    pos, tgt = cam["pos"], cam["target"]
    f = [tgt[i] - pos[i] for i in range(3)]
    fl = math.sqrt(sum(c * c for c in f))
    f = [c / fl for c in f]
    up = (0, 1, 0)
    r = [up[1] * f[2] - up[2] * f[1], up[2] * f[0] - up[0] * f[2], up[0] * f[1] - up[1] * f[0]]      # up x f  (left-handed)
    rl = math.sqrt(sum(c * c for c in r))
    r = [c / rl for c in r]
    u = [f[1] * r[2] - f[2] * r[1], f[2] * r[0] - f[0] * r[2], f[0] * r[1] - f[1] * r[0]]            # f x r
    d = [p[i] - pos[i] for i in range(3)]
    x, y, z = sum(r[i] * d[i] for i in range(3)), sum(u[i] * d[i] for i in range(3)), sum(f[i] * d[i] for i in range(3))
    t = math.tan(cam["fov"] / 2)
    nx, ny = x / (z * t * (w / h)), y / (z * t)
    return ((nx + 1) / 2 * w, (1 - ny) / 2 * h, z)


def unproject_to_plane_y(cam, px, py, w, h, plane_y=0.0):
    """CSS pixel -> world point where the camera ray meets the plane y = plane_y."""
    pos, tgt = cam["pos"], cam["target"]
    f = [tgt[i] - pos[i] for i in range(3)]
    fl = math.sqrt(sum(c * c for c in f))
    f = [c / fl for c in f]
    up = (0, 1, 0)
    r = [up[1] * f[2] - up[2] * f[1], up[2] * f[0] - up[0] * f[2], up[0] * f[1] - up[1] * f[0]]
    rl = math.sqrt(sum(c * c for c in r))
    r = [c / rl for c in r]
    u = [f[1] * r[2] - f[2] * r[1], f[2] * r[0] - f[0] * r[2], f[0] * r[1] - f[1] * r[0]]
    t = math.tan(cam["fov"] / 2)
    nx, ny = (px / w) * 2 - 1, 1 - (py / h) * 2
    d = [f[i] + r[i] * nx * t * (w / h) + u[i] * ny * t for i in range(3)]
    s = (plane_y - pos[1]) / d[1]
    return [pos[i] + d[i] * s for i in range(3)]


def pixel_ray(cam, px, py, w, h):
    """CSS pixel -> (origin, unit direction) of the camera ray, same model as project()."""
    pos, tgt = cam["pos"], cam["target"]
    f = [tgt[i] - pos[i] for i in range(3)]
    fl = math.sqrt(sum(c * c for c in f))
    f = [c / fl for c in f]
    up = (0, 1, 0)
    r = [up[1] * f[2] - up[2] * f[1], up[2] * f[0] - up[0] * f[2], up[0] * f[1] - up[1] * f[0]]
    rl = math.sqrt(sum(c * c for c in r))
    r = [c / rl for c in r]
    u = [f[1] * r[2] - f[2] * r[1], f[2] * r[0] - f[0] * r[2], f[0] * r[1] - f[1] * r[0]]
    t = math.tan(cam["fov"] / 2)
    nx, ny = (px / w) * 2 - 1, 1 - (py / h) * 2
    d = [f[i] + r[i] * nx * t * (w / h) + u[i] * ny * t for i in range(3)]
    dl = math.sqrt(sum(c * c for c in d))
    return list(pos), [c / dl for c in d]


def ray_sphere(o, d, c, rad):
    """Nearest intersection of the ray with a sphere, or None."""
    oc = [o[i] - c[i] for i in range(3)]
    b = sum(oc[i] * d[i] for i in range(3))
    cc = sum(x * x for x in oc) - rad * rad
    disc = b * b - cc
    if disc < 0:
        return None
    t = -b - math.sqrt(disc)
    if t < 0:
        return None
    return [o[i] + d[i] * t for i in range(3)]


def arc_to_cam(c):
    """Arc-rotate dump {alpha, beta, radius, target, fov} -> {pos, target, fov} (Babylon convention: x = r cos(a) sin(b), y = r cos(b), z = r sin(a) sin(b))."""
    a, b, r, t = c["alpha"], c["beta"], c["radius"], c["target"]
    return {"pos": [t[0] + r * math.cos(a) * math.sin(b), t[1] + r * math.cos(b), t[2] + r * math.sin(a) * math.sin(b)], "target": t, "fov": c["fov"]}


# ------------------------------------------------------------------ trace queries (the grader-injected lifecycle recorder)
def evs(d, name=None, **kw):
    out = []
    for e in ((d or {}).get("lt") or {}).get("ev", []):
        if (name is None or e["ev"] == name) and all(e.get(k) == v for k, v in kw.items()):
            out.append(e)
    return out


def final_names(d):
    return {m["i"]: m.get("name") for m in ((d or {}).get("meshState") or [])}


def mesh_evs(d, ev, name, **kw):
    """Events about the mesh whose FINAL name is `name` (a model may name a mesh after it adds it, or after it sets its instances): resolved through the recorder's mesh index,
    falling back to the name at call time when the event has no index."""
    fn = final_names(d)
    out = []
    for e in evs(d, ev, **kw):
        mi = e.get("mi")
        nm = fn.get(mi) if (mi is not None and mi in fn) else e.get("name")
        if nm == name:
            out.append(e)
    return out


def start_done_t(d):
    e = evs(d, "startEngine.done")
    return e[0]["t"] if e else None


def lifecycle_checks(d, scenes=1):
    """The scene was created once, registered once, built once, and never disposed, unregistered or rebuilt; one engine; the engine never disposed."""
    return [
        ("one scene context was created (the scene was not rebuilt)", len(evs(d, "createScene")) == scenes, str(scenes), str(len(evs(d, "createScene")))),
        ("registerScene ran once, and not on an already-registered scene", len(evs(d, "registerScene")) == scenes and not any(e["already"] for e in evs(d, "registerScene")),
         "%d call(s)" % scenes, "%d call(s)" % len(evs(d, "registerScene"))),
        ("the deferred build ran once", len(evs(d, "buildScene")) == scenes, str(scenes), str(len(evs(d, "buildScene")))),
        ("the scene was never disposed or unregistered", not evs(d, "disposeScene") and not evs(d, "unregisterScene"), "no dispose/unregister", "%d dispose, %d unregister" % (len(evs(d, "disposeScene")), len(evs(d, "unregisterScene")))),
        ("one engine, never disposed", len(evs(d, "createEngine")) == 1 and not evs(d, "disposeEngine"), "1 engine", "%d created, %d disposed" % (len(evs(d, "createEngine")), len(evs(d, "disposeEngine")))),
    ]


def late_add_checks(d, name, not_before_t=None, drain=True):
    """The named mesh was added AFTER registration (scene already built and registered when addToScene ran), at or after `not_before_t` (page-clock ms), and the
    per-frame material-swap drain gave it its own renderable. `drain=False` skips the renderable check (for a mesh whose material family was never built)."""
    adds = mesh_evs(d, "add", name, kind="mesh")
    out = [("mesh `%s` was added through addToScene" % name, len(adds) >= 1, ">= 1 add", "%d add(s)" % len(adds))]
    if not adds:
        return out + [("... after the scene was registered and built", False, "built == true at add time", "never added"), ("... and rendered through the swap drain", False, "a drain gave it a renderable", "never added")]
    a = adds[-1]
    out.append(("... after the scene was registered and built (not added before registerScene)", bool(a["built"]) and bool(a["registered"]), "built and registered at add time", "built=%s registered=%s" % (a["built"], a["registered"])))
    if not_before_t is not None:
        out.append(("... at the trigger, not before it", a["t"] >= not_before_t, "t >= %d ms" % not_before_t, "t = %d ms" % a["t"]))
    if drain:
        drains = [e for e in evs(d, "drain") if a.get("mi") in e["queued"]]
        owned = any(e["own"][e["queued"].index(a["mi"])] >= 1 for e in drains)
        out.append(("... and rendered through the swap drain (the drain built it a renderable)", owned, "drain with own renderable", "%d drain(s), own=%s" % (len(drains), [e["own"] for e in drains])))
    return out


def page_ok(d):
    errs = [e for e in (d or {}).get("errs", []) if e.startswith("uncaught") or e.startswith("unhandled")]
    return ("the page threw no uncaught error", not errs, "none", "; ".join(errs[:2]) or "none")


# ------------------------------------------------------------------ the grader page
class NoScene(Exception):
    pass


class Session:
    def __init__(self, cdt, port, ws, dump_js, init_js, task, page=1):
        self.cdt, self.port, self.ws, self.dump_js, self.init_js, self.task, self.page = cdt, port, ws, dump_js, init_js, task, page
        self.obs = {"dumps": {}, "shots": {}, "clicks": {}, "evals": {}, "notes": []}
        self.Png = None
        self.last = None

    # -- low level
    def dump(self, tries=3):
        """Dump the scene; a transient evaluate error (page navigating, daemon busy) is retried, never mistaken for a scene."""
        for k in range(tries):
            d = self.cdt.eval(self.dump_js, self.page)
            if "_eval_error" not in d:
                break
            self.obs.setdefault("notes", []).append("dump retry %d: %s" % (k + 1, str(d["_eval_error"])[:120]))
            time.sleep(0.6)
        self.last = d
        return d

    def now(self):
        d = self.cdt.eval("() => window.__LT ? Math.round(performance.now() - window.__LT.t0) : -1", self.page)
        return d if isinstance(d, int) else -1

    def shot(self, label, retries=2):
        path = os.path.join(self.ws, "%s-%s-%d.png" % (self.task["id"], label, int(time.time() * 1000) % 100000000))
        for k in range(retries + 1):
            self.cdt.run("take_screenshot", self.page, "--filePath", path)
            img = self.Png(path)
            sample = {img.px(x, y)[:3] for x in range(0, img.w, 97) for y in range(0, img.h, 89)}
            if len(sample) > 1 or k == retries:
                break
            time.sleep(1.0)
        self.obs["shots"][label] = img
        self.obs.setdefault("shot_paths", {})[label] = path
        return img

    def snap(self, label):
        """Screenshot first (so a timed task is looked at as early as possible), then dump; the page clock at the screenshot is kept in obs['shot_t']."""
        self.obs.setdefault("shot_t", {})[label] = self.now()
        img = self.shot(label)
        d = self.dump()
        self.obs["dumps"][label] = d
        return d, img

    def snap_dump(self, label):
        d = self.dump()
        self.obs["dumps"][label] = d
        return d

    def wait(self, sec):
        time.sleep(sec)

    def click(self, x, y, label):
        t = self.now()
        rc, out, err = self.cdt.run("click_at", self.page, x, y)
        self.obs["clicks"][label] = {"t": t, "rc": rc, "out": out[:120], "x": x, "y": y}
        return t

    def key(self, k):
        return self.cdt.run("press_key", self.page, k)

    def eval(self, label, js):
        v = self.cdt.eval(js, self.page)
        self.obs["evals"][label] = v
        return v

    def until_start_plus(self, sec, cap=40):
        """Sleep until the page clock is `sec` seconds past the startEngine resolution."""
        end = time.monotonic() + cap
        while time.monotonic() < end:
            d = self.cdt.eval("() => { const L = window.__LT; if (!L) return null; const e = L.ev.find(x => x.ev === 'startEngine.done'); return e ? [e.t, Math.round(performance.now() - L.t0)] : null; }", self.page)
            if isinstance(d, list) and d[1] - d[0] >= sec * 1000:
                return d
            time.sleep(0.25)
        return None

    # -- navigation
    def load(self, viewport=None, wait=12):
        t = self.task
        vp = viewport or t.get("viewport", "800x600x1")
        self.cdt.run("emulate", self.page, "--viewport", vp)
        url = "http://localhost:%d/%s/" % (self.port, t["id"])
        self.obs["navigate"] = self.cdt.json("navigate_page", self.page, "--url", url, "--ignoreCache", "true", "--initScript", self.init_js, "--timeout", 60000, timeout=90)
        end, d = time.monotonic() + wait, {"noScene": True}
        time.sleep(0.3)
        while time.monotonic() < end:
            v = self.cdt.eval("() => !!window.scene", self.page)
            if v is True or (isinstance(v, dict) and "_eval_error" in v and "navigat" not in str(v).lower()):
                break
            time.sleep(0.12)
        d = self.dump()
        self.obs["load"] = d
        self.obs["loaded"] = not d.get("noScene") and "_eval_error" not in d
        if not self.obs["loaded"]:
            raise NoScene()
        return d


def tier(cs):
    return {"ok": all(c[1] for c in cs), "checks": [{"label": x[0], "ok": bool(x[1]), "expected": x[2], "got": x[3]} for x in cs]}


def grade(task, session, Png):
    session.Png = Png
    try:
        task["scenario"](session)
    except NoScene:
        pass
    except Exception as e:      # noqa  (a grader bug must show, not pass)
        session.obs["scenario_error"] = "%s: %s" % (type(e).__name__, e)
    obs = session.obs
    final = obs["dumps"].get("final") or session.last or obs.get("load") or {}
    obs["final"] = final
    if "scenario_error" in obs:
        bad = [("the grader scenario ran to its end", False, "no error", obs["scenario_error"][:200])]
        return {"a": tier(bad), "b": tier(bad), "obs": obs}
    if not obs.get("loaded"):
        errs = (obs.get("load") or {}).get("errs", [])
        why = "`window.scene` was never set (page errors: %s)" % ("; ".join(errs[:3]) or "no error was reported")
        return {"a": tier([("the page set window.scene", False, "window.scene set", why)]), "b": tier([("the lifecycle was recorded", False, "a scene", why)]), "obs": obs}
    try:
        a = [page_ok(final)] + task["feature"](obs)
    except Exception as e:      # noqa  (a grader bug must show as a failed check, never crash the run or pass)
        a = [("the tier a checks ran", False, "no error", "%s: %s" % (type(e).__name__, e))]
    try:
        b = task["constraint"](obs)
    except Exception as e:      # noqa
        b = [("the tier b checks ran", False, "no error", "%s: %s" % (type(e).__name__, e))]
    return {"a": tier(a), "b": tier(b), "obs": obs}


# What the model is shown is what was expected and what was seen, never why (ruled 2026-10-21, #942): a cause in the feedback hands the model the fact
# the tutorial lets it discover and the interviews capture. Tier a's checks are observations of the page; tier b's name the mechanism, so a tier b
# failure is shown only as the task's own stated constraint, unmet, and only once the page does what it should: before that, tier b says nothing
# the model can use. Internal names a dump carries are shown as the words the task uses.
SHOWN_AS = (("standardGroupBuilder", "standard"), ("pbrGroupBuilder", "PBR"))
CONSTRAINT_UNMET = "the page does not do what the task's constraint says it must (re-read the task)"


def shown(text):
    text = str(text)
    for internal, word in SHOWN_AS:
        text = text.replace(internal, word)
    return text


def feedback(res):
    """What the model is told: right, or the failing observations, expected against seen; with none, the stated constraint unmet. Never a cause."""
    if res["a"]["ok"] and res["b"]["ok"]:
        return "Graded: right."
    seen = ["%s: expected %s, saw %s" % (shown(c["label"]), shown(c["expected"]), shown(c["got"])) for c in res["a"]["checks"] if not c["ok"]]
    if not seen:
        seen.append(CONSTRAINT_UNMET)
    return "Graded: wrong. " + "; ".join(seen)[:1500]


def diagnosis(res):
    """Every check of both tiers, as graded: for the grade's output directory, never sent to the model."""
    return {"a": res["a"], "b": res["b"]}
