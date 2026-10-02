#!/usr/bin/env python3
"""Write the ledger's fields onto every claim directory's README front matter (#32, planning 5935378835).

The values are scripts/ledger-fields.toml's, read from each record and its threads with the source beside each one;
this script only places them. For each `results/<dir>/README.md` named there, it removes any of the managed keys
already in the `+++` block and writes them again just before the block's first table, where TOML requires top-level
keys to sit. Where `rule_ratified` is absent it also appends one `**Ledger:**` line to the body's Conclusion, which
must be the last section, saying so with the absence's reason; an earlier Ledger line is replaced, and the body ends
in exactly one newline. It is idempotent: a second run changes nothing. It refuses a directory the values file does
not name, a value file naming a directory that does not exist, a `rule_ratified.digest` that is not the sha256 of the
file it names (`of`, default `decision-rule.toml`), and a README whose last section is not its Conclusion; every
refusal comes before the first write.

Usage: migrate-ledger-fields.py [--check]   (--check: exit 1 if any README would change, writing nothing)
"""
import hashlib, json, pathlib, re, sys, tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]
VALUES = ROOT / "scripts" / "ledger-fields.toml"
MANAGED = ("claim_issue", "supersedes", "rule_ratified", "rule_ratified_note", "window_start", "window_start_from", "absent")


def toml_value(v):
    if isinstance(v, str):
        return json.dumps(v, ensure_ascii=False)
    if isinstance(v, dict):
        return "{ " + ", ".join(f"{k} = {toml_value(x)}" for k, x in v.items()) + " }"
    raise SystemExit(f"migrate-ledger-fields: a value of type {type(v).__name__} is not a string or a table")


def rewrite(text: str, fields: dict, name: str) -> str:
    m = re.match(r"\+\+\+\n(.*?)\n\+\+\+\n", text, re.S)
    if not m:
        raise SystemExit(f"migrate-ledger-fields: {name}'s README has no +++ front matter")
    lines = [l for l in m.group(1).split("\n") if not re.match(rf"^({'|'.join(MANAGED)}) = ", l)]
    first_table = next((i for i, l in enumerate(lines) if re.match(r"^\[", l)), len(lines))
    while first_table > 0 and lines[first_table - 1] == "":
        first_table -= 1
    block = [f"{k} = {toml_value(fields[k])}" for k in MANAGED if k in fields]
    out = lines[:first_table] + block + lines[first_table:]
    front = "\n".join(out)
    tomllib.loads(front)  # it must still parse
    body = re.sub(r"\n+\*\*Ledger:\*\* [^\n]*\n*\Z", "", text[m.end():]).rstrip("\n") + "\n"
    why = (fields.get("absent") or {}).get("rule_ratified")
    if why:  # planning: where no ratification comment exists, say so in the field's absence and in the body
        heads = re.findall(r"^## (.+)$", body, re.M)
        if not heads or heads[-1].strip() != "Conclusion":
            raise SystemExit(f"migrate-ledger-fields: {name}'s README's last section is not its Conclusion")
        body = body.rstrip("\n") + f"\n\n**Ledger:** rule_ratified is absent: {why}.\n"
    return "+++\n" + front + "\n+++\n" + body


def main(argv):
    check = argv[:1] == ["--check"]
    values = tomllib.loads(VALUES.read_text(encoding="utf-8"))
    dirs = sorted(p.name for p in (ROOT / "results").iterdir() if p.is_dir() and re.match(r"\d{4}-\d{2}-\d{2}-", p.name))
    bad = sorted(set(dirs) ^ set(values))
    if bad:
        raise SystemExit(f"migrate-ledger-fields: the values file and results/ disagree on {bad}")
    for name in dirs:  # every digest first, so a refusal writes nothing
        rr = values[name].get("rule_ratified")
        if rr:
            target = ROOT / "results" / name / rr.get("of", "decision-rule.toml")
            if hashlib.sha256(target.read_bytes()).hexdigest() != rr["digest"]:
                raise SystemExit(f"migrate-ledger-fields: {name}'s rule_ratified.digest is not the sha256 of {target.name}")
    plan = []  # every README rewritten in memory first, so a refusal in any of them writes nothing
    for name in dirs:
        readme = ROOT / "results" / name / "README.md"
        old = readme.read_text(encoding="utf-8"); new = rewrite(old, values[name], name)
        if new != old:
            plan.append((readme, new))
    changed = len(plan)
    if not check:
        for readme, new in plan:
            readme.write_text(new, encoding="utf-8")
    print(f"migrate-ledger-fields: {len(dirs)} claim directories, {changed} {'would change' if check else 'changed'}")
    return 1 if (check and changed) else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
