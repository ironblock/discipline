# ------------------------------------------------------------------ tutorial tasks (the model learns the facts here; interview forks capture them)
# ---- T1: add a mesh to the running scene on a click (fact late-add)
def sc_t1(s):
    s.load()
    s.wait(1.0)
    s.snap("pre")
    s.click(400, 300, "c1")
    s.wait(1.8)
    s.snap("final")


def feat_t1(obs):
    pre, post, ip, iq = obs["dumps"]["pre"], obs["dumps"]["final"], obs["shots"]["pre"], obs["shots"]["final"]
    out = []
    check_box(pre, "base", [-2, 1, 0], 2, [0.2, 0.4, 0.9], out)
    px_presence(ip, "blue", 0, out, "before the click: the blue box is drawn on the left", half="left")
    px_presence(ip, "red", 100, out, "before the click: nothing red is drawn", minimum=None)
    check_box(post, "crate", [2, 1, 0], 2, [0.9, 0.1, 0.1], out)
    px_presence(iq, "red", 0, out, "after the click: the red box is drawn on the right", half="right")
    px_presence(iq, "blue", 0, out, "after the click: the blue box is still drawn on the left", half="left")
    check_free_cam(post, [0, 3, -10], [0, 1, 0], out)
    check_hemi(post, out)
    return out


def cons_t1(obs):
    d = obs["dumps"]["final"]
    return lifecycle_checks(d) + late_add_checks(d, "crate", not_before_t=obs["clicks"]["c1"]["t"] - 50)


task(id="t1-late-box", split="tutorial", kinds=["K1", "K2"], facts=["late-add"], scenario=sc_t1, feature=feat_t1, constraint=cons_t1,
     prompt=lambda sealed=False: make_prompt(
         "In ./t1-late-box, edit main.ts to build this scene with Babylon Lite and run it: a free camera at (0, 3, -10) looking at (0, 1, 0), as the scene's active camera; "
         "a hemispheric light pointing up; a box named `base`, 2 units along each edge, at (-2, 1, 0), with a standard material whose diffuse colour is blue (0.2, 0.4, 0.9). "
         "The scene starts with only that box. When the canvas is clicked, a second box named `crate`, 2 units along each edge, at (2, 1, 0), with a standard material whose diffuse colour "
         "is red (0.9, 0.1, 0.1), must appear in the running scene. The `crate` is added to the scene at click time (it does not exist in the scene before the click) while the engine keeps "
         "running: the engine and the scene are not torn down and rebuilt.", "t1-late-box", sealed)())


# ---- T2: a PBR mesh added late into a scene that has only standard materials (facts late-add, late-family)
def sc_t2(s):
    s.load()
    s.snap("pre")
    s.until_start_plus(8.0)
    s.wait(1.8)
    s.snap("final")


def feat_t2(obs):
    pre, post, ip, iq = obs["dumps"]["pre"], obs["dumps"]["final"], obs["shots"]["pre"], obs["shots"]["final"]
    out = []
    sd = start_done_t(pre)
    st = obs["shot_t"]["pre"]
    out.append(("the grader's first look was taken before the timer fired", sd is not None and st - sd < 7000, "< 7 s after startEngine", "%s ms" % (st - sd if sd is not None else "?")))
    fl = one_mesh(post, "floor", out)
    if fl:
        out.append(("floor is at the origin, 12 x 12, standard material, green (0.2, 0.6, 0.2)", near(fl.get("pos"), [0, 0, 0]) and near([extent(fl)[0], extent(fl)[2]], [12, 12]) and fl.get("matKind") == "standardGroupBuilder" and near((std_diffuse(fl) or [0])[:3], [0.2, 0.6, 0.2]),
                    "origin, 12 x 12, standard, (0.2, 0.6, 0.2)", "%s %s %s" % (fmt(fl.get("pos")), fl.get("matKind"), fmt(std_diffuse(fl)))))
    check_box_like = one_mesh(post, "ball", out)
    if check_box_like:
        m = check_box_like
        e = extent(m)
        out.append(("ball: sphere of diameter 2 at (-2, 1, 0), standard material, blue (0.2, 0.4, 0.9)", near(m.get("pos"), [-2, 1, 0]) and near(e, [2, 2, 2]) and m.get("matKind") == "standardGroupBuilder" and near((std_diffuse(m) or [0])[:3], [0.2, 0.4, 0.9]),
                    "(-2, 1, 0), 2, standard, (0.2, 0.4, 0.9)", "%s %s %s %s" % (fmt(m.get("pos")), fmt(e), m.get("matKind"), fmt(std_diffuse(m)))))
    p = one_mesh(post, "pillar", out)
    if p:
        mat = p.get("material") or {}
        out.append(("pillar: cylinder 3 high, 1.5 across, at (2, 1.5, 0)", near(p.get("pos"), [2, 1.5, 0]) and near(extent(p), [1.5, 3, 1.5], 0.02), "(2, 1.5, 0), (1.5, 3, 1.5)", "%s %s" % (fmt(p.get("pos")), fmt(extent(p)))))
        out.append(("pillar: PBR material, base colour yellow (0.9, 0.8, 0.1), metallic 0.1, roughness 0.5",
                    p.get("matKind") == "pbrGroupBuilder" and near((mat.get("baseColorFactor") or [0])[:3], [0.9, 0.8, 0.1]) and near([mat.get("metallicFactor")], [0.1]) and near([mat.get("roughnessFactor")], [0.5]),
                    "pbr (0.9, 0.8, 0.1) 0.1 0.5", "%s %s %s %s" % (p.get("matKind"), fmt(mat.get("baseColorFactor")), fmt(mat.get("metallicFactor")), fmt(mat.get("roughnessFactor")))))
    px_presence(ip, "green", 0, out, "before the timer: the green floor is drawn", minimum=3000)
    px_presence(ip, "blue", 0, out, "before the timer: the blue ball is drawn on the left", half="left")
    px_presence(ip, "yellow", 100, out, "before the timer: nothing yellow is drawn", minimum=None)
    px_presence(iq, "yellow", 0, out, "after the timer: the yellow pillar is drawn on the right", half="right", minimum=800)
    px_presence(iq, "blue", 0, out, "after the timer: the blue ball is still drawn", half="left")
    check_free_cam(post, [0, 3, -10], [0, 1, 0], out)
    check_hemi(post, out)
    return out


