#!/usr/bin/env python3
"""#826's shape on the floor: a trunk near the window with one interview fork always running beside it.

drive.py ARM FORK_MAX OUT.jsonl
  step 0: a cold ~130k-token trunk prompt (code dump), thinking off, max_tokens 4096 (prompt + output within
  max_seq_len 163,840 up to step 7's ~144k; replies are ~100 tokens)
  then one fork at a time, cut from the trunk's current history + an interview question, thinking on at
  reasoning_effort medium (the template renders no effort line in the system prompt for medium or for thinking off,
  so the fork shares the trunk's prefix; xhigh would put a line at the top and share nothing), min_tokens = max_tokens
  = FORK_MAX so each fork generates to its cap, as #826's long interview forks did; a new fork starts whenever the previous one ends
  steps 1..7: trunk turns 75 s apart (an agent's tool time, while the fork keeps generating), each appending
  ~2k tokens of new code as a tool result
  finally: one cold distinct ~130k prompt (max_tokens 1) as an extra cold-prefill sample
Every request's usage/timings go to OUT.jsonl. The server URL comes from $R3_URL."""
import json, os, sys, threading, time, urllib.request

URL = os.environ.get("R3_URL", "http://127.0.0.1:<port>")
ARM, FORK_MAX, OUT = sys.argv[1], int(sys.argv[2]), sys.argv[3]
HERE = os.path.dirname(os.path.abspath(__file__))
CORPUS = open(os.path.join(HERE, "corpus.txt")).read()
SIZES = os.path.join(HERE, "sizes.json")
TRUNK_TOKENS, STEP_TOKENS, STEPS, STEP_GAP = 130000, 2000, 7, 75
SYSTEM = ("You are a coding assistant working with a person on a C++/CUDA inference engine. "
          "Answer the question asked, concisely. When you refer to code, name the file and the function.")
lock = threading.Lock()

def log(rec):
    rec.update(arm=ARM, fork_max=FORK_MAX, t_end=time.time())
    with lock, open(OUT, "a") as f:
        f.write(json.dumps(rec) + "\n")
    print(json.dumps({k: rec.get(k) for k in ("kind", "step", "prompt_tokens", "cache_n", "prompt_ms", "ttft", "completion_tokens", "wall", "finish")}), flush=True)

