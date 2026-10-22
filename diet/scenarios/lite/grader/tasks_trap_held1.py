# ------------------------------------------------------------------ held-out tasks (the model meets the facts again with other meshes, names, values, scenes)
# ---- H1: late PBR torus on a click (late-add, late-family); the tutorial used a timer, a cylinder, a floor and other colours
def sc_h1(s):
    s.load()
    s.wait(1.0)
    s.snap("pre")
    s.click(400, 300, "c1")
    s.wait(1.8)
    s.snap("final")


def feat_h1(obs):
    pre, post, ip, iq = obs["dumps"]["pre"], obs["dumps"]["final"], obs["shots"]["pre"], obs["shots"]["final"]
    out = []
    fl = one_mesh(post, "floor", out)
    if fl:
        out.append(("floor: 14 x 14 at the origin, standard material, grey (0.5, 0.5, 0.5)", near(fl.get("pos"), [0, 0, 0]) and near([extent(fl)[0], extent(fl)[2]], [14, 14]) and fl.get("matKind") == "standardGroupBuilder" and near((std_diffuse(fl) or [0])[:3], [0.5, 0.5, 0.5]),
                    "origin, 14 x 14, standard, (0.5, 0.5, 0.5)", "%s %s %s" % (fmt(fl.get("pos")), fl.get("matKind"), fmt(std_diffuse(fl)))))
    pd = one_mesh(post, "pedestal", out)
    if pd:
        out.append(("pedestal: cylinder 2 high, 2 across, at (-3, 1, 0), standard material, yellow (0.9, 0.8, 0.1)", near(pd.get("pos"), [-3, 1, 0]) and near(extent(pd), [2, 2, 2], 0.02) and pd.get("matKind") == "standardGroupBuilder" and near((std_diffuse(pd) or [0])[:3], [0.9, 0.8, 0.1]),
                    "(-3, 1, 0), (2, 2, 2), standard, (0.9, 0.8, 0.1)", "%s %s %s" % (fmt(pd.get("pos")), fmt(extent(pd)), fmt(std_diffuse(pd)))))
    r = one_mesh(post, "ring", out)
    if r:
        mat = r.get("material") or {}
        out.append(("ring: torus of diameter 3 and thickness 0.8 at (3, 1.5, 0)", near(r.get("pos"), [3, 1.5, 0]) and near([extent(r)[0], extent(r)[2]], [3.8, 3.8], 0.15), "(3, 1.5, 0), 3.8 across", "%s %s" % (fmt(r.get("pos")), fmt(extent(r)))))
        out.append(("ring: PBR material, base colour magenta (0.9, 0.1, 0.8), metallic 0.2, roughness 0.45",
                    r.get("matKind") == "pbrGroupBuilder" and near((mat.get("baseColorFactor") or [0])[:3], [0.9, 0.1, 0.8]) and near([mat.get("metallicFactor")], [0.2]) and near([mat.get("roughnessFactor")], [0.45]),
                    "pbr (0.9, 0.1, 0.8) 0.2 0.45", "%s %s %s %s" % (r.get("matKind"), fmt(mat.get("baseColorFactor")), fmt(mat.get("metallicFactor")), fmt(mat.get("roughnessFactor")))))
    px_presence(ip, "yellow", 0, out, "before the click: the yellow pedestal is drawn on the left", half="left", minimum=800)
    px_presence(ip, "magenta", 100, out, "before the click: nothing magenta is drawn", minimum=None)
    px_presence(iq, "magenta", 0, out, "after the click: the magenta ring is drawn on the right", half="right", minimum=600)
    px_presence(iq, "yellow", 0, out, "after the click: the pedestal is still drawn", half="left", minimum=800)
    check_free_cam(post, [0, 4, -12], [0, 1, 0], out)
    check_hemi(post, out)
    return out


def cons_h1(obs):
    d = obs["dumps"]["final"]
    return lifecycle_checks(d) + late_add_checks(d, "ring", not_before_t=obs["clicks"]["c1"]["t"] - 50)


task(id="h1-late-torus", split="held-out", kinds=["K1", "K4"], facts=["late-add", "late-family"], scenario=sc_h1, feature=feat_h1, constraint=cons_h1,
     prompt=lambda sealed=False: make_prompt(
         "In ./h1-late-torus, edit main.ts to build this scene with Babylon Lite and run it: a free camera at (0, 4, -12) looking at (0, 1, 0), as the scene's active camera; a hemispheric light pointing up; "
         "a flat ground named `floor`, 14 x 14, at the origin, with a standard (Blinn-Phong) material whose diffuse colour is grey (0.5, 0.5, 0.5); a cylinder named `pedestal`, 2 high and 2 across, at (-3, 1, 0), "
         "with a standard material whose diffuse colour is yellow (0.9, 0.8, 0.1). When the canvas is clicked, a torus named `ring`, diameter 3 and thickness 0.8, at (3, 1.5, 0), with a PBR material "
         "(base colour magenta (0.9, 0.1, 0.8), metallic 0.2, roughness 0.45), must appear in the running scene. The `ring` is added to the scene at click time (it does not exist in the scene before the click) "
         "while the engine keeps running: the engine and the scene are not torn down and rebuilt. The floor and the pedestal keep their standard materials.", "h1-late-torus", sealed)())