def cons_t2(obs):
    d = obs["dumps"]["final"]
    return lifecycle_checks(d) + late_add_checks(d, "pillar", not_before_t=start_done_t(d) + 8000 - 400)


task(id="t2-late-pbr", a_needs_trace=True, split="tutorial", kinds=["K1", "K4"], facts=["late-add", "late-family"], scenario=sc_t2, feature=feat_t2, constraint=cons_t2,
     prompt=lambda sealed=False: make_prompt(
         "In ./t2-late-pbr, edit main.ts to build this scene with Babylon Lite and run it: a free camera at (0, 3, -10) looking at (0, 1, 0), as the scene's active camera; a hemispheric light pointing up; "
         "a flat ground named `floor`, 12 x 12, at the origin, with a standard (Blinn-Phong) material whose diffuse colour is green (0.2, 0.6, 0.2); a sphere named `ball`, diameter 2, at (-2, 1, 0), "
         "with a standard material whose diffuse colour is blue (0.2, 0.4, 0.9). Exactly 8 seconds after `startEngine` resolves, a cylinder named `pillar`, 3 units high and 1.5 across, at (2, 1.5, 0), "
         "with a PBR material (base colour yellow (0.9, 0.8, 0.1), metallic 0.1, roughness 0.5), must appear in the running scene. The `pillar` is added to the scene at that moment (it does not exist in the scene "
         "before) while the engine keeps running: the engine and the scene are not torn down and rebuilt. The floor and the ball keep their standard materials.", "t2-late-pbr", sealed)())

# ---- T3: hide/show a mesh and remove another, at runtime (facts late-hide, late-remove)
def sc_t3(s):
    s.load()
    s.wait(1.0)
    s.snap("pre")
    for i, lab in enumerate(("c1", "c2", "c3")):
        s.click(400, 300, lab)
        s.wait(1.5)
        s.snap("s%d" % (i + 1) if i < 2 else "final")


def feat_t3(obs):
    D, S = obs["dumps"], obs["shots"]
    out = []
    for lab, want_lamp, want_hoop in (("pre", True, True), ("s1", False, True), ("s2", True, True), ("final", True, False)):
        img, d = S[lab], D[lab]
        what = {"pre": "before any click", "s1": "after the 1st click", "s2": "after the 2nd click", "final": "after the 3rd click"}[lab]
        n = class_count(img, "yellow")
        out.append(("%s: the yellow lamp is %s" % (what, "drawn" if want_lamp else "not drawn"), (n >= 800) if want_lamp else (n < 100), "%s" % ("yellow >= 800 px" if want_lamp else "yellow < 100 px"), "%d yellow px" % n))
        n = class_count(img, "red")
        out.append(("%s: the red hoop is %s" % (what, "drawn" if want_hoop else "not drawn"), (n >= 300) if want_hoop else (n < 100), "%s" % ("red >= 300 px" if want_hoop else "red < 100 px"), "%d red px" % n))
        lamp = meshes_named(d, "lamp")
        out.append(("%s: `lamp` is still in the scene's mesh list (hidden, not removed)" % what, len(lamp) == 1, "1", str(len(lamp))))
        hoop = meshes_named(d, "hoop")
        out.append(("%s: `hoop` %s the scene's mesh list" % (what, "is in" if want_hoop else "is gone from"), (len(hoop) == 1) if want_hoop else (len(hoop) == 0), "1" if want_hoop else "0", str(len(hoop))))
    pre = D["pre"]
    lamp, hoop = (meshes_named(pre, "lamp") or [None])[0], (meshes_named(pre, "hoop") or [None])[0]
    if lamp:
        out.append(("lamp: sphere of diameter 2 at (-3, 1, 0), standard material, yellow (0.9, 0.8, 0.1)", near(lamp.get("pos"), [-3, 1, 0]) and near(extent(lamp), [2, 2, 2]) and lamp.get("matKind") == "standardGroupBuilder" and near((std_diffuse(lamp) or [0])[:3], [0.9, 0.8, 0.1]),
                    "(-3, 1, 0), 2, standard, (0.9, 0.8, 0.1)", "%s %s %s" % (fmt(lamp.get("pos")), fmt(extent(lamp)), fmt(std_diffuse(lamp)))))
    if hoop:
        out.append(("hoop: torus of diameter 2.4 and thickness 0.6 at (3, 1, 0), standard material, red (0.9, 0.1, 0.1)",
                    near(hoop.get("pos"), [3, 1, 0]) and near([extent(hoop)[0], extent(hoop)[2]], [3.0, 3.0], 0.15) and hoop.get("matKind") == "standardGroupBuilder" and near((std_diffuse(hoop) or [0])[:3], [0.9, 0.1, 0.1]),
                    "(3, 1, 0), (2.4 + 0.6) across", "%s %s %s" % (fmt(hoop.get("pos")), fmt(extent(hoop)), fmt(std_diffuse(hoop)))))
    check_free_cam(pre, [0, 3, -10], [0, 1, 0], out)
    check_hemi(pre, out)
    return out


