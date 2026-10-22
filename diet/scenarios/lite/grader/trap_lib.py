"""The Babylon Lite tasks' rig (#942; python3 -I, standard library only): what the graders and the scenario's services share.
  Cdt     the chrome-devtools CLI against ONE daemon (selected by session id)
  Daemon  one headless chrome-devtools daemon (own profile, own workspace); `vision` starts it with --experimentalVision, so click_at sends real input events
  Vite    a dev server rooted at a worktree with Lite's source aliased in read-only; LITE_TRACE=1 gives the GRADER flavour (lifecycle-recording wrappers)
  Png     a stdlib PNG decoder for pixel checks
Every process started here is ended by PID (vite: its own process group), session id, or its own unique profile path; never by a command pattern.
The Lite checkout is $LITE_REPO, else ~/git/reference/Babylon/Babylon-Lite: the pinned reference, outside the tree (services/lite.py checks its commit)."""
import json, os, re, shutil, signal, socket, struct, subprocess, time, uuid, zlib

HERE = os.path.dirname(os.path.abspath(__file__))
L = os.path.dirname(HERE)                                  # diet/scenarios/lite
HOME = os.path.expanduser("~")
LITE_REPO = os.environ.get("LITE_REPO") or os.path.join(HOME, "git", "reference", "Babylon", "Babylon-Lite")
LITE_SRC = os.path.join(LITE_REPO, "packages", "babylon-lite", "src")
VITE_BIN = os.path.join(LITE_REPO, "node_modules", ".bin", "vite")
VITE_CONFIG = os.path.join(L, "vite", "vite.config.mjs")
DUMP_JS = open(os.path.join(HERE, "dump.js")).read()
INIT_JS = open(os.path.join(HERE, "initscript.js")).read()
CDT_ENV_BASE = {"PATH": os.environ.get("PATH", "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"), "HOME": HOME, "LANG": "en_US.UTF-8", "TERM": "dumb",
                "CHROME_DEVTOOLS_MCP_NO_UPDATE_CHECKS": "1", "CHROME_DEVTOOLS_MCP_NO_USAGE_STATISTICS": "1"}
NOISE = ("ExperimentalWarning", "trace-warnings", "Update available", "npm install -g chrome-devtools-mcp")


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    p = s.getsockname()[1]
    s.close()
    return p


def real(p):
    return os.path.realpath(p)


def clean(text):
    return "\n".join(l for l in text.splitlines() if l.strip() and not any(n in l for n in NOISE))


class Cdt:
    def __init__(self, sid):
        self.sid = sid
        self.env = dict(CDT_ENV_BASE, CD4A_INTERNAL_DAEMON_SESSION_ID=sid)

    def run(self, *args, timeout=60, cwd=None):
        p = subprocess.run(["chrome-devtools", *map(str, args)], env=self.env, cwd=cwd, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=timeout)
        return p.returncode, clean(p.stdout), clean(p.stderr)

    def json(self, *args, timeout=60):
        rc, out, err = self.run(*args, "--output-format", "json", timeout=timeout)
        try:
            return json.loads(out)
        except ValueError:
            return {"error": out or err, "rc": rc}

    def alive(self):
        return "daemon is running" in self.run("status", timeout=30)[1]

    def eval(self, fn, page=1, timeout=60):
        r = self.json("evaluate_script", fn, "--pageId", page, timeout=timeout)
        m = re.search(r"```json\n(.*)\n```", r.get("message", ""), re.S)
        if not m:
            return {"_eval_error": r.get("message") or r.get("error") or str(r)}
        try:
            return json.loads(m.group(1))
        except ValueError:
            return {"_eval_error": m.group(1)}


class Daemon:
    def __init__(self, run_dir, label, workspace, port, vision=False, extra_ports=()):
        self.extra_ports = list(extra_ports)
        self.dir = os.path.join(run_dir, label)
        self.profile = os.path.join(self.dir, "chrome-profile")
        self.sid = str(uuid.uuid4())
        self.workspace, self.port, self.vision = real(workspace), port, vision
        self.cdt = Cdt(self.sid)
        self.socket = None

    def start(self):
        os.makedirs(self.dir, exist_ok=True)
        args = ["start", "--headless=true", "--userDataDir", self.profile, "--logFile", os.path.join(self.dir, "daemon.log"), "--workspace", self.workspace,
                "--categoryExtensions=false", "--no-performance-crux"]
        for p in [self.port] + self.extra_ports:
            args += ["--allowedUrlPattern", "http://localhost:%d/*" % p, "--allowedUrlPattern", "http://127.0.0.1:%d/*" % p]
        if self.vision:
            args.append("--experimentalVision=true")
        rc, out, err = self.cdt.run(*args, timeout=120, cwd=self.workspace)
        if rc != 0:
            raise RuntimeError("chrome-devtools start exited %s: %s %s" % (rc, out, err))
        rc, out, err = self.cdt.run("list_pages", timeout=120)
        if rc != 0 or "Pages" not in out:
            raise RuntimeError("daemon started but Chrome did not: %s %s" % (out, err))
        m = re.search(r"socket=(\S+)", self.cdt.run("status")[1])
        self.socket = real(m.group(1)) if m else None
        return self

    def stop(self):
        try:
            if self.cdt.alive():
                self.cdt.run("stop", timeout=60)
        except Exception:  # noqa
            pass
        time.sleep(1.5)
        left = subprocess.run(["pgrep", "-f", "--", "--user-data-dir=" + self.profile], capture_output=True, text=True).stdout.split()   # our own unique profile path
        for pid in left:
            try:
                os.kill(int(pid), signal.SIGTERM)
            except (ProcessLookupError, ValueError):
                pass
        return len(left)


