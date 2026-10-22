# ---- H8: GPU picking at CSS pixels on a device-pixel-ratio-2 screen with the buffer capped (pick-fn-css, dpr-cap); an orbit camera, three spheres, four clicks
CAM8 = {"alpha": -math.pi / 2, "beta": 1.1, "radius": 12.0, "target": [0, 1, 0], "fov": 0.8}
CAM8_FLAT = arc_to_cam(CAM8)
SPH8 = [("a", [-3, 1, 0], [0.9, 0.1, 0.1]), ("b", [0, 1, 0], [0.1, 0.9, 0.1]), ("c", [3, 1, 0], [0.1, 0.2, 0.9])]


def sc_h8(s):
    s.load(viewport="800x600x2")
    s.wait(1.0)
    s.snap_dump("pre")
    pts = [("c1", project(CAM8_FLAT, [3, 1, 0], 800, 600)[:2]), ("c2", project(CAM8_FLAT, [-3, 1, 0], 800, 600)[:2]), ("c3", project(CAM8_FLAT, [0, 1, 0], 800, 600)[:2]), ("c4", (60, 40))]
    for lab, (px, py) in pts:
        s.click(px, py, lab)
        s.wait(1.3)
        s.eval("pick-" + lab, "() => window.pickResult === undefined ? null : JSON.parse(JSON.stringify(window.pickResult))")
    s.snap_dump("final")


def feat_h8(obs):
    pre, E, C = obs["dumps"]["pre"], obs["evals"], obs["clicks"]
    out = []
    cv = pre.get("cv") or {}
    out.append(("the screen is at device-pixel ratio 2 (the grader's emulation took)", cv.get("dpr") == 2, "2", str(cv.get("dpr"))))
    out.append(("the canvas drawing buffer is one pixel per CSS pixel (never larger), at device-pixel ratio 2", cv.get("w") is not None and cv["w"] <= cv["cw"] and cv["h"] <= cv["ch"] and cv["w"] >= 0.9 * cv["cw"], "%sx%s" % (cv.get("cw"), cv.get("ch")), "%sx%s" % (cv.get("w"), cv.get("h"))))
    cam = pre.get("camera") or {}
    out.append(("the active camera is an orbit (arc-rotate) camera with alpha -pi/2, beta 1.1, radius 12, targeting (0, 1, 0)", cam.get("kind") == "arc" and near([cam.get("alpha"), cam.get("beta"), cam.get("radius")], [-math.pi / 2, 1.1, 12.0]) and near(cam.get("target"), [0, 1, 0]),
                "arc (-1.571, 1.1, 12) (0, 1, 0)", fmt([cam.get("kind"), cam.get("alpha"), cam.get("beta"), cam.get("radius"), cam.get("target")])))
    for name, pos, rgb in SPH8:
        m = one_mesh(pre, name, out)
        if m:
            out.append(("%s: sphere of diameter 2 at %s, standard material, diffuse %s" % (name, fmt(pos), fmt(rgb)), near(m.get("pos"), pos) and near(extent(m), [2, 2, 2]) and m.get("matKind") == "standardGroupBuilder" and near((std_diffuse(m) or [0])[:3], rgb),
                        "standard, %s" % fmt(rgb), "%s %s %s" % (fmt(m.get("pos")), m.get("matKind"), fmt(std_diffuse(m)))))
    check_hemi(pre, out)
    for lab, name, cen in (("c1", "c", [3, 1, 0]), ("c2", "a", [-3, 1, 0]), ("c3", "b", [0, 1, 0]), ("c4", None, None)):
        pr = E.get("pick-" + lab)
        pr = pr if isinstance(pr, dict) and "_eval_error" not in pr else {}
        px, py = C[lab].get("x"), C[lab].get("y")
        out.append(("click %s: window.pickResult.name is %s" % (lab, name or "null (nothing under the click)"), pr.get("name") == name, str(name), fmt(pr.get("name")) if pr else "window.pickResult not set"))
        css = pr.get("css")
        out.append(("click %s: pickResult.css is the click's CSS position" % lab, isinstance(css, list) and len(css) == 2 and abs(css[0] - px) <= 2 and abs(css[1] - py) <= 2, "(%.0f, %.0f)" % (px, py), fmt(css)))
        if cen:
            o, dd = pixel_ray(CAM8_FLAT, px, py, 800, 600)
            hit = ray_sphere(o, dd, cen, 1.0)
            pt = pr.get("point")
            out.append(("click %s: pickResult.point is the front of the sphere under the click (within 0.15)" % lab, hit is not None and isinstance(pt, list) and near(pt, hit, 0.15), fmt(hit), fmt(pt)))
        else:
            out.append(("click %s: pickResult.point is null" % lab, pr.get("point") is None and bool(pr), "null", fmt(pr.get("point")) if pr else "window.pickResult not set"))
    return out


