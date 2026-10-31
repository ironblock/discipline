"""A seeded TypeScript fixture for the ast-grep tutorial (#971): python3 -I gen.py SEED OUT TRUTH.

Writes a small project under OUT (`package.json`, `src/`, a barrel `src/index.ts`) and the answer to every task
family to TRUTH, both from the same construction: the generator places each function, call and export itself and
records where, so the truth never comes from ast-grep, the tool under test. A seed is one instance: the same skills,
with new file layout, new names and new values. `--selftest` builds a few seeds and checks the truth's invariants.
"""
import json, os, random, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gen2  # noqa: E402

VERBS = ["load", "parse", "build", "merge", "fetch", "store", "render", "split", "scan", "pack", "route", "sync",
         "trim", "sort", "index", "probe", "seal", "stamp", "drain", "weigh"]
NOUNS = ["ledger", "parcel", "beacon", "ticket", "bundle", "shard", "quota", "relay", "anchor", "harbor", "cipher",
         "socket", "vertex", "lattice", "meadow", "garnet", "pylon", "sprocket", "turbine", "canopy", "glacier", "ember"]
DIRS = ["core", "io", "model", "net", "util", "store", "view", "jobs"]
FILES = ["main", "helpers", "types", "shared", "flow", "codec", "state", "rules"]


def cap(word):
    return word[0].upper() + word[1:]


class Instance:
    def __init__(self, seed):
        self.rng = random.Random(seed)
        self.seed = seed
        self.used = set()
        self.files = {}  # path -> list of lines

    def word(self, pool, fmt=lambda w: w):
        while True:
            w = fmt(self.rng.choice(pool))
            if w not in self.used:
                self.used.add(w)
                return w

    def fn_name(self):
        while True:
            name = self.rng.choice(VERBS) + cap(self.rng.choice(NOUNS))
            if name not in self.used:
                self.used.add(name)
                return name

    def emit(self, path, line):
        self.files.setdefault(path, []).append(line)
        return len(self.files[path])  # 1-based


