"""Tier (c), the PROCESS tier: DEFINED here, computed later from a session log (python3 -I; standard library only).
A session is cheap or expensive in tokens, commands and source reads. For each trap fact this file says which reads count as the model RE-DERIVING that fact.

DEFINITION
  A "read" is one shell tool call (the session log's `bash` calls) that uses a reading verb (cat, sed, head, tail, grep, egrep, rg, find, ls, awk, nl, less, more, wc, xargs, sort, bat, tree)
  and names the Lite checkout: the path the run's `lite` service checked at start (its provides.json `root`), else $LITE_REPO, else
  ~/git/reference/Babylon/Babylon-Lite.
  A read TARGETS a fact when either
    (path)    a path it names matches one of the fact's `files` (prefix match under the checkout: a directory entry matches everything inside it), or
    (keyword) it searches the checkout (grep/rg/find) for one of the fact's `keywords` (a regex alternative, whole word), or
    (lab)     it names a lab scene listed for the fact in LAB_SCENES (lab scenes that call the fact's API; derived from the checkout by `lab_scenes()` below, so a copy of the
              pilot's finding "the checkout's 250+ lab scenes near-copy most small tasks" is counted, not hidden).
  A fact is RE-DERIVED in a session when at least one read before the session's LAST write to main.ts targets it. Reads of the OTHER facts' files do not count for it.
  Reported per (session, fact): re_derived (bool), n_reads, n_distinct_files, first_read_seq, and the commands, tokens and seconds from the first targeting read to the first
  right-graded attempt (the cost of the re-derivation). Reads that target no listed fact are "other reads" and are reported as a total only.
  Docs reads count: `docs/lite/...` entries are listed for the facts they describe. A read of a contradicting doc does not count as re-deriving unless a source file of the same fact was also read
  in the session (the contradiction is only resolved by the source); both are reported (`doc_only` vs `source_read`).
"""
import json, os, re, subprocess

# The checkout a run read, unless a caller names the one its `lite` service checked.
REF = os.environ.get("LITE_REPO") or os.path.join(os.path.expanduser("~"), "git", "reference", "Babylon", "Babylon-Lite")
SRC = "packages/babylon-lite/src/"
READ_VERBS = r"\b(cat|grep|egrep|rg|sed|head|tail|find|ls|awk|less|more|wc|nl|bat|tree|xargs|sort)\b"