def cons_h8(obs):
    d, C = obs["dumps"]["final"], obs["clicks"]
    out = lifecycle_checks(d)
    eng = evs(d, "createEngine")
    mx = (eng[0].get("vals") or {}).get("maxDevicePixelRatio") if eng else None
    out.append(("the engine was created with maxDevicePixelRatio <= 1", mx is not None and mx <= 1, "createEngine(canvas, { maxDevicePixelRatio: 1 })", fmt(mx)))
    picks = evs(d, "pick")
    for lab in ("c1", "c2", "c3"):
        c = C[lab]
        out.append(("pickAsync was called at the click's CSS pixel position (%s)" % lab, any(abs(p["x"] - c["x"]) <= 2 and abs(p["y"] - c["y"]) <= 2 for p in picks), "pickAsync(picker, %.0f, %.0f)" % (c["x"], c["y"]), fmt([(p["x"], p["y"]) for p in picks])))
    out.append(("the answers came from Lite's GPU picker (>= 3 resolved picks)", len(evs(d, "pick.done")) >= 3, ">= 3 resolved picks", str(len(evs(d, "pick.done")))))
    return out


task(id="h8-click-probe", split="held-out", kinds=["K1", "K2", "K5", "K6"], facts=["pick-fn-css", "dpr-cap"], viewport="800x600x2", scenario=sc_h8, feature=feat_h8, constraint=cons_h8,
     prompt=lambda sealed=False: make_prompt(
         "In ./h8-click-probe, edit main.ts to build this scene with Babylon Lite and run it: an orbit (arc-rotate) camera with alpha -pi/2, beta 1.1, radius 12, targeting (0, 1, 0), as the scene's active camera; a hemispheric "
         "light pointing up; three spheres, each diameter 2, with standard materials: `a` at (-3, 1, 0) with diffuse colour red (0.9, 0.1, 0.1), `b` at (0, 1, 0) with diffuse colour green (0.1, 0.9, 0.1) and `c` at (3, 1, 0) "
         "with diffuse colour blue (0.1, 0.2, 0.9). The canvas fills the window through CSS. This page is shown on a high-DPI screen (device-pixel ratio 2): the canvas drawing buffer must never be larger than one pixel per CSS "
         "pixel. On every click on the canvas, use Lite's GPU picking to find the mesh under the click and set `window.pickResult = { name, point, css }`, where `name` is the picked mesh's name (or null when nothing is under "
         "the click), `point` is the picked point in world space as [x, y, z] (or null), and `css` is the click position in CSS pixels, relative to the canvas, as [x, y].", "h8-click-probe", sealed, dpr="2")())


# ---- H9: another hand-built mesh, a wedge (winding)
CAM9 = {"pos": [0, 4, -9], "target": [0, 0.5, 0], "fov": 0.8}