def cons_t3(obs):
    d, c = obs["dumps"]["final"], obs["clicks"]
    out = lifecycle_checks(d)
    sv = mesh_evs(d, "setVisible", "lamp")
    seq = [e["v"] for e in sv]
    out.append(("the lamp was hidden and shown through setSubtreeVisible (false, then true)", len(seq) >= 2 and seq[0] is False and seq[1] is True, "setSubtreeVisible(lamp, false) then (lamp, true)", "calls: %s" % seq))
    out.append(("the lamp was never removed from the scene", not mesh_evs(d, "removeFromScene", "lamp"), "no removeFromScene(lamp)", "%d" % len(mesh_evs(d, "removeFromScene", "lamp"))))
    rm = mesh_evs(d, "removeFromScene", "hoop")
    out.append(("the hoop was taken out through removeFromScene, after the 3rd click", len(rm) == 1 and rm[0]["t"] >= c["c3"]["t"] - 50, "one removeFromScene(hoop) at or after click 3", "%d call(s)%s" % (len(rm), (", t=%d" % rm[0]["t"]) if rm else "")))
    return out


task(id="t3-toggle-remove", split="tutorial", kinds=["K4", "K3", "K2"], facts=["late-hide", "late-remove"], scenario=sc_t3, feature=feat_t3, constraint=cons_t3,
     prompt=lambda sealed=False: make_prompt(
         "In ./t3-toggle-remove, edit main.ts to build this scene with Babylon Lite and run it: a free camera at (0, 3, -10) looking at (0, 1, 0), as the scene's active camera; a hemispheric light pointing up; "
         "a sphere named `lamp`, diameter 2, at (-3, 1, 0), with a standard material whose diffuse colour is yellow (0.9, 0.8, 0.1); a torus named `hoop`, diameter 2.4 and thickness 0.6, at (3, 1, 0), with a standard material "
         "whose diffuse colour is red (0.9, 0.1, 0.1). Each click on the canvas does the next thing, in order: the 1st click hides `lamp` (it stops being drawn but stays in the scene's mesh list); the 2nd click shows `lamp` again; "
         "the 3rd click removes `hoop` from the scene for good (it is gone from the scene's mesh list and no longer drawn). The engine keeps running throughout.", "t3-toggle-remove", sealed)())


# ---- T4: a per-frame edit of a material that is already drawing (fact material-dirty)
def sc_t4(s):
    s.load()
    s.snap("early")
    s.until_start_plus(21.0)
    s.wait(0.4)
    s.snap("final")


def mean_px(img, sx, sy, half=8):
    p = img.region(sx - half, sy - half, sx + half + 1, sy + half + 1)
    return [sum(q[k] for q in p) / len(p) for k in range(3)]


