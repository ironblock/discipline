"""The answer is a command (#971): parse it, refuse what it may not be, and run what it may, confined.

The model's answer is model-written text, run by the grader out of the model's sight. So:
- **Parsed, never a shell.** `shlex`, POSIX rules. Refused: an operator (`;`, `|`, `&`, a redirection, a subshell),
  a backtick, a `$` inside double quotes (a shell would expand it there, so the command is wrong as written), a
  variable assignment, and a parse error.
- **Only ast-grep.** The first word is `ast-grep` or `sg`, by name alone (not a path); it runs the binary
  `ast-grep` names on PATH, never `sg`, which is `newgrp` on Linux.
- **Only a search.** The subcommand is `run` (or none, which is `run`), `outline` or `scan`, and every option one the
  filter knows (ast-grep 0.44.0's, per subcommand), read as clap reads them: short flags cluster (`-Uj1`), a valued
  one takes the rest of its token or the next word, and an optional value (`--debug-query`) only its `=` form.
  Refused: rewriting in place (`-U`, `--update-all`), `--interactive`, a config or rule file (`-c`, `--config`,
  scan's `-r`, `--rule`, `--outline-rules`: the answer holds its rule, `--inline-rules`; run's `-r` is `--rewrite`,
  which only prints), an option it doesn't know, and any path that is absolute, home-relative or climbs (`..`).
- **Confined.** It runs with no shell, in a pristine copy of the fixture, with a bare environment, a timeout and an
  output cap; on macOS under Seatbelt with no network, no writes, and no reads under the home directory but the copy.

`matches(argv, root)` runs it with `--json=stream` in place of any `--json` it gave and returns the matched lines as
`path:line`, lines from 1. `refusal(command)` is the reason a command may not run, or None.
"""
import json, os, shlex, shutil, subprocess, sys, tempfile

TOOLS = {"ast-grep", "sg"}
SEARCHES = {"run", "outline", "scan"}
# ast-grep 0.44.0's options, per subcommand, read from its `--help`. A short flag that takes a value takes the rest of
# its token (`-lts`, `-j1`) or the next word; others may cluster (`-Uj1` is `-U -j 1`), so a cluster is read letter
# by letter. An option not listed here is refused: an answer has no business with one the filter doesn't know.
SHORT_VALUED = {"run": set("pkrlcjABC"), "scan": set("rcjABC"), "outline": set("lcj")}
SHORT_PLAIN = {"run": set("iUh"), "scan": set("iUh"), "outline": set("h")}
SHORT_FORBIDDEN = {"run": set("iUc"), "scan": set("iUcr"), "outline": set("c")}  # `-r` is `--rewrite` in run, `--rule` in scan
LONG_VALUED = {"--pattern", "--kind", "--rewrite", "--lang", "--config", "--threads", "--after", "--before", "--context",
               "--color", "--filter", "--format", "--globs", "--heading", "--inline-rules", "--inspect", "--items", "--match",
               "--max-results", "--no-ignore", "--outline-rules", "--report-style", "--selector", "--strictness", "--type",
               "--view", "--rule"}
# Options whose value is optional: only `--flag=value` carries one, so the next word is never theirs.
LONG_OPTIONAL = {"--debug-query", "--error", "--hint", "--info", "--json", "--off", "--warning"}
LONG_PLAIN = {"--files-with-matches", "--follow", "--include-metadata", "--no-default-outline-rules", "--pub-members",
              "--stdin", "--update-all", "--interactive", "--help"}
LONG_FORBIDDEN = {"--update-all", "--interactive", "--config", "--rule", "--outline-rules"}
TIMEOUT_S, MAX_BYTES = 20, 4 << 20


class Refused(Exception):
    pass


