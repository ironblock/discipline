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
sys.argv = ["main.py"] + sys.argv[1:]
runpy.run_path(os.path.join(T, "main.py"), run_name="__main__")