def build(seed):
    """The instance's files and its truth."""
    g = Instance(seed)
    rng = g.rng
    types = [g.word(NOUNS, cap) for _ in range(4)]
    target, arrow_target, other = types[0], types[1], types[2]
    paths = []
    for d in rng.sample(DIRS, 3):
        for f in rng.sample(FILES, 2):
            paths.append(f"src/{d}/{f}.ts")
    log_fn, zero_fn, track = g.fn_name(), g.fn_name(), "$track"
    maybe = g.word(NOUNS, lambda w: cap(w) + "Spec")
    truth = {"seed": seed, "params": {}, "answers": {}}
    funcs, exported, interfaces = [], [], []
    lines_of = {"log-in-try": [], "emit-calls": [], "track-calls": [], "zero-calls": []}

    # The interfaces, one per type, spread over the files; one more is never public.
    for name in types + [g.word(NOUNS, lambda w: cap(w) + "Row")]:
        path = rng.choice(paths)
        line = g.emit(path, f"export interface {name} {{")
        g.emit(path, f"  id: string;")
        g.emit(path, f"  weight?: number;")
        g.emit(path, "}")
        g.emit(path, "")
        interfaces.append({"name": name, "path": path, "line": line})

    # Helpers every body calls: the log function, the event emitter, the tracker and the zero-call function.
    helper = paths[0]
    g.emit(helper, f"export function {log_fn}(message: string, detail?: string): void {{")
    g.emit(helper, "  console.log(message, detail);")
    g.emit(helper, "}")
    g.emit(helper, "")
    g.emit(helper, "export const $emit = (event: string): string => event;")
    g.emit(helper, "export const $track = (what: string): number => what.length;")
    g.emit(helper, "")
    g.emit(helper, f"export function {zero_fn}(count: number): number {{")
    g.emit(helper, "  return count * 2;")
    g.emit(helper, "}")
    g.emit(helper, "")
    exported += [log_fn, zero_fn]
    zero_calls = rng.choice([0, 0, 1, 2])
    tracks = 0

    # The functions: each a declaration or an arrow constant, with a return type drawn so every family has its
    # positives and its look-alikes.
    plan = (["decl"] * 9) + (["arrow"] * 4)
    rng.shuffle(plan)
    for i, kind in enumerate(plan):
        name, path = g.fn_name(), rng.choice(paths[1:])
        ret = rng.choice([target, target, f"Promise<{target}>", arrow_target, other, None])
        if kind == "arrow" and i % 2 == 0:
            ret = arrow_target
        awaits = rng.choice(["none", "top", "nested", "nested"]) if kind == "decl" else "none"
        if awaits != "none" and ret is not None and not ret.startswith("Promise"):
            ret = f"Promise<{ret}>"
        logs = rng.choice(["none", "plain", "try", "try-nested"]) if kind == "decl" else "none"
        emits = rng.random() < 0.35 and kind == "decl"
        n_track = rng.choice([0, 0, 1, 2]) if kind == "decl" else 0
        is_export = rng.random() < 0.8
        asy = "async " if awaits != "none" else ""
        sig_ret = f": {ret}" if ret else ""
        exp = "export " if is_export else ""
        body = []  # (text, what the line is a match of, if anything)
        if awaits == "top":
            body.append((f"  const got = await Promise.resolve(id);", None))
        if awaits == "nested":
            body += [(f"  for (const part of id.split(',')) {{", None), (f"    if (part) {{", None),
                     (f"      await Promise.resolve(part);", None), (f"    }}", None), (f"  }}", None)]
        if logs == "plain":
            body.append((f"  {log_fn}(id);", None))
        if logs == "try":
            body += [(f"  try {{", None), (f"    {log_fn}(id);", "log-in-try"), (f"    JSON.stringify(id);", None),
                     (f"  }} catch (err) {{", None), (f"    void err;", None), (f"  }}", None)]
        if logs == "try-nested":
            body += [(f"  try {{", None), (f"    if (id.length > 1) {{", None), (f"      {log_fn}(id);", "log-in-try"),
                     (f"      Math.max(id.length, 1);", None), (f"    }}", None), (f"  }} finally {{", None), (f"    void id;", None),
                     (f"  }}", None)]
        if emits:
            body.append((f"  $emit('{rng.choice(NOUNS)}');", "emit-calls"))
        for _ in range(n_track):
            body.append((f"  $track('{rng.choice(NOUNS)}');", "track-calls"))
        tracks += n_track
        if zero_calls and kind == "decl" and sum(f.get("zero", 0) for f in funcs) < zero_calls:
            body.append((f"  {zero_fn}({rng.randint(2, 99)});", "zero-calls"))
            zero = 1
        else:
            zero = 0
        ret_expr = "{ id }" if ret and (target in ret or arrow_target in ret or other in ret) else "undefined"
        if kind == "decl":
            line = g.emit(path, f"{exp}{asy}function {name}(id: string){sig_ret} {{")
            for text, what in body:
                at = g.emit(path, text)
                if what:
                    lines_of[what].append(f"{path}:{at}")
            g.emit(path, f"  return {ret_expr} as never;")
            g.emit(path, "}")
        else:
            line = g.emit(path, f"{exp}const {name} = (id: string){sig_ret} => ({ret_expr} as never);")
        g.emit(path, "")
        funcs.append({"name": name, "kind": kind, "path": path, "line": line, "ret": ret, "awaits": awaits, "logs": logs,
                      "emits": emits, "zero": zero, "export": is_export})
        if is_export:
            exported.append(name)
    zero_calls = sum(f["zero"] for f in funcs)

    # A class whose method returns the target type: a look-alike for the declaration family.
    cls_path = rng.choice(paths[1:])
    cls = g.word(NOUNS, lambda w: cap(w) + "Desk")
    g.emit(cls_path, f"export class {cls} {{")
    desk_method = f"{cls_path}:{g.emit(cls_path, f'  {g.fn_name()}(id: string): {target} {{')}"
    g.emit(cls_path, f"    return {{ id }} as never;")
    g.emit(cls_path, "  }")
    g.emit(cls_path, "}")
    g.emit(cls_path, "")

    # Decoys a text search finds and a structural one doesn't: the zero-call function in a comment and a string,
    # and the maybe-defined name the same way unless it is defined.
    decoy = rng.choice(paths[1:])
    g.emit(decoy, f"// {zero_fn}() is kept for callers outside this package; {maybe} was planned here.")
    g.emit(decoy, f"export const NOTE = \"call {zero_fn}() only through the {maybe} path\";")
    g.emit(decoy, "")
    defined = rng.random() < 0.5
    if defined:
        mpath = rng.choice(paths[1:])
        # Its kind varies by seed (2026-10-27): the ask names no kind, so a --type filter must not pass by
        # the generator's habit of one.
        kind = rng.choice(["type", "interface", "function", "class"])
        first = {"type": f"export type {maybe} = {{ name: string }};", "interface": f"export interface {maybe} {{ name: string }}",
                 "function": f"export function {maybe}(name: string): void {{ void name; }}",
                 "class": f"export class {maybe} {{ name = ''; }}"}[kind]
        maybe_line = f"{mpath}:{g.emit(mpath, first)}"
        g.emit(mpath, "")

    # The barrel: some exported functions, a few under another name, and all but one interface but the never-public
    # one; the rest are module exports, not the package's public surface.
    public_fns, barrel = [], []
    for name in exported:
        if rng.random() < 0.6:
            src = next((f["path"] for f in funcs if f["name"] == name), helper)
            if rng.random() < 0.25:
                alias = name + "Public"
                barrel.append(f"export {{ {name} as {alias} }} from './{src[4:-3]}';")
                public_fns.append(alias)
            else:
                barrel.append(f"export {{ {name} }} from './{src[4:-3]}';")
                public_fns.append(name)
    hidden = rng.sample(interfaces, 2)
    for itf in interfaces:
        if itf not in hidden:
            barrel.append(f"export type {{ {itf['name']} }} from './{itf['path'][4:-3]}';")
    barrel_lines, type_lines = [], []
    for line in barrel:
        at = f"src/index.ts:{g.emit('src/index.ts', line)}"
        barrel_lines.append(at)
        if line.startswith("export type"):
            type_lines.append(at)

    decls = [f for f in funcs if f["kind"] == "decl"]
    line_fn = rng.choice(decls)
    line_itf = rng.choice(interfaces)
    # Distractors (Dispatch, 2026-10-27): what an over-broad form would also match, so execution can tell it from a right
    # one. A near-name beside each outlined item (an unanchored --match catches it), and a `let` arrow returning the arrow
    # type (a rule without its const catches it). None of them is in any family's truth.
    near = rng.choice(paths[1:])
    g.emit(near, f"export function {line_fn['name']}View(id: string): void {{")
    g.emit(near, "  void id;")
    g.emit(near, "}")
    g.emit(near, "")
    g.emit(near, f"export interface {line_itf['name']}View {{")
    g.emit(near, "  id: string;")
    g.emit(near, "}")
    g.emit(near, "")
    # And a suffix near-name (round 4): `--match 'Name$'`, anchored at one end only, catches it too.
    g.emit(near, f"export function pre{line_fn['name']}(id: string): void {{")
    g.emit(near, "  void id;")
    g.emit(near, "}")
    g.emit(near, "")
    g.emit(near, f"export interface Legacy{line_itf['name']} {{")
    g.emit(near, "  id: string;")
    g.emit(near, "}")
    g.emit(near, "")
    g.emit(near, f"export let {g.fn_name()} = (id: string): {arrow_target} => ({{ id }} as never);")
    g.emit(near, "")
    # Round 5 (Dispatch, 2026-10-28): a two-argument log call inside a try, so `log($A)` misses it, and a barrel line
    # exporting two types at once, so `export type { $X }` misses it. Both are in their families' truth.
    two = rng.choice(paths[1:])
    two_fn = g.fn_name()
    g.emit(two, f"export function {two_fn}(id: string): void {{")
    g.emit(two, "  try {")
    lines_of["log-in-try"].append(f"{two}:{g.emit(two, f'    {log_fn}(id, {rng.choice(NOUNS)!r});')}")
    g.emit(two, "  } catch (err) {")
    g.emit(two, "    void err;")
    g.emit(two, "  }")
    g.emit(two, "}")
    g.emit(two, "")
    # And two parameters on a function and a const arrow of the target types, so `($A)` misses them.
    wide = rng.choice(paths[1:])
    wide_decl = {"name": g.fn_name(), "kind": "decl", "path": wide, "ret": target, "awaits": "none"}
    wide_decl["line"] = g.emit(wide, f"export function {wide_decl['name']}(id: string, size: number): {target} {{")
    g.emit(wide, "  return { id, size } as never;")
    g.emit(wide, "}")
    g.emit(wide, "")
    wide_arrow = {"name": g.fn_name(), "kind": "arrow", "path": wide, "ret": arrow_target}
    wide_arrow["line"] = g.emit(wide, f"export const {wide_arrow['name']} = (id: string, size: number): {arrow_target} => ({{ id, size }} as never);")
    g.emit(wide, "")
    # And an async function expression with an await (Dispatch, 2026-10-28): not a declaration, so a rule whose kinds
    # include `function_expression` also catches it. Not in any family's truth.
    fexpr = f"{wide}:{g.emit(wide, f'export const {g.fn_name()} = async function (id: string): Promise<void> {{')}"
    g.emit(wide, "  await Promise.resolve(id);")
    g.emit(wide, "};")
    g.emit(wide, "")
    pair_path = rng.choice(paths[1:])
    pair = [g.word(NOUNS, lambda w: cap(w) + "Pair") for _ in range(2)]
    for name in pair:
        g.emit(pair_path, f"export interface {name} {{")
        g.emit(pair_path, "  id: string;")
        g.emit(pair_path, "}")
        g.emit(pair_path, "")
    at_pair = f"src/index.ts:{g.emit('src/index.ts', f'export type {{ {pair[0]}, {pair[1]} }} from {chr(39)}./{pair_path[4:-3]}{chr(39)};')}"
    barrel_lines.append(at_pair)
    type_lines.append(at_pair)
    # The decoy families' content (gen2.py, 2026-10-29), after every task family's draw: no task asks for them, and
    # `usable` still requires them, so the instances, and the seed each resolves to, stay what they were.
    params2, answers2 = gen2.build(g, rng, paths, helper, {"target": target, "other": other, "funcs": funcs + [wide_decl],
                                                         "desk_method": desk_method, "fexpr": [fexpr], "barrel_lines": barrel_lines})
    truth["params"] = {"target": target, "arrow_target": arrow_target, "zero_fn": zero_fn, "maybe": maybe,
                       "log_fn": log_fn, "line_fn": line_fn["name"], "line_fn_exported": line_fn["export"],
                       "line_itf": line_itf["name"]}
    # Each family's truth is the set of lines a right command matches, `path:line` with lines from 1: what the
    # generator wrote there, never what ast-grep says.
    at = lambda f: f"{f['path']}:{f['line']}"
    a = truth["answers"]
    a["ret-decl"] = sorted([at(f) for f in decls if f["ret"] == target] + [at(wide_decl)])
    a["ret-arrow"] = sorted([at(f) for f in funcs if f["kind"] == "arrow" and f["ret"] == arrow_target] + [at(wide_arrow)])
    a["zero-calls"] = sorted(lines_of["zero-calls"])
    a["maybe-defined"] = [maybe_line] if defined else []
    a["public-surface"] = sorted(barrel_lines)
    a["public-types"] = sorted(type_lines)
    a["await-any"] = sorted(at(f) for f in decls if f["awaits"] != "none")
    a["log-in-try"] = sorted(lines_of["log-in-try"])
    a["emit-calls"] = sorted(lines_of["emit-calls"])
    a["track-calls"] = sorted(lines_of["track-calls"])
    a["fn-outline"] = [at(line_fn)]
    a["itf-outline"] = [f"{line_itf['path']}:{line_itf['line']}"]
    truth["params"].update(params2)
    a.update({k: sorted(v) for k, v in answers2.items()})
    a["two-params"] = gen2.two_params(g.files)
    return g.files, truth