# ---- H2: replace one mesh by another at runtime (late-add, late-remove)
def sc_h2(s):
    s.load()
    s.wait(1.0)
    s.snap("pre")
    s.click(400, 300, "c1")
    s.wait(1.8)
    s.snap("final")


def feat_h2(obs):
    pre, post, ip, iq = obs["dumps"]["pre"], obs["dumps"]["final"], obs["shots"]["pre"], obs["shots"]["final"]
    out = []
    o = one_mesh(pre, "orb", out)
    if o:
        out.append(("orb: sphere of diameter 2 at (-2, 1, 0), standard material, red (0.9, 0.1, 0.1)", near(o.get("pos"), [-2, 1, 0]) and near(extent(o), [2, 2, 2]) and o.get("matKind") == "standardGroupBuilder" and near((std_diffuse(o) or [0])[:3], [0.9, 0.1, 0.1]),
                    "(-2, 1, 0), 2, standard, (0.9, 0.1, 0.1)", "%s %s" % (fmt(o.get("pos")), fmt(std_diffuse(o)))))
    check_box(pre, "rock", [3, 1, 0], 2, [0.2, 0.4, 0.9], out)
    px_presence(ip, "red", 0, out, "before the click: the red orb is drawn on the left", half="left")
    px_presence(ip, "blue", 0, out, "before the click: the blue rock is drawn on the right", half="right")
    px_presence(ip, "green", 100, out, "before the click: nothing green is drawn", minimum=None)
    one_mesh(post, "orb", out, present=False)
    check_box(post, "cube", [-2, 1, 0], 2, [0.1, 0.9, 0.1], out)
    one_mesh(post, "rock", out)
    px_presence(iq, "red", 100, out, "after the click: the red orb is no longer drawn", minimum=None)
    px_presence(iq, "green", 0, out, "after the click: a green cube is drawn on the left", half="left")
    px_presence(iq, "blue", 0, out, "after the click: the blue rock is still drawn on the right", half="right")
    check_free_cam(post, [0, 3, -10], [0, 1, 0], out)
    check_hemi(post, out)
    return out


def cons_h2(obs):
    d, c = obs["dumps"]["final"], obs["clicks"]["c1"]
    out = lifecycle_checks(d)
    rm = mesh_evs(d, "removeFromScene", "orb")
    out.append(("the orb was taken out through removeFromScene, at or after the click", len(rm) == 1 and rm[0]["t"] >= c["t"] - 50, "one removeFromScene(orb)", "%d call(s)" % len(rm)))
    return out + late_add_checks(d, "cube", not_before_t=c["t"] - 50)


task(id="h2-swap-orb", split="held-out", kinds=["K1"], facts=["late-add", "late-remove"], scenario=sc_h2, feature=feat_h2, constraint=cons_h2,
     prompt=lambda sealed=False: make_prompt(
         "In ./h2-swap-orb, edit main.ts to build this scene with Babylon Lite and run it: a free camera at (0, 3, -10) looking at (0, 1, 0), as the scene's active camera; a hemispheric light pointing up; "
         "a sphere named `orb`, diameter 2, at (-2, 1, 0), with a standard material whose diffuse colour is red (0.9, 0.1, 0.1); a box named `rock`, 2 units along each edge, at (3, 1, 0), with a standard material "
         "whose diffuse colour is blue (0.2, 0.4, 0.9). When the canvas is clicked, the `orb` is removed from the scene (it is gone from the scene's mesh list and no longer drawn) and, in the same click, a box named `cube`, "
         "2 units along each edge, at (-2, 1, 0), with a standard material whose diffuse colour is green (0.1, 0.9, 0.1), appears in its place. The `cube` is added to the scene at click time (it does not exist in the "
         "scene before the click) while the engine keeps running: the engine and the scene are not torn down and rebuilt.", "h2-swap-orb", sealed)())

# ---- H3: click to drop markers where the pointer is (late-add, pick-fn-css)
CAM3 = {"pos": [0, 7, -9], "target": [0, 0, 0], "fov": 0.8}
CLICKS3 = [("c1", (300, 420)), ("c2", (540, 330))]


