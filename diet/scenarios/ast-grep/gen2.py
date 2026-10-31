"""The fixture's decoy families (#971, 2026-10-29): `gen.build` calls `build` after the task families' draws.

No task asks for these families: their content is more for a task's command to wrongly catch, on the same instances
the tasks run on (ref.py checks the tasks' references there). Every line here comes after the task families' draws.
One barrel export (`export { a as b } from …`) joins the barrel's lines, so it is in t3's public-surface truth; no other
line is in a task family's truth. Each family keeps the truth of the lines the generator wrote, as the tasks' do, and
`gen.usable` still requires them, so the instances, and the seed each resolves to, are what they were when these
families had tasks of their own. Families, by gap:
- return type: `ret-method` (class methods returning T), `ret-async` (async declarations returning `Promise<T>`);
- miss: `zero-new` (`new C(…)` of a class rarely built), `itf-maybe` (an interface that may not exist);
- barrel: `public-aliases` (barrel names re-exported under an alias), `public-values` (barrel statements exporting values);
- stop-by: `throw-in-for` (`throw` inside a `for (;;)` at any depth), `report-in-catch` (calls inside a `catch`, any depth);
- dollar: `store-get` (`$store.get(…)`), `log-calls` (`$log(…)`);
- outline: `cls-outline` (one class), `alias-outline` (one type alias);
- kind filter: `kind-dir`, `kind-file`, `kind-hidden` (an item whose kind varies by seed: under src/, in one file, unexported);
- field: `init-call` (variables initialised by a call to X), `cond-call` (`if`s whose condition is a call to X),
  `prop-arrow` (object properties whose value is an arrow);
- function kinds: `fexpr-await` (function expressions with an await), `gen-decl` (generator declarations),
  `nested-decl` (declarations nested in a declaration);
- `...`: `spread-call` (calls to X with a spread argument), `rest-decl` (declarations with a rest parameter),
  `array-spread` (array literals with a spread);
- exactly one: `one-call` (calls to X with one argument), `two-params` (declarations with exactly two parameters),
  `none-call` (calls to X with no argument).
"""
import re

KINDS = ("type", "interface", "function", "class")


def item(g, path, name, kind, exported=True):
    """One item of KIND named NAME: its first line's number."""
    exp = "export " if exported else ""
    text = {"type": f"{exp}type {name} = {{ name: string }};", "interface": f"{exp}interface {name} {{ name: string }}",
            "function": f"{exp}function {name}(name: string): void {{ void name; }}", "class": f"{exp}class {name} {{ name = ''; }}"}[kind]
    line = g.emit(path, text)
    g.emit(path, "")
    return line


def body(g, path, lines):
    """A function wrapping LINES (each `(text, family or None)`): the families' `path:line`s."""
    out = {}
    g.emit(path, f"export function {g.fn_name()}(id: string): void {{")
    for text, fam in lines:
        at = g.emit(path, "  " + text)
        if fam:
            out.setdefault(fam, []).append(f"{path}:{at}")
    g.emit(path, "  void id;")
    g.emit(path, "}")
    g.emit(path, "")
    return out


