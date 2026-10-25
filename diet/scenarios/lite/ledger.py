"""Lite's public surface as a pointer ledger (#976): `python3 -I ledger.py ROOT OUT [--absolute]`, or `--selftest`.

One row per public name the package's barrel (`packages/babylon-lite/src/index.ts`) exports, values and types, as a TSV with the header
`name	description	query` (#976's format, the maintainer's): the query is a whole runnable `ast-grep outline` command, anchored on one
name, and the description says what running it shows. No ast-grep output is ever written: no line numbers, no signatures, no spans. Running
the query is how a reader gets the current answer.

How a row's query is chosen ("as specific as necessary, as broad as we could get away with"):
  - the name is the definition's own: an alias (`X as Y`) is row `Y`, its query anchored on `X`;
  - the path is the whole source tree, so the row survives a file move, narrowed to the file the barrel names only when the tree holds more
    than one item of that name; relative to the checkout, as #976's example rows are, or under `--absolute` the checkout's own path, so the
    query runs from anywhere (a model's worktree);
  - the view is `signatures` for a function, a constant or a variable, and `expanded` for everything else (interfaces, type aliases,
    classes, enums), as the found item's kind says.

The gate (classical, no model): every query is run as written, with `--json=stream` added so its items are parsed and counted. A row is
written only when the query finds the count its description declares (1: no template declares another) and every item it finds is named the
definition's name. A miss prints `nothing found` and exits 0, so the count is graded, never the exit status. A refused name goes to the
refusal report instead, with the count its query found. The TSV is written at run time, never committed.

OUT/ledger.tsv is the ledger; OUT/ledger-refused.tsv the refusals (`name`, `defined`, `found`, `why`); OUT/ledger.json what was built (the
checkout's commit, ast-grep's version, the counts, the ledger's sha256).
"""
import concurrent.futures
import hashlib
import json
import os
import re
import shlex
import subprocess
import sys

SRC = "packages/babylon-lite/src"
BARREL = SRC + "/index.ts"
EXPORT = re.compile(r"""^export\s+(type\s+)?\{([^}]*)\}\s*from\s*["']([^"']+)["']""", re.M | re.S)
# What running a row's query shows, by the found item's kind (model-facing: the maintainer's text, DRAFT until approved).
TEMPLATES = {
    "function": "its signature, with its file and line",
    "constant": "its declaration, with its file and line",
    "variable": "its declaration, with its file and line",
    "interface": "its fields, one per line, with line numbers",
    "struct": "its definition, one line per member, with line numbers",
    "class": "its members, one per line, with line numbers",
    "enum": "its members, one per line, with line numbers",
}
ALIAS = "; exported as {public}, defined as {defined}"
SIGNATURES = {"function", "constant", "variable"}
DECLARED = 1


def barrel(text):
    """[(public, defined, file, is_type)] in the barrel's order: each named export, its alias resolved, its file relative to the checkout."""
    out = []
    for m in EXPORT.finditer(text):
        whole_type, names, spec = bool(m.group(1)), m.group(2), m.group(3)
        target = os.path.normpath(os.path.join(SRC, spec))
        target = re.sub(r"\.js$", ".ts", target)
        for part in re.sub(r"//[^\n]*|/\*.*?\*/", "", names, flags=re.S).split(","):
            part = part.strip()
            if not part:
                continue
            is_type = whole_type
            if part.startswith("type "):
                is_type, part = True, part[len("type "):].strip()
            defined, _, public = part.partition(" as ")
            out.append((public.strip() or defined.strip(), defined.strip(), target, is_type))
    return out


def query(path, name, view):
    return "ast-grep outline %s --match '^%s$' --view %s" % (path, name, view)


def items_of(run, q):
    """The items `q` finds, parsed from its JSON stream: [{name, symbolType, path}]."""
    found = []
    for line in run(q + " --json=stream").splitlines():
        if line.strip():
            d = json.loads(line)
            found += [dict(i, path=d.get("path")) for i in d.get("items", []) if i.get("role", "item") == "item"]
    return found