def sc_h3(s):
    s.load()
    s.wait(1.0)
    s.snap_dump("pre")
    for lab, (x, y) in CLICKS3:
        s.click(x, y, lab)
        s.wait(1.6)
        s.snap(lab)
    s.snap_dump("final")


def feat_h3(obs):
    pre, post, D, S = obs["dumps"]["pre"], obs["dumps"]["final"], obs["dumps"], obs["shots"]
    out = []
    f = one_mesh(pre, "field", out)
    if f:
        out.append(("field: 12 x 12 at the origin, standard material, grey (0.5, 0.5, 0.5)", near(f.get("pos"), [0, 0, 0]) and near([extent(f)[0], extent(f)[2]], [12, 12]) and f.get("matKind") == "standardGroupBuilder" and near((std_diffuse(f) or [0])[:3], [0.5, 0.5, 0.5]),
                    "origin, 12 x 12, standard, (0.5, 0.5, 0.5)", "%s %s %s" % (fmt(f.get("pos")), f.get("matKind"), fmt(std_diffuse(f)))))
    for i, (lab, (x, y)) in enumerate(CLICKS3):
        want = unproject_to_plane_y(CAM3, x, y, 800, 600, 0.0)
        name = "marker-%d" % (i + 1)
        ms = meshes_named(post, name)
        out.append(("click %d added `%s` (once)" % (i + 1, name), len(ms) == 1, "1", str(len(ms))))
        if len(ms) == 1:
            m = ms[0]
            out.append(("`%s` is a 1-unit box standing on the field at the ground point under click %d (x, z within 0.3)" % (name, i + 1), near(extent(m), [1, 1, 1]) and near(m.get("pos"), [want[0], 0.5, want[2]], 0.3),
                        fmt([want[0], 0.5, want[2]]), "%s %s" % (fmt(m.get("pos")), fmt(extent(m)))))
            out.append(("`%s`: standard material, yellow (0.9, 0.8, 0.1)" % name, m.get("matKind") == "standardGroupBuilder" and near((std_diffuse(m) or [0])[:3], [0.9, 0.8, 0.1]), "standard (0.9, 0.8, 0.1)", "%s %s" % (m.get("matKind"), fmt(std_diffuse(m)))))
        n = class_count(S[lab], "yellow")
        out.append(("after click %d: yellow is drawn (>= %d px)" % (i + 1, 300 * (i + 1)), n >= 300 * (i + 1), ">= %d" % (300 * (i + 1)), str(n)))
    cams = post.get("camera") or {}
    out.append(("the active camera is a free camera at (0, 7, -9) looking at the origin", cams.get("kind") == "free" and near(cams.get("pos"), [0, 7, -9]) and near(cams.get("target"), [0, 0, 0]), "free (0,7,-9) -> (0,0,0)", fmt([cams.get("kind"), cams.get("pos"), cams.get("target")])))
    check_hemi(post, out)
    return out


def cons_h3(obs):
    d, C = obs["dumps"]["final"], obs["clicks"]
    out = lifecycle_checks(d)
    picks = evs(d, "pick")
    for lab, (x, y) in CLICKS3:
        out.append(("pickAsync was called at click %s's CSS position" % lab, any(abs(p["x"] - x) <= 2 and abs(p["y"] - y) <= 2 for p in picks), "pickAsync(picker, %d, %d)" % (x, y), fmt([(p["x"], p["y"]) for p in picks])))
    out.append(("the picked points came from Lite's GPU picker (2 resolved picks, both hits on the field)", len([p for p in evs(d, "pick.done") if p.get("hit") and p.get("name") == "field"]) >= 2, ">= 2 hits on `field`", fmt([(p.get("name"), p.get("hit")) for p in evs(d, "pick.done")])))
    for i, (lab, _) in enumerate(CLICKS3):
        out += late_add_checks(d, "marker-%d" % (i + 1), not_before_t=C[lab]["t"] - 50)
    return out


task(id="h3-click-drop", split="held-out", kinds=["K1", "K2", "K5"], facts=["late-add", "pick-fn-css"], scenario=sc_h3, feature=feat_h3, constraint=cons_h3,
     prompt=lambda sealed=False: make_prompt(
         "In ./h3-click-drop, edit main.ts to build this scene with Babylon Lite and run it: a free camera at (0, 7, -9) looking at (0, 0, 0), as the scene's active camera; a hemispheric light pointing up; "
         "a flat ground named `field`, 12 x 12, at the origin, with a standard material whose diffuse colour is grey (0.5, 0.5, 0.5). The scene starts with only the field. On every click on the canvas, use Lite's GPU "
         "picking to find the point on the `field` under the click, and add a box named `marker-N` (N = 1 for the first click, 2 for the second, ...), 1 unit along each edge, with a standard material whose diffuse "
         "colour is yellow (0.9, 0.8, 0.1), standing on the field with its centre at (picked x, 0.5, picked z). The markers are added to the running scene at click time while the engine keeps running: the engine and "
         "the scene are not torn down and rebuilt.", "h3-click-drop", sealed)())


