# apiprobe.py: four facts of the serving TabbyAPI, read over HTTP with the serving key (passed in the environment).
import json, os, sys, urllib.request, urllib.error
PORT = int(sys.argv[1]); KEY = os.environ["KEY"]
H = {"Content-Type": "application/json", "Authorization": f"Bearer {KEY}"}
def call(method, path, body=None):
    req = urllib.request.Request(f"http://127.0.0.1:{PORT}{path}", method=method, headers=H,
                                 data=json.dumps(body).encode() if body is not None else None)
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            return r.status, r.read().decode(errors="replace")
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode(errors="replace")
msg = [{"role": "user", "content": "What is 17 * 23? Answer with the number only."}]
for name, method, path, body in [
    ("effort_high", "POST", "/v1/chat/completions", {"messages": msg, "max_tokens": 64, "temperature": 0, "reasoning_effort": "high"}),
    ("effort_xhigh", "POST", "/v1/chat/completions", {"messages": msg, "max_tokens": 512, "temperature": 0, "reasoning_effort": "xhigh"}),
    ("slots", "GET", "/slots", None),
    ("default", "POST", "/v1/chat/completions", {"messages": msg, "max_tokens": 512, "temperature": 0}),
]:
    st, txt = call(method, path, body)
    rec = {"check": name, "http": st}
    try:
        j = json.loads(txt)
        if "choices" in j:
            m = j["choices"][0]["message"]
            rec.update(content=m.get("content"), content_starts_with_blank_line=(m.get("content") or "").startswith("\n\n"),
                       reasoning_chars=len(m.get("reasoning_content") or ""), has_timings="timings" in j,
                       timings_keys=sorted(j.get("timings", {}).keys()))
        else:
            rec["body"] = txt[:400]
    except Exception:
        rec["body"] = txt[:400]
    print(json.dumps(rec), flush=True)
