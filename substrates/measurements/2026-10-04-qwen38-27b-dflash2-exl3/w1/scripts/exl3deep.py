# exl3deep.py (#335): the #143 profile bench (deepb.py) on ExLlamaV3's own generator. Same corpus, offsets and question,
# rendered through the model's chat template; greedy, 256 new tokens; rep 0 cold prefill, reps 1-2 warm.
# PHASES="depth:conc,...". Exits 3 if free VRAM after load is below --min-free-mib (the window then steps the pool down).
import argparse, glob, json, os, subprocess, threading, time, torch, jinja2
from exllamav3 import Generator, Job, model_init
from exllamav3.generator.sampler import GreedySampler
home = os.path.expanduser("~")
files = sorted(set(f for pat in ["src/*.cpp", "src/*.h", "common/*.cpp", "common/*.h", "tools/**/*.cpp", "ggml/src/**/*.c",
                                   "ggml/src/**/*.cpp", "ggml/src/**/*.cu", "ggml/src/**/*.cuh", "ggml/src/**/*.h", "ggml/include/*.h"]
                   for f in glob.glob(home + "/git/llama.cpp/" + pat, recursive=True)))
corpus = "".join(f"\n\n// ===== {f.split('/git/llama.cpp/')[1]} =====\n" + open(f, errors="replace").read() for f in files)
Q = "\n\nReview the code above. Identify the three most likely bugs, explain each, and propose a fix with code."
peak = [0]
def sampler(stop):
    while not stop.is_set():
        try: peak[0] = max(peak[0], int(subprocess.check_output(["nvidia-smi", "--query-gpu=memory.used", "--format=csv,noheader,nounits"]).split()[0]))
        except Exception: pass
        time.sleep(0.5)
def main():
    ap = argparse.ArgumentParser(allow_abbrev=False)
    model_init.add_args(ap, cache=True, add_sampling_args=False, add_draft_model_args=True, default_cache_size=163840, default_autosplit_max_batch_size=2)
    ap.add_argument("--tag", default="exl3"); ap.add_argument("--phases", required=True); ap.add_argument("--min-free-mib", type=int, default=0)
    args = ap.parse_args()
    t0 = time.time()
    model, config, cache, tokenizer, draft_model, draft_config, draft_cache = model_init.init(args)
    free, total = torch.cuda.mem_get_info()
    used = int(subprocess.check_output(["nvidia-smi", "--query-gpu=memory.used", "--format=csv,noheader,nounits"]).split()[0])
    print(json.dumps({"tag": args.tag, "event": "loaded", "load_s": round(time.time() - t0), "cache_size": args.cache_size,
                      "smi_used_mib": used, "smi_free_mib": 24576 - used, "torch_free_mib": free // 2**20}), flush=True)
    if 24576 - used < args.min_free_mib:
        print(json.dumps({"tag": args.tag, "event": "too_little_free", "need": args.min_free_mib}), flush=True); os._exit(3)
    gen = Generator(model=model, cache=cache, tokenizer=tokenizer, draft_model=draft_model, draft_cache=draft_cache,
                    num_draft_tokens=args.num_draft_tokens, max_chunk_size=args.chunk_size)
    print(json.dumps({"tag": args.tag, "event": "generator", "num_draft_tokens": gen.num_draft_tokens}), flush=True)
    tmpl = jinja2.Environment(loader=jinja2.BaseLoader()).from_string(open(os.path.join(args.model_dir, "chat_template.jinja")).read())
    tmpl.globals["raise_exception"] = lambda m: (_ for _ in ()).throw(Exception(m))
    def ids_for(i, depth):
        probe = corpus[:60000]; cpt = len(probe) / tokenizer.encode(probe, add_bos=False).shape[-1]
        n = int(depth * cpt); off = (i * 1_500_000) % max(1, len(corpus) - 2 * n - 1)
        for _ in range(5):
            ids = tokenizer.encode(tmpl.render(messages=[{"role": "user", "content": corpus[off:off + n] + Q}], add_generation_prompt=True), add_bos=False, encode_special_tokens=True)
            t = ids.shape[-1]
            if abs(t - depth) <= depth * 0.01: break
            n = int(n * depth / t)
        return ids
    stop = threading.Event(); threading.Thread(target=sampler, args=(stop,), daemon=True).start()
    for ph in args.phases.split(","):
        depth, c = (int(x) for x in ph.split(":"))
        prompts = [ids_for(i, depth) for i in range(c)]
        for rep in range(3):
            for i in range(c): gen.enqueue(Job(input_ids=prompts[i], max_new_tokens=256, sampler=GreedySampler(), stop_conditions=[], identifier=i))
            fin = {}; t0 = time.time()
            while gen.num_remaining_jobs():
                for r in gen.iterate():
                    if r.get("stage") == "error": fin[r.get("identifier")] = {"error": str(r)[:300]}
                    elif r.get("eos"): fin[r["identifier"]] = r
            wall = time.time() - t0; ok = [r for r in fin.values() if "error" not in r]
            acc = sum(r.get("accepted_draft_tokens", 0) for r in ok); rej = sum(r.get("rejected_draft_tokens", 0) for r in ok)
            print(json.dumps({"tag": args.tag, "depth": depth, "conc": c, "rep": rep, "wall_s": round(wall, 1),
                "tg": [round((r["new_tokens"] - 1) / r["time_generate"], 2) for r in ok if r.get("time_generate")],
                "agg_tok_s": round(sum(r["new_tokens"] for r in ok) / wall, 1) if ok else None,
                "prompt_n": [r.get("prompt_tokens") for r in ok], "cached": [r.get("cached_tokens") for r in ok],
                "pp": [round((r.get("prompt_tokens", 0) - r.get("cached_tokens", 0)) / r["time_prefill"]) if r.get("time_prefill") else None for r in ok],
                "acc": acc, "rej": rej, "a": round(acc / (acc + rej), 3) if acc + rej else None, "smi_peak_mib": peak[0],
                "errors": [r["error"] for r in fin.values() if "error" in r]}), flush=True)
    stop.set(); print(json.dumps({"tag": args.tag, "event": "done", "smi_peak_mib": peak[0], "free_at_peak_mib": 24576 - peak[0]}), flush=True)
if __name__ == "__main__":
    main()
