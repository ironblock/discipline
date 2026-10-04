# exl3spd.py: the production bench (deep.py) re-done on ExLlamaV3's own generator: same wikitext text, same
# per-request offsets, same "continue" prompt rendered through the model's chat template, greedy, 256 new tokens.
# rep 0 = warm-up (prefill), reps 1-2 = cache-warm decode. PHASES="depth:conc,...".
import argparse, json, os, re, time, torch, jinja2
from exllamav3 import Generator, Job, model_init
from exllamav3.generator.sampler import GreedySampler

PROMPT = "\n\nContinue the passage above in the same register for several paragraphs."


def main():
    ap = argparse.ArgumentParser(allow_abbrev=False)
    model_init.add_args(ap, cache=True, add_sampling_args=False, add_draft_model_args=True,
                        default_cache_size=131072, default_autosplit_max_batch_size=4)
    ap.add_argument("--tag", default="exl3")
    ap.add_argument("--phases", default=os.environ.get("PHASES", "10000:1,10000:2,10000:4,100000:1"))
    args = ap.parse_args()

    t0 = time.time()
    model, config, cache, tokenizer, draft_model, draft_config, draft_cache = model_init.init(args)
    free, total = torch.cuda.mem_get_info()
    print(json.dumps({"tag": args.tag, "event": "loaded", "load_s": round(time.time() - t0),
                      "vram_used_gib": round((total - free) / 2**30, 1)}), flush=True)
    gen = Generator(model=model, cache=cache, tokenizer=tokenizer, draft_model=draft_model, draft_cache=draft_cache,
                    num_draft_tokens=args.num_draft_tokens or 3, max_chunk_size=args.chunk_size, dynamic_draft_tokens=bool(getattr(args, "dynamic_draft", False)), draft_confidence=getattr(args, "draft_confidence", 0.4))

    tmpl = jinja2.Environment(loader=jinja2.BaseLoader()).from_string(
        open(os.path.join(args.model_dir, "chat_template.jinja")).read())
    tmpl.globals["raise_exception"] = lambda m: (_ for _ in ()).throw(Exception(m))
    text = re.sub(r"\n+", " ", open(os.path.expanduser("~/setup/wikitext-2-raw/wiki.test.raw"), errors="replace").read())
    probe = text[:40000]
    cpt = len(probe) / tokenizer.encode(probe, add_bos=False).shape[-1]

    def ids_for(i, depth):
        off = i * 60000
        msg = [{"role": "user", "content": text[off:off + int(depth * cpt)] + PROMPT}]
        s = tmpl.render(messages=msg, add_generation_prompt=True)
        return tokenizer.encode(s, add_bos=False, encode_special_tokens=True)

    first = True
    for ph in args.phases.split(","):
        depth, c = (int(x) for x in ph.split(":"))
        for rep in range(3):
            jobs = []
            for i in range(c):
                ids = ids_for(i, depth)
                if first:
                    print(json.dumps({"tag": args.tag, "event": "prompt_tail", "tail": tokenizer.decode(ids[:, -24:])[0]}), flush=True)
                    first = False
                j = Job(input_ids=ids, max_new_tokens=256, sampler=GreedySampler(), stop_conditions=[], identifier=i)
                jobs.append(j); gen.enqueue(j)
            fin = {}; t0 = time.time()
            while gen.num_remaining_jobs():
                for r in gen.iterate():
                    if r.get("stage") == "error":
                        fin[r.get("identifier")] = {"error": str(r)[:200]}
                    elif r.get("eos"):
                        fin[r["identifier"]] = r
            wall = time.time() - t0
            ok = [r for r in fin.values() if "error" not in r]
            per = [round((r["new_tokens"] - 1) / r["time_generate"], 2) for r in ok if r.get("time_generate")]
            acc = sum(r.get("accepted_draft_tokens", 0) for r in ok)
            rej = sum(r.get("rejected_draft_tokens", 0) for r in ok)
            print(json.dumps({"tag": args.tag, "depth": depth, "conc": c, "rep": rep, "wall_s": round(wall, 1),
                              "per_req": per, "agg_wall": round(sum(r["new_tokens"] for r in ok) / wall, 1),
                              "pn": [r.get("prompt_tokens") for r in ok], "cached": [r.get("cached_tokens") for r in ok],
                              "pp": [round((r.get("prompt_tokens", 0) - r.get("cached_tokens", 0)) / r["time_prefill"])
                                     if r.get("time_prefill") else None for r in ok],
                              "a": round(acc / (acc + rej), 3) if acc + rej else None,
                              "errors": [r["error"] for r in fin.values() if "error" in r]}), flush=True)


if __name__ == "__main__":
    main()