def feat_t4(obs):
    e, f, ie, iff = obs["dumps"]["early"], obs["dumps"]["final"], obs["shots"]["early"], obs["shots"]["final"]
    out = []
    sd = start_done_t(e)
    st = obs["shot_t"]["early"]
    out.append(("the grader's first look was taken in the first 8 s of the fade", sd is not None and st - sd < 8000, "< 8 s after startEngine", "%s ms" % (st - sd if sd is not None else "?")))
    b = one_mesh(f, "beacon", out)
    cam = (f.get("camera") or {})
    if b:
        out.append(("beacon: sphere of diameter 2 at (0, 1, 0), standard material", near(b.get("pos"), [0, 1, 0]) and near(extent(b), [2, 2, 2]) and b.get("matKind") == "standardGroupBuilder", "(0, 1, 0), 2, standard", "%s %s %s" % (fmt(b.get("pos")), fmt(extent(b)), b.get("matKind"))))
        dc = std_diffuse(b)
        out.append(("after the fade the material's diffuse colour is blue (0.1, 0.2, 0.9)", dc is not None and near(dc[:3], [0.1, 0.2, 0.9], 0.03), "(0.1, 0.2, 0.9)", fmt(dc[:3] if dc else None)))
    be = (meshes_named(e, "beacon") or [None])[0]
    dc0 = std_diffuse(be) if be else None
    out.append(("early in the fade the material's diffuse colour is still mostly red (r > b)", bool(dc0) and dc0[0] > dc0[2], "r > b", fmt(dc0[:3] if dc0 else None)))
    p0, p1 = project({"pos": cam.get("pos") or [0, 3, -10], "target": cam.get("target") or [0, 1, 0], "fov": cam.get("fov") or 0.8}, [0, 1, 0], 800, 600)[:2], None
    m0, m1 = mean_px(ie, p0[0], p0[1]), mean_px(iff, p0[0], p0[1])
    out.append(("early in the fade the sphere is drawn reddish (r > b + 20)", m0[0] > m0[2] + 20, "r > b + 20", fmt(m0)))
    out.append(("after the fade the sphere is drawn blue (b > r + 40)", m1[2] > m1[0] + 40, "b > r + 40", fmt(m1)))
    check_free_cam(f, [0, 3, -10], [0, 1, 0], out)
    check_hemi(f, out)
    return out


def cons_t4(obs):
    e, f = obs["dumps"]["early"], obs["dumps"]["final"]
    be, bf = (meshes_named(e, "beacon") or [{}])[0], (meshes_named(f, "beacon") or [{}])[0]
    out = lifecycle_checks(f)
    out.append(("the sphere kept one material object the whole time (edited in place, not replaced)", be.get("matId") is not None and be.get("matId") == bf.get("matId"), "same material before and after", "%s -> %s" % (be.get("matId"), bf.get("matId"))))
    dirty = [x for x in evs(f, "matDirty") if x.get("mat") == bf.get("matId")]
    out.append(("the edits were published with markMaterialUboDirty (>= 20 calls on that material over the fade)", len(dirty) >= 20, ">= 20 markMaterialUboDirty calls", "%d" % len(dirty)))
    return out


task(id="t4-color-fade", a_needs_trace=True, split="tutorial", kinds=["K3", "K4", "K2"], facts=["material-dirty"], scenario=sc_t4, feature=feat_t4, constraint=cons_t4,
     prompt=lambda sealed=False: make_prompt(
         "In ./t4-color-fade, edit main.ts to build this scene with Babylon Lite and run it: a free camera at (0, 3, -10) looking at (0, 1, 0), as the scene's active camera; a hemispheric light pointing up; "
         "a sphere named `beacon`, diameter 2, at (0, 1, 0), with a standard material whose diffuse colour starts red (0.9, 0.1, 0.1). From the moment `startEngine` resolves, `beacon`'s diffuse colour fades linearly "
         "from red (0.9, 0.1, 0.1) to blue (0.1, 0.2, 0.9) over 20 seconds and then stays blue, updated on every frame from a per-frame callback. The sphere keeps its one material: that material object is edited "
         "in place on each frame; it is not replaced by a new one.", "t4-color-fade", sealed)())

# ---- T5: a sun that casts a shadow (facts shadow-attach, shadow-register, shadow-receive, shadow-light-pos)
SUN5 = [-1, -2, -1]


def shadow_points_t5(cam):
    c = arc_to_cam(cam)
    pr = lambda p: project(c, p, 800, 600)[:2]
    return c, pr


