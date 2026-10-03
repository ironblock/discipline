# apicheck.py PORT TAG: the client-facing behaviours our callers rely on, run identically against either server.
import json, sys, time, urllib.request
PORT, TAG = int(sys.argv[1]), sys.argv[2]
def post(path, payload, stream=False):
    req = urllib.request.Request(f"http://127.0.0.1:{PORT}{path}", data=json.dumps(payload).encode(),
                                 headers={"Content-Type": "application/json", "Authorization": "Bearer x"})
    r = urllib.request.urlopen(req, timeout=600)
    return r if stream else json.load(r)
def ntok(s):
    try: return len(post("/tokenize", {"content": s})["tokens"])
    except Exception: return len(post("/v1/token/encode", {"text": s, "add_bos_token": False})["tokens"])
def out(name, **kv): print(json.dumps({"tag": TAG, "check": name, **kv}), flush=True)
Q = "What is 17 * 23? Answer briefly."
for name, extra in [("default", {}), ("think_off", {"chat_template_kwargs": {"enable_thinking": False}}),
                    ("effort_low", {"reasoning_effort": "low"}), ("effort_high", {"reasoning_effort": "high"})]:
    try:
        d = post("/v1/chat/completions", {"messages": [{"role": "user", "content": Q}], "max_tokens": 1024,
                                          "temperature": 0, **extra})
        m = d["choices"][0]["message"]; rc = m.get("reasoning_content") or ""; c = m.get("content") or ""
        u = d.get("usage", {}); t = d.get("timings", {})
        out(name, finish=d["choices"][0].get("finish_reason"), reasoning_chars=len(rc), content=c[:80],
            think_tag_in_content="<think>" in c or "</think>" in c, usage_completion=u.get("completion_tokens"),
            usage_prompt=u.get("prompt_tokens"), timings_predicted_n=t.get("predicted_n"),
            retokenized_out=ntok(rc) + ntok(c))
    except Exception as e: out(name, error=str(e)[:200])
tools = [{"type": "function", "function": {"name": "get_weather", "description": "Current weather for a city",
          "parameters": {"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"]}}}]
try:
    d = post("/v1/chat/completions", {"messages": [{"role": "user", "content": "What's the weather in Paris right now?"}],
                                      "tools": tools, "max_tokens": 1024, "temperature": 0})
    m = d["choices"][0]["message"]
    out("tool_call", finish=d["choices"][0].get("finish_reason"), tool_calls=m.get("tool_calls"), content=(m.get("content") or "")[:120])
except Exception as e: out("tool_call", error=str(e)[:200])
try:
    r = post("/v1/chat/completions", {"messages": [{"role": "user", "content": Q}], "max_tokens": 512, "temperature": 0,
                                      "stream": True, "stream_options": {"include_usage": True}}, stream=True)
    n, rc, c, usage, t0, first = 0, 0, 0, None, time.time(), None
    for line in r:
        line = line.decode().strip()
        if not line.startswith("data:") or line == "data: [DONE]": continue
        ch = json.loads(line[5:]); n += 1
        if ch.get("usage"): usage = ch["usage"]
        for chc in ch.get("choices", []):
            dl = chc.get("delta", {}); rc += len(dl.get("reasoning_content") or ""); c += len(dl.get("content") or "")
            if first is None and (dl.get("reasoning_content") or dl.get("content")): first = time.time() - t0
    out("stream", chunks=n, reasoning_chars=rc, content_chars=c, usage=usage, ttft_s=round(first or -1, 3))
except Exception as e: out("stream", error=str(e)[:200])
