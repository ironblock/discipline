() => {
  const s = window.scene;
  const errs = window.__errs || [];
  const LT = window.__LT;
  const lt = LT ? { ev: LT.ev.slice(), now: Math.round(performance.now() - LT.t0) } : null;
  const canvas = document.getElementById("renderCanvas");
  const cv = canvas ? { w: canvas.width, h: canvas.height, cw: canvas.clientWidth, ch: canvas.clientHeight, dpr: devicePixelRatio } : null;
  if (!s) return { noScene: true, errs, lt, cv, warns: window.__warns || [] };
  const v3 = (o) => (o && typeof o === "object" && "x" in o ? [o.x, o.y, o.z] : null);
  const prim = (m) => {
    if (!m) return null;
    const out = {};
    for (const k of Object.keys(m)) {
      if (k[0] === "_") continue;
      const v = m[k];
      if (typeof v === "number" || typeof v === "boolean" || typeof v === "string") out[k] = v;
      else if (Array.isArray(v) && v.length <= 4 && v.every((x) => typeof x === "number")) out[k] = v.slice();
      else if (v && typeof v === "object" && !Array.isArray(v) && Object.keys(v).length <= 8 && Object.values(v).every((x) => ["number", "string", "boolean"].includes(typeof x))) out[k] = Object.assign({}, v);
    }
    return out;
  };
  const wp = (n) => { try { const w = n.worldMatrix; return [w[12], w[13], w[14]]; } catch (e) { return null; } };
  const nOwn = (m) => (s._renderables || []).filter((r) => r.mesh === m).length;
  const mesh = (m) => ({
    name: m.name, pos: v3(m.position), scale: v3(m.scaling), world: wp(m), visible: m.visible, receiveShadows: !!m.receiveShadows,
    bmin: m.boundMin ? Array.from(m.boundMin) : null, bmax: m.boundMax ? Array.from(m.boundMax) : null,
    parent: m.parent ? (m.parent.name === undefined ? "(unnamed)" : m.parent.name) : null,
    parentPos: m.parent ? v3(m.parent.position) : null,
    material: prim(m.material), matId: LT && m.material ? LT.matid(m.material) : null, matKind: m.material && m.material._buildGroup ? (m.material._buildGroup.name || "anon") : null,
    own: nOwn(m), hasColorBuf: !!(m._gpu && m._gpu.hasColor), idx: m._gpu ? m._gpu.indexCount : null,
    ti: m.thinInstances ? { count: m.thinInstances.count, matrices: Array.from(m.thinInstances.matrices.slice(0, m.thinInstances.count * 16)), colors: m.thinInstances.colors ? Array.from(m.thinInstances.colors.slice(0, m.thinInstances.count * 4)) : null } : null,
  });
  const c = s.camera;
  const cam = c && {
    pos: v3(c.position), target: v3(c.target),
    alpha: typeof c.alpha === "number" ? c.alpha : null, beta: typeof c.beta === "number" ? c.beta : null,
    radius: typeof c.radius === "number" ? c.radius : null,
    kind: c.alpha !== undefined ? "arc" : c.position ? "free" : "other", fov: c.fov, near: c.nearPlane, far: c.farPlane,
  };
  const meshState = lt ? LT.meshes.map((m, i) => ({ i, name: m.name, inScene: s.meshes.indexOf(m) >= 0, own: nOwn(m), visible: m.visible })) : null;
  const tasks = (s._frameGraph && s._frameGraph._tasks ? s._frameGraph._tasks.map((t) => t.name) : null);
  return {
    errs, warns: window.__warns || [], lt, cv, meshState, tasks,
    now: Math.round(performance.now()),
    scene: { built: s._built, queue: s._materialSwapQueue ? s._materialSwapQueue.length : null, renderables: s._renderables ? s._renderables.length : null, drawCalls: s.surface && s.surface.engine ? s.surface.engine.drawCallCount : null },
    pick: window.pick === undefined ? null : (function () { try { return JSON.parse(JSON.stringify(window.pick)); } catch (e) { return String(window.pick); } })(),
    meshes: s.meshes.map(mesh),
    camera: cam,
    lights: s.lights.map((l) => ({ type: l.lightType, dir: v3(l.direction), pos: v3(l.position), intensity: l.intensity, hasShadow: !!l.shadowGenerator, shadowKind: l.shadowGenerator ? (l.shadowGenerator.kind || l.shadowGenerator.type || "sg") : null, diffuse: l.diffuse || null })),
    clear: s.clearColor ? { r: s.clearColor.r, g: s.clearColor.g, b: s.clearColor.b, a: s.clearColor.a, isArr: Array.isArray(s.clearColor) } : null,
    fog: s.fog ? prim(s.fog) : null,
    sgCount: s.shadowGenerators ? s.shadowGenerators.length : null,
  };
}