def row(entry, run, prefix=""):
    """(row, None) for a name the gate admits, or (None, refusal): row = (name, description, query); refusal = (name, defined, found, why).
    `prefix` is the checkout's path in a query, empty for one relative to it."""
    public, defined, target, _ = entry
    at = lambda path: os.path.join(prefix, path) if prefix else path
    if not re.fullmatch(r"[A-Za-z_$][\w$]*", defined):
        return None, (public, defined, 0, "not a name an anchored query can match")
    found, path = items_of(run, query(at(SRC), defined, "signatures")), at(SRC)
    if len(found) > DECLARED:
        narrowed = items_of(run, query(at(target), defined, "signatures"))
        if len(narrowed) != DECLARED:
            return None, (public, defined, len(found), "the tree holds %d, and its barrel file %d" % (len(found), len(narrowed)))
        found, path = narrowed, at(target)
    if len(found) != DECLARED:
        return None, (public, defined, len(found), "the tree holds %d" % len(found))
    kind = found[0].get("symbolType")
    if kind not in TEMPLATES:
        return None, (public, defined, len(found), "no description for a %s" % kind)
    q = query(path, defined, "signatures" if kind in SIGNATURES else "expanded")
    # The gate: the query as written, counted and named.
    got = items_of(run, q)
    if len(got) != DECLARED:
        return None, (public, defined, len(got), "its query finds %d" % len(got))
    if any(i.get("name") != defined for i in got):
        return None, (public, defined, len(got), "its query finds %s" % sorted({i.get("name") for i in got}))
    description = TEMPLATES[kind] + (ALIAS.format(public=public, defined=defined) if public != defined else "")
    return (public, description, q), None


def runner(root):
    def run(q):
        r = subprocess.run(shlex.split(q), cwd=root, capture_output=True, text=True, timeout=120)
        if r.returncode != 0:
            raise RuntimeError("ledger: `%s` exited %d: %s" % (q, r.returncode, r.stderr[-500:]))
        return r.stdout
    return run


def tsv(rows):
    for r in rows:
        for field in r:
            if "\t" in field or "\n" in field:
                raise ValueError("ledger: a field holds a tab or a newline: %r" % (r,))
    return "name\tdescription\tquery\n" + "".join("\t".join(r) + "\n" for r in rows)