# ---- H4: a click hides one mesh and re-tints another (late-hide, material-dirty); the tutorials used a removal and a timed fade
def sc_h4(s):
    s.load()
    s.wait(1.0)
    s.snap("pre")
    s.click(400, 300, "c1")
    s.wait(1.5)
    s.snap("s1")
    s.click(400, 300, "c2")
    s.wait(1.5)
    s.snap("final")


def swap_checks(d, name, not_before_t):
    """`name` was in the scene from the first build and its material was reassigned after the build: the setter queued a swap and the per-frame drain gave it a renderable;
    it was neither removed nor added again, and nothing was rebuilt."""
    adds = mesh_evs(d, "add", name, kind="mesh")
    sw = [e for e in mesh_evs(d, "enqueueSwap", name) if e["t"] >= not_before_t]
    out = [("`%s` was added once, before registration, and never added again" % name, len(adds) == 1 and not adds[0]["built"], "one add at build time", "%d add(s): %s" % (len(adds), [a["built"] for a in adds])),
           ("`%s` was never removed" % name, not mesh_evs(d, "removeFromScene", name), "no removeFromScene", "%d" % len(mesh_evs(d, "removeFromScene", name))),
           ("`%s`'s material was reassigned at the trigger (the mesh.material setter queued a swap)" % name, len(sw) >= 1, ">= 1 swap queued at or after the trigger", "%d" % len(sw))]
    mi = adds[0]["mi"] if adds else None
    drains = [e for e in evs(d, "drain") if mi in e["queued"] and e["t"] >= not_before_t]
    owned = any(e["own"][e["queued"].index(mi)] >= 1 for e in drains)
    out.append(("... and the swap drain built it a renderable for the new material", owned, "drain with own renderable", "%d drain(s), own=%s" % (len(drains), [e["own"] for e in drains])))
    return out


def feat_h4(obs):
    D, S = obs["dumps"], obs["shots"]
    out = []
    for lab, want in (("pre", dict(cyan=True, yellow=True, magenta=False)), ("s1", dict(cyan=False, yellow=False, magenta=True)), ("final", dict(cyan=True, yellow=True, magenta=False))):
        what = {"pre": "before any click", "s1": "after the 1st click", "final": "after the 2nd click"}[lab]
        for col, side in (("cyan", "left"), ("yellow", "right"), ("magenta", "right")):
            n = class_count(S[lab], col, 0 if side == "left" else 400, 0, 400 if side == "left" else 800, 600)
            out.append(("%s: %s is %s on the %s" % (what, col, "drawn" if want[col] else "not drawn", side), (n >= 800) if want[col] else (n < 100), ">= 800 px" if want[col] else "< 100 px", "%d px" % n))
        panel = meshes_named(D[lab], "panel")
        out.append(("%s: `panel` is still in the scene's mesh list" % what, len(panel) == 1, "1", str(len(panel))))
        gem = (meshes_named(D[lab], "gem") or [None])[0]
        dc = std_diffuse(gem) if gem else None
        exp = [0.9, 0.1, 0.8] if lab == "s1" else [0.9, 0.8, 0.1]
        out.append(("%s: `gem`'s material diffuse colour is %s" % (what, fmt(exp)), dc is not None and near(dc[:3], exp), fmt(exp), fmt(dc[:3] if dc else None)))
    pre = D["pre"]
    pn = (meshes_named(pre, "panel") or [None])[0]
    if pn:
        out.append(("panel: box of 2 units at (-2.5, 1, 0), standard material, cyan (0.1, 0.8, 0.9)", near(pn.get("pos"), [-2.5, 1, 0]) and near(extent(pn), [2, 2, 2]) and pn.get("matKind") == "standardGroupBuilder" and near((std_diffuse(pn) or [0])[:3], [0.1, 0.8, 0.9]), "(-2.5, 1, 0), 2, standard, (0.1, 0.8, 0.9)", "%s %s" % (fmt(pn.get("pos")), fmt(std_diffuse(pn)))))
    gm = (meshes_named(pre, "gem") or [None])[0]
    if gm:
        out.append(("gem: sphere of diameter 2 at (2.5, 1, 0), standard material", near(gm.get("pos"), [2.5, 1, 0]) and near(extent(gm), [2, 2, 2]) and gm.get("matKind") == "standardGroupBuilder", "(2.5, 1, 0), 2, standard", "%s %s" % (fmt(gm.get("pos")), gm.get("matKind"))))
    check_free_cam(pre, [0, 3, -10], [0, 1, 0], out)
    check_hemi(pre, out)
    return out


