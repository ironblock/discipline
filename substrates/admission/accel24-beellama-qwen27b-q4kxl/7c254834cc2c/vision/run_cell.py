#!/usr/bin/env python3
"""#373's vision cell: three sequential requests to the floor's registered serving line, raw responses kept.

Usage: run_cell.py BASE_URL CELL_DIR. Reads CELL_DIR/image.png, writes CELL_DIR/request.json (the image as a data:
URI, no key, no model field) and CELL_DIR/responses/r1..r3.json (status, headers minus any auth or cookie, body
verbatim, start and end UTC). One client, one request at a time, nothing else sent. Stdlib only."""
import base64, datetime, json, pathlib, sys, urllib.error, urllib.request

PROMPT = "What text appears in this image? Answer with the text only."
SAMPLER = {"temperature": 0.6, "top_k": 20, "top_p": 1.0, "min_p": 0.0}  # the floor's registered sampler_card
MAX_TOKENS = 8192  # reasoning stays as the serving line has it, so the budget covers a thought and the answer
N = 3


def utc(): return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%fZ")


def main(base, cell):
    image = (cell / "image.png").read_bytes()
    request = {"messages": [{"role": "user", "content": [
        {"type": "image_url", "image_url": {"url": "data:image/png;base64," + base64.b64encode(image).decode()}},
        {"type": "text", "text": PROMPT}]}], "max_tokens": MAX_TOKENS, "stream": False, **SAMPLER}
    body = json.dumps(request, separators=(",", ":")).encode()
    (cell / "request.json").write_bytes(body)
    out = cell / "responses"; out.mkdir(exist_ok=True)
    for i in range(1, N + 1):
        started = utc()
        req = urllib.request.Request(base.rstrip("/") + "/v1/chat/completions", data=body,
                                     headers={"Content-Type": "application/json"}, method="POST")
        try:
            with urllib.request.urlopen(req, timeout=900) as r:
                status, headers, raw = r.status, dict(r.headers), r.read()
        except urllib.error.HTTPError as e:
            status, headers, raw = e.code, dict(e.headers), e.read()
        headers = {k: v for k, v in headers.items() if k.lower() not in ("authorization", "set-cookie", "cookie")}
        rec = {"n": i, "started": started, "ended": utc(), "status": status, "headers": headers,
               "body": raw.decode("utf-8", "replace")}
        (out / f"r{i}.json").write_text(json.dumps(rec, indent=1) + "\n")
        print(f"r{i} {status} {len(raw)} bytes", flush=True)


if __name__ == "__main__":
    main(sys.argv[1], pathlib.Path(sys.argv[2]))
