"""Tier (c), the PROCESS tier: DEFINED here, computed later from a session log (python3 -I; standard library only).
A session is cheap or expensive in tokens, commands and source reads. For each trap fact this file says which reads count as the model RE-DERIVING that fact.

DEFINITION
  A "read" is what one shell tool call (the session log's `bash` calls) does with the Lite checkout: the path the run's `lite` service checked
  at start (its provides.json `root`), else $LITE_REPO, else ~/git/reference/Babylon/Babylon-Lite. `reads_of` parses the call:
    - heredoc bodies are dropped first: what a heredoc writes is the model's own text, never a read, however much Lite it names;
    - the call is split into segments; each call starts in the worktree, and `cd` moves it, so a relative path after `cd <root>/...` resolves;
    - content verbs (cat, head, tail, nl, wc, ...; sed and awk past their script; `git -C <root> show HEAD:<path>`) read their file operands,
      search verbs (grep, rg, ...) their path operands, or the working directory when they name none, and listing verbs (ls, tree, find, ...)
      theirs; `sed -i` writes, `test` and the model's own files are no read;
    - `node_modules` under the checkout is tooling (a PNG decoder, vite), not Lite;
    - a page script (chrome-devtools evaluate_script) that fetches or imports `/@fs/<root>/...` reads that file over HTTP, a base literal joined
      with the file names beside it included.
  The classifier's two numbers, over `reads_fixtures.json` (known reads graded as the reads they are, known non-reads graded as none), are
  printed by --selftest, which fails unless both are whole.
  A read TARGETS a fact when either
    (path)    a path it names matches one of the fact's `files` (prefix match under the checkout: a directory entry matches everything inside it), or
    (keyword) it searches the checkout (grep/rg) with one of the fact's `keywords` in its own pattern (whole word), or
    (lab)     it names a lab scene listed for the fact in LAB_SCENES (lab scenes that call the fact's API; derived from the checkout by `lab_scenes()` below, so a copy of the
              pilot's finding "the checkout's 250+ lab scenes near-copy most small tasks" is counted, not hidden).
  A fact is RE-DERIVED for a task when a read in the task's own window (`task_windows`: from its first ask to its last grade) targets it. Reads
  of the OTHER facts' files do not count for it, and neither do another task's reads of the same fact. `task_reads` also reports every task's
  reads of the checkout whether or not it has facts, so a control task's reads are seen.
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


# What a shell segment does with the files it names (#932's audit of the interviews smoke run: tier c counted heredoc writes and pngjs
# tooling as reads, and missed relative reads after `cd`).
CONTENT_VERBS = {"cat", "head", "tail", "less", "more", "nl", "bat", "wc", "strings", "xxd", "od", "cut", "sort", "uniq", "diff", "cmp", "file"}
SCRIPT_VERBS = {"sed", "awk"}            # the first operand is a script, the rest are files
SEARCH_VERBS = {"grep", "egrep", "fgrep", "rg", "ag", "ack"}   # the first operand is a pattern, the rest are paths (none: the working directory)
LISTING_VERBS = {"ls", "tree", "find", "du", "stat"}
WRAPPERS = {"sudo", "command", "builtin", "nice", "time", "xargs"}
TOOLING = ("node_modules",)
# Each verb's options that take a value (head -n 80, grep -e PAT, find -name X): the value is no path. sed's -n takes none.
VALUED = {"head": ("-n", "-c"), "tail": ("-n", "-c"), "grep": ("-e", "-f", "-A", "-B", "-C", "-m"), "egrep": ("-e", "-f", "-A", "-B", "-C", "-m"),
          "fgrep": ("-e", "-f", "-A", "-B", "-C", "-m"), "rg": ("-e", "-f", "-A", "-B", "-C", "-m", "-g", "--glob", "-t", "--type"),
          "sed": ("-e", "-f"), "awk": ("-f", "-v", "-F"), "find": ("-name", "-iname", "-path", "-type", "-maxdepth", "-mindepth", "-newer"),
          "cut": ("-d", "-f", "-c"), "sort": ("-k", "-t"), "tree": ("-L", "-I", "-P"), "ls": ()}              # under the checkout but not Lite: the tools a run borrows (pngjs, vite)


def _strip_heredocs(cmd):
    """The command with every heredoc's body removed: what a heredoc writes is the model's own text, not a read."""
    out, lines, i = [], cmd.split("\n"), 0
    while i < len(lines):
        line = lines[i]
        out.append(line)
        ends = re.findall(r"<<-?\s*['\"]?([A-Za-z_][A-Za-z0-9_]*)['\"]?", line)
        i += 1
        for end in ends:
            while i < len(lines) and lines[i].strip() != end:
                i += 1
            i += 1
    return "\n".join(out)


def _segments(cmd):
    """Shell segments in order, each a token list: split at newlines, `;`, `&&`, `||` and `|`, quotes respected."""
    import shlex
    segs, cur = [], []
    try:
        lex = shlex.shlex(_strip_heredocs(cmd), posix=True, punctuation_chars=";&|")
        lex.whitespace = " \t\r"
        lex.whitespace_split = True
        tokens = list(lex)
    except ValueError:
        return []
    for t in tokens:
        if t in (";", "&&", "||", "|", "&", "\n") or set(t) <= set(";&|") and t:
            if cur:
                segs.append(cur)
            cur = []
        elif "\n" in t and not t.strip():
            if cur:
                segs.append(cur)
            cur = []
        else:
            cur.append(t)
    if cur:
        segs.append(cur)
    out = []
    for seg in segs:
        # A newline inside an unquoted run of tokens ends a command too.
        piece = []
        for t in seg:
            if t == "\n":
                if piece:
                    out.append(piece)
                piece = []
            else:
                piece.append(t)
        if piece:
            out.append(piece)
    return out


def _under(path, root):
    """`path` relative to the checkout, '' for the checkout itself, or None when it is outside it or is tooling under it."""
    path = os.path.normpath(path)
    root = os.path.normpath(root)
    if path == root:
        return ""
    if not path.startswith(root + "/"):
        return None
    rel = path[len(root) + 1:]
    if rel.split("/")[0] in TOOLING:
        return None
    return rel


def _resolve(token, cwd):
    if token.startswith("/"):
        return token
    return os.path.join(cwd, token) if cwd else None


def _shell_reads(cmd, root):
    """[(relpath, how, pattern)] of one bash call's reads of the checkout. Each call starts in the worktree (cwd unknown, never the
    checkout); `cd` moves it within the call. how is content, listing or search; pattern is a search's pattern."""
    reads, cwd = [], None
    for seg in _segments(cmd):
        while seg and re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*=.*", seg[0]):
            seg = seg[1:]
        while seg and os.path.basename(seg[0]) in WRAPPERS:
            seg = seg[1:]
        if not seg:
            continue
        verb, args = os.path.basename(seg[0]), seg[1:]
        if verb == "cd":
            target = args[0] if args else None
            cwd = _resolve(target, cwd) if target and target != "-" else None
            continue
        if verb == "git":
            where = None
            if "-C" in args and args.index("-C") + 1 < len(args):
                where = _resolve(args[args.index("-C") + 1], cwd)
                args = args[:args.index("-C")] + args[args.index("-C") + 2:]
            where = where or cwd
            sub = args[0] if args else ""
            base = _under(where, root) if where else None
            if base is None:
                continue
            if sub == "show":
                for a in args[1:]:
                    if ":" in a and not a.startswith("-"):
                        reads.append((a.split(":", 1)[1].lstrip("/"), "content", None))
            elif sub in ("grep", "log", "blame", "cat-file", "diff"):
                reads.append((base, "search" if sub == "grep" else "content", None))
            continue
        operands = []
        skip = False
        for a in args:
            if skip:
                skip = False
                continue
            if a.startswith("-"):
                # An option that takes a value (head -n 80, grep -e PAT, sed -e SCRIPT, find -name X): its value is no path.
                if a in VALUED.get(verb, ()):
                    skip = True
                continue
            operands.append(a)
        if verb in SEARCH_VERBS:
            pattern_given = any(a in ("-e", "-f") for a in args)
            pattern = None if pattern_given else (operands[0] if operands else None)
            if pattern_given:
                i = [j for j, a in enumerate(args) if a in ("-e",)]
                pattern = args[i[0] + 1] if i and i[0] + 1 < len(args) else None
            paths = operands if pattern_given else operands[1:]
            # A search with no path reads its input when it is piped to, else the working directory.
            if not paths and cwd and seg is not None:
                paths = [cwd]
            for t in paths:
                r = _under(_resolve(t, cwd) or "", root)
                if r is not None:
                    reads.append((r, "search", pattern))
        elif verb in SCRIPT_VERBS:
            if verb == "sed" and any(a == "-i" or a.startswith("-i") for a in args):
                continue
            files = operands[1:] if not any(a in ("-e", "-f") for a in args) else operands
            for t in files:
                r = _under(_resolve(t, cwd) or "", root)
                if r is not None:
                    reads.append((r, "content", None))
        elif verb in CONTENT_VERBS or verb in LISTING_VERBS:
            how = "content" if verb in CONTENT_VERBS else "listing"
            paths = operands or ([cwd] if verb in LISTING_VERBS and cwd else [])
            for t in paths:
                r = _under(_resolve(t, cwd) or "", root)
                if r is not None:
                    reads.append((r, how, None))
    return reads