class Vite:
    def __init__(self, root, port, log_path, cache, trace=False):
        self.root, self.port, self.log_path, self.cache, self.trace, self.proc = real(root), port, log_path, cache, trace, None

    def start(self, timeout=60):
        env = {"PATH": "/usr/local/bin:/usr/bin:/bin", "HOME": HOME, "LITE_REPO": LITE_REPO, "LITE_SRC": LITE_SRC, "VITE_ROOT": self.root, "VITE_CACHE": self.cache, "VITE_PORT": str(self.port)}
        if self.trace:
            env["LITE_TRACE"] = "1"
        os.makedirs(self.cache, exist_ok=True)
        self.proc = subprocess.Popen([VITE_BIN, "--config", VITE_CONFIG], env=env, cwd=L, stdin=subprocess.DEVNULL, stdout=open(self.log_path, "w"), stderr=subprocess.STDOUT, start_new_session=True)
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            if self.proc.poll() is not None:
                raise RuntimeError("vite exited %s: %s" % (self.proc.returncode, open(self.log_path).read()[-500:]))
            try:
                socket.create_connection(("127.0.0.1", self.port), timeout=1).close()
                return self
            except OSError:
                time.sleep(0.3)
        raise RuntimeError("vite did not listen")

    def stop(self):
        if self.proc and self.proc.poll() is None:
            for sig in (signal.SIGTERM, signal.SIGKILL):
                try:
                    os.killpg(self.proc.pid, sig)
                except ProcessLookupError:
                    break
                try:
                    self.proc.wait(timeout=10)
                    break
                except subprocess.TimeoutExpired:
                    continue


# ------------------------------------------------------------------ pixels
class Png:
    """8-bit RGB/RGBA non-interlaced PNG -> .w .h .px(x, y) = (r, g, b, a)."""

    def __init__(self, path):
        data = open(path, "rb").read()
        assert data[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
        pos, idat, ihdr = 8, b"", None
        while pos < len(data):
            n, typ = struct.unpack(">I4s", data[pos:pos + 8])
            body = data[pos + 8:pos + 8 + n]
            if typ == b"IHDR":
                ihdr = struct.unpack(">IIBBBBB", body)
            elif typ == b"IDAT":
                idat += body
            pos += 12 + n
        self.w, self.h, depth, ctype, _, _, interlace = ihdr
        assert depth == 8 and ctype in (2, 6) and interlace == 0, "unsupported PNG %s" % (ihdr,)
        bpp = 3 if ctype == 2 else 4
        raw = zlib.decompress(idat)
        stride = self.w * bpp
        rows, prev = [], bytearray(stride)
        i = 0
        for _ in range(self.h):
            ft = raw[i]
            line = bytearray(raw[i + 1:i + 1 + stride])
            i += 1 + stride
            if ft == 1:
                for k in range(bpp, stride):
                    line[k] = (line[k] + line[k - bpp]) & 255
            elif ft == 2:
                for k in range(stride):
                    line[k] = (line[k] + prev[k]) & 255
            elif ft == 3:
                for k in range(stride):
                    left = line[k - bpp] if k >= bpp else 0
                    line[k] = (line[k] + ((left + prev[k]) >> 1)) & 255
            elif ft == 4:
                for k in range(stride):
                    a = line[k - bpp] if k >= bpp else 0
                    b = prev[k]
                    c = prev[k - bpp] if k >= bpp else 0
                    p = a + b - c
                    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                    pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                    line[k] = (line[k] + pr) & 255
            rows.append(line)
            prev = line
        self.rows, self.bpp = rows, bpp

    def px(self, x, y):
        x, y = max(0, min(self.w - 1, int(x))), max(0, min(self.h - 1, int(y)))
        r = self.rows[y]
        o = x * self.bpp
        return (r[o], r[o + 1], r[o + 2], r[o + 3] if self.bpp == 4 else 255)

    def region(self, x0, y0, x1, y1, step=1):
        return [self.px(x, y) for y in range(int(y0), int(y1), step) for x in range(int(x0), int(x1), step)]

    def mean(self, x0, y0, x1, y1, step=2):
        p = self.region(x0, y0, x1, y1, step)
        n = max(1, len(p))
        return tuple(sum(c[k] for c in p) / n for k in range(3))

    def count_near(self, rgb, tol, x0=0, y0=0, x1=None, y1=None, step=2):
        x1, y1 = x1 or self.w, y1 or self.h
        return sum(1 for p in self.region(x0, y0, x1, y1, step) if all(abs(p[k] - rgb[k]) <= tol for k in range(3)))


def close(a, b, tol):
    return all(abs(x - y) <= tol for x, y in zip(a[:3], b[:3]))