def words(command):
    """The command's words, or Refused: no operators, no backticks, no `$` a shell would expand."""
    if "`" in command:
        raise Refused("a backtick")
    # In double quotes bash turns `\$` into `$`, where shlex would keep the backslash: cook it as bash would.
    quote, i, cooked = None, 0, []
    while i < len(command):
        c = command[i]
        if c == "\\" and quote != "'":
            if quote == '"' and command[i + 1:i + 2] == "$":
                cooked.append("$")
            else:
                cooked.append(command[i:i + 2])
            i += 2
            continue
        if quote is None and c in "'\"":
            quote = c
        elif c == quote:
            quote = None
        elif quote == '"' and c == "$":
            raise Refused("a `$` in double quotes, which the shell expands")
        cooked.append(c)
        i += 1
    lex = shlex.shlex("".join(cooked), posix=True, punctuation_chars=";&|<>()")
    lex.whitespace_split = True
    try:
        toks = list(lex)
    except ValueError as e:
        raise Refused(f"it does not parse: {e}")
    for t in toks:
        if t and set(t) <= set(";&|<>()"):
            raise Refused(f"a shell operator `{t}`")
    if not toks:
        raise Refused("it is empty")
    return toks


LONG_OF = {"p": "--pattern", "k": "--kind", "r": "--rewrite", "l": "--lang", "j": "--threads", "A": "--after",
           "B": "--before", "C": "--context", "i": "--interactive", "U": "--update-all", "h": "--help", "c": "--config"}


def parse(command):
    """The command's canonical form, or Refused: `{"sub", "opts": {long name: [values]}, "flags": [long names],
    "paths": [normalised]}`. A bare optional value (`--json`) is a flag; with `=`, an option."""
    argv = words(command)
    if "=" in argv[0] and not argv[0].startswith("-"):
        raise Refused("a variable assignment")
    if argv[0] not in TOOLS:
        raise Refused(f"`{argv[0]}` is not ast-grep")
    args = argv[1:]
    sub = "run"
    if args and not args[0].startswith("-"):
        if args[0] not in SEARCHES:
            raise Refused(f"`{args[0]}` is not a search")
        sub, rest = args[0], args[1:]
    else:
        rest = args
    form = {"sub": sub, "opts": {}, "flags": [], "paths": [], "argv": ["ast-grep"] + args}
    put = lambda name, value: form["opts"].setdefault(name, []).append(value)
    i, ended = 0, False
    while i < len(rest):
        a = rest[i]
        i += 1
        if ended or not a.startswith("-") or a == "-":
            if a.startswith(("/", "~")) or ".." in a.split("/"):
                raise Refused(f"the path `{a}` leaves the project")
            p = a.rstrip("/") or "."
            while p.startswith("./"):
                p = p[2:]
            form["paths"].append(p or ".")
            continue
        if a == "--":
            ended = True
            continue
        if a.startswith("--"):
            flag, eq, val = a.partition("=")
            if flag in LONG_FORBIDDEN or (sub != "scan" and flag == "--rule") or (sub == "scan" and flag == "--rewrite"):
                raise Refused(f"`{flag}` is not allowed in an answer")
            if flag in LONG_VALUED:
                if not eq:
                    if i >= len(rest):
                        raise Refused(f"`{flag}` has no value")
                    val = rest[i]
                    i += 1
                put(flag, val)
            elif flag in LONG_OPTIONAL:
                put(flag, val) if eq else form["flags"].append(flag)
            elif flag in LONG_PLAIN:
                form["flags"].append(flag)
            else:
                raise Refused(f"`{flag}` is not an option the filter knows")
            continue
        for k, c in enumerate(a[1:], 1):  # a short flag, or a cluster of them
            if c in SHORT_FORBIDDEN[sub]:
                raise Refused(f"`-{c}` is not allowed in `{sub}` in an answer")
            if c in SHORT_VALUED[sub]:
                if k == len(a) - 1:  # its value is the next word
                    if i >= len(rest):
                        raise Refused(f"`-{c}` has no value")
                    put(LONG_OF[c], rest[i])
                    i += 1
                else:  # the rest of the token is its value
                    put(LONG_OF[c], a[k + 1:])
                break
            if c not in SHORT_PLAIN[sub]:
                raise Refused(f"`-{c}` is not an option of `{sub}` the filter knows")
            form["flags"].append(LONG_OF[c])
    return form


def checked(command):
    """The argv to run, or Refused."""
    return parse(command)["argv"]


def profile(root):
    home = os.path.realpath(os.path.expanduser("~"))
    return ("(version 1)(allow default)(deny network*)(deny file-write* (require-not (literal \"/dev/null\")))"
            f"(deny file-read* (require-all (subpath \"{home}\") (require-not (subpath \"{os.path.realpath(root)}\"))))")


