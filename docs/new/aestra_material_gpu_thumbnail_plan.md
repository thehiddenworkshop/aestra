# GPU Material Thumbnails — Scope

Status: design (no code yet). Motivated by: materials that sample textures,
displace vertices, or call functions get no browser thumbnail — they fall back
to the flat material icon (e.g. "Mesh Material Lab", which uses a normal texture
and breath displacement).

## Goal

Render browser thumbnails for materials the current CPU rasterizer refuses, by
rendering the material on domain-appropriate geometry with its textures actually
bound — the "scene preview" the rejection messages already point to. Give
textured / displacement / mesh-domain materials a faithful thumbnail instead of
the generic icon.

Exit criterion: a material that samples a texture, displaces vertices, or targets
the Mesh/Ribbon domain shows a correct rendered thumbnail in the asset browser,
matching what the shipped renderer produces, with no misleading placeholder.

## Where we are today

Material thumbnails go through `render_material_asset_preview`
(`apps/aestra-editor/src/material_graph/asset_preview.rs`): a **CPU rasterizer**
that evaluates the material per-pixel on synthetic geometry and **rejects**
anything it cannot render faithfully without a GPU scene —
`SampleTexture*`, `vertex_offset`, `FunctionCall`/`CustomWeslCall`, screen
derivatives. On rejection the thumbnail falls back to the kind icon.

The in-graph output preview uses the *permissive* variant (checker for textures,
displacement ignored) — a rough authoring aid, deliberately not used for the
browser because it would show wrong content (checker where a real normal map is).

Meanwhile, **effect thumbnails already render materials on the GPU**: `effect::GpuJob`
spawns a camera + presented instances on an isolated render layer, binds the
effect's textures and meshes, waits for the sim to settle, screenshots the
128×128 target, and reframes. Presented instances already exercise the full
material pipeline (`compile_material_program` / `MaterialRuntimeBinding`). So the
capability exists — it is just not reachable for a *standalone* material.

## Design

### Preview by synthesizing a minimal instance
Rather than build a second material-rendering path, **synthesize a minimal scene
that renders the target material in its domain**, then reuse the effect
thumbnail's GPU scaffolding (isolated layer, image-target camera, texture/mesh
binding, settle, screenshot, `framing::fit`).

Domain → preview geometry (`MaterialDomain` = Sprite | Mesh | Ribbon | Decal | Screen):
- **Sprite** → a single camera-facing quad (one static particle).
- **Mesh** → a unit sphere (default) so surface shading + normal maps + breath
  displacement read clearly; the material's own mesh is not required for a
  material preview.
- **Ribbon** → a short curved ribbon strip.
- **Decal / Screen** → out of first scope; keep the CPU path or icon (rare in
  authoring, and a decal needs a receiver surface).

The synthetic instance binds the material's referenced textures (loaded like the
effect path decodes them) so the preview shows the real maps, not a checker.

### Routing (keep the fast path)
- Simple materials (no texture/vertex/function/derivative) keep the **CPU path** —
  it is fast, synchronous-ish on the worker pool, and already correct.
- Materials the CPU path rejects route to a **GPU material job**. So the GPU cost
  is paid only where it is actually needed.

### Reuse and isolation
- Factor the effect `GpuJob`'s shared plumbing (image target, isolated layer,
  readiness/settle, screenshot capture, reframe) so both effect and material
  previews use it; the material job differs only in *what it spawns* (a synthetic
  material instance vs. effect instances).
- Its own render layer, distinct from the effect capture layer (30) and the hover
  live-preview layer (29) — e.g. 28.
- One GPU preview at a time across effect + material jobs (extend the existing
  single-slot discipline, or a small shared queue), so background previews never
  contend with the viewport.

### Bounds
Apply the same bounded-preview limits as effects: cap expressions, textures,
texture dimensions, and reject custom-WESL/function materials (still not run in
background previews — the graph is the place for those). A rejected material
falls back to the icon exactly as today.

## Milestones

- **M-MG1 — Shared preview render harness.** Extract the effect `GpuJob`'s
  camera/layer/target/settle/screenshot/reframe into a reusable piece; effect
  previews keep working through it (no behavior change). Pure refactor + tests.
- **M-MG2 — Sprite/Mesh material job.** Synthesize a one-instance scene for a
  Sprite or Mesh material with textures bound, render via the harness, capture.
  Route CPU-rejected Sprite/Mesh materials to it in `thumbnails::update`.
- **M-MG3 — Ribbon domain.** Add the ribbon-strip preview geometry.
- **M-MG4 — Polish.** Framing per domain, background/lighting parity with the
  shipped look, and (optional) fold into the disk cache from the thumbnail-cache
  plan so these renders persist.

## Risks / decision checkpoint

- **Faithfulness over prettiness.** The whole point is to *not* show misleading
  previews; if a material cannot be rendered faithfully in the background (custom
  WESL, unusual domain), fall back to the icon rather than fake it. Preserve the
  strict-reject discipline; the GPU path widens what can be rendered *correctly*,
  it does not lower the bar.
- **Concurrency / cost.** Two GPU preview producers (effects, materials) plus the
  hover live preview must share one slot and stay off the viewport's budget.
  M-MG1's harness owns this; validate before adding the material producer.
- **Scope creep.** First scope is Sprite + Mesh (what authors mostly make).
  Ribbon is M-MG3; Decal/Screen stay out until there's a real need.
- Reassess after M-MG2 on a real project: do the previously-iconned materials now
  match the renderer, and is the added GPU load acceptable during browsing?
