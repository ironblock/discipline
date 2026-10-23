"""The sealed session's Babylon Lite (#948): `python3 -I bundle.py ROOT OUT`, or `build(root, out)` from the `lite` service.

A sealed session's page still runs Lite, but from a built bundle instead of the source. The bundle is built from a `git archive` copy of
ROOT at its HEAD, never from the checkout itself:
  - the copy's trap internals (RENAMED, below) are renamed whole-word to `_r1`…`_r9`, refused if any is left or a new name collides;
  - Vite's library build with esbuild minify, names kept (the grader's dump names a material by its class), no sourcemap;
  - every chunk but the entry named by its hash alone, never by a module path;
  - whitespace minified too (Vite's ES library mode keeps it).
The copy is deleted as soon as the bundle is built (a stray source copy is what the seal probe's sweep looks for). OUT/dist/index.js is
the bundle; OUT/build.json says what was built. `check(dist, scene_core)` is the identifier fixture: the bundle names none of RENAMED, no
chunk names a module path, no comment line of scene-core.ts and no map is in it.
"""
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile

# The internals whose names state a trap's mechanism (late-add, late-family): renamed, so the bundle shows the API, not the mechanism.
RENAMED = ["_built", "_deferredBuilders", "_materialSwapQueue", "_prePasses", "_rebuildSingle", "enqueueMaterialSwap", "processMaterialSwaps",
           "_processSwaps", "_renderableVersion"]
SRC = os.path.join("packages", "babylon-lite", "src")
WORD = r"(?<![\w$])%s(?![\w$])"

CONFIG = """export default {
  root: %(pkg)s,
  cacheDir: %(cache)s,
  logLevel: "warn",
  esbuild: { keepNames: true },
  build: {
    lib: { entry: %(entry)s, formats: ["es"], fileName: "index" },
    outDir: %(dist)s,
    emptyOutDir: true,
    sourcemap: false,
    minify: "esbuild",
    rollupOptions: { output: { chunkFileNames: "[hash].js", entryFileNames: "index.js", assetFileNames: "[hash][extname]" } },
  },
};
"""

WHITESPACE = """
import { createRequire } from "module"; import { readdirSync, readFileSync, writeFileSync, realpathSync } from "fs";
const require = createRequire(realpathSync(process.argv[1]));
const esbuild = require("esbuild");
const dir = process.argv[2];
for (const f of readdirSync(dir).filter((f) => f.endsWith(".js"))) {
  const path = dir + "/" + f;
  writeFileSync(path, esbuild.transformSync(readFileSync(path, "utf8"), { loader: "js", minifyWhitespace: true }).code);
}
console.log(esbuild.version);
"""


def renamed(src):
    """Rename RENAMED whole-word in every file under `src`, in order, to `_r1`…: refused when a new name is already a word there, or any
    old one is left. -> {name: occurrences renamed}."""
    files = [os.path.join(d, f) for d, _, fs in os.walk(src) for f in fs if f.endswith((".ts", ".js", ".mjs"))]
    texts = {f: open(f, encoding="utf-8").read() for f in files}
    blob = "".join(texts.values())
    counts = {}
    for i, name in enumerate(RENAMED, 1):
        new = "_r%d" % i
        if re.search(WORD % re.escape(new), blob):
            raise SystemExit("bundle: %s is already a name in the source" % new)
        counts[name] = 0
        for f, text in texts.items():
            text, n = re.subn(WORD % re.escape(name), new, text)
            texts[f], counts[name] = text, counts[name] + n
    left = [n for n in RENAMED if any(re.search(WORD % re.escape(n), t) for t in texts.values())]
    if left:
        raise SystemExit("bundle: still named after the rename: %s" % left)
    for f, text in texts.items():
        open(f, "w", encoding="utf-8").write(text)
    return counts


def build(root, out):
    """Build the sealed bundle of the checkout at `root` into `out` (see the module's doc). -> what build.json says."""
    copy, dist = os.path.join(out, "copy"), os.path.join(out, "dist")
    for path in (copy, dist):
        if os.path.exists(path):
            shutil.rmtree(path)
    os.makedirs(copy)
    head = subprocess.run(["git", "-C", root, "rev-parse", "HEAD"], capture_output=True, text=True, check=True).stdout.strip()
    archive = subprocess.run(["git", "-C", root, "archive", "--format=tar", head], capture_output=True, check=True).stdout
    tar = os.path.join(out, "copy.tar")
    open(tar, "wb").write(archive)
    try:
        with tarfile.open(tar) as t:
            t.extractall(copy, filter="data")
        os.remove(tar)
        # The build reads the checkout's installed dependencies, never its source.
        for rel in ("node_modules", os.path.join("packages", "babylon-lite", "node_modules")):
            if os.path.isdir(os.path.join(root, rel)):
                os.symlink(os.path.join(root, rel), os.path.join(copy, rel))
        counts = renamed(os.path.join(copy, SRC))
        pkg = os.path.join(copy, "packages", "babylon-lite")
        config = os.path.join(out, "vite.bundle.mjs")
        q = json.dumps
        open(config, "w").write(CONFIG % {"pkg": q(pkg), "cache": q(os.path.join(out, "cache")), "entry": q(os.path.join(copy, SRC, "index.ts")),
                                          "dist": q(dist)})
        vite = os.path.join(root, "node_modules", ".bin", "vite")
        built = subprocess.run([vite, "build", "--config", config], cwd=pkg, capture_output=True, text=True)
        if built.returncode != 0:
            raise SystemExit("bundle: vite build failed: %s" % (built.stderr or built.stdout)[-2000:])
        esbuild = subprocess.run(["node", "--input-type=module", "-e", WHITESPACE, os.path.join(root, "node_modules", "vite", "package.json"), dist],
                                 capture_output=True, text=True)
        if esbuild.returncode != 0:
            raise SystemExit("bundle: the whitespace pass failed: %s" % esbuild.stderr[-2000:])
        scene_core = open(os.path.join(root, SRC, "scene", "scene-core.ts"), encoding="utf-8").read()
    finally:
        # No source copy outlives the build.
        shutil.rmtree(copy, ignore_errors=True)
        if os.path.exists(tar):
            os.remove(tar)
    found = check(dist, scene_core)
    if any(found.values()):
        raise SystemExit("bundle: the identifier fixture failed: %s" % found)
    said = {"commit": head, "renamed": counts, "esbuild": esbuild.stdout.strip(), "files": len(os.listdir(dist)),
            "index_sha256": hashlib.sha256(open(os.path.join(dist, "index.js"), "rb").read()).hexdigest(), "check": found}
    json.dump(said, open(os.path.join(out, "build.json"), "w"), indent=1, sort_keys=True)
    return said


