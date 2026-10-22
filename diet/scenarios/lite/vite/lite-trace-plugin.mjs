// GRADER-ONLY instrumentation of Babylon Lite's scene lifecycle, applied as a vite `transform` on the aliased Lite TS source.
// The model's own dev server (no LITE_TRACE) serves the untouched source; only the grader's server loads this plugin.
// It renames each wrapped exported function to `__lt_<name>` and appends a same-named exported wrapper that records an event into
// `globalThis.__LT` (installed by scripts/initscript.js in the GRADER's browser before any page script), then calls the original.
// The wrappers never change arguments, return values or ordering. Nothing is written to the Lite checkout.
//
// Because a wrapper is declared with the original's name in the original's module scope, intra-module calls (registerScene -> buildScene,
// addToScene -> addToScene) go through it too. Cross-module calls go through the module's export, which is the wrapper.

const WRAPPERS = {
    "/scene/scene-core.ts": {
        createSceneContext: `export function createSceneContext(surface: any, options?: any): any {
    const s = __lt_createSceneContext(surface, options);
    __LT?.push("createScene", { scene: __LT.sid(s) });
    return s;
}`,
        addToScene: `export function addToScene(scene: any, entity: any): void {
    const kind = "entities" in entity ? "container" : "lightType" in entity ? "light" : "_gpu" in entity && "material" in entity ? "mesh" : "children" in entity ? "node" : "other";
    const isMesh = kind === "mesh";
    const reg = isRenderingContextRegistered(scene.surface, scene);
    const qBefore = scene._materialSwapQueue.length;
    __LT?.push("add", { scene: __LT.sid(scene), kind, name: entity.name ?? null, mi: isMesh ? __LT.mid(entity) : null, built: scene._built, registered: reg,
        hasMaterial: isMesh ? !!entity.material : null, matKind: isMesh && entity.material ? (entity.material._buildGroup?.name || "anon") : null });
    const r = __lt_addToScene(scene, entity);
    if (isMesh) __LT?.push("add.after", { scene: __LT.sid(scene), mi: __LT.mid(entity), queued: scene._materialSwapQueue.indexOf(entity) >= 0, queueGrew: scene._materialSwapQueue.length - qBefore });
    return r;
}`,
        disposeScene: `export function disposeScene(scene: any): void {
    __LT?.push("disposeScene", { scene: __LT.sid(scene) });
    return __lt_disposeScene(scene);
}`,
        buildScene: `export async function buildScene(scene: any): Promise<void> {
    __LT?.push("buildScene", { scene: __LT.sid(scene), builtBefore: scene._built });
    return __lt_buildScene(scene);
}`,
        registerScene: `export async function registerScene(scene: any): Promise<void> {
    __LT?.push("registerScene", { scene: __LT.sid(scene), already: isRenderingContextRegistered(scene.surface, scene), shadow: false });
    return __lt_registerScene(scene);
}`,
        registerSceneWithShadowSupport: `export async function registerSceneWithShadowSupport(scene: any): Promise<void> {
    __LT?.push("registerScene", { scene: __LT.sid(scene), already: isRenderingContextRegistered(scene.surface, scene), shadow: true });
    return __lt_registerSceneWithShadowSupport(scene);
}`,
        unregisterScene: `export function unregisterScene(scene: any): void {
    __LT?.push("unregisterScene", { scene: __LT.sid(scene) });
    return __lt_unregisterScene(scene);
}`,
    },
    "/scene/scene-remove.ts": {
        removeFromScene: `export function removeFromScene(scene: any, mesh: any): void {
    __LT?.push("removeFromScene", { scene: __LT.sid(scene), mi: __LT.mid(mesh), name: mesh.name ?? null, built: scene._built });
    return __lt_removeFromScene(scene, mesh);
}`,
    },
    "/scene/mesh-scene-registry.ts": {
        enqueueMaterialSwap: `export function enqueueMaterialSwap(scene: any, mesh: any): void {
    __LT?.push("enqueueSwap", { scene: __LT.sid(scene), mi: __LT.mid(mesh), name: mesh.name ?? null, already: scene._materialSwapQueue.indexOf(mesh) >= 0 });
    return __lt_enqueueMaterialSwap(scene, mesh);
}`,
    },
    "/scene/scene-material-swap.ts": {
        processMaterialSwaps: `export function processMaterialSwaps(scene: any): void {
    const queued = scene._materialSwapQueue.map((m: any) => __LT.mid(m));
    const r = __lt_processMaterialSwaps(scene);
    __LT?.push("drain", { scene: __LT.sid(scene), queued, own: queued.map((mi: number) => scene._renderables.filter((x: any) => x.mesh === __LT.meshes[mi]).length) });
    return r;
}`,
    },
    "/scene/visibility.ts": {
        setSubtreeVisible: `export function setSubtreeVisible(node: any, v: boolean): void {
    __LT?.push("setVisible", { name: node.name ?? null, mi: "_gpu" in node ? __LT.mid(node) : null, v });
    return __lt_setSubtreeVisible(node, v);
}`,
    },
    "/material/material-dirty.ts": {
        markMaterialUboDirty: `export function markMaterialUboDirty(m: any): void {
    __LT?.push("matDirty", { mat: __LT.matid(m) });
    return __lt_markMaterialUboDirty(m);
}`,
    },
    "/mesh/mesh-factories.ts": {
        createMeshFromData: `export function createMeshFromData(engine: any, name: string, positions: Float32Array, normals: Float32Array, indices: Uint32Array, ...rest: any[]): any {
    __LT?.push("meshFromData", { name, nIdx: indices.length, nVert: positions.length / 3, hasColors: !!rest[4], internal: (new Error().stack || "").split("\\n").slice(2, 4).some((l: string) => l.includes("mesh-factories")) });
    return (__lt_createMeshFromData as any)(engine, name, positions, normals, indices, ...rest);
}`,
    },
    "/mesh/thin-instance.ts": {
        setThinInstances: `export function setThinInstances(mesh: any, matrices: any, count: number): void {
    __LT?.push("thin.set", { name: mesh.name ?? null, mi: __LT.mid(mesh), count, len: matrices.length });
    return __lt_setThinInstances(mesh, matrices, count);
}`,
        addThinInstance: `export function addThinInstance(mesh: any, matrix: any): number {
    __LT?.push("thin.add", { name: mesh.name ?? null, mi: __LT.mid(mesh) });
    return __lt_addThinInstance(mesh, matrix);
}`,
        setThinInstanceColors: `export function setThinInstanceColors(mesh: any, colors: Float32Array): void {
    __LT?.push("thin.colors", { name: mesh.name ?? null, mi: __LT.mid(mesh), len: colors.length, hadInstances: !!mesh.thinInstances });
    return __lt_setThinInstanceColors(mesh, colors);
}`,
    },
    "/picking/gpu-picker.ts": {
        pickAsync: `export async function pickAsync(picker: any, x: number, y: number, options?: any): Promise<any> {
    __LT?.push("pick", { x, y });
    const r = await __lt_pickAsync(picker, x, y, options);
    __LT?.push("pick.done", { x, y, hit: r.hit, name: r.pickedMesh ? r.pickedMesh.name : null, point: r.pickedPoint ? Array.from(r.pickedPoint) : null });
    return r;
}`,
    },
    "/engine/engine.ts": {
        createEngine: `export async function createEngine(canvas: any, options?: any): Promise<any> {
    __LT?.push("createEngine", { opts: options ? Object.keys(options) : [], vals: options ? Object.fromEntries(Object.entries(options).filter(([, v]) => ["number", "string", "boolean"].includes(typeof v))) : {} });
    return __lt_createEngine(canvas, options);
}`,
        startEngine: `export function startEngine(engine: any): Promise<void> {
    __LT?.push("startEngine.call", {});
    return __lt_startEngine(engine).then(() => { __LT?.push("startEngine.done", {}); });
}`,
        disposeEngine: `export function disposeEngine(engine: any): void {
    __LT?.push("disposeEngine", {});
    return __lt_disposeEngine(engine);
}`,
    },
};

export function liteTrace(liteSrc) {
    return {
        name: "lite-trace",
        enforce: "pre",
        transform(code, id) {
            const file = id.split("?")[0];
            if (!file.startsWith(liteSrc)) return null;
            const key = file.slice(liteSrc.length);
            const spec = WRAPPERS[key];
            if (!spec) return null;
            let out = code;
            for (const name of Object.keys(spec)) {
                const re = new RegExp("export (async )?function " + name + "\\(");
                if (!re.test(out)) throw new Error("lite-trace: " + key + " has no `export function " + name + "(` (the instrumented source changed)");
                out = out.replace(re, (_, a) => (a || "") + "function __lt_" + name + "(");
            }
            const prelude = "const __LT: any = (globalThis as any).__LT;\n";
            const tail = "\n" + Object.values(spec).join("\n") + "\n";
            return { code: prelude + out + tail, map: null };
        },
    };
}