def build(g, rng, paths, helper, ctx):
    """CTX: the task families' `target`, `other`, `funcs`, `desk_method` and `fexpr` (`path:line`s), and `barrel_lines` (the list
    the barrel's lines go in, the tasks' `public-surface` truth). Returns (params, answers)."""
    from gen import NOUNS, cap
    target, other = ctx["target"], ctx["other"]
    pick = lambda: rng.choice(paths[1:])
    ans = {k: [] for k in ("ret-method", "ret-async", "zero-new", "itf-maybe", "public-aliases", "public-values",
                           "throw-in-for", "report-in-catch", "store-get", "log-calls", "cls-outline", "alias-outline",
                           "kind-dir", "kind-file", "kind-hidden", "init-call", "cond-call", "prop-arrow", "fexpr-await",
                           "gen-decl", "nested-decl", "spread-call", "rest-decl", "array-spread", "one-call", "two-params",
                           "none-call")}
    add = lambda found: [ans[k].extend(v) for k, v in found.items()]
    p = {}

    # Return type: methods. The tasks' Desk method returns the target; a second class has one more, and look-alikes.
    ans["ret-method"].append(ctx["desk_method"])
    board_path, board = pick(), g.word(NOUNS, lambda w: cap(w) + "Board")
    g.emit(board_path, f"export class {board} {{")
    ans["ret-method"].append(f"{board_path}:{g.emit(board_path, f'  {g.fn_name()}(id: string): {target} {{ return {{ id }} as never; }}')}")
    g.emit(board_path, f"  {g.fn_name()}(id: string): Promise<{target}> {{ return {{ id }} as never; }}")
    g.emit(board_path, f"  {g.fn_name()}(id: string): {other} {{ return {{ id }} as never; }}")
    g.emit(board_path, f"  {g.fn_name()}(id: string) {{ return id; }}")
    g.emit(board_path, f"  {g.fn_name()} = (id: string): {target} => ({{ id }} as never);")
    g.emit(board_path, "}")
    g.emit(board_path, "")
    # Async declarations returning Promise<target>: the tasks', and one sure of each, async and not.
    ans["ret-async"] += [f"{f['path']}:{f['line']}" for f in ctx["funcs"] if f["kind"] == "decl" and f["awaits"] != "none"
                         and f["ret"] == f"Promise<{target}>"]
    ap = pick()
    ans["ret-async"].append(f"{ap}:{g.emit(ap, f'export async function {g.fn_name()}(id: string): Promise<{target}> {{ return {{ id }} as never; }}')}")
    g.emit(ap, f"export function {g.fn_name()}(id: string): Promise<{target}> {{ return Promise.resolve({{ id }} as never); }}")
    g.emit(ap, f"export async function {g.fn_name()}(id: string): Promise<{other}> {{ return {{ id }} as never; }}")
    g.emit(ap, "")

    # Miss: `new Kit(...)`, 0 to 2 times; a near-name class built, a static call, a comment.
    kit = g.word(NOUNS, lambda w: cap(w) + "Kit")
    kp = pick()
    g.emit(kp, f"export class {kit} {{ static make(): {kit} {{ return new {kit}View() as never; }} }}")
    g.emit(kp, f"export class {kit}View {{}}")
    g.emit(kp, "")
    news = rng.choice([0, 0, 1, 2])
    add(body(g, pick(), [(f"const made{i} = new {kit}('{rng.choice(NOUNS)}');", "zero-new") for i in range(news)]
             + [(f"void {kit}.make();", None), (f"// new {kit}() is built by its static make", None)]))
    p["new_cls"] = kit
    # An interface that may not exist: a comment always, a near-name always, the interface half the time.
    shape = g.word(NOUNS, lambda w: cap(w) + "Shape")
    sp = pick()
    g.emit(sp, f"// {shape} was planned here.")
    g.emit(sp, f"export interface {shape}View {{ id: string }}")
    if rng.random() < 0.5:
        ans["itf-maybe"].append(f"{sp}:{g.emit(sp, f'export interface {shape} {{ id: string }}')}")
    g.emit(sp, "")
    p["maybe_itf"] = shape

    # Barrel: one sure alias (the tasks' families add some by chance), and a module's own alias (a directory search catches it).
    alias_src = pick()
    aliased = g.fn_name()
    g.emit(alias_src, f"export function {aliased}(id: string): void {{ void id; }}")
    g.emit(alias_src, f"function {g.fn_name()}Local(id: string): void {{ void id; }}")
    g.emit(alias_src, "")
    local = g.fn_name()
    g.emit(alias_src, f"const {local} = (id: string): void => void id;")
    g.emit(alias_src, f"export {{ {local} as {local}Alias }};")
    g.emit(alias_src, "")
    at = f"src/index.ts:{g.emit('src/index.ts', f'export {{ {aliased} as {aliased}Public }} from {chr(39)}./{alias_src[4:-3]}{chr(39)};')}"
    ctx["barrel_lines"].append(at)
    barrel = g.files["src/index.ts"]
    ans["public-aliases"] = sorted(f"src/index.ts:{i + 1}" for i, line in enumerate(barrel) if " as " in line)
    ans["public-values"] = sorted(f"src/index.ts:{i + 1}" for i, line in enumerate(barrel) if line.startswith("export {"))

    # Stop-by: throws inside `for (;;)`, at depth 1 and 2; throws in a for-of, a while and no loop; reports in catch.
    tp = pick()
    g.emit(tp, f"export function {g.fn_name()}(n: number): void {{")
    g.emit(tp, "  for (let i = 0; i < n; i++) {")
    ans["throw-in-for"].append(f"{tp}:{g.emit(tp, '    if (i > 9) throw new Error(String(i));')}")
    g.emit(tp, "    if (i > 3) {")
    ans["throw-in-for"].append(f"{tp}:{g.emit(tp, '      throw new Error(`deep ${i}`);')}")
    g.emit(tp, "    }")
    g.emit(tp, "  }")
    g.emit(tp, "  for (const part of [n]) {")
    g.emit(tp, "    if (part < 0) throw new Error('of');")
    g.emit(tp, "  }")
    g.emit(tp, "  while (n > 99) throw new Error('while');")
    g.emit(tp, "  if (n < -9) throw new Error('none');")
    g.emit(tp, "}")
    g.emit(tp, "")
    report = g.fn_name()
    g.emit(helper, f"export function {report}(err: unknown): void {{ void err; }}")
    g.emit(helper, "")
    rp = pick()
    g.emit(rp, f"export function {g.fn_name()}(id: string): void {{")
    g.emit(rp, "  try {")
    g.emit(rp, f"    {report}(id);")
    g.emit(rp, "  } catch (err) {")
    ans["report-in-catch"].append(f"{rp}:{g.emit(rp, f'    {report}(err);')}")
    g.emit(rp, "    if (err) {")
    ans["report-in-catch"].append(f"{rp}:{g.emit(rp, f'      {report}(String(err));')}")
    g.emit(rp, "    }")
    g.emit(rp, "  }")
    g.emit(rp, f"  {report}(null);")
    g.emit(rp, "}")
    g.emit(rp, "")
    p["report_fn"] = report

    # Dollar: `$store.get(...)` and `$log(...)`, with `$store.set`, `store.get`, `$logger` and `log` beside them.
    g.emit(helper, "export const $store = { get(k: string): string { return k; }, set(k: string): void { void k; } };")
    g.emit(helper, "export const store = { get(k: string): string { return k; } };")
    g.emit(helper, "export function $log(m: string): void { void m; }")
    g.emit(helper, "export function $logger(m: string): void { void m; }")
    g.emit(helper, "export function log(m: string): void { void m; }")
    g.emit(helper, "")
    calls = [(f"$store.get('{rng.choice(NOUNS)}');", "store-get") for _ in range(rng.choice([1, 2, 3]))]
    calls += [("$store.set('k');", None), ("store.get('k');", None)]
    calls += [(f"$log('{rng.choice(NOUNS)}');", "log-calls") for _ in range(rng.choice([1, 2, 3]))]
    calls += [("$logger('m');", None), ("log('m');", None)]
    rng.shuffle(calls)
    add(body(g, pick(), calls))

    # Outline: one class and one type alias, each with a near-name of each side.
    hub, alias = g.word(NOUNS, lambda w: cap(w) + "Hub"), g.word(NOUNS, lambda w: cap(w) + "Alias")
    op = pick()
    ans["cls-outline"].append(f"{op}:{g.emit(op, f'export class {hub} {{ id = 0; }}')}")
    g.emit(op, f"export class {hub}View {{ id = 0; }}")
    g.emit(op, f"export class Legacy{hub} {{ id = 0; }}")
    ans["alias-outline"].append(f"{op}:{g.emit(op, f'export type {alias} = {{ id: string }};')}")
    g.emit(op, f"export type {alias}View = {{ id: string }};")
    g.emit(op, f"export type Legacy{alias} = {{ id: string }};")
    g.emit(op, "")
    p["line_cls"], p["line_alias"] = hub, alias

    # Kind filter: three items, each of a kind drawn per seed; near-names beside each.
    for fam, key, exported in (("kind-dir", "kind_dir", True), ("kind-file", "kind_file_item", True), ("kind-hidden", "kind_hidden", False)):
        kind = rng.choice(KINDS)
        name = g.word(NOUNS, lambda w: cap(w) + "Item") if kind != "function" else g.fn_name() + "Item"
        path = pick()
        ans[fam].append(f"{path}:{item(g, path, name, kind, exported)}")
        item(g, path, name + "View", rng.choice(KINDS), exported)
        p[key] = name
        if fam == "kind-file":
            p["kind_file"] = path

    # Field: initialisers that are a call to X; `if`s whose condition is one; properties whose value is an arrow.
    init, cond = g.fn_name(), g.fn_name()
    g.emit(helper, f"export function {init}(k: string): string {{ return k; }}")
    g.emit(helper, f"export function {cond}(k: string): boolean {{ return k.length > 0; }}")
    g.emit(helper, "export function wrap(k: string): string { return k; }")
    g.emit(helper, "")
    lines = [(f"const v{i} = {init}('{rng.choice(NOUNS)}');", "init-call") for i in range(rng.choice([1, 2]))]
    lines += [(f"let w0 = {init}('w');", "init-call"), (f"const x0 = wrap({init}('x'));", None), (f"{init}('bare');", None),
              (f"const y0 = wrap('y');", None)]
    lines += [(f"if ({cond}(id)) {{ void 0; }}", "cond-call"), (f"if (!{cond}(id)) {{ void 0; }}", None),
              (f"if (id) {{ {cond}(id); }}", None), (f"while ({cond}('')) {{ break; }}", None)]
    lines += [("const conf = {", None), ("  run: (k: string) => k,", "prop-arrow"), ("  nested: {", None), ("    go: () => 0,", "prop-arrow"), ("  },", None),
              ("  plain: 1,", None), ("  meth(k: string) { return k; },", None), ("};", None)]
    # `nested: { go: () => 0 }`: `go` is a property whose value is an arrow, on that line.
    add(body(g, pick(), lines))
    p["init_fn"], p["cond_fn"] = init, cond

    # Function kinds: expressions with an await (the tasks' one among them), generators, nested declarations.
    fp = pick()
    for text in [f"export const {g.fn_name()} = async function (id: string): Promise<void> {{ await Promise.resolve(id); }};",
                 f"export const {g.fn_name()} = async function named(id: string): Promise<void> {{ await Promise.resolve(id); }};"]:
        ans["fexpr-await"].append(f"{fp}:{g.emit(fp, text)}")
    g.emit(fp, f"export const {g.fn_name()} = async function (id: string): Promise<void> {{ void id; }};")
    g.emit(fp, f"export const {g.fn_name()} = async (id: string): Promise<void> => {{ await Promise.resolve(id); }};")
    ans["gen-decl"].append(f"{fp}:{g.emit(fp, f'export function* {g.fn_name()}(n: number): Generator<number> {{ yield n; }}')}")
    ans["gen-decl"].append(f"{fp}:{g.emit(fp, f'function* {g.fn_name()}(): Generator<number> {{ yield 1; }}')}")
    g.emit(fp, f"export const {g.fn_name()} = function* (): Generator<number> {{ yield 2; }};")
    g.emit(fp, f"export function {g.fn_name()}(): void {{")
    ans["nested-decl"].append(f"{fp}:{g.emit(fp, f'  function {g.fn_name()}(): void {{ void 0; }}')}")
    g.emit(fp, f"  const inner = (): void => {{ void 0; }};")
    g.emit(fp, "  inner();")
    g.emit(fp, "}")
    g.emit(fp, f"export const {g.fn_name()} = (): void => {{")
    g.emit(fp, f"  function {g.fn_name()}(): void {{ void 0; }}")
    g.emit(fp, "};")
    g.emit(fp, "")
    # The tasks' function expression with an await (gen.py's `wide` file) is one of this family's.
    ans["fexpr-await"] += ctx["fexpr"]

    # `...`: spread arguments, rest parameters, array spreads (object spreads beside them).
    spread, one, none = g.fn_name(), g.fn_name(), g.fn_name()
    g.emit(helper, f"export function {spread}(...xs: number[]): void {{ void xs; }}")
    g.emit(helper, f"export function {one}(...xs: unknown[]): void {{ void xs; }}")
    g.emit(helper, f"export function {none}(...xs: unknown[]): void {{ void xs; }}")
    g.emit(helper, "")
    calls = [(f"{spread}(...[{i}, 2]);", "spread-call") for i in range(rng.choice([1, 2]))]
    calls += [(f"{spread}(1, 2);", None), (f"{spread}([3] as never);", None), (f"wrap(...['s'] as [string]);", None)]
    calls += [(f"{one}('{rng.choice(NOUNS)}');", "one-call") for _ in range(rng.choice([1, 2]))]
    calls += [(f"{one}();", None), (f"{one}('a', 'b');", None), (f"{one}('a', 'b', 'c');", None)]
    calls += [(f"{none}();", "none-call") for _ in range(rng.choice([1, 2]))]
    calls += [(f"{none}('x');", None)]
    calls += [("const arr0 = [...[1], 2];", "array-spread"), ("const arr1 = [0, ...[1]];", "array-spread"),
              ("const arr2 = [1, 2];", None), ("const obj0 = { ...{ a: 1 } };", None)]
    rng.shuffle(calls)
    add(body(g, pick(), calls))
    p["spread_fn"], p["one_fn"], p["none_fn"] = spread, one, none
    rp2 = pick()
    for text in [f"export function {g.fn_name()}(...xs: number[]): void {{ void xs; }}",
                 f"export function {g.fn_name()}(a: string, ...rest: number[]): void {{ void a; void rest; }}"]:
        ans["rest-decl"].append(f"{rp2}:{g.emit(rp2, text)}")
    g.emit(rp2, f"export function {g.fn_name()}(xs: number[]): void {{ void xs; }}")
    # Rest patterns that are no declaration's parameter: a destructuring, and an arrow's.
    g.emit(rp2, "export const [head0, ...tail0] = [1, 2];")
    g.emit(rp2, f"export const {g.fn_name()} = (...xs: number[]): void => void xs;")
    g.emit(rp2, "")
    # The decoys' helpers with a rest parameter are rest declarations too.
    for i, line in enumerate(g.files[helper]):
        if re.match(r"export function \w+\(\.\.\.", line):
            ans["rest-decl"].append(f"{helper}:{i + 1}")

    # Exactly two parameters: one untyped and one typed declaration sure; every declaration counted after.
    tw = pick()
    g.emit(tw, f"export function {g.fn_name()}(a: string, b: number) {{ void a; void b; }}")
    g.emit(tw, f"export function {g.fn_name()}(a: string, b: number): {other} {{ return {{ a, b }} as never; }}")
    g.emit(tw, f"export function {g.fn_name()}(a: string, b: number, c: boolean): void {{ void a; void b; void c; }}")
    g.emit(tw, "")
    return p, ans