def cons_h4(obs):
    d, D = obs["dumps"]["final"], obs["dumps"]
    out = lifecycle_checks(d)
    seq = [e["v"] for e in mesh_evs(d, "setVisible", "panel")]
    out.append(("the panel was hidden and shown through setSubtreeVisible (false, then true)", len(seq) >= 2 and seq[0] is False and seq[1] is True, "setSubtreeVisible(panel, false) then (panel, true)", "calls: %s" % seq))
    out.append(("the panel was never removed from the scene", not mesh_evs(d, "removeFromScene", "panel"), "no removeFromScene(panel)", "%d" % len(mesh_evs(d, "removeFromScene", "panel"))))
    ids = [(meshes_named(D[l], "gem") or [{}])[0].get("matId") for l in ("pre", "s1", "final")]
    out.append(("the gem kept one material object (edited in place, not replaced)", ids[0] is not None and ids[0] == ids[1] == ids[2], "same material throughout", str(ids)))
    dirty = [e for e in evs(d, "matDirty") if e.get("mat") == ids[2]]
    out.append(("each edit of the gem's material was published with markMaterialUboDirty (>= 2 calls)", len(dirty) >= 2, ">= 2 markMaterialUboDirty calls", str(len(dirty))))
    return out


task(id="h4-toggle-tint", split="held-out", kinds=["K1", "K4", "K2"], facts=["late-hide", "material-dirty"], scenario=sc_h4, feature=feat_h4, constraint=cons_h4,
     prompt=lambda sealed=False: make_prompt(
         "In ./h4-toggle-tint, edit main.ts to build this scene with Babylon Lite and run it: a free camera at (0, 3, -10) looking at (0, 1, 0), as the scene's active camera; a hemispheric light pointing up; "
         "a box named `panel`, 2 units along each edge, at (-2.5, 1, 0), with a standard material whose diffuse colour is cyan (0.1, 0.8, 0.9); a sphere named `gem`, diameter 2, at (2.5, 1, 0), with a standard material "
         "whose diffuse colour starts yellow (0.9, 0.8, 0.1). Each click on the canvas does the next thing, in order: the 1st click hides `panel` (it stops being drawn but stays in the scene's mesh list) and turns `gem` "
         "magenta (0.9, 0.1, 0.8); the 2nd click shows `panel` again and turns `gem` back to yellow (0.9, 0.8, 0.1). `gem` keeps its one material: that material object is edited in place; it is not replaced by a new one. "
         "The engine keeps running throughout.", "h4-toggle-tint", sealed)())

# ---- H5: swap a mesh's material to another family at runtime (late-family); the tutorial added a new PBR mesh instead
def sc_h5(s):
    s.load()
    s.snap("pre")
    s.until_start_plus(9.0)
    s.wait(1.8)
    s.snap("final")


def feat_h5(obs):
    pre, post, ip, iq = obs["dumps"]["pre"], obs["dumps"]["final"], obs["shots"]["pre"], obs["shots"]["final"]
    out = []
    sd = start_done_t(pre)
    st = obs["shot_t"]["pre"]
    out.append(("the grader's first look was taken before the timer fired", sd is not None and st - sd < 8000, "< 8 s after startEngine", "%s ms" % (st - sd if sd is not None else "?")))
    fl = one_mesh(post, "floor", out)
    if fl:
        out.append(("floor: 12 x 12 at the origin, standard material, grey (0.4, 0.4, 0.4)", near(fl.get("pos"), [0, 0, 0]) and near([extent(fl)[0], extent(fl)[2]], [12, 12]) and fl.get("matKind") == "standardGroupBuilder" and near((std_diffuse(fl) or [0])[:3], [0.4, 0.4, 0.4]),
                    "origin, 12 x 12, standard, (0.4, 0.4, 0.4)", "%s %s %s" % (fmt(fl.get("pos")), fl.get("matKind"), fmt(std_diffuse(fl)))))
    s0 = (meshes_named(pre, "statue") or [None])[0]
    if s0:
        out.append(("before the timer: statue is a sphere of diameter 2.4 at (0, 1.2, 0) with a standard material, grey (0.6, 0.6, 0.6)", near(s0.get("pos"), [0, 1.2, 0]) and near(extent(s0), [2.4] * 3, 0.02) and s0.get("matKind") == "standardGroupBuilder" and near((std_diffuse(s0) or [0])[:3], [0.6, 0.6, 0.6]),
                    "(0, 1.2, 0), 2.4, standard, (0.6, 0.6, 0.6)", "%s %s %s" % (fmt(s0.get("pos")), s0.get("matKind"), fmt(std_diffuse(s0)))))
    s1 = one_mesh(post, "statue", out)
    if s1:
        mat = s1.get("material") or {}
        out.append(("after the timer: statue has a PBR material, base colour green (0.1, 0.8, 0.2), metallic 0.1, roughness 0.4",
                    s1.get("matKind") == "pbrGroupBuilder" and near((mat.get("baseColorFactor") or [0])[:3], [0.1, 0.8, 0.2]) and near([mat.get("metallicFactor")], [0.1]) and near([mat.get("roughnessFactor")], [0.4]),
                    "pbr (0.1, 0.8, 0.2) 0.1 0.4", "%s %s %s %s" % (s1.get("matKind"), fmt(mat.get("baseColorFactor")), fmt(mat.get("metallicFactor")), fmt(mat.get("roughnessFactor")))))
    px_presence(ip, "green", 100, out, "before the timer: nothing green is drawn", minimum=None)
    cc = class_centroid(iq, "green")
    out.append(("after the timer: the green sphere is drawn in the middle of the frame (>= 1500 px, centroid x within 60 of 400)", bool(cc) and cc[2] >= 1500 and abs(cc[0] - 400) < 60, ">= 1500 green px around x = 400", fmt(cc)))
    check_free_cam(post, [0, 3, -10], [0, 1, 0], out)
    check_hemi(post, out)
    return out