def feat_t5(obs):
    d, img = obs["dumps"]["final"], obs["shots"]["final"]
    out = []
    cam = d.get("camera") or {}
    out.append(("the active camera is an orbit (arc-rotate) camera", cam.get("kind") == "arc", "arc-rotate", cam.get("kind", "no camera")))
    out.append(("camera alpha -pi/2, beta 1.0, radius 14, targeting (0, 0.5, 0)", cam.get("kind") == "arc" and near([cam.get("alpha"), cam.get("beta"), cam.get("radius")], [-math.pi / 2, 1.0, 14.0]) and near(cam.get("target"), [0, 0.5, 0]),
                "(-1.571, 1, 14) (0, 0.5, 0)", fmt([cam.get("alpha"), cam.get("beta"), cam.get("radius")]) + " " + fmt(cam.get("target"))))
    g = one_mesh(d, "ground", out)
    if g:
        out.append(("ground: 12 x 12 at the origin, standard material, grey (0.8, 0.8, 0.8)", near(g.get("pos"), [0, 0, 0]) and near([extent(g)[0], extent(g)[2]], [12, 12]) and g.get("matKind") == "standardGroupBuilder" and near((std_diffuse(g) or [0])[:3], [0.8, 0.8, 0.8]),
                    "origin, 12 x 12, standard, (0.8, 0.8, 0.8)", "%s %s %s" % (fmt(g.get("pos")), g.get("matKind"), fmt(std_diffuse(g)))))
    check_box(d, "block", [0, 1, 0], 2, [0.9, 0.2, 0.2], out)
    suns = [l for l in d.get("lights", []) if l.get("type") == "directional"]
    ok = False
    if suns and suns[0].get("dir"):
        v = suns[0]["dir"]
        n = math.sqrt(sum(c * c for c in v)) or 1
        t = math.sqrt(sum(c * c for c in SUN5))
        ok = sum(v[i] / n * SUN5[i] / t for i in range(3)) > 0.999
    out.append(("a directional light shining along (-1, -2, -1)", ok, "directional (-1, -2, -1)", fmt([(l.get("type"), l.get("dir")) for l in d.get("lights", [])])))
    check_hemi(d, out)
    if cam.get("kind") == "arc":
        c, pr = shadow_points_t5(cam)
        shadow, lit1, lit2 = pr([-1.5, 0, -1.5]), pr([2.5, 0, -1.5]), pr([3.5, 0, -3.0])
        ls, l1, l2 = region_luma(img, *shadow), region_luma(img, *lit1), region_luma(img, *lit2)
        out.append(("the lit ground to the right of the block is bright (luma > 110)", l1 > 110 and l2 > 110, "> 110", "%.0f, %.0f" % (l1, l2)))
        out.append(("the block's shadow falls on the ground at (-1.5, 0, -1.5): darker than the lit ground (< 85%)", ls < 0.85 * min(l1, l2), "< 0.85 x lit ground", "shadow %.0f vs lit %.0f" % (ls, min(l1, l2))))
        px_presence(img, "red", 0, out, "the red block is drawn", minimum=800)
    return out


def cons_t5(obs):
    d = obs["dumps"]["final"]
    out = lifecycle_checks(d)
    sun = [l for l in d.get("lights", []) if l.get("type") == "directional"]
    out.append(("the sun has a shadow generator attached (light.shadowGenerator)", bool(sun) and bool(sun[0].get("hasShadow")), "shadow generator on the directional light", fmt([l.get("hasShadow") for l in d.get("lights", [])])))
    out.append(("the scene was registered with shadow support (a shadow task is in the frame graph)", "shadow" in (d.get("tasks") or []) and any(e.get("shadow") for e in evs(d, "registerScene")), "registerSceneWithShadowSupport, task 'shadow'", fmt(d.get("tasks"))))
    return out


task(id="t5-sun-shadow", split="tutorial", kinds=["K1", "K3", "K4", "K5", "K6"], facts=["shadow-attach", "shadow-register", "shadow-receive", "shadow-light-pos"], scenario=lambda s: (s.load(), s.wait(1.5), s.snap("final")), feature=feat_t5, constraint=cons_t5,
     prompt=lambda sealed=False: make_prompt(
         "In ./t5-sun-shadow, edit main.ts to build this scene with Babylon Lite and run it: an orbit (arc-rotate) camera with alpha -pi/2, beta 1.0, radius 14, targeting (0, 0.5, 0), as the scene's active camera; "
         "a hemispheric light pointing up with intensity 0.4; a directional light (the sun) shining along (-1, -2, -1) with intensity 1; a flat ground named `ground`, 12 x 12, at the origin, with a standard material whose "
         "diffuse colour is grey (0.8, 0.8, 0.8); a box named `block`, 2 units along each edge, at (0, 1, 0), with a standard material whose diffuse colour is red (0.9, 0.2, 0.2). The sun casts a real-time shadow "
         "(shadow mapping with a PCF directional shadow generator, not a painted shadow): the `block`'s shadow falls on the `ground`, on the side the light points to.", "t5-sun-shadow", sealed)())


# ---- T6: GPU picking at CSS pixels, on a device-pixel-ratio-2 screen with the drawing buffer capped (facts pick-fn-css, dpr-cap)
CAM6 = {"pos": [0, 3, -10], "target": [0, 1, 0], "fov": 0.8}


def sc_t6(s):
    s.load(viewport="800x600x2")
    s.wait(1.0)
    s.snap_dump("pre")
    for lab, (px, py) in (("c1", project(CAM6, [2, 1, 0], 800, 600)[:2]), ("c2", project(CAM6, [-2, 1, 0], 800, 600)[:2]), ("c3", (400, 60))):
        s.click(px, py, lab)
        s.wait(1.4)
        s.eval("pick-" + lab, "() => window.pickResult === undefined ? null : JSON.parse(JSON.stringify(window.pickResult))")
    s.snap_dump("final")


