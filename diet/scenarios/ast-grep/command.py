"""The answer is a command (#971): parse it, refuse what it may not be, and run what it may, confined.

The model's answer is model-written text, run by the grader out of the model's sight. So:
- **Parsed by bash, never by a lexer of ours** (`words`): bash runs it twice, confined, with `ast-grep` and `sg` a
  recorder that runs nothing. Refused: anything but exactly one recorded call, so an operator (`;`, `|`, `&`, a
  redirection, a subshell), a substitution, a backtick or another program; a `$` bash expands (the two runs differ
  in what it would expand to, and an unset variable fails); an assignment before the command; input fed to it; and
  a parse error. A `$` bash leaves literal, and ANSI-C quoting (`$'…'`), are read as bash reads them.
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
import json, os, shutil, subprocess, sys, tempfile

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
FORMAT = {"--report-style", "--color", "--heading", "--format"}


class Refused(Exception):
    pass


MARK = "@@diet-recorder@@"
# The recorder stands in for `ast-grep` and `sg` while bash parses an answer: shell builtins only, so the parse run
# may exec nothing but bash and it (sh only as its interpreter). It prints, between markers: its argv, NUL-separated;
# the values of the variables bash was given, and SHLVL; every exported name (`export -p`); and `stdin` when it was
# fed any.
RECORDER = ("#!/bin/sh\n"
            f"printf '%s' '{MARK}'; printf '%s\\0' \"$0\" \"$@\"\n"
            f"printf '%s' '{MARK}'; printf '%s\\0' \"$PATH\" \"$HOME\" \"$DIET_PROBE\" \"$SHLVL\"\n"
            f"printf '%s' '{MARK}'; export -p\n"
            f"printf '%s' '{MARK}'; if IFS= read -r line || [ -n \"$line\" ]; then printf 'stdin'; fi\n"
            f"printf '%s\\n' '{MARK}'\n")
# Bash's own exports, beyond the variables it was given.
BASH_OWN = {"_", "PWD", "OLDPWD", "SHLVL"}
# SHLVL as the recorder sees it when the parse run's bash started it: bash starts at 1 (it is given none), and the
# recorder's sh counts one more. A bash the answer starts (`/bin/bash -c …`) counts another, and the parse run's own
# SHLVL is readonly, so an assignment can't take that back.
RECORDER_SHLVL = "2"
# Before the answer: unset variables fail, SHLVL is fixed, and the DEBUG trap writes every simple command bash is
# about to run, with its subshell depth, to the trace pipe, fd FD (inside substitutions too, `set -T`). The answer's
# commands are the ones traced after SENTINEL. After the answer, CODA writes `$!`, set only if the answer started a
# background job; an answer that ends bash before it (`exit`, `exec`), or clears the trap, never shows it traced.
SENTINEL = f"0 : {MARK}"
# Bash 3.2 calls `$!` unbound under `set -u` whatever its default, so the coda lifts it first.
CODA = "set +u\nprintf 'END:%s\\0' \"$!\" >&{fd}"
CODA_TRACED = ["0 set +u", "0 printf 'END:%s\\0' \"$!\" 1>&{fd}"]
PRELUDE = ("set -u\nset -T\nreadonly SHLVL\ntrap 'printf \"%s\\0\" \"$BASH_SUBSHELL $BASH_COMMAND\" >&{fd}' DEBUG\n"
           + SENTINEL[2:] + "\n")


def parse_profile(root, rec):
    """command.py's profile for ROOT, and exec of bash and the recorder alone, sh only as the recorder's interpreter."""
    return (profile(root) + "(deny process-exec)"
            f"(allow process-exec (literal \"/bin/bash\") (subpath \"{os.path.realpath(rec)}\"))"
            "(allow process-exec-interpreter (literal \"/bin/sh\"))")


def recorded(command, run):
    """Bash's words for COMMAND in one parse run, numbered RUN (0 or 1), or Refused. The runs differ in the working
    directory, HOME, PATH (the recorder's own directory), a variable of the drive's (`DIET_PROBE`) and the positional
    parameters, so a word that expands to any of them differs between the runs."""
    with tempfile.TemporaryDirectory(prefix=f"parse-{run}-") as d:
        work, home, rec = (os.path.join(d, n) for n in ("work", "home", "bin"))
        for p in (work, home, rec):
            os.makedirs(p)
        for name in TOOLS:
            path = os.path.join(rec, name)
            with open(path, "w") as f:
                f.write(RECORDER)
            os.chmod(path, 0o755)
        env = {"PATH": rec, "HOME": home, "DIET_PROBE": f"probe-{run}"}
        trace_r, trace_w = os.pipe()
        try:
            cmd = ["/usr/bin/sandbox-exec", "-p", parse_profile(work, rec), "/bin/bash", "--noprofile", "--norc", "-c",
                   PRELUDE.format(fd=trace_w) + command + "\n" + CODA.format(fd=trace_w) + "\n", f"arg0-{run}", f"arg1-{run}"]
            try:
                done = subprocess.run(cmd, cwd=work, env=env, capture_output=True, timeout=TIMEOUT_S,
                                      stdin=subprocess.DEVNULL, pass_fds=(trace_w,))
            except subprocess.TimeoutExpired:
                raise Refused(f"bash ran past {TIMEOUT_S} s on it")
        finally:
            os.close(trace_w)
            with os.fdopen(trace_r, "rb") as f:
                traced = f.read().decode("utf-8", "replace").split("\0")[:-1]
        out, err = done.stdout.decode("utf-8", "replace"), done.stderr.decode("utf-8", "replace").strip()
        said = f" (bash said: {err[:160]})" if err else ""
        if SENTINEL not in traced:
            raise Refused("bash never reached the answer" + said)
        traced = traced[traced.index(SENTINEL) + 1:]
        coda = [t.format(fd=trace_w) for t in CODA_TRACED]
        if len(traced) < 3 or traced[-3:-1] != coda or not traced[-1].startswith("END:"):
            raise Refused("bash never finished the answer as written (an `exit`, an `exec`, a cleared trap, or an error)" + said)
        if traced[-1] != "END:":
            raise Refused("it starts a background job (`&`)")
        traced = traced[:-3]
        if any(not t.startswith("0 ") for t in traced):
            raise Refused("it runs in a subshell (`( … )`)")
        parts = out.split(MARK)
        calls = (len(parts) - 1) // 5
        if len(traced) != 1 or calls != 1:
            raise Refused(f"it is not one ast-grep call: bash ran {len(traced)} command(s), and the recorder {calls} time(s)" + said)
        argv, given, exported, stdin = parts[1].split("\0")[:-1], parts[2].split("\0")[:-1], parts[3], parts[4]
        if stdin:
            raise Refused("it feeds ast-grep input (a heredoc, a here-string or `<`), which the check never gives it")
        if given[3:] != [RECORDER_SHLVL]:
            raise Refused(f"ast-grep runs under a shell the answer starts (SHLVL {given[3:]})")
        names = {l.split()[1].split("=", 1)[0] for l in exported.splitlines() if l.startswith("export ") and len(l.split()) > 1}
        if given[:3] != [env["PATH"], env["HOME"], env["DIET_PROBE"]] or names - set(env) - BASH_OWN:
            raise Refused("it sets the environment ast-grep runs in (an assignment before the command)")
        return [os.path.basename(argv[0])] + argv[1:]


def words(command):
    """The command's words as bash reads them, or Refused. Bash parses it, never a lexer of ours: twice, confined,
    with `ast-grep` and `sg` a recorder that runs nothing. It must be exactly one call of the recorder, from the parse
    run's bash itself, and nothing else, with no input fed to it, no environment of its own, and the same words in
    both runs. So no operator, substitution or other program passes, and neither does a `$` bash expands; one bash
    leaves literal (`$'…'`, a `$` before a quote or at a word's end) is read as bash reads it. macOS only: off it,
    bash would run the model's text unconfined, so every answer is refused."""
    if sys.platform != "darwin":
        raise Refused("an answer is parsed by bash under Seatbelt, which this host lacks (macOS only)")
    if not command.strip():
        raise Refused("it is empty")
    first, second = recorded(command, 0), recorded(command, 1)
    if first != second:
        raise Refused("it expands to what the shell's environment holds (a `$` that bash expands)")
    return first


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
    # The check reads JSON, so it drops the command's own output format (--json, and --report-style, --color, --heading
    # and --format, which ast-grep won't take beside --json), with a separate value, then asks for --json=stream.
    args, skip = [], False
    for a in argv[1:]:
        if skip:
            skip = False
            continue
        flag = a.split("=", 1)[0]
        if flag == "--json":
            continue
        if flag in FORMAT:
            skip = "=" not in a
            continue
        args.append(a)
    args.append("--json=stream")
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
    # Bash as the parser (2026-10-31): what it would expand, run, feed or set.
    ("a home variable", 'ast-grep run -p "$HOME" src'),
    ("an unset variable", 'ast-grep run -p "${x}" src'),
    ("the search path", 'ast-grep run -p "$PATH" src'),
    ("the working directory", 'ast-grep run -p "$PWD" src'),
    ("a positional parameter", 'ast-grep run -p "$1" src'),
    ("an unquoted variable of the drive's", "ast-grep run -p $DIET_PROBE src"),
    ("a no-op after it", "ast-grep run -p 'x' src; true"),
    ("two searches", "ast-grep run -p 'x' src; ast-grep run -p 'y' src"),
    ("a process substitution", "ast-grep run -p 'x' <(echo src)"),
    ("a here-string", "ast-grep run -p 'x' src <<< hi"),
    ("a heredoc", "ast-grep run -p 'x' src <<EOF\nhi\nEOF"),
    ("input from a readable file", "ast-grep run -p 'x' src < /etc/hosts"),
    ("env before it", "env ast-grep run -p 'x' src"),
    ("env by its path", "/usr/bin/env ast-grep run -p 'x' src"),
    ("clearing the trap", "trap - DEBUG; ast-grep run -p 'x' src; true"),
    ("clearing it through builtin", "builtin trap - DEBUG; ast-grep run -p 'x' src; true"),
    ("an export first", "export X=1; ast-grep run -p 'x' src"),
    ("a PATH assignment", "PATH=/bin ast-grep run -p 'x' src"),
    ("a HOME assignment", "HOME=/tmp ast-grep run -p 'x' src"),
    ("exec in bash's place", "exec ast-grep run -p 'x' src"),
    ("bash by its path", "/bin/bash -c 'ast-grep run -p x src'"),
    ("bash by its variable", "\"$BASH\" -c 'ast-grep run -p x src'"),
    ("bash with SHLVL assigned", "SHLVL=0 /bin/bash -c 'ast-grep run -p x src'"),
    ("sh by its path", "/bin/sh -c 'ast-grep run -p x src'"),
    ("sh by its path, resetting SHLVL inside", "/bin/sh -c 'SHLVL=1 ast-grep run -p x src'"),
    ("an echo of the command", "echo ast-grep run -p 'x' src"),
]
# A declared limit, pinned as accepted (Dispatch, 2026-10-31; README, "Declared limits"): a bash the answer starts can
# reset SHLVL before ast-grep runs, and nothing else tells it from the parse run's own bash. It is still graded on the
# words bash gives ast-grep. A fix that closes it fails this case, visibly.
LIMITS = [("an inner bash resetting SHLVL", "/bin/bash -c 'SHLVL=1 ast-grep run -p x src'", ["ast-grep", "run", "-p", "x", "src"])]
# What bash makes of an answer: its words, exactly (command_words.json, the regrade's five false wrongs among them).
WORDS = json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "command_words.json")))
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
    real, sys.platform = sys.platform, "linux"
    try:
        if refusal("ast-grep run -p 'x' src") is None:
            print("command: off macOS an answer was not refused"); bad += 1
    finally:
        sys.platform = real
    if sys.platform != "darwin":
        print(f"command: skipped: bash parses answers under Seatbelt, on macOS only; off it every answer is refused, {bad} wrong")
        return 1 if bad else 0
    for c in WORDS:
        try:
            got = words(c["command"])
        except Refused as e:
            got = f"refused: {e}"
        if got != c["words"]:
            print(f"command: words, {c['why']}: {got!r}"); bad += 1
    for why, cmd, want in LIMITS:
        try:
            got = words(cmd)
        except Refused as e:
            got = f"refused: {e}"
        if got != want:
            print(f"command: the declared limit '{why}' changed (update README's declared limits): {got!r}"); bad += 1
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
            # Output-format flags can't sit beside --json (ast-grep refuses --report-style with it): the check drops them.
            for styled in ("ast-grep scan --inline-rules 'id: q\nlanguage: ts\nrule:\n  pattern: foo()' --report-style short src",
                           "ast-grep run -p 'foo()' --color never --heading never src"):
                got = matches(checked(styled), root)
                if got != ["src/a.ts:2"]:
                    print(f"command: an output-format flag voided the check's matches: {got}: {styled!r}"); bad += 1
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
    print(f"command: {len(WORDS)} read as bash reads them, {len(REFUSED)} refused and {len(ALLOWED)} allowed, "
          f"{len(LIMITS)} declared limit pinned, {bad} wrong")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(selftest() if sys.argv[1:] == ["--selftest"] else (print(refusal(sys.argv[1]) or "allowed") or 0))