DECL = re.compile(r"^\s*(?:export )?(?:async )?function (\w+)\((.*?)\)(?::[^{]*)?\s*\{")


def params_of(text):
    """The parameter count of a parameter list written by this generator: commas at depth 0, plus one."""
    depth, n = 0, 0 if not text.strip() else 1
    for ch in text:
        depth += ch in "([{<"
        depth -= ch in ")]}>"
        n += ch == "," and depth == 0
    return n


def two_params(files):
    """Every function declaration the generator wrote (not a generator, not an expression) with exactly two
    parameters, `path:line`: read from the generator's own lines, never from ast-grep."""
    out = []
    for path, lines in files.items():
        for i, line in enumerate(lines):
            m = DECL.match(line)
            if m and params_of(m.group(2)) == 2:
                out.append(f"{path}:{i + 1}")
    return sorted(out)


def holds(p):
    """Per decoy family, what each truth line holds as written: a check of the construction alone (gen.py's selftest)."""
    t = p["target"]
    return {
        "ret-method": lambda s: s.startswith("  ") and f"): {t} {{" in s and "function" not in s,
        "ret-async": lambda s: "async function " in s and f"): Promise<{t}> {{" in s,
        "zero-new": lambda s: f"new {p['new_cls']}(" in s,
        "itf-maybe": lambda s: s.startswith(f"export interface {p['maybe_itf']} "),
        "public-aliases": lambda s: s.startswith("export {") and " as " in s,
        "public-values": lambda s: s.startswith("export {"),
        "throw-in-for": lambda s: "throw new Error" in s and "of" not in s.split("throw")[1][:12],
        "report-in-catch": lambda s: f"{p['report_fn']}(" in s,
        "store-get": lambda s: "$store.get(" in s,
        "log-calls": lambda s: "$log(" in s,
        "cls-outline": lambda s: s.startswith(f"export class {p['line_cls']} "),
        "alias-outline": lambda s: s.startswith(f"export type {p['line_alias']} "),
        "kind-dir": lambda s: f" {p['kind_dir']} " in s or f" {p['kind_dir']}(" in s,
        "kind-file": lambda s: f" {p['kind_file_item']} " in s or f" {p['kind_file_item']}(" in s,
        "kind-hidden": lambda s: not s.startswith("export") and (f" {p['kind_hidden']} " in s or f" {p['kind_hidden']}(" in s),
        "init-call": lambda s: s.lstrip().startswith(("const ", "let ")) and f"= {p['init_fn']}(" in s,
        "cond-call": lambda s: f"if ({p['cond_fn']}(" in s,
        "prop-arrow": lambda s: "=> " in s and ":" in s,
        "fexpr-await": lambda s: "= async function" in s,
        "gen-decl": lambda s: "function* " in s and "= function*" not in s,
        "nested-decl": lambda s: s.startswith("  function "),
        "spread-call": lambda s: f"{p['spread_fn']}(..." in s,
        "rest-decl": lambda s: "function " in s and "..." in s,
        "array-spread": lambda s: "= [" in s and "..." in s,
        "one-call": lambda s: re.search(rf"{p['one_fn']}\('[a-z]+'\)", s) is not None,
        "two-params": lambda s: (m := DECL.match(s)) is not None and params_of(m.group(2)) == 2,
        "none-call": lambda s: f"{p['none_fn']}()" in s,
    }
