# rtx6000ada-host vision bring-up checks. Key is read from TabbyAPI's api_tokens.yml, never argv.
import base64, io, json, sys, time, threading, subprocess, urllib.request, yaml
from PIL import Image, ImageDraw, ImageFont
BASE = "http://127.0.0.1:<port>"
KEY = yaml.safe_load(open("$HOME/src/tabbyAPI/api_tokens.yml"))["api_key"]
H = {"Authorization": "Bearer " + KEY, "Content-Type": "application/json"}

def req(path, body=None, timeout=900):
    r = urllib.request.Request(BASE + path, data=None if body is None else json.dumps(body).encode(), headers=H)
    with urllib.request.urlopen(r, timeout=timeout) as f:
        return json.load(f)

def img(w, h):
    im = Image.new("RGB", (w, h), "white"); d = ImageDraw.Draw(im)
    s = w / 1024
    try: font = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", int(90 * s))
    except Exception: font = ImageFont.load_default()
    d.text((int(80 * s), int(80 * s)), "VIOLET 4729", fill="black", font=font)
    d.ellipse([int(120*s), int(400*s), int(420*s), int(700*s)], fill="red")
    d.rectangle([int(600*s), int(400*s), int(900*s), int(700*s)], fill="blue")
    b = io.BytesIO(); im.save(b, "PNG"); return base64.b64encode(b.getvalue()).decode()

class VramSampler(threading.Thread):
    def __init__(s): super().__init__(daemon=True); s.peak = 0; s.stop = False
    def run(s):
        while not s.stop:
            o = subprocess.run(["nvidia-smi", "--query-gpu=memory.used", "--format=csv,noheader,nounits"], capture_output=True, text=True).stdout
            s.peak = max(s.peak, int(o.strip())); time.sleep(0.2)

Q = "What text is written in this image, and what shapes and colors do you see? Answer in one short sentence."
def image_call(w, h):
    body = {"model": "x", "max_tokens": 2048, "messages": [{"role": "user", "content": [
        {"type": "image_url", "image_url": {"url": "data:image/png;base64," + img(w, h)}},
        {"type": "text", "text": Q}]}]}
    t = time.time(); r = req("/v1/chat/completions", body)
    return r, time.time() - t

mode = sys.argv[1]
if mode == "confirm":
    m = req("/v1/model"); p = m.get("parameters") or {}
    print("model", m["id"], "max_seq_len", p.get("max_seq_len"), "cache_size", p.get("cache_size"), "cache_mode", p.get("cache_mode"))
    r = req("/v1/chat/completions", {"model": "x", "max_tokens": 64, "messages": [{"role": "user", "content": "Reply with the single word: ready"}],
                                      "chat_template_kwargs": {"enable_thinking": False}})
    print("warm:", repr(r["choices"][0]["message"]["content"]), r.get("usage", {}).get("completion_tokens"))
elif mode == "image":
    w, h = int(sys.argv[2]), int(sys.argv[3])
    vs = VramSampler(); vs.start()
    r, dt = image_call(w, h); vs.stop = True; vs.join()
    u = r.get("usage", {})
    print(f"{w}x{h}: {dt:.1f}s prompt_tokens={u.get('prompt_tokens')} peak_vram={vs.peak} MiB")
    print("answer:", repr(r["choices"][0]["message"]["content"].strip()[:300]))
elif mode == "beside":
    # image request while a ~100k-token text request is prefilling/decoding
    text = open("$HOME/setup/kld/prose.txt").read()[:400000]
    deep = {"model": "x", "max_tokens": 256, "messages": [{"role": "user", "content": text + "\n\nContinue the passage."}]}
    vs = VramSampler(); vs.start()
    out = {}
    th = threading.Thread(target=lambda: out.setdefault("deep", req("/v1/chat/completions", deep))); th.start()
    time.sleep(8)
    r, dt = image_call(2048, 2048)
    th.join(); vs.stop = True; vs.join()
    print(f"beside: image {dt:.1f}s, deep prompt_tokens={out['deep']['usage']['prompt_tokens']} peak_vram={vs.peak} MiB")
    print("answer:", repr(r["choices"][0]["message"]["content"].strip()[:300]))

if mode == "fill":
    # 4 concurrent distinct ~138k-token prompts (fills the 573,440 pool), image request mid-fill
    target = int(sys.argv[2]) if len(sys.argv) > 2 else 138000
    srcs = [open("$HOME/setup/kld/prose.txt").read(), open("$HOME/setup/kld/code.txt").read()]
    prompts = []
    for i in range(4):
        src = srcs[i % 2]; off = (i // 2) * (len(src) // 2)
        lo, hi = 1000, len(src) // 2
        while hi - lo > 2000:   # size the slice by the server's tokenizer
            mid = (lo + hi) // 2
            n = req("/v1/token/encode", {"text": src[off:off + mid]})["length"]
            lo, hi = (mid, hi) if n < target else (lo, mid)
        prompts.append(f"[{i}] " + src[off:off + lo] + "\n\nSummarize in one line.")
    vs = VramSampler(); vs.start(); res = {}
    def go(i): res[i] = req("/v1/chat/completions", {"model": "x", "max_tokens": 16, "messages": [{"role": "user", "content": prompts[i]}]}, timeout=1800)
    ths = [threading.Thread(target=go, args=(i,)) for i in range(4)]
    t0 = time.time(); [t.start() for t in ths]
    time.sleep(60); isz = int(sys.argv[3]) if len(sys.argv) > 3 else 2048; r, dt = image_call(isz, isz)
    [t.join() for t in ths]; vs.stop = True; vs.join()
    print("fill:", [res[i]["usage"]["prompt_tokens"] for i in range(4)], f"wall {time.time()-t0:.0f}s, image {dt:.1f}s, peak_vram={vs.peak} MiB, free_at_peak={49140-vs.peak} MiB")
    print("answer:", repr(r["choices"][0]["message"]["content"].strip()[:200]))