def _http_reads(cmd, root):
    """[(relpath, "http", None)]: the checkout's files a page script fetches or imports over Vite's /@fs/ (the sealed smoke's channel),
    a base literal joined with the quoted relative file names beside it included."""
    reads = []
    prefix = "/@fs" + os.path.normpath(root) + "/"
    quoted = re.findall(r"'([^']*)'", cmd) + re.findall(r'"([^"]*)"', cmd) + re.findall(r"`([^`]*)`", cmd)
    # `/@fs` and the absolute path, as a page writes it; a doubled slash after it reads the same.
    quoted = [re.sub(r"^/@fs/+", "/@fs/", q) for q in quoted]
    bases = [q[len(prefix):] for q in quoted if q.startswith(prefix)]
    for b in bases:
        if b.endswith("/"):
            joined = [q for q in quoted if not q.startswith("/") and re.fullmatch(r"[\w./-]+\.\w+", q)]
            reads.extend((b + j, "http", None) for j in joined)
            if not joined:
                reads.append((b, "http", None))
        else:
            reads.append((b, "http", None))
    return [(r, h, p) for r, h, p in reads if r.split("/")[0] not in TOOLING]


def reads_of(cmd, root=REF):
    """Every read one bash command makes of the checkout: [(relpath, how, pattern)], how in content, listing, search, http."""
    return _shell_reads(cmd, root) + _http_reads(cmd, root)