def usable(truth):
    """Every family but the misses has a match, so no right command is the one that matches nothing; the misses
    (`zero-calls`, `maybe-defined`, and the decoys' `zero-new` and `itf-maybe`) may match nothing, on purpose."""
    return all(v for k, v in truth["answers"].items() if k not in ("zero-calls", "maybe-defined", "zero-new", "itf-maybe"))


def write(seed, out, truth_path):
    """The first usable instance at or after SEED: its files under OUT and its truth at TRUTH."""
    for s in range(seed, seed + 1000):
        files, truth = build(s)
        if usable(truth):
            break
    else:
        raise SystemExit(f"gen: no usable instance in 1000 seeds from {seed}")
    for path, lines in files.items():
        os.makedirs(os.path.join(out, os.path.dirname(path)), exist_ok=True)
        with open(os.path.join(out, path), "w") as f:
            f.write("\n".join(lines) + "\n")
    with open(os.path.join(out, "package.json"), "w") as f:
        f.write(json.dumps({"name": "fixture", "private": True, "type": "module", "main": "src/index.ts"}, indent=2) + "\n")
    with open(truth_path, "w") as f:
        json.dump(truth, f, indent=1, sort_keys=True)
    return truth


def selftest():
    import tempfile
    bad = 0
    seen = set()
    for seed in range(1, 21):
        with tempfile.TemporaryDirectory() as d:
            truth = write(seed, os.path.join(d, "repo"), os.path.join(d, "truth.json"))
            again = write(seed, os.path.join(d, "again"), os.path.join(d, "truth2.json"))
            text = lambda r: {p: open(os.path.join(dp, p)).read() for dp, _, fs in os.walk(r) for p in fs}
            if truth != again or text(os.path.join(d, "repo")).keys() != text(os.path.join(d, "again")).keys():
                print(f"gen: seed {seed} is not reproducible"); bad += 1
            p = truth["params"]
            # Every truth line holds, as written, what its family matches: a check of the construction alone.
            holds = {"ret-decl": lambda t: "function " in t and f"): {p['target']} {{" in t,
                     "ret-arrow": lambda t: t.lstrip("export ").startswith("const ") and f"): {p['arrow_target']} =>" in t,
                     "zero-calls": lambda t: f"{p['zero_fn']}(" in t and not t.lstrip().startswith(("//", "export")),
                     "maybe-defined": lambda t: any(f"export {k} {p['maybe']}" in t for k in ("type", "interface", "function", "class")),
                     "public-surface": lambda t: t.startswith("export "), "public-types": lambda t: t.startswith("export type {"),
                     "await-any": lambda t: "async function " in t, "log-in-try": lambda t: f"{p['log_fn']}(id" in t,
                     "emit-calls": lambda t: "$emit(" in t, "track-calls": lambda t: "$track(" in t,
                     "fn-outline": lambda t: f"function {p['line_fn']}(" in t, "itf-outline": lambda t: f"interface {p['line_itf']} " in t}
            holds.update(gen2.holds(p))
            for fam, lines in truth["answers"].items():
                for spot in lines:
                    path, n = spot.rsplit(":", 1)
                    text = open(os.path.join(d, "repo", path)).read().split("\n")[int(n) - 1]
                    if not holds[fam](text):
                        print(f"gen: seed {seed}: {fam} points at {text!r}"); bad += 1
            seen.add(truth["params"]["target"])
    if len(seen) < 5:
        print(f"gen: 20 seeds drew only {len(seen)} target types"); bad += 1
    print(f"gen: 20 seeds, {bad} problem(s)")
    return 1 if bad else 0


if __name__ == "__main__":
    if sys.argv[1:] == ["--selftest"]:
        sys.exit(selftest())
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    write(int(sys.argv[1]), sys.argv[2], sys.argv[3])
