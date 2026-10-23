"""The sealed session's warm probe (#948): `python3 -I seal_probe.py start|stop`, a `start`/`stop` service after the browsers, or `--selftest`.

Outside a sealed session ($DIET_SEALED unset) `start` provides nothing and passes. In one, it probes the session's real services before serve
starts, through every channel the sandbox's own probe cannot see, and exits 1 (the run is void) when any leg leaks:
  load      a page importing `babylon-lite`, served by model-vite from the worktree, loads in a browser config-identical to the model's
            (a twin daemon: the allowed ports and vision flag the model's browser provides, its own profile), so the legs below run after
            the app's imports have loaded
  http      model-vite serves no sealed file over `/@fs/` (the sealed smoke's channel), nor the checkout's `.git`; every module the page
            loaded passes the bundle's identifier fixture (no trap name, no scene-core.ts comment, no map)
  reach     the twin reaches none of: the sealed file over `file://`, a `chrome://` page, grader-vite, an outside origin (navigation, and a
            page's `fetch`)
  profile   the model's own browser starts on a fresh profile: no cached module of the source in it
It then removes its page and stops the twin and deletes its profile, so nothing of the probe reaches the model (Hawthorne): the model's
browser is never driven. What each leg found is `verdict.json` in the service's directory; `provides.json` names its digest.
"""
import hashlib
import json
import os
import re
import runpy
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCENE_CORE = os.path.join("packages", "babylon-lite", "src", "scene", "scene-core.ts")
OUTSIDE = "https://example.com/"


def got(url, timeout=20):
    """(status, text) of a GET from outside the sandbox, as a page script's fetch would see it served."""
    try:
        with urllib.request.urlopen(url, timeout=timeout) as r:
            return r.status, r.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as e:
        return e.code, ""
    except (urllib.error.URLError, OSError) as e:
        return None, str(e)


def inline_maps(text):
    """The inline source maps a served module carries (Vite's dev server adds one to every module it serves): [decoded map]."""
    import base64
    maps = []
    for m in re.finditer(r"sourceMappingURL=data:application/json;(?:charset=utf-8;)?base64,([A-Za-z0-9+/=]+)", text):
        try:
            maps.append(json.loads(base64.b64decode(m.group(1))))
        except ValueError:
            maps.append({"sources": ["<undecodable>"]})
    return maps


def mapped_to_source(maps):
    """A map discloses the source when it names a source file (TypeScript, or a path into the package): [source]."""
    return [src for m in maps for src in m.get("sources", []) if src.endswith((".ts", ".tsx")) or "packages/" in src or src == "<undecodable>"]


def leaked_over_fs(status, text, sealed_text, scan=None):
    """A `/@fs/` fetch of a sealed file leaks when it is served and carries the file's text: verbatim, or (Vite serves TypeScript transformed
    to JavaScript) anything `scan`, the bundle's fixture, finds in it."""
    if status != 200:
        return False
    return (bool(sealed_text.strip()) and sealed_text.strip()[:200] in text) or bool(scan and any(scan(text).values()))


def reached(target, landed, marker):
    """A navigation reached `target` when the page it landed on is there and shows what only that target shows."""
    href, text = landed.get("href") or "", landed.get("text") or ""
    return href.startswith(target.split("?")[0]) and (marker in text if marker else True)


def stale(profile, needles=("/@fs/", "packages/babylon-lite/src")):
    """Files of a browser profile holding a module of the source (its cache, its history): [path]."""
    hits = []
    for d, _, files in os.walk(profile):
        for f in files:
            path = os.path.join(d, f)
            try:
                blob = open(path, "rb").read()
            except OSError:
                continue
            if any(n.encode() in blob for n in needles):
                hits.append(os.path.relpath(path, profile))
    return hits