def feat_h9(obs):
    d, img = obs["dumps"]["final"], obs["shots"]["final"]
    out = []
    m = one_mesh(d, "ramp", out)
    if m:
        sc, ps = m.get("scale") or [1, 1, 1], m.get("pos") or [0, 0, 0]
        wmin = [ps[i] + (m.get("bmin") or [9, 9, 9])[i] * sc[i] for i in range(3)]
        wmax = [ps[i] + (m.get("bmax") or [9, 9, 9])[i] * sc[i] for i in range(3)]
        out.append(("ramp: occupies (-2, 0, -1) .. (2, 2, 1) in world space", near(wmin, [-2, 0, -1], 0.05) and near(wmax, [2, 2, 1], 0.05), "(-2, 0, -1) .. (2, 2, 1)", "%s .. %s" % (fmt(wmin), fmt(wmax))))
        dc = std_diffuse(m)
        out.append(("ramp: standard material, diffuse (0.2, 0.6, 0.9)", m.get("matKind") == "standardGroupBuilder" and dc is not None and near(dc[:3], [0.2, 0.6, 0.9]), "standard (0.2, 0.6, 0.9)", "%s %s" % (m.get("matKind"), fmt(dc[:3] if dc else None))))
    check_free_cam(d, [0, 4, -9], [0, 0.5, 0], out)
    out.append(("a directional light shining along (0, -1, 1) (no other light)", len(d.get("lights", [])) == 1 and d["lights"][0].get("type") == "directional" and parallel(d["lights"][0].get("dir"), [0, -1, 1]), "one directional (0, -1, 1)", fmt([(l.get("type"), l.get("dir")) for l in d.get("lights", [])])))
    for p in ([0, 1, 0], [0, 1.5, 0.5]):
        sx, sy = project(CAM9, p, 800, 600)[:2]
        mp = [sum(q[k] for q in img.region(sx - 5, sy - 5, sx + 6, sy + 6)) / 121.0 for k in range(3)]
        lum = 0.299 * mp[0] + 0.587 * mp[1] + 0.114 * mp[2]
        out.append(("the slope at %s faces the light head-on: drawn bright blue (luma > 112, b > r + 80)" % fmt(p), lum > 112 and mp[2] > mp[0] + 80, "bright blue at (%.0f, %.0f)" % (sx, sy), fmt(mp)))
    n = class_count(img, "blue")
    out.append(("a blue solid is drawn (>= 6000 px)", n >= 6000, ">= 6000", str(n)))
    return out


def cons_h9(obs):
    d = obs["dumps"]["final"]
    out = lifecycle_checks(d)
    mf = [e for e in evs(d, "meshFromData") if not e.get("internal")]
    out.append(("the ramp was hand-built with a direct createMeshFromData call (>= 6 vertices, >= 24 indices; the engine's own factories do not count)", any(e["nIdx"] >= 24 and e["nVert"] >= 6 for e in mf), "createMeshFromData(engine, ..., positions, normals, indices)", fmt([(e["nVert"], e["nIdx"]) for e in mf]) if mf else "not called directly"))
    return out


task(id="h9-wedge", split="held-out", kinds=["K5"], facts=["winding"], scenario=lambda s: (s.load(), s.wait(1.2), s.snap("final")), feature=feat_h9, constraint=cons_h9,
     prompt=lambda sealed=False: make_prompt(
         "In ./h9-wedge, edit main.ts to build this scene with Babylon Lite and run it: a free camera at (0, 4, -9) looking at (0, 0.5, 0), as the scene's active camera; a directional light shining along (0, -1, 1) "
         "(no other light); a mesh named `ramp`: a closed wedge (a right triangular prism) whose flat bottom is 4 x 2 on the plane y = 0 (x from -2 to 2, z from -1 to 1), whose vertical back face is at z = 1 (2 high), and whose "
         "sloping top runs from the low front edge (y = 0 at z = -1) up to the back edge (y = 2 at z = 1); it has a standard material whose diffuse colour is (0.2, 0.6, 0.9). Build the `ramp`'s triangles by hand from raw vertex "
         "data (positions, normals and indices; not one of the engine's ready-made shapes), with every face drawn from the outside (each of the five faces is visible from outside the wedge, lit by its outward normal).", "h9-wedge", sealed)())

# ---- H10: another instanced mesh, with per-instance scale (thin-recipe)
S10 = [0.6, 1.0, 1.4, 1.8]
X10 = [-3, -1, 1, 3]
RGB10 = [[1, 0.5, 0.1], [0, 1, 1], [0, 1, 0], [1, 0, 1]]
ORDER10 = ["orange", "cyan", "green", "magenta"]


def class_top_y(img, name):
    ys = [y for y in range(0, img.h, 2) for x in range(0, img.w, 2) if cls(img.px(x, y)) == name]
    return min(ys) if ys else None