def cons_h5(obs):
    d = obs["dumps"]["final"]
    return lifecycle_checks(d) + swap_checks(d, "statue", start_done_t(d) + 9000 - 400)


task(id="h5-swap-to-pbr", a_needs_trace=True, split="held-out", kinds=["K1", "K4"], facts=["late-family"], scenario=sc_h5, feature=feat_h5, constraint=cons_h5,
     prompt=lambda sealed=False: make_prompt(
         "In ./h5-swap-to-pbr, edit main.ts to build this scene with Babylon Lite and run it: a free camera at (0, 3, -10) looking at (0, 1, 0), as the scene's active camera; a hemispheric light pointing up; "
         "a flat ground named `floor`, 12 x 12, at the origin, with a standard material whose diffuse colour is grey (0.4, 0.4, 0.4); a sphere named `statue`, diameter 2.4, at (0, 1.2, 0), with a standard material whose "
         "diffuse colour is grey (0.6, 0.6, 0.6). Exactly 9 seconds after `startEngine` resolves, the `statue`'s material is changed (the same mesh keeps being the `statue`; it is not removed or added again) to a PBR "
         "material with base colour green (0.1, 0.8, 0.2), metallic 0.1 and roughness 0.4, and the engine keeps running: the engine and the scene are not torn down and rebuilt.", "h5-swap-to-pbr", sealed)())


# ---- H6 / H7: two more sun-shadow scenes (shadow-attach, shadow-register, shadow-receive, shadow-light-pos); other meshes, light direction, camera, receiver material
def parallel(v, w):
    if not v:
        return False
    n, t = math.sqrt(sum(c * c for c in v)) or 1, math.sqrt(sum(c * c for c in w))
    return sum(v[i] / n * w[i] / t for i in range(3)) > 0.999


CAM6H = {"pos": [0, 7, -11], "target": [0, 0, 0], "fov": 0.8}


