# Thumbnail Disk Cache — Scope

Status: design (no code yet). Motivated by: effect thumbnails are regenerated on
every startup, and each is an expensive GPU render.

## Goal

Persist rendered **effect** thumbnails to disk so that, on startup and after
LRU eviction, an unchanged effect's preview loads from a cached image instead of
being re-rendered. Textures, meshes, and materials stay regenerate-on-demand —
they decode cheaply on IO threads and are not worth the invalidation risk.

Exit criterion: relaunching the editor on an unchanged project shows effect
thumbnails immediately (a fast image load), with no visible one-at-a-time GPU
re-render, and a changed effect (or any of its dependencies) still re-renders.

## Where we are today

`ThumbnailCache` (`apps/aestra-editor/src/asset_browser/thumbnails.rs`) is an
in-memory `Resource`:

- A `BTreeMap<ProjectSourceId, Entry>` of at most `CAPACITY = 128` previews with
  LRU eviction (`touched` tick), each an `Image` handle in `Assets<Image>`.
- Rebuilt per session; re-derived when the project content changes, gated by
  `content_epoch()` / `same_thumbnail_content()` using the per-source
  `fingerprint` in `ProjectTreeStamp`.
- Effects render through `effect::GpuJob`: spawn a camera + presented instances
  on an isolated layer, wait for the sim to settle, screenshot the 128×128
  target, then two `framing::fit` reframe passes. **One effect at a time**
  (`cache.gpu` is a single slot), so N effects populate serially.

The cost is uneven: textures/meshes/materials are cheap async decodes; **only
effects are expensive**, and they are serialized. That is the whole target.

## Design

### What is cached
- `EDGE`×`EDGE` (128²) RGBA, encoded as **PNG** on disk (a few KB each). This is
  the same buffer `accept()` already receives for effects
  (`Preview::Ready` bytes), plus the refined `framing` is *not* needed on disk —
  only the final pixels.
- Effects only. Other kinds bypass the disk cache entirely.

### Cache key (the load-bearing part)
Correctness is invalidation, not storage. The key must change whenever anything
that affects the rendered pixels changes:

```
key = hash(
    cache_format_version   // bump to invalidate all entries on a schema change
    renderer_version       // bump when the GPU render/framing path changes output
    resolved_fingerprint   // see below
)
```

`resolved_fingerprint` must cover the **resolved dependency graph**, not just the
effect file's bytes — an effect's preview depends on its child effect clips,
material programs, and referenced textures/meshes. Source: the same
`ResolvedEffectProject` that `effect::saved()` already produces. Fold together
the per-source `fingerprint`s (already tracked in `ProjectTreeStamp`) for:
- the root effect,
- every dependency effect,
- every material program,
- every referenced texture and mesh asset.

If any dependency fingerprint is missing/unreadable, treat as a miss and render
(never serve a possibly-stale frame).

### Location
User-level OS cache directory (e.g. `dirs`/`directories` cache dir under an
`aestra/thumbnails/` subfolder), **not** the project tree:
- keeps the project clean and unaffected by `git clean`,
- shared across projects on the machine,
- filename = the hex `key`; content = PNG.

(Alternative considered: `.aestra/thumbnails/` in the project, gitignored —
travels with the project and is shareable, but clutters the tree. Default to
user-level; revisit if sharing baked thumbnails becomes desirable.)

### Flow
1. When a wanted effect thumbnail is not in the in-memory cache, compute its
   `resolved_fingerprint` (already needed to render). Derive `key`.
2. **Load-before-render:** if `<cachedir>/<key>.png` exists and decodes to
   128×128, insert it as `Preview::Ready` (an IO decode on the existing worker
   pool — cheap) and skip the GPU job.
3. On miss, run the existing `GpuJob`. When it completes successfully, in
   `accept()` (or alongside it) **write** the PNG to `<cachedir>/<key>.png`.
4. The in-memory LRU stays the hot tier and is unchanged; disk is the cold tier
   consulted only on a miss.

### Bounding & hygiene
- Cap the cache directory (e.g. a few thousand entries or an MB budget); evict
  oldest by mtime when over budget, on a background sweep at startup.
- All writes go through a temp-file + atomic rename to avoid torn files.
- Reads are best-effort: any IO/decode error is a silent miss → render.

## Milestones

- **M-TC1 — Fingerprint + key.** Add `resolved_fingerprint(source)` from the
  resolved project, and `key()`. Pure function + tests; no IO. Wire a
  `renderer_version`/`cache_format_version` constant.
- **M-TC2 — Read path.** On a wanted, uncached effect, check disk first and load
  the PNG as `Preview::Ready` before enqueuing a `GpuJob`. Behind a setting,
  default on. Verify a warm relaunch skips GPU renders.
- **M-TC3 — Write path.** Persist the PNG when a `GpuJob` completes, atomically.
- **M-TC4 — Bounding + sweep.** Directory cap + oldest-mtime eviction at startup;
  temp-file writes; error-as-miss everywhere.

## Risks / decision checkpoint

- **Stale previews are worse than a slow render.** The invalidation key must be
  conservative: any doubt → miss → render. M-TC1's fingerprint coverage is the
  crux; review it before building the read/write paths.
- **Value depends on project size.** For a handful of effects the in-memory
  cache is already adequate and this is complexity for little gain. Worth
  building only if effect-thumbnail startup is actually slow. Reassess after
  M-TC2 with a real project: measure warm-vs-cold startup thumbnail latency.
- **Scope discipline:** effects only. Do not extend to textures/meshes/materials
  unless a measurement says otherwise.