def build(root, out, run=None, workers=8, absolute=False):
    live = run is None
    run = run or runner(root)
    entries = barrel(open(os.path.join(root, BARREL), encoding="utf-8").read())
    with concurrent.futures.ThreadPoolExecutor(workers) as pool:
        prefix = os.path.realpath(root) if absolute else ""
        results = list(pool.map(lambda e: row(e, run, prefix), entries))
    rows = [r for r, _ in results if r]
    refused = [x for _, x in results if x]
    os.makedirs(out, exist_ok=True)
    text = tsv(rows)
    open(os.path.join(out, "ledger.tsv"), "w").write(text)
    open(os.path.join(out, "ledger-refused.tsv"), "w").write(
        "name\tdefined\tfound\twhy\n" + "".join("%s\t%s\t%d\t%s\n" % r for r in refused))
    commit = subprocess.run(["git", "-C", root, "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()
    version = subprocess.run(["ast-grep", "--version"], capture_output=True, text=True).stdout.strip() if live else ""
    said = {"absolute": absolute, "commit": commit, "ast_grep": version, "exports": len(entries), "rows": len(rows), "refused": len(refused),
            "ledger_sha256": hashlib.sha256(text.encode()).hexdigest()}
    json.dump(said, open(os.path.join(out, "ledger.json"), "w"), indent=1, sort_keys=True)
    return said


def selftest():
    """The gate's judgments, both ways, on a fake ast-grep (a table of what each query finds), so it runs where ast-grep does not."""
    item = lambda name, kind, path="x.ts": {"role": "item", "name": name, "symbolType": kind, "path": path}
    finds = {
        # One in the tree: the broad query.
        (SRC, "createBox"): [item("createBox", "function")],
        # An alias: the query is anchored on the definition's name.
        (SRC, "normalizeVec3"): [item("normalizeVec3", "function", "a.ts"), item("normalizeVec3", "function", "b.ts")],
        (SRC + "/math/normalize-vec3-object.ts", "normalizeVec3"): [item("normalizeVec3", "function", "b.ts")],
        # Over-broad: two in the tree, two in its barrel file too.
        (SRC, "Camera"): [item("Camera", "interface"), item("Camera", "function")],
        (SRC + "/camera/camera.ts", "Camera"): [item("Camera", "interface"), item("Camera", "function")],
        # A miss: nothing found.
        (SRC, "Gone"): [],
        # A type: the expanded view.
        (SRC, "Vec3"): [item("Vec3", "interface")],
    }
    asked = []

    def run(q):
        asked.append(q)
        args = shlex.split(q)
        path, name = args[2], args[args.index("--match") + 1].strip("^$")
        return "".join(json.dumps({"path": path, "items": [i]}) + "\n" for i in finds.get((path, name), []))

    text = ('export { createBox } from "./mesh/mesh-factories.js";\n'
            'export { normalizeVec3 as normalizeVec3Object } from "./math/normalize-vec3-object.js";\n'
            'export type {\n  Camera, // the camera\n} from "./camera/camera.js";\nexport { Gone } from "./gone.js";\n'
            'export type { Vec3 } from "./math/types.js";\n')
    entries = barrel(text)
    assert [(p, d) for p, d, _, _ in entries] == [("createBox", "createBox"), ("normalizeVec3Object", "normalizeVec3"), ("Camera", "Camera"),
                                                  ("Gone", "Gone"), ("Vec3", "Vec3")], entries
    got = {e[0]: row(e, run) for e in entries}
    box, alias, broad, miss, vec = (got[k] for k in ("createBox", "normalizeVec3Object", "Camera", "Gone", "Vec3"))
    assert box == (("createBox", TEMPLATES["function"], query(SRC, "createBox", "signatures")), None), box
    assert alias[0] == ("normalizeVec3Object", TEMPLATES["function"] + ALIAS.format(public="normalizeVec3Object", defined="normalizeVec3"),
                        query(SRC + "/math/normalize-vec3-object.ts", "normalizeVec3", "signatures")), "an alias, narrowed to its barrel file"
    assert broad == (None, ("Camera", "Camera", 2, "the tree holds 2, and its barrel file 2")), broad
    assert miss == (None, ("Gone", "Gone", 0, "the tree holds 0")), "a miss is refused on its count, whatever the exit"
    assert vec[0][2] == query(SRC, "Vec3", "expanded"), vec
    # Under a prefix, the query names the checkout's own path.
    finds[("/srv/lite/" + SRC, "createBox")] = [item("createBox", "function")]
    assert row(entries[0], run, "/srv/lite")[0][2] == query("/srv/lite/" + SRC, "createBox", "signatures")
    # The gate counts and names what the written query finds, not what the search for it found.
    finds[(SRC, "createBox")] = [item("createBox", "function")]
    flaky = {"n": 0}

    def run_twice(q):
        flaky["n"] += 1
        return run(q) if flaky["n"] == 1 else ""
    assert row(entries[0], run_twice) == (None, ("createBox", "createBox", 0, "its query finds 0")), "the gate re-runs the query"
    finds[(SRC, "Wrong")] = [item("Other", "function")]
    wrong = row(("Wrong", "Wrong", "x.ts", False), run)
    assert wrong == (None, ("Wrong", "Wrong", 1, "its query finds ['Other']")), "the match must be the name: %r" % (wrong,)
    # No ast-grep output in a row: three fields, a name, a template, a command.
    out = tsv([box[0], alias[0], vec[0]])
    for line in out.splitlines()[1:]:
        name, description, q = line.split("\t")
        assert re.fullmatch(r"ast-grep outline \S+ --match '\^[\w$]+\$' --view (signatures|expanded)", q), q
        assert not re.search(r"\d+:|export |\(", description), description
    try:
        tsv([("a\tb", "c", "d")])
    except ValueError:
        pass
    else:
        raise AssertionError("a field with a tab is refused")
    print("ledger selftest ok")


if __name__ == "__main__":
    if sys.argv[1:] == ["--selftest"]:
        selftest()
    elif len(sys.argv) in (3, 4) and sys.argv[3:] in ([], ["--absolute"]):
        print(json.dumps(build(sys.argv[1], sys.argv[2], absolute=sys.argv[3:] == ["--absolute"]), indent=1, sort_keys=True))
    else:
        sys.exit("usage: ledger.py ROOT OUT [--absolute] | --selftest")