def feat_h10(obs):
    d, img = obs["dumps"]["final"], obs["shots"]["final"]
    out = []
    out.append(("exactly one mesh in the scene", len(d.get("meshes", [])) == 1, "1", str(len(d.get("meshes", [])))))
    col = one_mesh(d, "column", out)
    ti = (col or {}).get("ti")
    out.append(("`column` is drawn 4 times (instanced)", bool(ti) and ti.get("count") == 4, "4", fmt(ti.get("count") if ti else None)))
    if col:
        out.append(("column: cylinder 2 high and 1 across with a standard white (1, 1, 1) material", near(extent(col), [1, 2, 1], 0.02) and col.get("matKind") == "standardGroupBuilder" and near((std_diffuse(col) or [0])[:3], [1, 1, 1]), "(1, 2, 1), standard, white", "%s %s" % (fmt(extent(col)), fmt(std_diffuse(col)))))
    if ti and ti.get("count") == 4:
        mt = ti["matrices"]
        ok_pos = len(mt) == 64 and all(near(mt[16 * i + 12:16 * i + 15], [X10[i], S10[i], 0]) for i in range(4))
        out.append(("instance i stands on the ground at x = -3, -1, 1, 3 (translation (x, s, 0), s = 0.6, 1, 1.4, 1.8)", ok_pos, "(-3,.6,0) (-1,1,0) (1,1.4,0) (3,1.8,0)", " ".join(fmt(mt[16 * i + 12:16 * i + 15]) for i in range(4)) if len(mt) == 64 else "bad matrices"))
        ok_sc = len(mt) == 64 and all(near([mt[16 * i], mt[16 * i + 5], mt[16 * i + 10]], [1, S10[i], 1]) for i in range(4))
        out.append(("instance i is stretched in y only: scale (1, s, 1)", ok_sc, "(1,.6,1) (1,1,1) (1,1.4,1) (1,1.8,1)", " ".join(fmt([mt[16 * i], mt[16 * i + 5], mt[16 * i + 10]]) for i in range(4)) if len(mt) == 64 else "bad matrices"))
        c = ti.get("colors")
        out.append(("per-instance colours: orange (1, 0.5, 0.1), cyan (0, 1, 1), green (0, 1, 0), magenta (1, 0, 1)", bool(c) and len(c) == 16 and all(near(c[4 * i:4 * i + 3], RGB10[i]) for i in range(4)), "orange cyan green magenta", "none" if not c else ("%d numbers: " % len(c)) + fmt(c[:8])))
    cents = []
    tops = []
    for name in ORDER10:
        cc = class_centroid(img, name)
        cents.append(cc[0] if cc and cc[2] >= 300 else None)
        tops.append(class_top_y(img, name))
    out.append(("drawn left to right as orange, cyan, green, magenta (each colour present, centroids ascending)", all(c is not None for c in cents) and all(cents[i] < cents[i + 1] for i in range(3)), "ascending x", fmt(cents)))
    out.append(("each column is taller than the one to its left (top edges rise)", all(t is not None for t in tops) and all(tops[i] > tops[i + 1] for i in range(3)), "top y strictly decreasing", fmt(tops)))
    check_free_cam(d, [0, 3, -12], [0, 1, 0], out)
    check_hemi(d, out)
    return out


def cons_h10(obs):
    d = obs["dumps"]["final"]
    out = lifecycle_checks(d)
    ts = mesh_evs(d, "thin.set", "column")
    ta = mesh_evs(d, "thin.add", "column")
    tc = mesh_evs(d, "thin.colors", "column")
    out.append(("the instances were made with the thin-instance API (setThinInstances with 4 matrices, or four addThinInstance calls)", (len(ts) >= 1 and ts[-1]["count"] == 4 and ts[-1]["len"] >= 64) or len(ta) == 4,
                "setThinInstances(column, matrices(64), 4) or 4 x addThinInstance", "%s set, %s add" % (fmt([(e["count"], e["len"]) for e in ts]), len(ta))))
    out.append(("the colours were set with setThinInstanceColors AFTER the instances existed (4 x RGBA = 16 numbers)", len(tc) >= 1 and tc[-1]["hadInstances"] and tc[-1]["len"] == 16, "setThinInstanceColors(column, colors(16)) after the instances", fmt([(e["hadInstances"], e["len"]) for e in tc]) if tc else "not called"))
    return out