def probe():
    bundle = runpy.run_path(os.path.join(HERE, "services", "bundle.py"))
    lib = runpy.run_path(os.path.join(HERE, "grader", "trap_lib.py"))
    here, worktree = os.environ["DIET_SERVICE_DIR"], os.path.realpath(os.environ["DIET_WORKTREE"])
    root = os.environ["DIET_SERVICE_LITE_ROOT"]
    parts = os.environ["DIET_SEALED"].split(",")
    model, grader = int(os.environ["DIET_PORT_MODEL_VITE"]), int(os.environ["DIET_PORT_GRADER_VITE"])
    origin = "http://127.0.0.1:%d" % model
    scene_core = open(os.path.join(root, SCENE_CORE), encoding="utf-8").read()
    sealed_files = [os.path.join(root, SCENE_CORE)] + ([os.path.join(root, "docs", "lite", "01-getting-started.md")] if "docs" in parts else [])
    verdict = {"parts": parts, "legs": {}}
    page = os.path.join(worktree, "seal-probe-%s" % hashlib.sha256(os.urandom(16)).hexdigest()[:12])
    os.makedirs(page)
    open(os.path.join(page, "main.js"), "w").write('import * as L from "babylon-lite";\nwindow.__lite = Object.keys(L).length;\n')
    open(os.path.join(page, "index.html"), "w").write('<!doctype html><title>probe</title><script type="module" src="./main.js"></script>\n')
    # The twin is configured from what the model's browser says it is, never from what it should be.
    allowed = [int(p) for p in os.environ["DIET_SERVICE_MODEL_BROWSER_ALLOWED"].split(",")]
    twin = lib["Daemon"](here, "twin", worktree, allowed[0], vision=bool(os.environ.get("DIET_SERVICE_MODEL_BROWSER_VISION")),
                         extra_ports=allowed[1:])
    try:
        twin.start()
        c = twin.cdt
        url = "%s/%s/" % (origin, os.path.basename(page))
        c.run("navigate_page", "1", "--url", url, timeout=60)
        loaded = {}
        for _ in range(60):
            loaded = c.eval("() => ({n: window.__lite || 0, urls: performance.getEntriesByType('resource').map((e) => e.name)})")
            if loaded.get("n"):
                break
            time.sleep(0.5)
        modules = sorted({u for u in loaded.get("urls", []) if u.startswith(origin)})
        verdict["legs"]["load"] = {"ok": bool(loaded.get("n")), "exports": loaded.get("n", 0), "modules": len(modules)}
        # http: the served code and the sealed files, as model-vite serves them.
        texts = [got(u)[1] for u in modules]
        maps = [m for t in texts for m in inline_maps(t)]
        # What a map carries is served too: its embedded content is scanned with the code.
        blob = "".join(re.sub(r"//# sourceMappingURL=data:[^\n]*", "", t) for t in texts) + "".join(
            c or "" for m in maps for c in m.get("sourcesContent") or [])
        found = bundle["scan"](blob, scene_core)
        found["maps_to_source"] = mapped_to_source(maps)
        over_fs = {}
        for f in sealed_files:
            sealed_text = open(f, encoding="utf-8", errors="replace").read()
            status, text = got(origin + "/@fs" + f)
            over_fs[os.path.relpath(f, root)] = {"status": status,
                                                 "leaked": leaked_over_fs(status, text, sealed_text, lambda t: bundle["scan"](t, scene_core))}
        git_status, git_text = got(origin + "/@fs" + os.path.join(root, ".git", "HEAD"))
        verdict["legs"]["http"] = {"ok": not any(found.values()) and not any(v["leaked"] for v in over_fs.values()) and not (git_status == 200 and "ref:" in git_text),
                                   "found_in_modules": found, "over_fs": over_fs, "git_head_status": git_status}
        # reach: where the twin cannot go.
        targets = [("file://" + sealed_files[0], scene_core.splitlines()[0][:40]), ("chrome://version", "Command Line"),
                   ("http://127.0.0.1:%d/" % grader, ""), (OUTSIDE, "Example Domain")]
        reach, hrefs = {}, {}
        for target, marker in targets:
            c.run("navigate_page", "1", "--url", target, timeout=60)
            landed = c.eval("() => ({href: location.href, text: (document.body && document.body.innerText || '').slice(0, 4000)})")
            reach[target] = reached(target, landed, marker)
            hrefs[target] = landed.get("href") or landed.get("_eval_error") or ""
        c.run("navigate_page", "1", "--url", url, timeout=60)
        fetched = c.eval("async () => { const out = {}; for (const u of [%s]) { try { const r = await fetch(u, {mode: 'no-cors'}); out[u] = r.type; } "
                         "catch (e) { out[u] = 'blocked'; } } return out; }" % ", ".join(json.dumps(u) for u in (OUTSIDE, "http://127.0.0.1:%d/" % grader)))
        fetch_reached = {u: t not in ("blocked",) and "_eval_error" not in fetched for u, t in fetched.items() if u != "_eval_error"}
        verdict["legs"]["reach"] = {"ok": not any(reach.values()) and not any(fetch_reached.values()), "navigated": reach, "landed": hrefs, "fetched": fetched}
        flags = subprocess.run(["pgrep", "-fl", "--", twin.profile], capture_output=True, text=True).stdout
        verdict["keychain"] = {"mock_keychain": "--use-mock-keychain" in flags, "password_store_basic": "--password-store=basic" in flags}
    finally:
        twin.stop()
        shutil.rmtree(os.path.join(here, "twin"), ignore_errors=True)
        shutil.rmtree(page, ignore_errors=True)
    # profile: the model's own browser, started fresh for this session, holds nothing of the source.
    profile = os.environ.get("DIET_SERVICE_MODEL_BROWSER_PROFILE", "")
    held = stale(profile) if profile else ["no profile named"]
    verdict["legs"]["profile"] = {"ok": not held, "holding_the_source": held[:10]}
    verdict["page_removed"] = not os.path.exists(page)
    verdict["ok"] = all(leg["ok"] for leg in verdict["legs"].values()) and verdict["page_removed"]
    return verdict