def scan(blob, scene_core):
    """The fixture over any served code (#948): {names: {name: hits}, comments: [found], maps: [...]}, each empty when `blob` names no
    trap, holds no scene-core.ts comment line (over 30 characters) and points at no map."""
    names = {n: len(re.findall(WORD % re.escape(n), blob)) for n in RENAMED}
    comments = [l.strip() for l in scene_core.splitlines() if l.strip().startswith(("//", "*", "/**")) and len(l.strip()) > 30]
    return {
        "names": {n: c for n, c in names.items() if c},
        "comments": [c for c in comments if c in blob],
        "maps": ["sourceMappingURL"] if "sourceMappingURL" in blob else [],
    }


def check(dist, scene_core):
    """The identifier fixture over a built bundle: `scan` of its code, and {chunks: [path-named]} and its map files; every value empty
    when the bundle names no trap, no chunk path, no scene-core.ts comment line and no map."""
    files = sorted(os.listdir(dist))
    blob = "".join(open(os.path.join(dist, f), errors="replace").read() for f in files if f.endswith(".js"))
    found = scan(blob, scene_core)
    found["chunks"] = [f for f in files if f.endswith(".js") and f != "index.js" and not re.fullmatch(r"[A-Za-z0-9_-]{8}\.js", f)]
    found["maps"] = [f for f in files if f.endswith(".map")] + found["maps"]
    return found


def selftest():
    """The fixture's and the rename's proofs, both ways, without a build."""
    import tempfile
    scene_core = "// a comment line long enough to be counted by the check\nconst x = 1;\n"
    with tempfile.TemporaryDirectory() as d:
        dist = os.path.join(d, "dist")
        os.makedirs(dist)
        open(os.path.join(dist, "index.js"), "w").write("export const _r1=1;class Q{}")
        open(os.path.join(dist, "Ab3_x-9Z.js"), "w").write("export const y=2;")
        clean = check(dist, scene_core)
        assert not any(clean.values()), clean
        # Each leg catches its own leak.
        for name, text, leg in [("index.js", "const _materialSwapQueue=[];", "names"), ("scene-material-swap.js", "x", "chunks"),
                                ("index.js", "// a comment line long enough to be counted by the check", "comments"),
                                ("index.js", "//# sourceMappingURL=index.js.map", "maps"), ("index.js.map", "{}", "maps")]:
            path = os.path.join(dist, name)
            before = open(path).read() if os.path.exists(path) else None
            open(path, "a").write(text)
            assert check(dist, scene_core)[leg], (leg, check(dist, scene_core))
            if before is None:
                os.remove(path)
            else:
                open(path, "w").write(before)
        assert not any(check(dist, scene_core).values())
        src = os.path.join(d, "src")
        os.makedirs(src)
        open(os.path.join(src, "a.ts"), "w").write("q._rebuildSingle(); enqueueMaterialSwap(c, m); x._rebuildSingleton; y.$_built;\n")
        counts = renamed(src)
        text = open(os.path.join(src, "a.ts")).read()
        assert counts["_rebuildSingle"] == 1 and counts["enqueueMaterialSwap"] == 1, counts
        assert "q._r5();" in text and "_r6(c, m)" in text and "x._rebuildSingleton" in text and "y.$_built" in text, text
        open(os.path.join(src, "b.ts"), "w").write("const _r1 = 0;\n")
        try:
            renamed(src)
        except SystemExit as refused:
            assert "_r1 is already a name" in str(refused), refused
        else:
            raise AssertionError("a colliding new name is refused")
    print("bundle selftest ok")


if __name__ == "__main__":
    if sys.argv[1:] == ["--selftest"]:
        selftest()
    elif len(sys.argv) == 3:
        print(json.dumps(build(*sys.argv[1:]), indent=1, sort_keys=True))
    else:
        sys.exit("usage: bundle.py ROOT OUT | --selftest")