def feat_t6(obs):
    pre, E, C = obs["dumps"]["pre"], obs["evals"], obs["clicks"]
    out = []
    cv = pre.get("cv") or {}
    out.append(("the screen is at device-pixel ratio 2 (the grader's emulation took)", cv.get("dpr") == 2, "2", str(cv.get("dpr"))))
    out.append(("the canvas drawing buffer is one pixel per CSS pixel (never larger), at device-pixel ratio 2", cv.get("w") is not None and cv["w"] <= cv["cw"] and cv["h"] <= cv["ch"] and cv["w"] >= 0.9 * cv["cw"],
                "%sx%s" % (cv.get("cw"), cv.get("ch")), "%sx%s" % (cv.get("w"), cv.get("h"))))
    check_box_like = [("left", [-2, 1, 0], [0.9, 0.1, 0.1]), ("right", [2, 1, 0], [0.1, 0.9, 0.1])]
    for name, pos, rgb in check_box_like:
        m = one_mesh(pre, name, out)
        if m:
            out.append(("%s: sphere of diameter 2 at %s, standard material, diffuse %s" % (name, fmt(pos), fmt(rgb)), near(m.get("pos"), pos) and near(extent(m), [2, 2, 2]) and m.get("matKind") == "standardGroupBuilder" and near((std_diffuse(m) or [0])[:3], rgb),
                        "standard, %s" % fmt(rgb), "%s %s %s" % (fmt(m.get("pos")), m.get("matKind"), fmt(std_diffuse(m)))))
    for lab, name, cen in (("c1", "right", [2, 1, 0]), ("c2", "left", [-2, 1, 0]), ("c3", None, None)):
        pr = E.get("pick-" + lab)
        pr = pr if isinstance(pr, dict) and "_eval_error" not in pr else {}
        px, py = (C[lab].get("x"), C[lab].get("y"))
        out.append(("click %s: window.pickResult.name is %s" % (lab, name or "null (nothing under the click)"), pr.get("name") == name, str(name), fmt(pr.get("name")) if pr else "window.pickResult not set"))
        css = pr.get("css")
        out.append(("click %s: pickResult.css is the click's CSS position" % lab, isinstance(css, list) and len(css) == 2 and abs(css[0] - px) <= 2 and abs(css[1] - py) <= 2, "(%.0f, %.0f)" % (px, py), fmt(css)))
        if cen:
            o, d = pixel_ray(CAM6, px, py, 800, 600)
            hit = ray_sphere(o, d, cen, 1.0)
            pt = pr.get("point")
            out.append(("click %s: pickResult.point is the front of the sphere under the click (within 0.15)" % lab, hit is not None and isinstance(pt, list) and near(pt, hit, 0.15), fmt(hit), fmt(pt)))
        else:
            out.append(("click %s: pickResult.point is null" % lab, pr.get("point") is None and bool(pr), "null", fmt(pr.get("point")) if pr else "window.pickResult not set"))
    return out


def cons_t6(obs):
    d, C = obs["dumps"]["final"], obs["clicks"]
    out = lifecycle_checks(d)
    eng = evs(d, "createEngine")
    mx = (eng[0].get("vals") or {}).get("maxDevicePixelRatio") if eng else None
    out.append(("the engine was created with maxDevicePixelRatio <= 1", mx is not None and mx <= 1, "createEngine(canvas, { maxDevicePixelRatio: 1 })", fmt(mx)))
    picks = evs(d, "pick")
    for lab in ("c1", "c2"):
        c = C[lab]
        ok = any(abs(p["x"] - c["x"]) <= 2 and abs(p["y"] - c["y"]) <= 2 for p in picks)
        out.append(("pickAsync was called at the click's CSS pixel position (%s)" % lab, ok, "pickAsync(picker, %.0f, %.0f)" % (c["x"], c["y"]), fmt([(p["x"], p["y"]) for p in picks])))
    out.append(("the answer came from Lite's GPU picker (pick calls that resolved)", len(evs(d, "pick.done")) >= 2, ">= 2 resolved picks", str(len(evs(d, "pick.done")))))
    return out


task(id="t6-pick-dpr", split="tutorial", kinds=["K1", "K2", "K5", "K6"], facts=["pick-fn-css", "dpr-cap"], viewport="800x600x2", scenario=sc_t6, feature=feat_t6, constraint=cons_t6,
     prompt=lambda sealed=False: make_prompt(
         "In ./t6-pick-dpr, edit main.ts to build this scene with Babylon Lite and run it: a free camera at (0, 3, -10) looking at (0, 1, 0), as the scene's active camera; a hemispheric light pointing up; "
         "a sphere named `left`, diameter 2, at (-2, 1, 0), with a standard material whose diffuse colour is red (0.9, 0.1, 0.1); a sphere named `right`, diameter 2, at (2, 1, 0), with a standard material whose "
         "diffuse colour is green (0.1, 0.9, 0.1). The canvas fills the window through CSS. This page is shown on a high-DPI screen (device-pixel ratio 2): to keep rendering cheap, the canvas drawing buffer must never be larger "
         "than one pixel per CSS pixel. On every click on the canvas, use Lite's GPU picking to find the mesh under the click and set `window.pickResult = { name, point, css }`, where `name` is the picked mesh's name "
         "(or null when nothing is under the click), `point` is the picked point in world space as [x, y, z] (or null), and `css` is the click position in CSS pixels, relative to the canvas, as [x, y].", "t6-pick-dpr", sealed, dpr="2")())