FACTS = {
    "late-add": {"files": [SRC + "scene/scene-core.ts", SRC + "scene/mesh-scene-registry.ts", SRC + "scene/scene-material-swap.ts"],
                 "docs": ["docs/lite/01-getting-started.md", "docs/lite/architecture/01-scene.md"], "keywords": ["_built", "enqueueMaterialSwap", "processMaterialSwaps", "_materialSwapQueue", "after registerScene"]},
    "late-family": {"files": [SRC + "scene/scene-material-swap.ts", SRC + "scene/scene-core.ts", SRC + "material/standard/standard-material.ts", SRC + "material/pbr/pbr-material.ts", SRC + "render/renderable.ts"],
                    "docs": ["docs/lite/architecture/08-standard-material.md", "docs/lite/architecture/06-pbr-material.md", "docs/lite/architecture/27-render-pipeline.md"], "keywords": ["_rebuildSingle", "rebuildSingle", "_buildGroup", "_groups"]},
    "late-hide": {"files": [SRC + "scene/visibility.ts", SRC + "engine/engine.ts"], "docs": ["docs/lite/01-getting-started.md", "docs/lite/architecture/11-scene-hierarchy-parenting.md"], "keywords": ["setSubtreeVisible", "setMeshVisible", "bumpVisibilityEpoch", "visibility epoch"]},
    "late-remove": {"files": [SRC + "scene/scene-remove.ts", SRC + "mesh/mesh-dispose.ts", SRC + "scene/mesh-scene-registry.ts"], "docs": ["docs/lite/03-porting-guide.md"], "keywords": ["removeFromScene", "disposeMeshGpu"]},
    "material-dirty": {"files": [SRC + "material/material-dirty.ts", SRC + "material/observable-material.ts", SRC + "material/tracking/"], "docs": ["docs/lite/03-porting-guide.md"], "keywords": ["markMaterialUboDirty", "enableMaterialTracking", "_uboVersion"]},
    "shadow-attach": {"files": [SRC + "scene/scene-core.ts", SRC + "light/types.ts", SRC + "shadow/", SRC + "frame-graph/shadow-task.ts"], "docs": ["docs/lite/architecture/16-shadow-generator.md", "docs/lite/architecture/01-scene.md", "docs/lite/03-porting-guide.md"], "keywords": ["shadowGenerator", "shadowGenerators"]},
    "shadow-register": {"files": [SRC + "scene/scene-core.ts", SRC + "frame-graph/shadow-task.ts", SRC + "frame-graph/shadow-inputs.ts"], "docs": ["docs/lite/architecture/16-shadow-generator.md", "docs/lite/architecture/28-frame-graph.md"], "keywords": ["registerSceneWithShadowSupport", "setShadowTaskCasterMeshes", "ensureShadowTask"]},
    "shadow-receive": {"files": [SRC + "mesh/mesh.ts", SRC + "shadow/shadow-renderable.ts"], "docs": ["docs/lite/03-porting-guide.md", "docs/lite/architecture/16-shadow-generator.md"], "keywords": ["receiveShadows"]},
    "shadow-light-pos": {"files": [SRC + "shadow/pcf-directional-shadow-generator.ts", SRC + "shadow/esm-directional-shadow-generator.ts", SRC + "shadow/shadow-base.ts", SRC + "light/directional-light.ts", SRC + "light/light-matrix.ts"],
                         "docs": ["docs/lite/architecture/16-shadow-generator.md", "docs/lite/architecture/03-lights.md"], "keywords": ["orthoMinZ", "orthoMaxZ", "buildLightViewMatrix", "_computeDirectionalLightMatrix"]},
    "pick-fn-css": {"files": [SRC + "picking/gpu-picker.ts", SRC + "picking/picking-info.ts"], "docs": ["docs/lite/architecture/18-picking.md"], "keywords": ["pickAsync", "createGpuPicker", "disposePicker"]},
    "dpr-cap": {"files": [SRC + "engine/surface.ts", SRC + "engine/engine.ts"], "docs": ["docs/lite/architecture/22-engine.md"], "keywords": ["maxDevicePixelRatio", "devicePixelRatio"]},
    "winding": {"files": [SRC + "mesh/mesh-factories.ts", SRC + "mesh/create-box.ts", SRC + "material/standard/standard-pipeline.ts", SRC + "material/standard/standard-geometry-renderable.ts", SRC + "camera/camera.ts"],
                "docs": ["docs/lite/architecture/10-mesh-generators.md", "docs/lite/architecture/08-standard-material.md"], "keywords": ["createMeshFromData", "frontFace", "cullMode", "mat4PerspectiveLH", "mat4LookAtLH"]},
    "thin-recipe": {"files": [SRC + "mesh/thin-instance.ts", SRC + "mesh/thin-instance-gpu.ts"], "docs": ["docs/lite/architecture/12-thin-instances.md", "docs/lite/03-porting-guide.md"], "keywords": ["setThinInstances", "setThinInstanceColors", "addThinInstance", "thinInstances"]},
}


def lab_scenes(repo):
    """Lab scenes (lab/lite/src/lite/scene*.ts) that mention one of each fact's keywords (static, from the checkout)."""
    out = {}
    d = os.path.join(repo, "lab", "lite", "src", "lite")
    for name, f in FACTS.items():
        kw = [k for k in f["keywords"] if " " not in k and not k.startswith("_")]
        if not kw:
            out[name] = []
            continue
        r = subprocess.run(["grep", "-rlE", "|".join(kw), d], capture_output=True, text=True)
        out[name] = sorted(os.path.relpath(p, repo) for p in r.stdout.split())
    return out


