# Runs TabbyAPI's main.py unchanged, with a read-only hook that appends ExLlamaV3's cache counters to $R3_METRICS
# whenever a job finishes (eos) and at most every 10 s otherwise. Nothing the server does is altered.
import json, os, runpy, sys, time
T = os.path.expanduser("~/src/tabbyAPI"); sys.path.insert(0, T); os.chdir(T)
OUT = os.environ["R3_METRICS"]
import exllamav3.generator.generator as G
_orig = G.Generator.iterate
_last = [0.0]
def _dump(self, why, eos_ids):
    rc, cpu = self.recurrent_cache, getattr(self, "cpu_page_cache", None)
    d = {"t": time.time(), "why": why, "eos": eos_ids,
         "pagetable": dict(self.pagetable.metrics), "unref_pages": len(self.pagetable.unreferenced_pages),
         "max_pages": self.pagetable.max_pages,
         "recurrent": None if rc is None else {**rc.metrics, "n": len(rc), "bytes": rc.current_size, "max": rc.max_size},
         "cpu_tier": None if cpu is None else {**cpu.metrics, "n": len(cpu)}}
    with open(OUT, "a") as f:
        f.write(json.dumps(d) + "\n")
def iterate(self, *a, **k):
    r = _orig(self, *a, **k)
    try:
        eos = [str(x.get("identifier", x.get("serial", "?"))) for x in r if x.get("eos")]
        if eos or time.time() - _last[0] > 10:
            _dump(self, "eos" if eos else "tick", eos); _last[0] = time.time()
    except Exception as e:
        with open(OUT, "a") as f: f.write(json.dumps({"t": time.time(), "hook_error": repr(e)}) + "\n")
    return r
G.Generator.iterate = iterate

import exllamav3.generator.job as J
_orig_pfq = J.Job.prepare_for_queue
def _pfq(self, generator, serial_number, rq=False):
    try:
        return _orig_pfq(self, generator, serial_number, rq)
    except AssertionError as e:
        seqs = getattr(self, "sequences", [])
        d = {"t": time.time(), "crash_job": True, "rq": rq, "msg": str(e)[:200], "identifier": str(getattr(self, "identifier", "")),
             "input_len": [len(s.input_ids) if hasattr(s, "input_ids") else None for s in seqs],
             "seq_len": [len(s.sequence_ids) if hasattr(s, "sequence_ids") else None for s in seqs],
             "max_new_tokens": getattr(self, "max_new_tokens", None), "min_new_tokens": getattr(self, "min_new_tokens", None),
             "max_rq_tokens": getattr(self, "max_rq_tokens", None), "orig_max_rq_tokens": getattr(self, "orig_max_rq_tokens", None),
             "rq_headroom": getattr(self, "rq_headroom", None), "rq_margin": getattr(self, "rq_margin", None),
             "n_unique_hashes": len(getattr(self, "all_unique_hashes", []) or []),
             "num_draft_tokens": generator.num_draft_tokens, "boundary": generator.recurrent_checkpoint_interval,
             "max_pages": generator.pagetable.max_pages}
        with open(OUT, "a") as f: f.write(json.dumps(d) + "\n")
        raise
J.Job.prepare_for_queue = _pfq
sys.argv = ["main.py"] + sys.argv[1:]
runpy.run_path(os.path.join(T, "main.py"), run_name="__main__")
