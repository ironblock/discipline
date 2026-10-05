#!/usr/bin/env python3
"""The kwarg-delivery cell's raw capture against a TabbyAPI endpoint (#393, rulings 5986337117). Writes raw/kw.json and raw/refusal.json.
  - paired control: the same short question with chat_template_kwargs enable_thinking false, true and absent; each reply's
    `reasoning_content` characters are counted (TabbyAPI returns the reasoning in its own field);
  - refused levels: reasoning_effort at each level in --levels is POSTed in chat_template_kwargs; the status and the body's digest are kept;
  - rendered effort (a capability fact, ruling 4): the model directory's template file is rendered here, per level, with jinja2; the
    rendered string's digest, whether it differs from the default and where, and a refusal's text are recorded. The template digest is the
    file's bytes, from the model directory.
The key comes from TABBY_API_KEY and is never written."""
from __future__ import annotations
import argparse, hashlib, json, os, pathlib, urllib.error, urllib.request

def sha(b: bytes) -> str: return hashlib.sha256(b).hexdigest()

def post(base, key, body):
    h = {"Content-Type": "application/json"}
    if key: h["Authorization"] = f"Bearer {key}"
    req = urllib.request.Request(base + "/v1/chat/completions", data=json.dumps(body).encode(), headers=h)
    try:
        with urllib.request.urlopen(req, timeout=600) as r: return r.status, r.read()
    except urllib.error.HTTPError as e: return e.code, e.read()

def reply(base, key, kwargs, q):
    body = {"messages": [{"role": "user", "content": q}], "max_tokens": 512, "temperature": 0}
    if kwargs is not None: body["chat_template_kwargs"] = kwargs
    st, raw = post(base, key, body)
    if st != 200: return {"status": st, "body_sha256": sha(raw), "body_head": raw[:300].decode("utf-8", "replace")}
    m = json.loads(raw)["choices"][0]["message"]
    return {"status": st, "reasoning_chars": len(m.get("reasoning_content") or ""), "content": (m.get("content") or "").strip()[:200]}

def render(tpl, kwargs):
    from jinja2.sandbox import ImmutableSandboxedEnvironment
    env = ImmutableSandboxedEnvironment(trim_blocks=True, lstrip_blocks=True)
    def raise_exception(m): raise ValueError(m)
    env.globals["raise_exception"] = raise_exception
    return env.from_string(tpl).render(messages=[{"role": "user", "content": "What is 12 times 12?"}], add_generation_prompt=True, **kwargs)

def main(a) -> int:
    key = os.environ.get("TABBY_API_KEY"); out = pathlib.Path(a.out); out.mkdir(parents=True, exist_ok=True)
    tb = pathlib.Path(a.template_file).read_bytes(); tpl = tb.decode()
    q = "What is 12 times 12? Answer with the number only."
    off, on, dflt = (reply(a.endpoint, key, k, q) for k in ({"enable_thinking": False}, {"enable_thinking": True}, None))
    default_r = render(tpl, {})
    eff = {}
    for lvl in [None] + a.levels:
        k = "reasoning_effort=None" if lvl is None else f"reasoning_effort={lvl}"
        try:
            r = render(tpl, {} if lvl is None else {"reasoning_effort": lvl})
            diff = next((i for i, (x, y) in enumerate(zip(r, default_r)) if x != y), None if len(r) == len(default_r) else min(len(r), len(default_r)))
            eff[k] = {"sha256": sha(r.encode()), "same_as_default": r == default_r, "differs_at": None if diff is None else r[max(0, diff - 20):diff + 120]}
        except ValueError as e:
            eff[k] = {"error": str(e)}
    refusal = {}
    for lvl in a.levels + ["__accepted__"]:
        kw = {"reasoning_effort": a.accepted_level} if lvl == "__accepted__" else {"reasoning_effort": lvl}
        st, raw = post(a.endpoint, key, {"messages": [{"role": "user", "content": q}], "max_tokens": 32, "temperature": 0, "chat_template_kwargs": kw})
        refusal[a.accepted_level if lvl == "__accepted__" else lvl] = {"endpoint": "/v1/chat/completions", "status": st, "body_sha256": sha(raw), "body_head": raw[:300].decode("utf-8", "replace")}
    kw = {"props_template_sha256": sha(tb), "props_template_chars": len(tpl), "template_source": f"model directory file {pathlib.Path(a.template_file).name}",
          "template_mentions": {k: k in tpl for k in ("enable_thinking", "reasoning_effort", "reasoning_strength", "xhigh", "preserve_thinking")},
          "thinking_disabled": off, "thinking_enabled": on, "default": dflt,
          "negative_control": off.get("reasoning_chars") == 0, "positive_control": on.get("reasoning_chars", 0) > 0, "rendered_effort": eff}
    (out / "kw.json").write_text(json.dumps(kw, indent=1) + "\n"); (out / "refusal.json").write_text(json.dumps({"candidate": refusal}, indent=1) + "\n")
    print(json.dumps({"off": off.get("reasoning_chars"), "on": on.get("reasoning_chars"), "default": dflt.get("reasoning_chars"),
                      "refusal": {l: v["status"] for l, v in refusal.items()}}))
    return 0

if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("--endpoint", required=True); p.add_argument("--template-file", required=True); p.add_argument("--out", required=True)
    p.add_argument("--levels", nargs="+", default=["high", "none", "max"], help="levels expected to be refused")
    p.add_argument("--accepted-level", default="low", help="a level expected to be accepted, the control")
    raise SystemExit(main(p.parse_args()))
