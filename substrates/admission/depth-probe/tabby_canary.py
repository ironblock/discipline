#!/usr/bin/env python3
"""diet-inference's instruments/canary.py against a TabbyAPI endpoint (#393): the same replay, with the bearer key from TABBY_API_KEY
added to every request. The key is never printed or written. Usage is canary.py's, run from the diet-inference checkout:
  TABBY_API_KEY=... python3 tabby_canary.py --canary-dir <diet-inference>/instruments <canary.py's arguments>"""
import os, sys, urllib.request

def main() -> int:
    argv = sys.argv[1:]
    if "--canary-dir" not in argv: raise SystemExit("--canary-dir <diet-inference>/instruments is required")
    i = argv.index("--canary-dir"); d = argv[i + 1]; del argv[i:i + 2]
    key = os.environ.get("TABBY_API_KEY")
    if not key: raise SystemExit("TABBY_API_KEY is not set")
    orig = urllib.request.Request
    def request(url, *a, **kw):
        r = orig(url, *a, **kw); r.add_header("Authorization", f"Bearer {key}"); return r
    urllib.request.Request = request
    sys.path.insert(0, d); sys.argv = ["canary.py"] + argv
    import canary
    return canary.main()

if __name__ == "__main__":
    sys.exit(main())