task(id="h10-thin-columns", split="held-out", kinds=["K3", "K5"], facts=["thin-recipe"], scenario=lambda s: (s.load(), s.wait(1.2), s.snap("final")), feature=feat_h10, constraint=cons_h10,
     prompt=lambda sealed=False: make_prompt(
         "In ./h10-thin-columns, edit main.ts to build this scene with Babylon Lite and run it: a free camera at (0, 3, -12) looking at (0, 1, 0), as the scene's active camera; a hemispheric light pointing up; "
         "a single cylinder mesh named `column`, 2 high and 1 across, with a standard material whose diffuse colour is white (1, 1, 1), drawn 4 times with GPU instancing (one mesh, not four) at x = -3, -1, 1 and 3 "
         "(z = 0), each instance standing on the ground (y = 0) and stretched only in height: instance i is scaled by s in y (s = 0.6, 1.0, 1.4 and 1.8 for the four instances from left to right; no other scale, no rotation). "
         "The four instances are coloured, left to right: orange (1, 0.5, 0.1), cyan (0, 1, 1), green (0, 1, 0) and magenta (1, 0, 1).", "h10-thin-columns", sealed)())


# ---- H11 / H12: controls. They need no listed fact: only the basic API every tutorial task already uses (factories, materials, addToScene, registerScene, onBeforeRender).
def feat_h11(obs):
    d, img = obs["dumps"]["final"], obs["shots"]["final"]
    out = []
    cam = d.get("camera") or {}
    out.append(("the active camera is an orbit (arc-rotate) camera with alpha -pi/2, beta 1.2, radius 12, targeting (0, 1, 0)", cam.get("kind") == "arc" and near([cam.get("alpha"), cam.get("beta"), cam.get("radius")], [-math.pi / 2, 1.2, 12.0]) and near(cam.get("target"), [0, 1, 0]),
                "arc (-1.571, 1.2, 12) (0, 1, 0)", fmt([cam.get("kind"), cam.get("alpha"), cam.get("beta"), cam.get("radius"), cam.get("target")])))
    b = one_mesh(d, "base", out)
    if b:
        out.append(("base: 12 x 12 at the origin, standard material, grey (0.5, 0.5, 0.5)", near(b.get("pos"), [0, 0, 0]) and near([extent(b)[0], extent(b)[2]], [12, 12]) and b.get("matKind") == "standardGroupBuilder" and near((std_diffuse(b) or [0])[:3], [0.5, 0.5, 0.5]),
                    "origin, 12 x 12, standard, grey", "%s %s %s" % (fmt(b.get("pos")), b.get("matKind"), fmt(std_diffuse(b)))))
    for name, x, dia, rgb in (("s1", -3, 1.6, [0.9, 0.1, 0.1]), ("s2", 0, 2.0, [0.1, 0.9, 0.1]), ("s3", 3, 2.4, [0.1, 0.2, 0.9])):
        m = one_mesh(d, name, out)
        if m:
            out.append(("%s: sphere of diameter %g resting on the base at (%g, %g, 0), standard material, diffuse %s" % (name, dia, x, dia / 2, fmt(rgb)),
                        near(m.get("pos"), [x, dia / 2, 0]) and near(extent(m), [dia] * 3, 0.02) and m.get("matKind") == "standardGroupBuilder" and near((std_diffuse(m) or [0])[:3], rgb), "(%g, %g, 0), %g, standard, %s" % (x, dia / 2, dia, fmt(rgb)),
                        "%s %s %s" % (fmt(m.get("pos")), fmt(extent(m)), fmt(std_diffuse(m)))))
    check_hemi(d, out)
    cents = [(class_centroid(img, c) or (None, None, 0)) for c in ("red", "green", "blue")]
    out.append(("drawn left to right as red, green, blue (each present, centroids ascending)", all(c[0] is not None and c[2] >= 300 for c in cents) and cents[0][0] < cents[1][0] < cents[2][0], "ascending x", fmt([c[0] for c in cents])))
    return out


