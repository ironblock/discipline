"""What counts as reading ast-grep's docs (#971): `reads_of(cmd, skills)`, a parsed classifier of one bash call.

A pass that reads the docs rolls back, so what counts is parsed, never matched as text:
- `help`: `--help` or `-h` to `ast-grep` or `sg` on any subcommand, the `help` subcommand, or the tool bare, which
  prints the same help. An option's own value
  (a pattern, a rule, a rewrite) is never a flag, so `-p '--help'` is no read.
- `man`: `man ast-grep` or `man sg`; `tldr` and `cheat` the same.
- `web`: `curl`, `wget` or `open` of an ast-grep.github.io or ast-grep repository page.
- `installed`: any reading command on a path under INSTALLED, the tool's install prefix (`installed_prefix()`): its
  README, changelog and completions, and the binary itself (`strings` on it holds its embedded rules; rubric review,
  2026-10-27). Listing it or the `bin` directory (`ls`, `file`, `stat`) is no read, and a substitution naming the
  binary (`$(which ast-grep)`, `$(command -v sg)`) stands for its path.
- `skill`: any reading command (`cat`, `head`, `sed`, `grep`, `ls`, ...) on a path under SKILLS, the official skills'
  root when the condition gives them, with `cd` followed and `~` read as the home directory on both sides.

The call is split into simple commands on `;`, `&&`, `||`, `|`, `&` and newlines, quotes kept; `$(...)` and
backticks are commands too. A heredoc's body is text, unless it is fed to a shell (`bash`, `sh`), when its lines are
commands. `docs_reads_fixtures.json` holds known reads and non-reads; `--selftest` reports both numbers.
"""
import json, os, re, shlex, sys

TOOLS = {"ast-grep", "sg"}
VALUED = {"-p", "--pattern", "-r", "--rewrite", "--rule", "--inline-rules", "-m", "--match", "--filter", "-l", "--lang",
          "--selector", "--strictness", "-c", "--config", "--globs", "--json", "-A", "-B", "-C", "--context", "--items",
          "--view", "-j", "--threads", "--debug-query", "--format", "--color", "--report-style"}
READERS = {"cat", "head", "tail", "sed", "awk", "less", "more", "bat", "grep", "rg", "ls", "find", "tree", "wc", "nl",
           "cut", "sort", "uniq", "diff", "file", "stat", "xxd", "od", "strings", "jq", "python3", "python", "node", "open"}
WRAPPERS = {"command", "env", "exec", "time", "nice", "nohup", "sudo", "npx", "xargs", "builtin"}
# Commands that list or locate a file without reading what it holds.
LISTERS = {"ls", "file", "stat", "find", "tree", "wc"}
# A substitution that names the binary, `$(which ast-grep)` or `` `command -v sg` ``: the binary's path, not a command.
BINARY_SUB = re.compile(r"\$\(\s*(?:which|command\s+-v|type\s+-p)\s+(?:ast-grep|sg)\s*\)|`\s*(?:which|command\s+-v|type\s+-p)\s+(?:ast-grep|sg)\s*`")
WEB = re.compile(r"ast-grep\.github\.io|github\.com/ast-grep|raw\.githubusercontent\.com/ast-grep")


def heredocs(cmd):
    """The call with each heredoc's body removed, and the bodies fed to a shell, as more lines."""
    out, fed, lines, i = [], [], cmd.split("\n"), 0
    while i < len(lines):
        line = lines[i]
        out.append(line)
        m = re.search(r"<<-?\s*(['\"]?)(\w+)\1", line)
        i += 1
        if m:
            body = []
            while i < len(lines) and lines[i].strip() != m.group(2):
                body.append(lines[i]); i += 1
            i += 1
            if re.match(r"\s*(bash|sh|zsh)\b", line.split("<<")[0].split("|")[-1].split(";")[-1].strip() or "x"):
                fed.append("\n".join(body))
    return "\n".join(out), fed


def substitutions(text):
    return re.findall(r"\$\(([^()]*)\)", text) + re.findall(r"`([^`]*)`", text)


def simple_commands(cmd):
    """Each simple command's words, in order; `cd` kept as a command."""
    text, fed = heredocs(cmd)
    pieces = [text] + fed + substitutions(text)
    for piece in pieces:
        lex = shlex.shlex(piece.replace("\n", " ; "), posix=True, punctuation_chars=";&|<>()")
        lex.whitespace_split = True
        words = []
        try:
            for tok in lex:
                if tok and set(tok) <= set(";&|()"):
                    if words:
                        yield words
                    words = []
                else:
                    words.append(tok)
        except ValueError:  # an unclosed quote: what was read so far
            pass
        if words:
            yield words