def feat_h6(obs):
    d, img = obs["dumps"]["final"], obs["shots"]["final"]
    out = []
    cam = d.get("camera") or {}
    out.append(("a free camera at (0, 7, -11) looking at the origin is the active camera", cam.get("kind") == "free" and near(cam.get("pos"), [0, 7, -11]) and near(cam.get("target"), [0, 0, 0]), "free (0,7,-11) -> (0,0,0)", fmt([cam.get("kind"), cam.get("pos"), cam.get("target")])))
    g = one_mesh(d, "lawn", out)
    if g:
        out.append(("lawn: 14 x 14 at the origin, standard material, (0.7, 0.8, 0.7)", near(g.get("pos"), [0, 0, 0]) and near([extent(g)[0], extent(g)[2]], [14, 14]) and g.get("matKind") == "standardGroupBuilder" and near((std_diffuse(g) or [0])[:3], [0.7, 0.8, 0.7]),
                    "origin, 14 x 14, standard, (0.7, 0.8, 0.7)", "%s %s %s" % (fmt(g.get("pos")), g.get("matKind"), fmt(std_diffuse(g)))))
    b = one_mesh(d, "ball", out)
    if b:
        out.append(("ball: sphere of diameter 2 at (-2, 1, 0), standard material, blue (0.2, 0.4, 0.9)", near(b.get("pos"), [-2, 1, 0]) and near(extent(b), [2, 2, 2]) and b.get("matKind") == "standardGroupBuilder" and near((std_diffuse(b) or [0])[:3], [0.2, 0.4, 0.9]), "(-2, 1, 0), 2, standard, blue", "%s %s" % (fmt(b.get("pos")), fmt(std_diffuse(b)))))
    p = one_mesh(d, "post", out)
    if p:
        out.append(("post: cylinder 3 high and 1 across at (2, 1.5, 0), standard material, red (0.9, 0.2, 0.2)", near(p.get("pos"), [2, 1.5, 0]) and near(extent(p), [1, 3, 1], 0.02) and p.get("matKind") == "standardGroupBuilder" and near((std_diffuse(p) or [0])[:3], [0.9, 0.2, 0.2]), "(2, 1.5, 0), (1, 3, 1), standard, red", "%s %s %s" % (fmt(p.get("pos")), fmt(extent(p)), fmt(std_diffuse(p)))))
    suns = [l for l in d.get("lights", []) if l.get("type") == "directional"]
    out.append(("a directional light shining along (1, -2, -0.5)", bool(suns) and parallel(suns[0].get("dir"), [1, -2, -0.5]), "directional (1, -2, -0.5)", fmt([(l.get("type"), l.get("dir")) for l in d.get("lights", [])])))
    check_hemi(d, out)
    pr = lambda p_: project(CAM6H, p_, 800, 600)[:2]
    lit = [region_luma(img, *pr(q)) for q in ([0, 0, 3], [0, 0, -3], [5, 0, 3])]
    sb, sp = region_luma(img, *pr([-1.5, 0, -0.25])), region_luma(img, *pr([2.75, 0, -0.375]))
    out.append(("the lit lawn is bright (luma > 110)", min(lit) > 110, "> 110", fmt(["%.0f" % v for v in lit])))
    out.append(("the ball's shadow falls on the lawn at (-1.5, 0, -0.25): darker than the lit lawn (< 85%)", sb < 0.85 * min(lit), "< 0.85 x lit", "shadow %.0f vs lit %.0f" % (sb, min(lit))))
    out.append(("the post's shadow falls on the lawn at (2.75, 0, -0.375): darker than the lit lawn (< 85%)", sp < 0.85 * min(lit), "< 0.85 x lit", "shadow %.0f vs lit %.0f" % (sp, min(lit))))
    px_presence(img, "blue", 0, out, "the blue ball is drawn", minimum=800)
    px_presence(img, "red", 0, out, "the red post is drawn", minimum=500)
    return out


task(id="h6-lawn-shadows", split="held-out", kinds=["K1", "K3", "K4", "K5", "K6"], facts=["shadow-attach", "shadow-register", "shadow-receive", "shadow-light-pos"], scenario=lambda s: (s.load(), s.wait(1.5), s.snap("final")), feature=feat_h6, constraint=cons_t5,
     prompt=lambda sealed=False: make_prompt(
         "In ./h6-lawn-shadows, edit main.ts to build this scene with Babylon Lite and run it: a free camera at (0, 7, -11) looking at (0, 0, 0), as the scene's active camera; a hemispheric light pointing up with intensity 0.35; "
         "a directional light (the sun) shining along (1, -2, -0.5) with intensity 1; a flat ground named `lawn`, 14 x 14, at the origin, with a standard material whose diffuse colour is (0.7, 0.8, 0.7); a sphere named `ball`, "
         "diameter 2, at (-2, 1, 0), with a standard material whose diffuse colour is blue (0.2, 0.4, 0.9); a cylinder named `post`, 3 high and 1 across, at (2, 1.5, 0), with a standard material whose diffuse colour is red "
         "(0.9, 0.2, 0.2). The sun casts a real-time shadow (shadow mapping with a PCF directional shadow generator, not a painted shadow): both the `ball`'s and the `post`'s shadows fall on the `lawn`.", "h6-lawn-shadows", sealed)())


