# Babylon Lite TRAP tasks (#942; pre-registered). Loaded with runpy (python3 -I safe). Defines TASKS. Its prompts name the run's port and checkout as `{port.model_vite}` and `{service.lite.root}`, which the scenario runner fills.
# Each task: id, split ("tutorial"|"held-out"), kinds, facts, prompt(sealed) -> text with {port.model_vite}, scenario(session) (the grader's timeline), feature(obs) (tier a), constraint(obs) (tier b),
# process (tier c, defined only: which reads count as "re-derived"; computed later from the session log). The reference and wrong solutions the graders were verified against are kept private, outside the tree.
# Expected values are written by hand from the task text (never copied from engine output); pixel positions come from the hand-written camera model in grade_trap.project().
import math, os, runpy
HERE = os.path.dirname(os.path.abspath(__file__))
G = runpy.run_path(os.path.join(HERE, "grade_trap.py"))
near, fmt, cls, class_count, class_centroid, region_luma, project, unproject_to_plane_y, arc_to_cam, pixel_ray, ray_sphere = (G[k] for k in
    ("near", "fmt", "cls", "class_count", "class_centroid", "region_luma", "project", "unproject_to_plane_y", "arc_to_cam", "pixel_ray", "ray_sphere"))
evs, start_done_t, lifecycle_checks, late_add_checks, mesh_evs = G["evs"], G["start_done_t"], G["lifecycle_checks"], G["late_add_checks"], G["mesh_evs"]

SOURCE_CLAUSE = {
    False: "Its source and docs are on disk at {service.lite.root} (read-only: build only in your working directory). ",
    True: "No Lite source or docs are available to you in this run; work from what you already know about it. ",
}
PREAMBLE = (
    "Babylon Lite is a WebGPU 3D engine that is not in your training data. {SRC}Each task directory holds an index.html (with a canvas #renderCanvas) "
    "that loads ./main.ts, and a dev server already serves this working directory at http://localhost:{port.model_vite}/ and resolves `import ... from \"babylon-lite\"` to the Lite source. "
    "A Chrome browser is already running for you: drive it with the `chrome-devtools` command-line tool. It's a CLI; there are no browser MCP tools. "
    "Don't run its `start` or `stop`. A failed command still exits 0, so read its output. Give --filePath an absolute path. "
)
CLOSE = (
    " Once the engine is running, set `window.scene` to the scene object, so the page can be checked. "
    "The page is checked in a headless browser: it is loaded at http://localhost:{port.model_vite}/%s/ , `window.scene` is read, and where the task says so a real mouse click is sent to the canvas. "
    "The page's window is 800 x 600 CSS pixels at a device-pixel ratio of %s unless the task says otherwise."
)


def make_prompt(body, dirname, sealed=False, dpr="1"):
    def f(sealed_=sealed):
        return (PREAMBLE.replace("{SRC}", SOURCE_CLAUSE[bool(sealed_)]) + body + CLOSE % (dirname, dpr))
    return f


TASKS = []


def task(**kw):
    kw.setdefault("viewport", "800x600x1")
    TASKS.append(kw)
    return kw


# ------------------------------------------------------------------ shared check builders
def meshes_named(d, name):
    return [m for m in (d or {}).get("meshes", []) if m.get("name") == name]


def one_mesh(d, name, out, present=True):
    ms = meshes_named(d, name)
    out.append(("mesh `%s` is in the scene (once)" % name if present else "mesh `%s` is not in the scene" % name, (len(ms) == 1) if present else (len(ms) == 0), "1" if present else "0", str(len(ms))))
    return ms[0] if len(ms) == 1 else None


def extent(m):
    if not m or not m.get("bmin") or not m.get("bmax") or not m.get("scale"):
        return None
    return [(hi - lo) * abs(s) for lo, hi, s in zip(m["bmin"], m["bmax"], m["scale"])]


def std_diffuse(m):
    return ((m or {}).get("material") or {}).get("diffuseColor")


def check_box(d, name, pos, edge, rgb, out, family="standardGroupBuilder"):
    m = one_mesh(d, name, out)
    if not m:
        out += [("`%s` position" % name, False, fmt(pos), "no mesh"), ("`%s` edge length %g" % (name, edge), False, str(edge), "no mesh"), ("`%s` standard material, diffuse colour" % name, False, fmt(rgb), "no mesh")]
        return None
    out.append(("`%s` position" % name, near(m.get("pos"), pos), fmt(pos), fmt(m.get("pos"))))
    e = extent(m)
    out.append(("`%s` edge length %g on every axis" % (name, edge), near(e, [edge] * 3), fmt([edge] * 3), fmt(e)))
    dc = std_diffuse(m)
    out.append(("`%s` standard material, diffuse colour" % name, m.get("matKind") == family and dc is not None and near(dc[:3], rgb), "%s %s" % ("standard", fmt(rgb)), "%s %s" % (m.get("matKind"), fmt(dc[:3] if dc else None))))
    return m


def check_free_cam(d, pos, tgt, out):
    c = (d or {}).get("camera")
    out.append(("the active camera is a free camera", bool(c) and c.get("kind") == "free", "free camera", (c or {}).get("kind", "no camera")))
    out.append(("camera position", bool(c) and near(c.get("pos"), pos), fmt(pos), fmt((c or {}).get("pos"))))
    out.append(("camera looks at", bool(c) and near(c.get("target"), tgt), fmt(tgt), fmt((c or {}).get("target"))))


def check_hemi(d, out):
    ok = any(l.get("type") == "hemispheric" and l.get("dir") and near(l["dir"], [0, 1, 0]) for l in (d or {}).get("lights", []))
    out.append(("a hemispheric light pointing up", ok, "hemispheric (0, 1, 0)", fmt([(l.get("type"), l.get("dir")) for l in (d or {}).get("lights", [])]) or "no lights"))


def px_presence(img, name, lo, out, label, half=None, minimum=800):
    """colour class `name` present (>= minimum px) in the whole frame or the given half ('left'/'right'), or absent (< lo px) when minimum is None."""
    x0, x1 = (0, img.w // 2) if half == "left" else ((img.w // 2, img.w) if half == "right" else (0, img.w))
    n = class_count(img, name, x0, 0, x1, img.h)
    if minimum is None:
        out.append((label, n < lo, "fewer than %d %s px" % (lo, name), "%d %s px" % (n, name)))
    else:
        out.append((label, n >= minimum, "at least %d %s px%s" % (minimum, name, (" in the %s half" % half) if half else ""), "%d %s px" % (n, name)))
    return n


for _f in ("tasks_trap_defs.py", "tasks_trap_held1.py", "tasks_trap_held2.py"):
    if os.path.exists(os.path.join(HERE, _f)):
        exec(open(os.path.join(HERE, _f)).read())