def confine(cmd, root):
    """CMD under Seatbelt's profile for ROOT on macOS; as it is elsewhere, where the tutorial does not run."""
    return ["/usr/bin/sandbox-exec", "-p", profile(root)] + cmd if sys.platform == "darwin" else cmd


def run(argv, root):
    """Run a checked argv in ROOT: (exit, stdout)."""
    binary = shutil.which("ast-grep")
    if not binary:
        raise Refused("no ast-grep on PATH")
    args = [a for a in argv[1:] if a.split("=", 1)[0] != "--json"] + ["--json=stream"]
    cmd = confine([binary] + args, root)
    env = {"PATH": "/usr/bin:/bin", "HOME": root, "NO_COLOR": "1"}
    try:
        done = subprocess.run(cmd, cwd=root, env=env, capture_output=True, timeout=TIMEOUT_S, stdin=subprocess.DEVNULL)
    except subprocess.TimeoutExpired:
        raise Refused(f"it ran past {TIMEOUT_S} s")
    return done.returncode, done.stdout[:MAX_BYTES].decode("utf-8", "replace")


def matches(argv, root):
    """The lines the command matches, `path:line` from 1, from its JSON stream."""
    _, out = run(argv, root)
    found = set()
    for line in out.splitlines():
        try:
            item = json.loads(line)
        except ValueError:
            continue
        if "items" in item:  # outline: one object per file
            for it in item["items"]:
                found.add(f"{item['path']}:{it['range']['start']['line'] + 1}")
        elif "range" in item:
            found.add(f"{item['file']}:{item['range']['start']['line'] + 1}")
    return sorted(found)


def refusal(command):
    try:
        checked(command)
        return None
    except Refused as e:
        return str(e)


# Every refusal, red first: each must be refused, and each allowed search must not be.
REFUSED = [
    ("a second command", "ast-grep run -p 'x' src; rm -rf src"),
    ("a pipe", "ast-grep run -p 'x' src | sh"),
    ("an and-list", "ast-grep run -p 'x' src && curl example.com"),
    ("a background job", "ast-grep run -p 'x' src &"),
    ("an output redirection", "ast-grep run -p 'x' src > out.txt"),
    ("an input redirection", "ast-grep run -p 'x' < src/index.ts"),
    ("a subshell", "(ast-grep run -p 'x' src)"),
    ("a command substitution", "ast-grep run -p $(cat pat) src"),
    ("a backtick", "ast-grep run -p `cat pat` src"),
    ("a double-quoted metavariable", 'ast-grep run -p "foo($A)" src'),
    ("an unclosed quote", "ast-grep run -p 'foo src"),
    ("an empty answer", "   "),
    ("a variable assignment", "AST=1 ast-grep run -p 'x' src"),
    ("another tool", "rg -n foo src"),
    ("a shell", "bash -c 'ast-grep run -p x src'"),
    ("a python script", "python3 -c 'import os'"),
    ("ast-grep by path", "/tmp/ast-grep run -p 'x' src"),
    ("a new project", "ast-grep new project"),
    ("the language server", "ast-grep lsp"),
    ("rule tests", "ast-grep test"),
    ("completions", "ast-grep completions bash"),
    ("rewriting in place", "ast-grep run -p 'x' -r 'y' -U src"),
    ("rewriting in place, long", "ast-grep run -p 'x' --rewrite 'y' --update-all src"),
    ("interactive", "ast-grep run -p 'x' -i src"),
    ("a config file", "ast-grep scan -c sgconfig.yml"),
    ("a rule file, joined", "ast-grep scan --rule=rule.yml src"),
    ("an absolute path", "ast-grep run -p 'x' /etc"),
    ("a home path", "ast-grep run -p 'x' ~/secrets"),
    ("a climbing path", "ast-grep run -p 'x' ../services"),
    ("in-place rewrite in a cluster", "ast-grep run -p 'foo()' --rewrite 'bar()' -Uj1 src"),
    ("interactive in a cluster", "ast-grep run -p 'x' -iU src"),
    ("a path after an optional-value flag", "ast-grep run -p 'x' --debug-query /etc"),
    ("a scan's rule file, short", "ast-grep scan -r rule.yml src"),
    ("outline rules from a file", "ast-grep outline src --outline-rules rules.yml"),
    ("a config file at a cluster's end", "ast-grep run -p 'x' -hc sgconfig.yml src"),
    ("an option the filter doesn't know", "ast-grep run -p 'x' --frobnicate src"),
    ("a short option the filter doesn't know", "ast-grep run -p 'x' -Z src"),
    ("a run with a scan's rule flag", "ast-grep run --rule rule.yml src"),
    ("a path after the options' end", "ast-grep run -p 'x' -- /etc"),
]
ALLOWED = [
    ("a run search", "ast-grep run -l ts -p 'function $F($$$A): T { $$$B }' src"),
    ("the default subcommand", "ast-grep -p 'foo($$$A)' src"),
    ("the sg name", "sg run -p 'foo($$$A)' src"),
    ("an outline", "ast-grep outline src --match '^Foo$' --view signatures"),
    ("an inline-rule scan", "ast-grep scan --inline-rules 'id: a\nlanguage: ts\nrule:\n  pattern: await $E' src"),
    ("a pattern holding a semicolon in quotes", "ast-grep run -p 'a; b' src"),
    ("a rewrite that only prints", "ast-grep run -p 'x' --rewrite 'y' src"),
    ("its own json flag", "ast-grep run -p 'x' --json src"),
    ("a single-quoted lowercase dollar", "ast-grep run -p '$emit($$$A)' src"),
    ("an escaped dollar in double quotes", 'ast-grep run -p "\\$emit(\\$\\$\\$A)" src'),
    ("run's -r is a printed rewrite", "ast-grep run -p 'x' -r 'y' src"),
    ("a value glued to its short flag", "ast-grep run -lts -p 'x' src"),
    ("a cluster ending in a valued flag", "ast-grep run -p 'x' -A2 -j1 src"),
    ("a valued flag swallows the rest, as clap reads it", "ast-grep run -p 'x' -jc src"),
    ("an optional value given with =", "ast-grep run -l ts -p 'x' --debug-query=ast src"),
    ("an outline's language", "ast-grep outline -l ts src --match '^Foo$'"),
]