# ---- T7: a hand-built mesh must be wound the way Lite's culling expects (fact winding)
CAM7 = {"pos": [0, 3, -8], "target": [0, 1, 0], "fov": 0.8}


def warm_count(img, x0=0, y0=0, x1=None, y1=None):
    x1, y1 = x1 or img.w, y1 or img.h
    return sum(1 for y in range(y0, y1, 2) for x in range(x0, x1, 2) if (lambda p: p[0] > 100 and p[0] - p[2] > 60)(img.px(x, y))) * 4


def feat_t7(obs):
    d, img = obs["dumps"]["final"], obs["shots"]["final"]
    out = []
    m = one_mesh(d, "spire", out)
    if m:
        sc, ps = m.get("scale") or [1, 1, 1], m.get("pos") or [0, 0, 0]
        wmin = [ps[i] + (m.get("bmin") or [9, 9, 9])[i] * sc[i] for i in range(3)]
        wmax = [ps[i] + (m.get("bmax") or [9, 9, 9])[i] * sc[i] for i in range(3)]
        out.append(("spire: occupies (-1, 0, -1) .. (1, 2, 1) in world space (base 2 x 2 on y = 0, apex at height 2)", near(wmin, [-1, 0, -1], 0.05) and near(wmax, [1, 2, 1], 0.05), "(-1, 0, -1) .. (1, 2, 1)", "%s .. %s" % (fmt(wmin), fmt(wmax))))
        dc = std_diffuse(m)
        out.append(("spire: standard material, diffuse orange (0.9, 0.5, 0.1)", m.get("matKind") == "standardGroupBuilder" and dc is not None and near(dc[:3], [0.9, 0.5, 0.1]), "standard (0.9, 0.5, 0.1)", "%s %s" % (m.get("matKind"), fmt(dc[:3] if dc else None))))
    check_free_cam(d, [0, 3, -8], [0, 1, 0], out)
    out.append(("a directional light shining along (0, -0.3, 1)", any(l.get("type") == "directional" and l.get("dir") and abs(l["dir"][0]) < 1e-3 and l["dir"][2] > 0 and abs(l["dir"][1] / l["dir"][2] + 0.3) < 0.01 for l in d.get("lights", [])),
                "directional (0, -0.3, 1)", fmt([(l.get("type"), l.get("dir")) for l in d.get("lights", [])])))
    n = warm_count(img)
    out.append(("an orange solid is drawn (>= 4000 warm px)", n >= 4000, ">= 4000", str(n)))
    sx, sy = project(CAM7, [0, 2 / 3, -2 / 3], 800, 600)[:2]
    mp = [sum(q[k] for q in img.region(sx - 6, sy - 6, sx + 7, sy + 7)) / 169.0 for k in range(3)]
    out.append(("the front face (towards the camera, lit by the light) is drawn lit and orange: luma > 90, r > b + 60", 0.299 * mp[0] + 0.587 * mp[1] + 0.114 * mp[2] > 90 and mp[0] > mp[2] + 60, "bright orange at (%.0f, %.0f)" % (sx, sy), fmt(mp)))
    return out


def cons_t7(obs):
    d = obs["dumps"]["final"]
    out = lifecycle_checks(d)
    mf = [e for e in evs(d, "meshFromData") if not e.get("internal")]
    out.append(("the spire was hand-built with a direct createMeshFromData call (>= 5 vertices, >= 18 indices; the engine's own factories do not count)", any(e["nIdx"] >= 18 and e["nVert"] >= 5 for e in mf), "createMeshFromData(engine, ..., positions, normals, indices)", fmt([(e["nVert"], e["nIdx"]) for e in mf]) if mf else "not called directly"))
    return out


task(id="t7-custom-mesh", split="tutorial", kinds=["K5"], facts=["winding"], scenario=lambda s: (s.load(), s.wait(1.2), s.snap("final")), feature=feat_t7, constraint=cons_t7,
     prompt=lambda sealed=False: make_prompt(
         "In ./t7-custom-mesh, edit main.ts to build this scene with Babylon Lite and run it: a free camera at (0, 3, -8) looking at (0, 1, 0), as the scene's active camera; a directional light shining along (0, -0.3, 1) "
         "(no other light); a mesh named `spire` at the origin: a closed square pyramid whose base is 2 x 2 on the plane y = 0 (corners (+-1, 0, +-1)) and whose apex is at (0, 2, 0), with a standard material whose diffuse "
         "colour is orange (0.9, 0.5, 0.1). Build the `spire`'s triangles by hand from raw vertex data (positions, normals and indices; not one of the engine's ready-made shapes), with every face drawn from the outside "
         "(each of the five faces is visible from outside the pyramid, lit by its outward normal).", "t7-custom-mesh", sealed)())