def feat_h7(obs):
    d, img = obs["dumps"]["final"], obs["shots"]["final"]
    out = []
    cam = d.get("camera") or {}
    out.append(("the active camera is an orbit (arc-rotate) camera with alpha -pi/2, beta 1.15, radius 15 targeting (0, 0.5, 0)", cam.get("kind") == "arc" and near([cam.get("alpha"), cam.get("beta"), cam.get("radius")], [-math.pi / 2, 1.15, 15.0]) and near(cam.get("target"), [0, 0.5, 0]),
                "arc (-1.571, 1.15, 15) (0, 0.5, 0)", fmt([cam.get("kind"), cam.get("alpha"), cam.get("beta"), cam.get("radius"), cam.get("target")])))
    g = one_mesh(d, "deck", out)
    if g:
        mat = g.get("material") or {}
        out.append(("deck: 14 x 14 at the origin, PBR material, base colour (0.8, 0.8, 0.8), metallic 0, roughness 0.8", near(g.get("pos"), [0, 0, 0]) and near([extent(g)[0], extent(g)[2]], [14, 14]) and g.get("matKind") == "pbrGroupBuilder" and near((mat.get("baseColorFactor") or [0])[:3], [0.8, 0.8, 0.8])
                    and near([mat.get("metallicFactor")], [0]) and near([mat.get("roughnessFactor")], [0.8]), "origin, 14 x 14, pbr (0.8, 0.8, 0.8) 0 0.8", "%s %s %s %s %s" % (fmt(g.get("pos")), g.get("matKind"), fmt(mat.get("baseColorFactor")), fmt(mat.get("metallicFactor")), fmt(mat.get("roughnessFactor")))))
    h = one_mesh(d, "halo", out)
    if h:
        out.append(("halo: torus of diameter 3 and thickness 0.6 at (0, 2.5, 0), standard material, orange (0.9, 0.5, 0.1)", near(h.get("pos"), [0, 2.5, 0]) and near([extent(h)[0], extent(h)[2]], [3.6, 3.6], 0.15) and h.get("matKind") == "standardGroupBuilder" and near((std_diffuse(h) or [0])[:3], [0.9, 0.5, 0.1]),
                    "(0, 2.5, 0), 3.6 across, standard, orange", "%s %s %s" % (fmt(h.get("pos")), fmt(extent(h)), fmt(std_diffuse(h)))))
    suns = [l for l in d.get("lights", []) if l.get("type") == "directional"]
    out.append(("a directional light shining along (-0.4, -1, -0.7) with intensity 1.1", bool(suns) and parallel(suns[0].get("dir"), [-0.4, -1, -0.7]) and near([suns[0].get("intensity")], [1.1]), "directional (-0.4, -1, -0.7) x 1.1", fmt([(l.get("type"), l.get("dir"), l.get("intensity")) for l in d.get("lights", [])])))
    check_hemi(d, out)
    if cam.get("kind") == "arc":
        c = arc_to_cam(cam)
        pr = lambda p_: project(c, p_, 800, 600)[:2]
        lit = [region_luma(img, *pr(q)) for q in ([4, 0, 3], [-5, 0, 3], [5, 0, -2])]
        s1, s2 = region_luma(img, *pr([0.5, 0, -1.75])), region_luma(img, *pr([-2.5, 0, -1.75]))
        out.append(("the lit deck is bright (luma > 110)", min(lit) > 110, "> 110", fmt(["%.0f" % v for v in lit])))
        out.append(("the halo's ring-shaped shadow falls on the deck at (0.5, 0, -1.75) and (-2.5, 0, -1.75): darker than the lit deck (< 90%)", s1 < 0.9 * min(lit) and s2 < 0.9 * min(lit), "< 0.9 x lit", "shadow %.0f, %.0f vs lit %.0f" % (s1, s2, min(lit))))
        hole = region_luma(img, *pr([-1.0, 0, -1.75]))
        out.append(("... and the middle of the ring's shadow, at (-1, 0, -1.75), is lit again (a ring, not a disc)", hole > 0.93 * min(lit), "> 0.93 x lit", "hole %.0f vs lit %.0f" % (hole, min(lit))))
    px_presence(img, "orange", 0, out, "the orange halo is drawn", minimum=500)
    return out


task(id="h7-ring-shadow", split="held-out", kinds=["K1", "K3", "K4", "K5", "K6"], facts=["shadow-attach", "shadow-register", "shadow-receive", "shadow-light-pos"], scenario=lambda s: (s.load(), s.wait(1.5), s.snap("final")), feature=feat_h7, constraint=cons_t5,
     prompt=lambda sealed=False: make_prompt(
         "In ./h7-ring-shadow, edit main.ts to build this scene with Babylon Lite and run it: an orbit (arc-rotate) camera with alpha -pi/2, beta 1.15, radius 15, targeting (0, 0.5, 0), as the scene's active camera; "
         "a hemispheric light pointing up with intensity 0.3; a directional light (the sun) shining along (-0.4, -1, -0.7) with intensity 1.1; a flat ground named `deck`, 14 x 14, at the origin, with a PBR material "
         "(base colour (0.8, 0.8, 0.8), metallic 0, roughness 0.8); a torus named `halo`, diameter 3 and thickness 0.6, floating at (0, 2.5, 0), with a standard material whose diffuse colour is orange (0.9, 0.5, 0.1). "
         "The sun casts a real-time shadow (shadow mapping with a PCF directional shadow generator, not a painted shadow): the `halo`'s ring-shaped shadow falls on the `deck`.", "h7-ring-shadow", sealed)())