def is_read(cmd, root=REF):
    return root in cmd and bool(re.search(READ_VERBS, cmd))


def targets(cmd, lab=None, root=REF):
    """-> {fact: {"path": bool, "keyword": bool, "lab": bool, "doc": bool, "source": bool}} for one shell command (empty when it is not a read of the checkout)."""
    if not is_read(cmd, root):
        return {}
    out = {}
    for name, f in FACTS.items():
        hit = {"path": False, "keyword": False, "lab": False, "doc": False, "source": False}
        for p in f["files"]:
            if (root + "/" + p) in cmd or ((p.endswith("/") and p.rstrip("/") in cmd) and root in cmd):
                hit["path"] = hit["source"] = True
        for p in f["docs"]:
            if (root + "/" + p) in cmd or p in cmd:
                hit["path"] = hit["doc"] = True
        if re.search(r"\b(grep|egrep|rg|find)\b", cmd):
            for k in f["keywords"]:
                if re.search(r"(?<![A-Za-z0-9_])" + re.escape(k) + r"(?![A-Za-z0-9_])", cmd):
                    hit["keyword"] = hit["source"] = True
        for p in (lab or {}).get(name, []):
            if p in cmd or os.path.basename(p) in cmd:
                hit["lab"] = hit["source"] = True
        if any(hit.values()):
            out[name] = hit
    return out


def rederived(commands, lab=None, root=REF):
    """commands: the session's bash command strings in order, up to the last write to main.ts. -> {fact: {re_derived, source_read, doc_only, n_reads, first_index}}"""
    res = {}
    for i, c in enumerate(commands):
        for name, h in targets(c, lab, root).items():
            r = res.setdefault(name, {"n_reads": 0, "first_index": i, "source_read": False, "doc_only": True})
            r["n_reads"] += 1
            if h["source"]:
                r["source_read"] = True
                r["doc_only"] = False
    for r in res.values():
        r["re_derived"] = r["source_read"]
    return res


def selftest():
    """Tier c's red/green proof: a source read before the last write re-derives a fact, a docs-only read does not, and a read of another
    checkout, or of the model's own worktree, is no read of this one."""
    root = "/srv/reference/Babylon/Babylon-Lite"
    c1 = "cat %s/packages/babylon-lite/src/scene/scene-core.ts" % root
    c2 = "cat %s/docs/lite/01-getting-started.md" % root
    c3 = "grep -rn setSubtreeVisible %s/packages" % root
    c4 = "ls /srv/work/main.ts"
    c5 = "chrome-devtools evaluate_script '() => 1'"
    c6 = "cat /srv/elsewhere/Babylon-Lite/packages/babylon-lite/src/scene/scene-core.ts"
    r = rederived([c2, c4, c5], root=root)
    assert r["late-add"]["doc_only"] and not r["late-add"]["re_derived"], r
    r = rederived([c2, c1, c3], root=root)
    assert r["late-add"]["re_derived"] and r["late-hide"]["re_derived"] and r["shadow-attach"]["re_derived"], r      # scene-core.ts also holds addToScene's (missing) shadow routing
    assert not targets(c4, root=root) and not targets(c5, root=root)
    c7 = "cat /srv/elsewhere/Babylon-Lite/docs/lite/01-getting-started.md"
    assert not rederived([c6], root=root), "another checkout's source is not the run's"
    assert not rederived([c7], root=root), "another checkout's docs are not the run's"
    print("process_trap selftest ok")


if __name__ == "__main__":
    # `--selftest` checks the definition; `--lab-scenes REPO` prints the facts and the checkout's lab scenes that use each fact's API.
    import sys
    if sys.argv[1:] == ["--selftest"]:
        selftest()
    elif sys.argv[1:2] == ["--lab-scenes"] and len(sys.argv) == 3:
        print(json.dumps({"facts": FACTS, "lab_scenes": lab_scenes(sys.argv[2])}, indent=1))
    else:
        sys.exit("usage: process_trap.py --selftest | --lab-scenes REPO")