def strip_wrappers(words):
    while words and (words[0] in WRAPPERS or re.match(r"^\w+=", words[0])):
        words = words[1:]
        while words and words[0].startswith("-") and words[0] not in TOOLS:
            words = words[1:]
    return words


def under(path, root, cwd, home):
    expand = lambda p: home + p[1:] if p == "~" or p.startswith("~/") else p
    root = os.path.normpath(expand(root))
    path = expand(path)
    full = os.path.normpath(path if path.startswith("/") else os.path.join(cwd, path))
    return full == root or full.startswith(root + "/")


def installed_prefix():
    """The tool's install prefix on this machine (`/usr/local/Cellar/ast-grep/0.44.0` for a Homebrew install), where
    its README, changelog and completions sit beside the binary; None when it isn't on PATH."""
    import shutil
    found = shutil.which("ast-grep") or shutil.which("sg")
    return os.path.dirname(os.path.dirname(os.path.realpath(found))) if found else None


def reads_of(cmd, skills=None, cwd="/work", home="/HOME", installed=None):
    """The docs reads one bash call makes: a list of {"kind", "what"}."""
    found = []
    if installed:
        cmd = BINARY_SUB.sub(os.path.join(installed, "bin", "ast-grep"), cmd)
    for words in simple_commands(cmd):
        words = strip_wrappers([w for w in words if w not in {"<", ">", ">>", "2>", "2>&1"}])
        if not words:
            continue
        name = os.path.basename(words[0])
        if name == "cd" and len(words) > 1:
            target = words[1]
            target = home + target[1:] if target.startswith("~") else target
            cwd = os.path.normpath(target if target.startswith("/") else os.path.join(cwd, target))
            continue
        if name in TOOLS:
            args, i = words[1:], 0
            # Bare, or given only its global config, 0.44.0 prints its whole help, commands and options; a subcommand
            # given nothing prints only its usage error (`run`), or runs on the working directory (`outline`).
            if not [a for a in args if a not in {"-c", "--config"} and not a.endswith(".yml")]:
                found.append({"kind": "help", "what": " ".join(words)})
                continue
            while i < len(args):
                a = args[i]
                if a in {"--help", "-h"} or (i == 0 and a == "help"):
                    found.append({"kind": "help", "what": " ".join(words[:i + 2])})
                    break
                if a in VALUED:
                    i += 2
                    continue
                i += 1
            continue
        if name in {"man", "tldr", "cheat"} and any(os.path.basename(w) in TOOLS for w in words[1:]):
            found.append({"kind": "man", "what": " ".join(words[:2])})
            continue
        if name in {"curl", "wget", "open", "lynx", "w3m"} and any(WEB.search(w) for w in words[1:]):
            found.append({"kind": "web", "what": next(w for w in words[1:] if WEB.search(w))})
            continue
        if installed and name in READERS:
            # The binary itself is read too (`strings` on it holds its embedded rules); only listing it is no read.
            hit = next((w for w in words[1:] if not w.startswith("-") and under(w, installed, cwd, home)
                        and not (name in LISTERS and (os.path.basename(w) in TOOLS or w.rstrip("/").endswith("/bin")))), None)
            if hit:
                found.append({"kind": "installed", "what": hit})
                continue
        if skills and name in READERS:
            operands = [w for w in words[1:] if not w.startswith("-")]
            # A listing with no path lists where it stands, and so does a search given only its pattern.
            if not operands and name in {"ls", "find", "tree"} or len(operands) == 1 and name == "rg":
                operands.append(cwd)
            hit = next((w for w in operands if under(w, skills, cwd, home)), None)
            if hit:
                found.append({"kind": "skill", "what": hit})
    return found


def selftest():
    fx = json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "docs_reads_fixtures.json")))
    skills = fx["skills"]
    bad = 0
    for case in fx["reads"]:
        got = [r["kind"] for r in reads_of(case["cmd"], skills, installed=fx["installed"])]
        if got != case["kinds"]:
            print(f"docs_reads: read missed: {case['why']}: got {got}, want {case['kinds']}"); bad += 1
    for case in fx["non_reads"]:
        got = reads_of(case["cmd"], skills, installed=fx["installed"])
        if got:
            print(f"docs_reads: false read: {case['why']}: got {got}"); bad += 1
    print(f"docs_reads: {len(fx['reads'])} reads and {len(fx['non_reads'])} non-reads, {bad} misclassified")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(selftest() if sys.argv[1:] == ["--selftest"] else (print(json.dumps(reads_of(sys.argv[1], *sys.argv[2:3]))) or 0))