def is_read(cmd, root=REF):
    return bool(reads_of(cmd, root))


def targets(cmd, lab=None, root=REF):
    """-> {fact: {"path": bool, "keyword": bool, "lab": bool, "doc": bool, "source": bool}} for one shell command (empty when it is not a read
    of the checkout). Only what the command reads counts: a path it reads, or a search's own pattern, never text it writes."""
    reads = reads_of(cmd, root)
    if not reads:
        return {}
    out = {}
    for name, f in FACTS.items():
        hit = {"path": False, "keyword": False, "lab": False, "doc": False, "source": False}
        for rel, how, pattern in reads:
            for p in f["files"]:
                if how in ("content", "http") and (rel == p or (p.endswith("/") and rel.startswith(p))):
                    hit["path"] = hit["source"] = True
            for p in f["docs"]:
                if rel == p and how in ("content", "http"):
                    hit["path"] = hit["doc"] = True
            if how == "search" and pattern:
                for k in f["keywords"]:
                    if re.search(r"(?<![A-Za-z0-9_])" + re.escape(k) + r"(?![A-Za-z0-9_])", pattern):
                        hit["keyword"] = hit["source"] = True
            for p in (lab or {}).get(name, []):
                if rel == p and how in ("content", "http"):
                    hit["lab"] = hit["source"] = True
        if any(hit.values()):
            out[name] = hit
    return out


