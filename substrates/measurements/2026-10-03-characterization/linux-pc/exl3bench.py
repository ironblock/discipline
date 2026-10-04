# exl3bench.py: load an EXL3 model via exllamav3.model_init (accepts all its CLI flags: -m, -mcl, -mct, -ngr, -cs, -dm, ...),
# then time greedy 320-token generations for the same code/prose prompts as ab.sh.
import argparse, time, sys, torch
from exllamav3 import Generator, Job, model_init
from exllamav3.generator.sampler import GreedySampler
def main():
    parser = argparse.ArgumentParser(allow_abbrev=False)
    model_init.add_args(parser, cache=True, add_sampling_args=True, add_draft_model_args=True, default_cache_size=32768, default_autosplit_max_batch_size=1)
    parser.add_argument("--tag", default="exl3"); parser.add_argument("--ntok", type=int, default=320)
    args = parser.parse_args()
    t0=time.time()
    model, config, cache, tokenizer, draft_model, draft_config, draft_cache = model_init.init(args)
    print(f"[{args.tag}] loaded in {time.time()-t0:.0f}s; VRAM {torch.cuda.memory_allocated()/2**30:.1f} GiB alloc / {torch.cuda.mem_get_info()[1]/2**30 - torch.cuda.mem_get_info()[0]/2**30:.1f} GiB used", flush=True)
    gen = Generator(model=model, cache=cache, tokenizer=tokenizer, draft_model=draft_model, draft_cache=draft_cache)
    CODE='Write a complete Python module implementing an LRU cache with TTL expiry, thread safety, and statistics (hits, misses, evictions). Include docstrings and a small self-test under if __name__ == "__main__".'
    PROSE='Write a thoughtful 400-word essay on why the Mac Pro 2019 remains an interesting machine for local AI inference in 2026, covering memory bandwidth, PCIe expansion, and operating system support.'
    def chatml(u): return f"<|im_start|>user\n{u}<|im_end|>\n<|im_start|>assistant\n"
    for name, p in [("code", CODE), ("code2", CODE), ("prose", PROSE)]:
        ids = tokenizer.encode(chatml(p), add_bos=False, encode_special_tokens=True)
        t0=time.time()
        out, res = gen.generate(prompt=chatml(p), max_new_tokens=args.ntok, sampler=GreedySampler(), encode_special_tokens=True, completion_only=True, return_last_results=True, stop_conditions=[])
        dt=time.time()-t0
        ntok = res.get("new_tokens", args.ntok); pt = res.get("prompt_tokens", ids.shape[-1])
        extra = " ".join(f"{k}={res[k]}" for k in ("accepted_draft_tokens","rejected_draft_tokens","cached_tokens") if k in res)
        print(f"[{args.tag}] {name}: prompt_n={pt} predicted_n={ntok} predicted_per_second={ntok/dt:.2f} time={dt:.1f}s {extra} | text: {out[:60]!r}", flush=True)

if __name__ == "__main__":
    main()
