"""The answer is a command (#971): parse it, refuse what it may not be, and run what it may, confined.

The model's answer is model-written text, run by the grader out of the model's sight. So:
- **Parsed, never a shell.** `shlex`, POSIX rules. Refused: an operator (`;`, `|`, `&`, a redirection, a subshell),
  a backtick, a `$` inside double quotes (a shell would expand it there, so the command is wrong as written), a
  variable assignment, and a parse error.
- **Only ast-grep.** The first word is `ast-grep` or `sg`, by name alone (not a path); it runs the binary
  `ast-grep` names on PATH, never `sg`, which is `newgrp` on Linux.
- **Only a search.** The subcommand is `run` (or none, which is `run`), `outline` or `scan`. Refused: rewriting
  in place (`-U`, `--update-all`), `--interactive`, a config or rule file (`-c`, `--config`, `-r`, `--rule`: the
  answer must hold its rule, `--inline-rules`), and any path that is absolute, home-relative or climbs (`..`).
- **Confined.** It runs with no shell, in a pristine copy of the fixture, with a bare environment, a timeout and an
  output cap; on macOS under Seatbelt with no network, no writes, and no reads under the home directory but the copy.

`matches(argv, root)` runs it with `--json=stream` in place of any `--json` it gave and returns the matched lines as
`path:line`, lines from 1. `refusal(command)` is the reason a command may not run, or None.
"""
import json, os, shlex, shutil, subprocess, sys, tempfile

TOOLS = {"ast-grep", "sg"}
SEARCHES = {"run", "outline", "scan"}
FORBIDDEN = {"-U", "--update-all", "-i", "--interactive", "-c", "--config", "-r", "--rule"}
VALUED = {"-p", "--pattern", "--rewrite", "--inline-rules", "-m", "--match", "--filter", "-l", "--lang",
          "--selector", "--strictness", "--globs", "-A", "-B", "-C", "--context", "--items", "--view", "-j",
          "--threads", "--kind", "--color", "--format", "--report-style", "--debug-query"}
TIMEOUT_S, MAX_BYTES = 20, 4 << 20


class Refused(Exception):
    pass


def words(command):
    """The command's words, or Refused: no operators, no backticks, no `$` a shell would expand."""
    if "`" in command:
        raise Refused("a backtick")
    quote, i = None, 0
    while i < len(command):
        c = command[i]
        if c == "\\" and quote != "'":
            i += 2
            continue
        if quote is None and c in "'\"":
            quote = c
        elif c == quote:
            quote = None
        elif quote == '"' and c == "$":
            raise Refused("a `$` in double quotes, which the shell expands")
        i += 1
    lex = shlex.shlex(command, posix=True, punctuation_chars=";&|<>()")
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


def checked(command):
    """The argv to run, or Refused."""
    argv = words(command)
    if "=" in argv[0] and not argv[0].startswith("-"):
        raise Refused("a variable assignment")
    if argv[0] not in TOOLS:
        raise Refused(f"`{argv[0]}` is not ast-grep")
    args = argv[1:]
    sub = args[0] if args and not args[0].startswith("-") and args[0] in SEARCHES | {"new", "lsp", "test", "completions", "help"} else None
    if sub is not None and sub not in SEARCHES:
        raise Refused(f"`{sub}` is not a search")
    i = 1 if sub else 0
    while i < len(args):
        a = args[i]
        flag = a.split("=", 1)[0]
        if flag in FORBIDDEN:
            raise Refused(f"`{flag}` is not allowed in an answer")
        if flag in VALUED and "=" not in a:
            i += 2
            continue
        if not a.startswith("-") and (a.startswith(("/", "~")) or ".." in a.split("/")):
            raise Refused(f"the path `{a}` leaves the project")
        i += 1
    return ["ast-grep"] + args


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
    ("a rule file", "ast-grep scan -r rule.yml src"),
    ("a rule file, joined", "ast-grep scan --rule=rule.yml src"),
    ("an absolute path", "ast-grep run -p 'x' /etc"),
    ("a home path", "ast-grep run -p 'x' ~/secrets"),
    ("a climbing path", "ast-grep run -p 'x' ../services"),
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