def rederived(commands, lab=None, root=REF):
    """commands: the bash command strings of a window, in order. -> {fact: {re_derived, source_read, doc_only, n_reads, first_index}}"""
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


def task_windows(lines):
    """{task: (first ask seq, last grade seq)} from a session log's lines: each task's own window, so a read is the task's whose ask it
    follows and whose last grade it precedes (the audit: a whole half's reads were credited to every task sharing a fact)."""
    asks, grades = {}, {}
    for line in lines:
        if line.get("kind") == "ask" and line.get("task"):
            asks.setdefault(line["task"], line["seq"])
        if line.get("kind") == "grade":
            grades[line["task"]] = line["seq"]
    return {t: (asks[t], grades[t]) for t in asks if t in grades}


def task_reads(lines, root=REF, lab=None):
    """{task: {"lite_reads", "source_reads", "doc_reads", "facts"}} per task window: every read of the checkout, whether or not the task
    has facts (a control task's reads count too), and the facts its reads target."""
    calls = [(line["seq"], json.loads(line.get("arguments") or "{}").get("command", "")) for line in lines
             if line.get("kind") == "tool_call" and line.get("name") == "bash"]
    out = {}
    for task, (lo, hi) in task_windows(lines).items():
        mine = [c for seq, c in calls if lo < seq <= hi]
        reads = [r for c in mine for r in reads_of(c, root)]
        out[task] = {
            "lite_reads": len(reads),
            "source_reads": sum(1 for r, how, _ in reads if not r.startswith("docs/") and how != "listing"),
            "doc_reads": sum(1 for r, how, _ in reads if r.startswith("docs/") and how in ("content", "http")),
            "facts": rederived(mine, lab, root),
        }
    return out


FIXTURES = os.path.join(os.path.dirname(os.path.abspath(__file__)), "reads_fixtures.json")


def fixture_numbers(reads_of=None, targets=None):
    """The classifier's two numbers over reads_fixtures.json: known reads graded as the reads they are (paths, and facts where given),
    and known non-reads graded as no read. -> (right reads, reads, right non-reads, non-reads, the cases it got wrong)."""
    reads_of = reads_of or globals()["reads_of"]
    targets = targets or globals()["targets"]
    root = "/srv/reference/Babylon/Babylon-Lite"
    fx = json.load(open(FIXTURES))
    wrong, ok_reads, ok_non = [], 0, 0
    for case in fx["reads"]:
        cmd = case["cmd"].replace("ROOT", root)
        got = sorted({r for r, _, _ in reads_of(cmd, root)})
        facts = sorted(targets(cmd, root=root))
        if got == sorted(case["reads"]) and ("facts" not in case or facts == sorted(case["facts"])):
            ok_reads += 1
        else:
            wrong.append((case["why"], got, facts))
    for case in fx["non_reads"]:
        cmd = case["cmd"].replace("ROOT", root)
        if not reads_of(cmd, root) and not targets(cmd, root=root):
            ok_non += 1
        else:
            wrong.append((case["why"], reads_of(cmd, root), sorted(targets(cmd, root=root))))
    return ok_reads, len(fx["reads"]), ok_non, len(fx["non_reads"]), wrong


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
    ok_r, n_r, ok_n, n_n, wrong = fixture_numbers()
    print("reads graded as reads: %d of %d; non-reads graded as no read: %d of %d" % (ok_r, n_r, ok_n, n_n))
    assert not wrong, wrong
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