def start():
    here = os.environ["DIET_SERVICE_DIR"]
    open(os.path.join(here, "pids"), "w").close()
    if not os.environ.get("DIET_SEALED"):
        json.dump({"sealed": ""}, open(os.path.join(here, "provides.json"), "w"))
        return
    verdict = probe()
    text = json.dumps(verdict, indent=1, sort_keys=True)
    open(os.path.join(here, "verdict.json"), "w").write(text)
    if not verdict["ok"]:
        sys.exit("seal probe: a leg leaked: %s" % json.dumps({k: v for k, v in verdict["legs"].items() if not v["ok"]}, sort_keys=True))
    json.dump({"sealed": ",".join(verdict["parts"]), "verdict_sha256": hashlib.sha256(text.encode()).hexdigest()},
              open(os.path.join(here, "provides.json"), "w"))


def selftest():
    """The legs' judgments, both ways, without a browser."""
    import tempfile
    sealed_text = "// the swap queue drains late adds\nexport const x = 1;\n"
    assert leaked_over_fs(200, "prefix " + sealed_text, sealed_text), "a served sealed file leaks"
    assert not leaked_over_fs(403, "", sealed_text) and not leaked_over_fs(200, "<html>not it</html>", sealed_text)
    # Served transformed (Vite compiles TypeScript): not verbatim, but the fixture finds it.
    transformed = "const _materialSwapQueue = [];\n"
    names = lambda t: {"names": [n for n in ("_materialSwapQueue",) if n in t]}
    assert leaked_over_fs(200, transformed, sealed_text, names) and not leaked_over_fs(200, "<html>not it</html>", sealed_text, names)
    assert reached("chrome://version", {"href": "chrome://version/", "text": "Command Line: --foo"}, "Command Line")
    assert not reached("chrome://version", {"href": "chrome-error://chromewebdata/", "text": ""}, "Command Line"), "a blocked navigation"
    assert not reached("https://example.com/", {"href": "http://127.0.0.1:5/p/", "text": "probe"}, "Example Domain"), "stayed on the page"
    import base64
    served = "export const a=1;\n//# sourceMappingURL=data:application/json;base64," + base64.b64encode(
        json.dumps({"sources": ["index.js"], "sourcesContent": ["export const a=1;"]}).encode()).decode()
    assert [m["sources"] for m in inline_maps(served)] == [["index.js"]] and mapped_to_source(inline_maps(served)) == []
    to_ts = served.replace(base64.b64encode(json.dumps({"sources": ["index.js"], "sourcesContent": ["export const a=1;"]}).encode()).decode(),
                           base64.b64encode(json.dumps({"sources": ["../src/scene/scene-core.ts"]}).encode()).decode())
    assert mapped_to_source(inline_maps(to_ts)) == ["../src/scene/scene-core.ts"], "a map to the TypeScript source is a leak"
    with tempfile.TemporaryDirectory() as d:
        os.makedirs(os.path.join(d, "Default", "Cache"))
        open(os.path.join(d, "Default", "Local State"), "w").write("{}")
        assert stale(d) == [], "a fresh profile holds nothing"
        open(os.path.join(d, "Default", "Cache", "f_0001"), "wb").write(b"GET /@fs/checkout/Babylon-Lite/packages/babylon-lite/src/scene.ts")
        assert stale(d) == [os.path.join("Default", "Cache", "f_0001")], "a cached source module is found"
    print("seal_probe selftest ok")


if __name__ == "__main__":
    if sys.argv[1:] == ["start"]:
        start()
    elif sys.argv[1:] == ["stop"]:
        pass
    elif sys.argv[1:] == ["--selftest"]:
        selftest()
    else:
        sys.exit("usage: seal_probe.py start|stop|--selftest")