# ---- T8: one mesh drawn many times with GPU instancing and per-instance colours (fact thin-recipe)
ORDER8 = ["red", "green", "blue", "yellow", "magenta", "white"]
RGB8 = [[1, 0, 0], [0, 1, 0], [0, 0, 1], [1, 1, 0], [1, 0, 1], [1, 1, 1]]


def feat_t8(obs):
    d, img = obs["dumps"]["final"], obs["shots"]["final"]
    out = []
    out.append(("exactly one mesh in the scene", len(d.get("meshes", [])) == 1, "1", str(len(d.get("meshes", [])))))
    post = one_mesh(d, "post", out)
    ti = (post or {}).get("ti")
    out.append(("`post` is drawn 6 times (instanced)", bool(ti) and ti.get("count") == 6, "6", fmt(ti.get("count") if ti else None)))
    if ti and ti.get("count") == 6:
        mt = ti["matrices"]
        pos = [mt[16 * i + 12:16 * i + 15] for i in range(6)] if len(mt) == 96 else None
        out.append(("instances at x = -5, -3, -1, 1, 3, 5 (y = 0.5, z = 0)", pos is not None and all(near(p, [-5 + 2 * i, 0.5, 0]) for i, p in enumerate(pos)), "(-5,.5,0) (-3,.5,0) ... (5,.5,0)", " ".join(fmt(p) for p in pos) if pos else "bad matrices"))
        out.append(("instances are unit boxes (no rotation or scale)", len(mt) == 96 and all(near([mt[16 * i], mt[16 * i + 5], mt[16 * i + 10]], [1, 1, 1]) for i in range(6)), "unit scale", "see matrices"))
        c = ti.get("colors")
        out.append(("per-instance colours: red, green, blue, yellow, magenta, white", bool(c) and len(c) == 24 and all(near(c[4 * i:4 * i + 3], RGB8[i]) for i in range(6)), "red green blue yellow magenta white", "none" if not c else ("%d numbers: " % len(c)) + fmt(c[:8])))
    else:
        out += [("instances at x = -5 .. 5", False, "", "no instances"), ("per-instance colours", False, "", "no instances")]
    cents = []
    for name in ORDER8:
        cc = class_centroid(img, name)
        cents.append(cc[0] if cc and cc[2] >= 300 else None)
    out.append(("drawn left to right as red, green, blue, yellow, magenta, white (each colour present, centroids ascending)", all(c is not None for c in cents) and all(cents[i] < cents[i + 1] for i in range(5)), "ascending x", fmt(cents)))
    check_free_cam(d, [0, 3, -12], [0, 0.5, 0], out)
    check_hemi(d, out)
    return out


def cons_t8(obs):
    d = obs["dumps"]["final"]
    out = lifecycle_checks(d)
    ts = mesh_evs(d, "thin.set", "post")
    ta = mesh_evs(d, "thin.add", "post")
    tc = mesh_evs(d, "thin.colors", "post")
    out.append(("the instances were made with the thin-instance API (setThinInstances with 6 matrices, or six addThinInstance calls)", (len(ts) >= 1 and ts[-1]["count"] == 6 and ts[-1]["len"] >= 96) or len(ta) == 6,
                "setThinInstances(post, matrices(96), 6) or 6 x addThinInstance", "%s set, %s add" % (fmt([(e["count"], e["len"]) for e in ts]), len(ta))))
    out.append(("the colours were set with setThinInstanceColors AFTER the instances existed (6 x RGBA = 24 numbers)", len(tc) >= 1 and tc[-1]["hadInstances"] and tc[-1]["len"] == 24, "setThinInstanceColors(post, colors(24)) after setThinInstances", fmt([(e["hadInstances"], e["len"]) for e in tc]) if tc else "not called"))
    return out


task(id="t8-thin-posts", split="tutorial", kinds=["K3", "K5"], facts=["thin-recipe"], scenario=lambda s: (s.load(), s.wait(1.2), s.snap("final")), feature=feat_t8, constraint=cons_t8,
     prompt=lambda sealed=False: make_prompt(
         "In ./t8-thin-posts, edit main.ts to build this scene with Babylon Lite and run it: a free camera at (0, 3, -12) looking at (0, 0.5, 0), as the scene's active camera; a hemispheric light pointing up; "
         "a single box mesh named `post`, 1 unit along each edge, with a standard material whose diffuse colour is white (1, 1, 1), drawn 6 times with GPU instancing (one mesh, not six) at x = -5, -3, -1, 1, 3 and 5 "
         "(y = 0.5, z = 0), with no rotation or scale. The six instances are coloured, left to right: red (1, 0, 0), green (0, 1, 0), blue (0, 0, 1), yellow (1, 1, 0), magenta (1, 0, 1) and white (1, 1, 1).", "t8-thin-posts", sealed)())