def selftest():
    bad = 0
    for why, cmd in REFUSED:
        if refusal(cmd) is None:
            print(f"command: not refused: {why}: {cmd!r}"); bad += 1
    for why, cmd in ALLOWED:
        r = refusal(cmd)
        if r is not None:
            print(f"command: refused: {why}: {r}"); bad += 1
    if shutil.which("ast-grep") and sys.platform == "darwin":  # the confinement, where the tutorial runs
        with tempfile.TemporaryDirectory() as d:
            root = os.path.join(d, "fixture")
            os.makedirs(os.path.join(root, "src"))
            open(os.path.join(root, "src", "a.ts"), "w").write("export function foo(): number { return 1; }\nfoo();\n")
            outside = os.path.join(os.path.realpath(os.path.expanduser("~")), f".command-probe-{os.getpid()}")
            got = matches(checked("ast-grep run -p 'foo()' src"), root)
            if got != ["src/a.ts:2"]:
                print(f"command: a search in the copy matched {got}"); bad += 1
            # The confinement itself, tested on the profile: no write in the copy, no read under home outside it.
            target = os.path.join(root, "written")
            subprocess.run(confine(["/usr/bin/touch", target], root), capture_output=True)
            if os.path.exists(target):
                print("command: the confinement let a write into the copy"); bad += 1
            probe_dir = os.path.join(os.path.realpath(os.path.expanduser("~")), f"command-probe-{os.getpid()}")
            os.makedirs(probe_dir)
            try:
                open(os.path.join(probe_dir, "secret.txt"), "w").write("outside\n")
                read = subprocess.run(confine(["/bin/cat", os.path.join(probe_dir, "secret.txt")], root), capture_output=True, text=True)
                if "outside" in read.stdout:
                    print("command: the confinement let a read under home outside the copy"); bad += 1
                inside = subprocess.run(confine(["/bin/cat", os.path.join(root, "src", "a.ts")], root), capture_output=True, text=True)
                if "foo" not in inside.stdout:
                    print("command: the confinement blocked a read of the copy itself"); bad += 1
            finally:
                shutil.rmtree(probe_dir)
    print(f"command: {len(REFUSED)} refused and {len(ALLOWED)} allowed, {bad} wrong")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(selftest() if sys.argv[1:] == ["--selftest"] else (print(refusal(sys.argv[1]) or "allowed") or 0))
