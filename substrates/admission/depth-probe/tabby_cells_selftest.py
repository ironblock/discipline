#!/usr/bin/env python3
"""Selftest for tabby_cells.py and tabby_fingerprint.py: a synthetic record under the registered TabbyAPI id, init then check green,
then each seeded fault red with its signature. Exit 0 only if the control is green and every fault is red."""
import json, pathlib, subprocess, sys, tempfile, shutil, tomllib
HERE = pathlib.Path(__file__).resolve().parent; sys.path.insert(0, str(HERE))
import tabby_cells as tc, tabby_fingerprint as tf
SUB = "ada48-tabbyapi-exl3-qwen38flashnext-2p05"

def make(root: pathlib.Path) -> pathlib.Path:
    reg = tc.reg_entry(SUB); fp = tf.build(reg["engine_identity"], "reasoning on", "synthetic", "t" * 64, {"main": "0" * 64})
    d = root / SUB / fp["sha256"][:12]; (d / "raw").mkdir(parents=True); r = d / "raw"
    (d / "fingerprint.json").write_text(json.dumps(fp))
    (r / "identity.json").write_text(json.dumps({"engine_identity": reg["engine_identity"]})); (r / "model.json").write_text(json.dumps({"id": "m"}))
    (r / "weights-sha256.txt").write_text("".join(f"{w}  shard\n" for w in reg["weights_main"]))
    (r / "kw.json").write_text(json.dumps({"props_template_sha256": "t" * 64, "thinking_disabled": {"reasoning_chars": 0}, "thinking_enabled": {"reasoning_chars": 90}, "default": {"reasoning_chars": 80}}))
    (r / "refusal.json").write_text(json.dumps({"candidate": {"high": {"status": 400}, "none": {"status": 400}, "low": {"status": 200}}}))
    (r / "canary.log").write_text("rate: 36/36 = 1.000\n")
    (r / "fill.json").write_text(json.dumps({"np": 4, "per_slot_target": 100, "n_ctx": 400, "vram_peak_mib": 40000, "vram_total_mib": 49140, "vram_free_at_peak_mib": 900, "bar_mib": 300, "passes_bar": True, "requests": [{"ok": 1}]}))
    return d

def run(d): return subprocess.run([sys.executable, "-B", str(HERE / "tabby_cells.py"), "check", str(d)], capture_output=True, text=True)

def main() -> int:
    ok = True
    def expect(name, d, want_rc, sig=""):
        nonlocal ok; r = run(d); good = r.returncode == want_rc and sig in r.stdout; ok &= good; print(("ok   " if good else "FAIL ") + name + ("" if good else f" rc={r.returncode} {r.stdout.strip()[:200]}"))
    with tempfile.TemporaryDirectory() as t:
        t = pathlib.Path(t); d = make(t / "c"); tc.init(d)
        expect("control: init then check is green", d, 0)
        for name, edit, sig in (
            ("flipped word", lambda d: (d / "cells.toml").write_text((d / "cells.toml").read_text().replace('word = "pass"', 'word = "fail"', 1)), "says 'fail'"),
            ("tampered raw byte", lambda d: (d / "raw/canary.log").write_text("rate: 36/36 = 1.000\n "), "does not hash"),
            ("headroom under the bar", lambda d: (d / "raw/fill.json").write_text((d / "raw/fill.json").read_text().replace('"vram_free_at_peak_mib": 900', '"vram_free_at_peak_mib": 100')), "[headroom]"),
            ("a capped dry run is not a pass", lambda d: (d / "raw/fill.json").write_text((d / "raw/fill.json").read_text().replace('"bar_mib": 300,', '"bar_mib": 300, "capped_dry_run": true,')), "[headroom]"),
            ("refused level returns 500", lambda d: (d / "raw/refusal.json").write_text((d / "raw/refusal.json").read_text().replace('"high": {"status": 400}', '"high": {"status": 500}')), "[kwarg_delivery]"),
            ("engine identity differs", lambda d: (d / "raw/identity.json").write_text(json.dumps({"engine_identity": "0" * 64})), "[identity]"),
            ("the maintainer's reason removed", lambda d: (d / "cells.toml").write_text((d / "cells.toml").read_text().replace(tc.HAZARD, "x")), "omits the maintainer's reason")):
            e = make(t / name.replace(" ", "-")); tc.init(e); edit(e); expect(f"fault: {name}", e, 1, sig)
        fp = json.loads((d / "fingerprint.json").read_text()); r = subprocess.run([sys.executable, str(HERE / "tabby_fingerprint.py"), "--check", str(d)], capture_output=True, text=True)
        ok &= r.returncode == 0; print(("ok   " if r.returncode == 0 else "FAIL ") + "fingerprint --check on the control")
        bad = tf.build("x", "r", "s", "t", {}); (d / "fingerprint.json").write_text(json.dumps(bad)); r = subprocess.run([sys.executable, str(HERE / "tabby_fingerprint.py"), "--check", str(d)], capture_output=True, text=True)
        ok &= r.returncode == 1; print(("ok   " if r.returncode == 1 else "FAIL ") + "fingerprint --check refuses a record not named by its sha")
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
