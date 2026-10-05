#!/usr/bin/env python3
# #401 follow-up on rtx6000ada-host: size TabbyAPI + ExLlamaV3 1.5.4's KV pool (cache_size) so that a full four-request
# fill leaves about 300 MiB free (the v0.1.0 headroom bar, #143 5984376546). max_seq_len stays 262144 (per request).
# Every candidate runs on a side port (no auth) with the live config except cache_size; this script only signals the
# TabbyAPI processes it started (Popen handles). Production is stopped/started by the caller.
import json, os, subprocess, sys, threading, time, urllib.request
HOME = os.path.expanduser("~"); T = f"{HOME}/src/tabbyAPI"; V = f"{HOME}/venvs/tabby154/bin/python"
OUT = f"{HOME}/w401/out"; TOTAL = 49140; TARGET_FREE = 300; MAXSEQ = 262144
def log(*a):
    s = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()) + " " + " ".join(str(x) for x in a)
    print(s, flush=True); open(f"{HOME}/w401/size.log", "a").write(s + "\n")
def smi(): return int(subprocess.check_output(["nvidia-smi", "--query-gpu=memory.used", "--format=csv,noheader,nounits"]).split()[0])
def call(path, body=None, timeout=3600):
    r = urllib.request.Request(f"http://127.0.0.1:<side-port>{path}", data=json.dumps(body).encode() if body is not None else None,
                               headers={"Content-Type": "application/json"}, method="POST" if body is not None else "GET")
    with urllib.request.urlopen(r, timeout=timeout) as f: return json.load(f)
def start(cs):
    base = open(f"{T}/config-401.yml").read()
    cfg = "\n".join(("  cache_size: %d" % cs) if l.strip().startswith("cache_size:") else l for l in base.splitlines()) + "\n"
    path = f"{T}/config-401-cs{cs}.yml"; open(path, "w").write(cfg)
    p = subprocess.Popen([V, "main.py", "--config", os.path.basename(path)], cwd=T,
                         stdout=open(f"{OUT}/tabby-cs{cs}.log", "w"), stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL)
    for _ in range(200):
        time.sleep(3)
        if p.poll() is not None: return p, None
        try:
            if "healthy" in json.dumps(call("/health", timeout=5)): time.sleep(3); return p, smi()
        except Exception: pass
    return p, None
def stop(p):
    p.terminate()
    try: p.wait(60)
    except subprocess.TimeoutExpired: p.kill(); p.wait(30)
    time.sleep(3)
def fill(cs):
    # four requests sharing the pool, each a slot-unique filler to (cs/4 - 1024) tokens, capped at max_seq_len
    per = min(cs // 4 - 1024, MAXSEQ - 1024)
    unit = "Ledger line {}: the quick brown fox jumps over the lazy dog near the riverbank. "
    tpu = len(call("/v1/token/encode", {"text": unit.format("s0-000000") * 200})["tokens"]) / 200
    texts = []
    for i in range(4):
        tag = f"s{i}-{time.time_ns() % 1000000:06d}"; n = int(per / tpu)
        t = unit.format(tag) * n
        while len(call("/v1/token/encode", {"text": t})["tokens"]) > per: n -= 100; t = unit.format(tag) * n
        texts.append(t)
    peak = [0]; stop_ev = threading.Event()
    def sampler():
        while not stop_ev.is_set(): peak[0] = max(peak[0], smi()); time.sleep(0.5)
    res = [None] * 4
    def one(i):
        t0 = time.time()
        try:
            r = call("/v1/completions", {"prompt": texts[i], "max_tokens": 16, "temperature": 0})
            res[i] = {"prompt_tokens": r.get("usage", {}).get("prompt_tokens"), "wall_s": round(time.time() - t0, 1)}
        except Exception as e: res[i] = {"error": repr(e)[:200], "wall_s": round(time.time() - t0, 1)}
    th = threading.Thread(target=sampler); th.start()
    ws = [threading.Thread(target=one, args=(i,)) for i in range(4)]; [w.start() for w in ws]; [w.join() for w in ws]
    time.sleep(2); stop_ev.set(); th.join()
    return {"cache_size": cs, "per_request": per, "requests": res, "peak_mib": peak[0], "free_at_peak_mib": TOTAL - peak[0]}

if __name__ == "__main__":
    reads = {}
    for cs in (262144, 524288):                      # two load readings give the KV cost per token
        p, used = start(cs); stop(p); reads[cs] = used; log("load", cs, "used_mib", used)
    if None in reads.values(): log("ABORT: a calibration load failed"); sys.exit(2)
    per_tok = (reads[524288] - reads[262144]) / (524288 - 262144)
    log("kv_mib_per_token", round(per_tok, 5), f"({per_tok * 1024:.1f} KiB)")
    # first guess: leave TARGET_FREE plus 600 MiB for the fill's transients, then confirm with a real fill and step down
    cs = int(((TOTAL - TARGET_FREE - 600 - reads[262144]) / per_tok + 262144) // 4096 * 4096)
    results = []
    while cs >= 262144:
        p, used = start(cs)
        if used is None: log("load failed at", cs); stop(p); cs -= 32768; continue
        f = fill(cs); f["load_used_mib"] = used; stop(p); results.append(f)
        log("fill", json.dumps(f))
        if f["free_at_peak_mib"] >= TARGET_FREE and not any("error" in (r or {}) for r in f["requests"]): break
        cs -= 32768
    json.dump({"load_reads": reads, "kv_mib_per_token": per_tok, "fills": results}, open(f"{OUT}/size401.json", "w"), indent=1)
    log("CHOSEN", cs if results and results[-1]["free_at_peak_mib"] >= TARGET_FREE else "none")