def post(path, body):
    r = urllib.request.Request(URL + path, data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(r, timeout=1800) as f:
        return json.load(f)

def ntok(text):
    return post("/v1/token/encode", {"text": text})["length"]

def cut(offset, target):
    """Largest corpus slice from offset whose token count stays under target (bisection, cached by key)."""
    sizes = json.load(open(SIZES)) if os.path.exists(SIZES) else {}
    key = f"{offset}:{target}"
    if key not in sizes:
        lo, hi = 1000, target * 6
        while hi - lo > 500:
            mid = (lo + hi) // 2
            lo, hi = (mid, hi) if ntok(CORPUS[offset:offset + mid]) < target else (lo, mid)
        sizes[key] = lo
        json.dump(sizes, open(SIZES, "w"))
    return CORPUS[offset:offset + sizes[key]]

def stream(body, stop=None):
    """POST a streaming chat completion; returns (content, final usage/timings record). stop() -> True aborts."""
    body = dict(body, stream=True, stream_options={"include_usage": True})
    r = urllib.request.Request(URL + "/v1/chat/completions", data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
    t0 = time.time(); ttft = None; content = []; last = {}; finish = None; aborted = False
    with urllib.request.urlopen(r, timeout=3600) as f:
        for raw in f:
            line = raw.decode().strip()
            if not line.startswith("data: ") or line == "data: [DONE]":
                continue
            j = json.loads(line[6:])
            for c in j.get("choices") or []:
                d = c.get("delta") or {}
                if (d.get("content") or d.get("reasoning_content")) and ttft is None:
                    ttft = time.time() - t0
                content.append(d.get("content") or "")
                finish = c.get("finish_reason") or finish
            if "usage" in j or "timings" in j:
                last = j
            if stop is not None and stop():
                aborted = True
                break
    u, tm = last.get("usage") or {}, last.get("timings") or {}
    return "".join(content), {"prompt_tokens": u.get("prompt_tokens"), "completion_tokens": u.get("completion_tokens"),
                              "cached_tokens": (u.get("prompt_tokens_details") or {}).get("cached_tokens"),
                              "cache_n": tm.get("cache_n"), "prompt_n": tm.get("prompt_n"), "prompt_ms": tm.get("prompt_ms"),
                              "predicted_n": tm.get("predicted_n"), "predicted_per_second": tm.get("predicted_per_second"),
                              "draft_n": tm.get("draft_n"), "draft_n_accepted": tm.get("draft_n_accepted"),
                              "ttft": ttft, "wall": time.time() - t0, "finish": "aborted" if aborted else finish}

trunk = [{"role": "system", "content": SYSTEM},
         {"role": "user", "content": "Here is a dump of the engine's source:\n\n" + cut(0, TRUNK_TOKENS) +
          "\n\nIn one sentence: which file defines the CUDA flash-attention launch?"}]
NO_THINK = {"chat_template_kwargs": {"enable_thinking": False}}
done = threading.Event(); state = {"forks": 0}

def trunk_step(i):
    t_start = time.time()
    text, rec = stream({"model": "x", "max_tokens": 4096, "messages": trunk, **NO_THINK})
    rec.update(kind="trunk", step=i, t_start=t_start); log(rec)
    if rec.get("prompt_tokens"):
        state["last_total"] = rec["prompt_tokens"] + (rec.get("completion_tokens") or 0)
    trunk.append({"role": "assistant", "content": text.strip()})

QUESTIONS = ["How does the code above manage the KV cache across concurrent sequences, and what would you change? Think it through carefully.",
             "Walk through how speculative decoding verifies drafted tokens in the code above, and where it could go wrong.",
             "Which parts of the code above would break if the context length doubled? Reason it out in detail.",
             "Explain how the server schedules prefill against decode in the code above, and propose an improvement."]

def forks():
    while not done.is_set():
        msgs = list(trunk) + [{"role": "user", "content": "Interview question: " + QUESTIONS[state["forks"] % len(QUESTIONS)]}]
        state["forks"] += 1; n = state["forks"]; t_start = time.time()
        try:
            cap = min(FORK_MAX, 163840 - state.get("last_total", 140000) - int(os.environ.get("R3_FORK_MARGIN", "8192")))   # clear of the pool edge: a fork at prompt + max_tokens within ~1k of it crashed the generator (pilot 4)
            _, rec = stream({"model": "x", "max_tokens": cap, "min_tokens": cap, "messages": msgs, "reasoning_effort": "medium"}, stop=done.is_set)
            rec["cap"] = cap
        except Exception as e:
            rec = {"error": repr(e)}
        rec.update(kind="fork", step=n, t_start=t_start, trunk_len=len(trunk)); log(rec)

trunk_step(0)
ft = threading.Thread(target=forks, daemon=True); ft.start()
time.sleep(15)
for i in range(1, STEPS + 1):
    if i > 1:
        time.sleep(STEP_GAP)   # an agent's tool time between trunk steps; the fork keeps generating meanwhile
    trunk.append({"role": "user", "content": "Tool output:\n\n" + cut(4_000_000 + i * 20_000, STEP_TOKENS) +
                  "\n\nIn one sentence: what does the file above do?"})
    trunk_step(i)
done.set(); ft.join(timeout=120)
t_start = time.time()
_, rec = stream({"model": "x", "max_tokens": 1, **NO_THINK,
                 "messages": [{"role": "system", "content": SYSTEM}, {"role": "user", "content": cut(6_000_000, TRUNK_TOKENS) + "\n\nOK?"}]})
rec.update(kind="cold", step=0, t_start=t_start); log(rec)