task(id="h11-control-trio", split="held-out", kinds=[], facts=[], control=True, scenario=lambda s: (s.load(), s.wait(1.2), s.snap("final")), feature=feat_h11, constraint=lambda obs: lifecycle_checks(obs["dumps"]["final"]),
     prompt=lambda sealed=False: make_prompt(
         "In ./h11-control-trio, edit main.ts to build this scene with Babylon Lite and run it: an orbit (arc-rotate) camera with alpha -pi/2, beta 1.2, radius 12, targeting (0, 1, 0), as the scene's active camera; "
         "a hemispheric light pointing up; a flat ground named `base`, 12 x 12, at the origin, with a standard material whose diffuse colour is grey (0.5, 0.5, 0.5); three spheres resting on it, each with a standard material: "
         "`s1`, diameter 1.6, at (-3, 0.8, 0), diffuse colour red (0.9, 0.1, 0.1); `s2`, diameter 2, at (0, 1, 0), diffuse colour green (0.1, 0.9, 0.1); `s3`, diameter 2.4, at (3, 1.2, 0), diffuse colour blue (0.1, 0.2, 0.9).",
         "h11-control-trio", sealed)())


def sc_h12(s):
    s.load()
    s.snap("a")
    s.wait(0.6)
    s.snap_dump("b")
    s.wait(1.2)
    s.snap_dump("final")


def feat_h12(obs):
    D, img = obs["dumps"], obs["shots"]["a"]
    out = []
    for lab in ("a", "b", "final"):
        d = D[lab]
        sd = start_done_t(d)
        b = (meshes_named(d, "bob") or [None])[0]
        if not b or sd is None:
            out.append(("sample %s: `bob` is in the scene" % lab, False, "1", "0"))
            continue
        t = (d["lt"]["now"] - sd) / 1000.0
        want = 1 + 0.8 * math.sin(2 * math.pi * t / 4.0)
        out.append(("sample %s (t = %.2f s after startEngine): bob's y follows 1 + 0.8 sin(2 pi t / 4) within 0.3 (x = 0, z = 0)" % (lab, t), abs(b["pos"][1] - want) < 0.3 and abs(b["pos"][0]) < 1e-3 and abs(b["pos"][2]) < 1e-3, "y = %.2f" % want, fmt(b["pos"])))
    b = (meshes_named(D["final"], "bob") or [None])[0]
    if b:
        out.append(("bob: sphere of diameter 2, standard material, green (0.1, 0.9, 0.1)", near(extent(b), [2, 2, 2]) and b.get("matKind") == "standardGroupBuilder" and near((std_diffuse(b) or [0])[:3], [0.1, 0.9, 0.1]), "2, standard, (0.1, 0.9, 0.1)", "%s %s" % (fmt(extent(b)), fmt(std_diffuse(b)))))
    check_free_cam(D["final"], [0, 3, -10], [0, 1, 0], out)
    check_hemi(D["final"], out)
    px_presence(img, "green", 0, out, "the green sphere is drawn", minimum=800)
    return out


def cons_h12(obs):
    d = obs["dumps"]["final"]
    out = lifecycle_checks(d)
    out.append(("`bob` was added once (the motion is a position update, not an add/remove each frame)", len(mesh_evs(d, "add", "bob", kind="mesh")) == 1 and not evs(d, "removeFromScene"), "1 add, no removal", "%d add(s), %d removal(s)" % (len(mesh_evs(d, "add", "bob", kind="mesh")), len(evs(d, "removeFromScene")))))
    return out


task(id="h12-control-bob", a_needs_trace=True, split="held-out", kinds=[], facts=[], control=True, scenario=sc_h12, feature=feat_h12, constraint=cons_h12,
     prompt=lambda sealed=False: make_prompt(
         "In ./h12-control-bob, edit main.ts to build this scene with Babylon Lite and run it: a free camera at (0, 3, -10) looking at (0, 1, 0), as the scene's active camera; a hemispheric light pointing up; "
         "a sphere named `bob`, diameter 2, with a standard material whose diffuse colour is green (0.1, 0.9, 0.1). From the moment `startEngine` resolves, `bob` moves up and down: on every frame, from a per-frame callback, "
         "its position is (0, 1 + 0.8 sin(2 pi t / 4), 0) where t is the number of seconds since `startEngine` resolved.", "h12-control-bob", sealed)())
