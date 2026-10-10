# Bevy 0.20 upgrade and adoption plan

Date: 2026-10-10. Status: dependencies isolated, 0.20 patches qualified; portable
compiler implemented and shader, storage, extraction, draw-binding and queue boundaries qualified;
shipping engine migration pending.
Repository baseline inspected: `e8191982`, Bevy 0.19.1.

## Decision

Upgrade, but separate **compatibility** from **feature adoption**. The renderer and
two local Bevy patches are the critical path. Replacing widget internals should be
small follow-up changes, not a simultaneous editor rewrite.

Aestra already composes upstream Feathers controls in
`apps/aestra-editor/src/feathers/`. Keep that boundary and its semantic commands,
styling and undo integration; upstream should own more of the interaction machinery.
Keep engine-neutral authoring, compilation and playback independent of Bevy.

The [release announcement](https://bevy.org/news/bevy-0-20/) introduces scrubbable
number inputs, color input, dropdowns, lazy menus, selectable scrollable lists and
headless tabs. These are the strongest immediate opportunities for Aestra.

## B20-0 — Dependency feasibility, patch audit and baseline

Do this before a broad source migration. Record exact resolved versions, not guessed
future plugin versions.

| Dependency | Current Aestra | Upgrade action |
| --- | --- | --- |
| Bevy | 0.19.1 | Move the workspace and direct `bevy_winit` dependency to compatible 0.20 releases. |
| Rust | Pinned 1.98.1 | Keep it: Bevy 0.20 declares Rust 1.97.1 as its minimum. |
| wgpu | 29.0.4 | Align with Bevy's wgpu 30; include standalone GPU benchmarks and fluid tests. |
| glam | 0.32.1 | Align shared math types with Bevy's 0.33.2. |
| Naga | Direct compiler 30.0.1; shipping wgpu still uses 29.0.4 | Portable validation moved first; align the renderer's wgpu graph during the engine switch. Only WGSL text crosses this boundary. |
| WESL | Direct compiler 0.6.0; shipping Bevy still uses 0.3.2 | Portable composition uses the 0.6 API; Bevy's internal shader compiler changes with the engine. The showcase dev-dependency inherits the workspace pin. |
| encase | 0.12.1 | Bevy still uses 0.12; verify resolution and buffer-layout tests rather than forcing a major bump. |
| SVG | Aestra-owned icon bridge, resvg 0.47 / tiny-skia 0.12 | Engine-neutral rasterization, native Bevy ImageNode; no bevy_resvg dependency. |
| avian3d / bevy_rapier3d | 0.7.0 / 0.36.0 | Explicitly suspended standalone adapters, outside the active workspace; restore only after compatibility tests. |

Version evidence: released [Bevy manifest](https://github.com/bevyengine/bevy/blob/v0.20.0/Cargo.toml),
[render manifest](https://raw.githubusercontent.com/bevyengine/bevy/v0.20.0/crates/bevy_render/Cargo.toml),
[math manifest](https://raw.githubusercontent.com/bevyengine/bevy/v0.20.0/crates/bevy_math/Cargo.toml),
and [shader manifest](https://raw.githubusercontent.com/bevyengine/bevy/v0.20.0/crates/bevy_shader/Cargo.toml).

The [2026-10-09 preflight](../../benchmarks/bevy-upgrade/preflight-2026-10-09.md)
confirmed that the latest published SVG/physics plugins targeted 0.19. An isolated
copy of all 17 workspace member manifests resolves, but introduces incompatible
0.19/0.20 engine graphs. Resolution alone is not compatibility. Recheck published
releases before implementation; do not assume a cached development branch is current.
The user subsequently approved replacing the SVG bridge and temporarily suspending
the two optional physics adapters. See the dependency-isolation follow-up below;
the original 17-member preflight report remains historical, not a current graph claim.

Acceptance:

- A resolution spike covers the entire workspace, examples and optional features.
  Inspect `cargo tree -d` and inverse dependency trees for incompatible Bevy/wgpu/math
  types. Do not accept two engine versions crossing Aestra's ECS or render boundary.
- No physics adapter is silently removed from workspace validation to get a green build.
  If an upstream dependency is unavailable, record the blocker and seek approval for
  waiting, a narrowly maintained port, or an explicitly separate adapter release.
- Preserve development-only dynamic linking and standalone/static release builds.
- Capture existing editor interactions, thumbnails, material/volume rendering,
  fireworks timing, steady-state allocations and GPU workloads before changing them.
  Record hardware, backend, feature set and toolchain with the evidence.

### Local patches: evaluate independently

`vendor/bevy_input_focus/AESTRA_PATCH.md` documents event-time modifier handling and
the custom `KeyboardInputSnapshot` API consumed by the editor. Verify equivalent
upstream behavior for same-frame Ctrl+A/Ctrl+V, text before/after chords, held left/right
modifiers, focus loss, repeat, AZERTY/QWERTZ and deferred editor shortcuts. Remove the
patch and snapshot adapter together only when those regressions pass. Otherwise port
the minimal fix to actual 0.20 source and document its provenance.

`vendor/bevy_pbr/AESTRA_PATCH.md` documents retained cluster buffers, logical resets,
bounded asynchronous readback ownership and camera/mode cleanup. Compare upstream
code and run the existing unit and native lifetime/allocation tests. The integration
test `bevy/aestra-bevy/tests/cluster_buffer_reuse.rs` imports patch-only diagnostics:
removal also requires replacement test instrumentation, not deletion of the assertions.

Do not relabel vendored 0.19 source as 0.20, edit Cargo registry files, or assume these
fixes landed upstream. Publishing a crate does not propagate root Cargo patches.

Exit: a documented dependency/patch decision and reproducible pre-upgrade baseline.

### B20-0 execution status — 2026-10-09

- Dependency audit and reproducible isolated probe delivered in `benchmarks/bevy-upgrade/`.
- Full manifest graph tested with all workspace features, including benchmark GPU,
  showcase audio and development dynamic linking; original manifests/lockfile unchanged.
- Published 0.20 keyboard regression reproduced: held Ctrl passes, same-frame Ctrl+A
  fails. Cluster source audit still finds per-frame recreation and unbounded pending
  readback ownership. Both fixes must be retained/ported until qualified replacements pass.
- Current baseline: 11 portable contract/shader tests, 8 editor input/precision tests,
  4 cluster unit tests and 2 fresh sequential native lifetime tests pass.
- The original dependency gate was blocked by SVG/physics plugins. The approved
  isolation follow-up removes those dependencies from the active upgrade graph.
  Full visual/performance baseline refresh remains required before B20-1 is
  accepted. The subsequent 0.20 patch qualification is recorded below.

### Approved dependency isolation — 2026-10-09

- Replace `bevy_resvg` with `feathers/icon/svg.rs`: an Aestra-owned, UI-icon-only
  asset loader using engine-neutral resvg, shared native Bevy Image subassets and
  ImageNode tinting. Preserve 64px oversampling for current 8–28px controls, vector
  aspect ratio, transparent padding and antialiased straight-alpha edges. The SVG
  sources remain editable; asset reloads update the shared raster, not every entity.
  Text/font and external-image rendering are intentionally outside this icon boundary.
- Automation curves import tiny-skia directly, no longer through a Bevy plugin.
- Explicitly exclude Avian/Rapier from the active workspace and ordinary compilation
  CI, preserving their source and independent manifests. Formatting remains checked.
  This suspends adapter availability, not Aestra's collision contract or native GPU
  collision implementation. [Restoration procedure](../../bevy/PHYSICS_ADAPTERS.md).
- Keep Bevy 0.19.1 and both existing local fixes during this preparatory change.
  Re-run the isolated 0.20 resolution probe; a clean graph is not source compatibility
  or permission to remove the input-focus/cluster patches.
- Verified: active 15-member manifests resolve a single 0.20 engine graph;
  workspace check/strict Clippy and formatting pass; 1,168 editor tests and 3 editor
  architecture tests pass (10 existing opt-in tests remain ignored). Both standalone
  adapters type-check on 0.19. See the [follow-up report](../../benchmarks/bevy-upgrade/dependency-isolation-2026-10-09.md)
  for exact scope and remaining native/high-DPI validation.

### Bevy 0.20 patch qualification — 2026-10-09

- Import actual published 0.20.0 sources into `vendor/bevy_input_focus_020` and
  `vendor/bevy_pbr_020`, preserving licenses/assets and recording original file
  hashes and archive checksums. Port only keyboard dispatch and cluster ownership
  fixes; keep upstream WESL, extraction, specialization constants and light-texture
  decal-count changes. Handle wgpu 30's fallible mapped ranges without leaking
  pending readback ownership.
- `benchmarks/bevy-upgrade/patches-020` is a real-source, independently locked
  workspace. Its patch table selects both candidates; the shipping workspace still
  selects its unchanged 0.19 patches. No registry source is edited and no old crate
  is relabelled. `run-020-patches.ps1` checks provenance, graph selection, stable
  source/binary hashes and retains separate native logs.
- Verified on Rust 1.98.1 MSVC: nine keyboard tests (including actual 0.20 text
  input), 40 upstream focus tests, four cluster unit tests, and both sequential
  native GPU lifetime tests. NVIDIA RTX 4070 SUPER, Vulkan, driver 616.92.
  The overflow fixture recovers 32/64 initial capacities to 512/8192, then retains
  generations for 240 updates. Named private/staging allocations retire to zero
  on CPU-mode switch and final camera removal; observed staging peak is two/view
  against the enforced eight-slot ceiling.
- This qualifies the candidate dependency fixes, not the migrated editor or
  fireworks visuals/performance. Native qualification uses a minimal headless,
  non-pipelined host; production scheduling and feature combinations remain part
  of B20-3. See the [qualification report](../../benchmarks/bevy-upgrade/patch-qualification-2026-10-09.md).

## B20-1 — Renderer, shader and GPU compatibility

This is the largest risk, not the Cargo version edit.

Bevy removes its naga_oil shader dialect in favor of WESL; ordinary WGSL remains
supported. Convert Bevy-importing shaders and conditionals, remove the obsolete
`shader_format_wesl` feature, and verify embedded module paths and specialization
constants. See the [WESL migration](https://github.com/bevyengine/bevy/pull/25088).

| Aestra hotspot | Work and evidence required |
| --- | --- |
| `bevy/aestra-bevy-render/src/gpu/volume.rs` | Port inline Bevy imports/conditional prepass code; validate volume depth, lighting and alpha on GPU. |
| `bevy/aestra-bevy-render/src/gpu/material_lighting.rs` | Port generated lighting composition; test bindings and lit/unlit variants, not only source-string snapshots. |
| `apps/aestra-editor/src/feathers/shaders/` | Port graph grid/wire shaders; verify graph clipping, zoom and antialiasing. |
| `crates/aestra-gpu/src/shader.rs`, material lowering and fluid kernels | Upgrade compiler APIs without making portable kernels depend on Bevy's shader modules. Keep portable WGSL output and target validation. |
| `bevy/aestra-bevy-render/src/gpu.rs`, `extension_stages.rs`, particle lights | Port extraction, resource ownership and cleanup while preserving per-instance isolation. |
| `bevy/aestra-bevy-render/src/gpu/render.rs` and benchmark GPU code | Audit custom render phases, pipeline layouts, indirect draws, depth/MSAA and raw wgpu calls. |
| Fireworks pipeline readiness handling | Preserve retryable-versus-fatal errors and readiness holds; never consume show time while pipelines are unavailable. |

Extraction is now reusable across app worlds through `bevy_extract`; migrate derives,
manual implementations and render-app labels explicitly. Follow
[extraction changes](https://github.com/bevyengine/bevy/pull/22852) and
[temporary entity changes](https://github.com/bevyengine/bevy/pull/24419).

Weak ordering changes also require a dependency audit. ECS access conflicts do not
express all dependencies through GPU queues, interior mutability or asynchronous
channels. Preserve explicit order for simulation, upload, extraction, prepare,
readback and rendering where needed. Do not mechanically replace strong chains with
weak ones. See [weak ordering](https://github.com/bevyengine/bevy/pull/25128).

Additional compatibility checklist from the
[migration guide](https://bevy.org/learn/migration-guides/0-19-to-0-20/):
`WgpuWrapper` removal, depth/stencil attachment changes, typed `ShaderBuffer`, bind-group
builders and view compositing APIs. `Tonemapping::None` becomes passthrough;
`Linear` preserves its former behavior. Audit every visual oracle's intent.

Acceptance:

- Shader contracts validate on all currently supported portable targets, including
  negative/error cases and authored external shader assets.
- GPU buffer layouts, event routing, distance emission, trails/ribbons, culling,
  fluid coupling, lighting and transformed/nested instances retain their contracts.
- Native 2D/3D, HDR/LDR, MSAA/depth and multiple-camera cases render correctly.
- Shader/compiler cache fingerprints invalidate incompatible generated artifacts.
  Do not change the authored effect schema merely because the backend changed.
- No production GPU waits, CPU particle mirroring, replay requirement or per-particle
  ECS entities are introduced. PlaybackOnly remains the scalable game default.
- Investigate image differences before updating references; never mass-approve them.

### B20-1 first slice — Portable compiler

- Direct compiler dependencies now pin WESL 0.6.0 and Naga 30.0.1. Port virtual
  composition to the new `Compiler` API, preserving public-by-default authored
  helpers, entry points and error sources. No portable shader imports Bevy.
- Compiler identity participates in material/pipeline fingerprints and thumbnail
  disk-cache keys, independently of authored schema and buffer ABI versions.
- Add import/shadowing and negative shader regressions. The reviewed generated
  simulation diff is three redundant pairs of `while` parentheses; sprite output
  is unchanged. Native testing also exposed and repaired stale nine-word fluid
  fixtures to use the existing ten-word state ABI constant, without weakening tests.
- The shipping engine remains 0.19.1/wgpu 29/glam 0.32. Its internal Naga/WESL
  versions temporarily coexist with the independent compiler; only WGSL strings
  cross the boundary. This is not acceptance of a mixed ECS/render engine graph.
  See the [implementation and validation report](../../benchmarks/bevy-upgrade/portable-compiler-2026-10-09.md).
- Verified on pinned Rust/MSVC: 105 portable GPU tests, 34 fluid contracts, 46
  hardware-required fluid tests, 10 thumbnail-cache tests, 94 project tests and
  three GPU/CPU conformance tests pass. Seven existing fluid benchmarks/soaks stay
  ignored. Workspace check/strict Clippy, formatting and refreshed resolution pass.

### B20-1 second slice — Bevy shader composition

- Extract the production volume/material composition into a shared, engine-neutral
  source module. Keep the shipping 0.19 dialect unchanged; the candidate 0.20
  dialect stays test-only until the coherent engine switch selects `Shader::from_wesl`.
- Prepare real WESL imports (`bevy_pbr::render::*`, `bevy_pbr::prepass::utils`),
  `@if` depth/lighting branches and `constants::MATERIAL_BIND_GROUP`, sharing
  ray setup, plugin marchers and bounded clustered-light helpers between dialects.
- Qualify through actual Bevy 0.20 `ShaderCache` and the locked graph's published
  shader libraries, not substitute View/cluster/UI definitions. Cover fire/smoke
  and liquid volumes, depth/MSAA and storage/uniform variants, generated lit smoke
  with neutral 2D fallback, and unchanged editor graph bodies using
  `bevy_ui_render::ui_vertex_output::UiVertexOutput`.
- Add explicit, serial native wgpu 30 module/render-pipeline validation to the
  migration runner. Inferred layouts and a fixture UI vertex establish shader
  compilation/linkage, **not** the real Bevy Material/UiMaterial host layouts,
  editor interaction, visual parity or performance. Record shader inputs and binary
  hashes alongside the existing patch qualification.
- See [implementation and checks](../../benchmarks/bevy-upgrade/shader-composition-2026-10-09.md).
  This prepares the shader switch; it does not claim the editor already runs 0.20.

### B20-1 third slice — Storage-buffer upload and asset extraction

- Route the renderer's eleven storage-buffer constructors, four update sites,
  indirect usage and CPU-byte access through `gpu/storage_buffers.rs`. The current
  0.19 adapter preserves metadata and encoded bytes; construction now moves the
  encoded vector into the asset instead of cloning it a second time.
- Share `storage_encoding.rs` with the real 0.20 qualification. Preserve encase's
  WGSL layout for emitters, renderers, particles, globals, draw parameters and word
  arrays. No Rust-memory casts, particle ABI or authored-schema change.
- Compile the candidate `storage_buffers_020.rs` against published 0.20's owned
  byte-vector API. Cover extraction draining CPU data, reinitializing after drain,
  repeated updates before extraction, and grow/shrink without concatenating frames.
- Exercise real `GpuShaderBuffer::take_gpu_data` / `prepare_asset` on a hardware
  Vulkan GPU: byte-for-byte uploads, same-size identity reuse, size/label/usage
  invalidation via `RenderChangedShaderBuffers`, indirect usage and GPU-only resize
  preservation/zero-filled tail. Native waits/readback are test-only.
- Extend the serial qualification runner and source/binary hashing. **64** candidate
  checks pass, including four explicit native tests; current renderer regressions
  pass **106** tests with three pre-existing native tests ignored. Workspace check
  and strict Clippy pass. See the
  [implementation and evidence](../../benchmarks/bevy-upgrade/storage-buffers-2026-10-09.md).

This qualifies the storage asset boundary, not the full render-world extraction
schedule, custom bind-group refresh or editor on 0.20. The candidate adapter is
selected only at the coherent engine switch; the shipping engine remains 0.19.1.

### B20-1 fourth slice — App-labeled draw and optional world extraction

- Move the actual `GpuDrawInstance`, semantic binding, wireframe geometry and
  sprite-cull bounds into `gpu/draw_instance.rs`; move `AestraWorldSdf` into
  `gpu/world_sdf.rs`, preserving its public reexports and packed Arc ownership.
  No substitute fixture components or per-particle entities.
- Keep the shipping `gpu/extraction.rs` adapter and separately compile the real
  0.20 `gpu/extraction_020.rs` with explicitly `RenderApp`-labeled traits/plugins.
  Share the visibility policy and transform/bounds update. Hidden draws return
  `None` so Bevy removes previous data; a visibility-filter shortcut would skip it.
- Add explicit render-resource cleanup when the host removes its optional world
  SDF. Upstream resource extraction copies changes/presence but retains an old
  render copy after main-world removal. Required renderer resources are unaffected.
- Run identical contracts through actual 0.19/0.20 extraction and render schedules:
  deferred commands visible before prepare, all draw fields, shared allocations,
  current transform/bounds, hide/restore, component removal/reinsertion, independent
  owners, despawn and recycled entity generation, SDF replace/clear/remove/reinsert.
  The candidate also checks actual mesh inputs and safe component removal with no
  render app. **73** candidate checks, **112** shipping renderer regressions and
  **3** hardware-required GPU/CPU conformance checks pass. Workspace check/strict
  Clippy and candidate extraction strict Clippy pass. See
  [implementation and evidence](../../benchmarks/bevy-upgrade/extraction-2026-10-09.md).

### B20-1 fifth slice — Effect, domain and light extraction metadata

- Move the complete production effect-buffer/stateful descriptors, domain/field
  targets, light selection inputs/source identity and readback settings/frame into
  small shared modules. Preserve public reexports, handle/Arc ownership and all
  runtime preparation/execution behavior; no stand-in extraction components.
- Extend both extraction adapters with the same clone policy. The candidate uses
  explicit `RenderApp`-labeled component/resource traits and plugins; the shipping
  graph still selects only the 0.19 adapter. Optional light readback registration
  stays with its plugin, rather than becoming an always-enabled transport.
- Share four additional real-schedule contracts: every effect/dispatch field,
  PlaybackOnly/replay policy and epoch updates, stage targets/SDF removal, shared
  ownership, per-owner component removal/reinsertion/despawn, light artifact and
  clip-path identity, empty selection, budget/mode updates and frame wrap values.
  Required resources are disabled through their settings, not removed as if they
  were optional world geometry.
- Keep the trail checkpoint context independent of GPU allocation code so the
  extraction fixture shares the actual type without importing a mock renderer.
  **77** candidate checks, **116** shipping renderer regressions and **3**
  hardware-required GPU/CPU conformance checks pass; workspace check/strict Clippy,
  candidate extraction strict Clippy and formatting checks pass.
  See [implementation and evidence](../../benchmarks/bevy-upgrade/state-extraction-2026-10-09.md).

### B20-1 sixth slice — Renderer settings, pacing and device publication

- Share actual renderer settings/defaults and adaptive pacing through both engine
  adapters; the 0.20 candidate uses app-labeled resource extraction and its real
  `Extract`/`MainWorld` paths. Preserve public APIs and all existing pacing limits.
- Share actual device discovery and backend publication. Test settings recovery,
  optional pacing removal/toggles without resetting history, backend/device changes
  and unchanged-frame resource change tracking. Keep backend fallback policy intact.
- Add a fifth serial native qualification process: real Vulkan/Bevy resources and
  the actual 0.20 extraction/publication system. This is not a draw or pipelined
  renderer/visual gate. No GPU wait, particle mirror or replay requirement is added.
- **93** candidate checks, **146** shipping renderer lib regressions and **3**
  hardware-required GPU/CPU conformance checks pass, alongside workspace check,
  strict Clippy, formatting and runner syntax checks. See
  [implementation and evidence](../../benchmarks/bevy-upgrade/control-extraction-2026-10-09.md).

### B20-1 seventh slice — Per-view visibility and retained draw phases

- Share production visibility-class selection, sampled-sprite retained-entry
  removal, transparent sorting and 2D/3D phase-item construction. Both shipping
  queue systems use the constructors; preserve entity IDs, indexed mode, GPU
  instancing metadata and renderer-order bias. Use each adapter's `MainEntity`.
- Compile all seven actual sprite-culling contracts against 0.20, using Bevy's
  camera projection API without deprecated math calls or lint suppression. Move
  the three existing phase regressions with their implementation; add four shared
  contracts for metadata, retained updates/isolated removal, camera-dependent sort
  and CPU/GPU visibility lists.
- **107** candidate checks, **150** shipping renderer lib regressions and **3**
  hardware-required GPU/CPU conformance checks pass, alongside workspace check,
  strict Clippy, formatting and runner syntax checks. Existing native processes
  remain serial; this slice does not qualify full queues, custom draw commands,
  pipeline specialization, mesh/prepass bindings or visual parity. See
  [implementation and evidence](../../benchmarks/bevy-upgrade/view-phases-2026-10-09.md).

### B20-1 eighth slice — Production pipeline specialization and explicit layouts

- Share the actual sprite/mesh specializer, semantic draw keys, depth layouts and
  material ABI translator; the shipping renderer uses these modules unchanged in
  policy. Retain the public material layout API and existing regressions.
- Default 0.20's new per-stage pipeline-constant fields without changing the
  shipping graph. Enable the actual 2D render plugin in the independent fixture.
- Add draw-key separation and MSAA/deformed-wireframe policy contracts. A sixth
  serial native check validates **192** descriptor requests with Aestra's explicit
  layouts through real Bevy PipelineCache: 2D/3D, SDR/HDR, MSAA, lighting, depth fade,
  blend and diagnostic paths. Shader composition uses real locked WESL libraries;
  flat WGSL feeds PipelineCache without inferred layouts or stand-in view types.
- **113** candidate checks, **150** shipping renderer lib regressions and **3**
  hardware-required GPU/CPU conformance checks pass; workspace check, strict Clippy and fixture lint,
  formatting and runner syntax checks pass. This is pipeline compilation/layout
  evidence, not draw submission or visual parity. Semantic triangle-list meshes,
  draw-command bodies and actual mesh/prepass/view binding integration remain.
  See [implementation and evidence](../../benchmarks/bevy-upgrade/pipeline-specialization-2026-10-09.md).

### B20-1 ninth slice — Actual draw commands, mesh and prepass bindings

- Share the production command tuples, components, preparation systems and
  producer-owned sort/compaction/culling/submission records. Shipping compute
  producers and render consumers use the same records; preserve dispatch gates,
  buffer/indirect ownership, fallback precedence and bounded diagnostics.
- Keep shipping queue installation/order unchanged. Isolate the depth prepass
  accessor rename in 0.19/0.20 adapters, and share the existing material sampler
  translator. No authored/GPU ABI change or new production wait/readback.
- A seventh serial native test executes all five real registered command tuples
  through TrackedRenderPass with actual view groups, prepasses and mesh allocations.
  **18 draws** cover 2D, 3D 1x/4x MSAA, semantic/legacy/depth-fade sprites,
  indexed/nonindexed deformed meshes, deformed wireframes, authored lit smoke and
  a fixture-textured smoke variant. Use the real ViewKeyCache rather than guessing
  view flags. Validate texture/sampler/uniform binding layouts, not pixels.
- Test sort readiness skips, view-cull > compact > direct fallback by BufferId,
  missing bindings/mesh skips, unchanged mesh reuse and stale geometry/depth cleanup.
  Synthetic completed producer records qualify routing, not compute dispatch.
  Existing wireframe regressions also run on 0.20. **120** candidate checks and
  **150** shipping renderer lib regressions and **3** hardware-required GPU/CPU
  conformance checks pass;
  workspace/fixture strict Clippy, formatting and runner syntax checks pass.
  See [implementation and evidence](../../benchmarks/bevy-upgrade/draw-bindings-2026-10-10.md).

### B20-1 tenth slice — Installed queues and retained-phase lifecycle

- Compile the actual `gpu/render.rs` installation and 2D/3D queues in the locked
  0.20 fixture. Share the producer ordering sets without changing shipping order,
  and preserve public embedded shader paths through reexports.
- An eighth serial native gate runs real extraction, ShaderBuffer/Image/Mesh
  asset preparation, visibility classes, RenderLayers, ViewKeyCache, pipeline
  initialization, queueing, bind-group preparation and the transparent render
  graph. Actual embedded WESL assets enter Bevy's AssetServer/PipelineCache;
  no flattened shader substitution or manual DrawFunctions invocation.
- Verify exact phase membership and submitted effect owners across 2D, 3D 1x/4x
  MSAA, semantic/legacy/depth sprites, deformed meshes and wireframes. Exercise
  material rejection, unavailable meshes, hide/show, layer changes, mesh-to-sprite
  transitions, restoration, draw despawn and view despawn.
- Reproduced and fixed stale retained entries on unsupported materials/meshes and
  per-view visibility loss; preserve unrelated phase commands and exact
  render/main identity. Mesh-to-sprite preparation now retires its old geometry
  and layout. These fixes are shared with shipping 0.19, not candidate-only copies.
- Producer-set markers qualify preparation ordering only. Mesh instance counts
  are fixture writes after preparation, not production compute dispatch. This
  gate does not qualify pipelined rendering, pixels, allocation/performance parity
  or the full simulation/sort/compaction/culling pipeline.
- **123** candidate checks (eight explicit serial native tests), **151** shipping
  renderer lib regressions and **3** hardware-required GPU/CPU conformance checks
  pass, as do workspace/candidate strict Clippy and formatting.
  Validation and evidence: [queue/schedule report](../../benchmarks/bevy-upgrade/queue-schedule-2026-10-10.md).

### B20-1 eleventh slice — Actual alpha compute to installed queues

- Import production `gpu/alpha_sort.rs` directly into the locked 0.20 fixture.
  Replace ambient imports with explicit dependencies; shared simulation/sort sets
  preserve shipping simulation-before-sort-before-draw ordering.
- A ninth serial native gate installs the real producer alongside the real
  extraction/render queues. Normal ShaderBuffer asset preparation, embedded WGSL
  loading, PipelineCache, preparation, classify/merge/finish dispatch and actual
  per-view draw bindings execute through `App::update`, not manual producer calls.
- Test-only GPU observation checks exact permutations for opposite cameras,
  camera movement, capacity 257/513, zero/partial/full populations, output reuse,
  same-capacity source replacement, growth, settings changes and teardown.
  Controlled dispatch gating before and after success verifies that prepared
  but undispatched draws skip, without consuming uninitialized/stale indices.
- A tenth serial native gate runs the existing 42-case sparse-pool contract
  unchanged in intent on wgpu 30, through capacity 65,537. A narrow test-only map
  adapter handles wgpu's fallible view API and the independent glam boundary.
- This does not run actual simulation or trail producers, asynchronous pipeline
  readiness, pipelined rendering, or visual/performance comparison. All GPU waits,
  copies and permutation readbacks are confined to tests; production buffer usage
  flags, allocation policy and playback/replay APIs remain unchanged.
- **127** candidate checks (ten serial native tests), **151** shipping renderer
  lib regressions, the shipping 42-case native alpha contract and **3** required
  GPU/CPU conformance checks pass. Workspace/candidate strict Clippy, formatting,
  runner syntax and source/executable immutability gates pass.
- Validation and evidence: [alpha compute report](../../benchmarks/bevy-upgrade/alpha-compute-2026-10-10.md).

### B20-1 twelfth slice — Actual trail producers to installed queues

- Import production trail compaction and per-view culling installers, preparation
  and compute dispatch directly into the locked candidate. Shared producer sets
  preserve simulation → compaction → culling → draw ordering.
- Extract the bounded timestamp transport and render-world preparation context,
  retaining public timing/profile APIs. Narrow 0.19/0.20 adapters handle GPU
  wrapper removal, RenderDevice construction and fallible mapped views.
  Array-based conversion preserves the neutral glam matrix/vector ABI.
- The native installed-producer contract checks owners 70/1,025, stable compacted
  candidate indices, two per-view indirect counts, exact consumed command buffers,
  output reuse/growth, source rebinding, camera motion, dispatch fallback combinations,
  stale-epoch fail-open, empty/expired histories and draw/view retirement.
- Timestamp ownership and bounded recycling are exercised on real hardware.
  Seeded history records are not actual simulation, controlled dispatch gates are
  not async shader-readiness qualification, and synchronous rendering does not
  establish pipelined, visual or performance parity. No production GPU waits,
  particle readbacks or per-particle ECS are introduced.
- **131** candidate checks (12 explicit serial native tests), **151** shipping
  renderer lib regressions and **5** hardware-required shipping GPU/CPU/trail
  conformance checks pass. Workspace/candidate strict Clippy, formatting,
  runner syntax and source/executable immutability gates pass.
- Validation and evidence: [trail compute report](../../benchmarks/bevy-upgrade/trail-compute-2026-10-10.md).

### B20-1 thirteenth slice — Analytic simulation to installed queues

- Extract production analytic pipeline setup, device gate, eight-binding layout,
  bind-group preparation and particle reset/simulate/ribbon dispatch into a shared
  module. Shipping keeps its observation ordering, timestamps and host scheduler;
  the candidate imports this actual code instead of duplicating the GPU dispatch.
- The native candidate starts with compiler-lowered authored inputs and zeroed
  particle storage. Actual simulation generates particles, compacted alive slots,
  counters and indirect commands before installed per-view sorting and drawing.
- Check five times (empty/live/expired), CPU-reference positions/size/age, exact
  counts/compaction, opposite-camera permutations, same-capacity source replacement,
  257→513-slot growth and effect/draw retirement under `PlaybackOnly`.
- All 13 analytic pipeline descriptors compile; trail/history entries are not
  dispatched by this slice. The test supplies time, not the production host
  scheduler: stateful/coupled simulation, actual trail-history scheduling and
  async/pipelined rendering remain open. No production readback or GPU wait is added.
- **132** candidate checks (13 explicit serial native tests) and **151** shipping
  renderer lib regressions plus **5** hardware-required shipping conformance
  checks pass, as do workspace/candidate strict Clippy,
  formatting, runner syntax and source/executable immutability gates.
- Validation and evidence: [analytic simulation report](../../benchmarks/bevy-upgrade/analytic-simulation-2026-10-10.md).

### B20-1 fourteenth slice — Actual trail-history simulation to installed queues

- Share the production history-recording block with the candidate, preserving
  update/paged/ribbon order, diagnostics, timestamps and missing-buffer handling.
  Import the actual paged dispatcher with explicit dependencies; no parallel
  implementation of history or page/merge scheduling is introduced.
- Start from compiler-lowered authored records and zeroed GPU history/aux storage.
  Actual analytic simulation and history dispatch feed installed compaction,
  per-view culling, bindings and transparent draw commands in the same frame.
- Exercise inline 70-owner and paged 1,025-owner pools, same-capacity source
  replacement and output growth. Check exact samples/owner identities/counters,
  repeated-time idempotence, world-space samples across emitter movement,
  retained tails after head death, partial/complete expiry and epoch resets.
  Verify compacted segment membership, near/far camera counts, exact consumed
  indirect buffers and effect/draw teardown under `PlaybackOnly`.
- The test controller supplies observation times and poses: this is shared
  dispatch integration, not qualification of the complete host scheduler,
  stateful/routed simulation, replay, async shader readiness or pipelined rendering.
  No shader changes, production readbacks/waits or public API changes are added.
- **133** candidate checks (14 explicit serial native tests), **151** shipping
  renderer lib regressions and **5** hardware-required shipping GPU/CPU/trail
  conformance checks pass. Workspace/candidate strict Clippy, formatting,
  runner syntax and source/executable immutability gates pass.
- Validation and evidence: [trail simulation report](../../benchmarks/bevy-upgrade/trail-simulation-2026-10-10.md).

### B20-1 fifteenth slice — Independent stateful simulation to installed queues

- Share persistent-state allocation/invalidation, pipeline setup, tick parameter
  packing, independent fixed-tick scheduling and presentation/compaction between
  shipping and candidate. Keep the public shader path and playback APIs unchanged.
- Start with compiler-authored records and empty persistent storage. Compare GPU
  particles with the CPU stateful reference, then verify installed alpha ordering
  and actual draw bindings for opposite cameras.
- Cover death/free-slot reuse, stop-emission within a batch, burst-once semantics,
  repeated-time idempotence, bounded preview catch-up, same-capacity reseeding,
  capacity growth, persistent allocation reuse and removal. `PlaybackOnly` retains
  no checkpoints. GPU waits/readbacks remain test-only.
- Fix the discovered independent batching bug: each encoded dispatch now receives
  its own tick, so bursts, cutoffs and recorded trace inputs do not reuse the
  segment's first tick. Correct the device gate to the actual twelve bindings.
- This is independent scheduler integration, not the complete host/coupled/routed
  scheduler, replay, asynchronous readiness, pipelined, visual or performance parity.
- **134** candidate checks (15 explicit serial native tests), **151** shipping
  renderer lib regressions and **36** hardware-required shipping conformance
  checks pass. Workspace/candidate strict Clippy, formatting, runner syntax and
  source/executable immutability gates pass.
- Validation and evidence: [stateful simulation report](../../benchmarks/bevy-upgrade/stateful-simulation-2026-10-10.md).

### B20-1 sixteenth slice — Coupled particle events/routes to installed queues

- Share the actual lockstep scheduler and stateful dispatcher: counter reset,
  event-link gathering/spawning, host-input bursts, output aggregation, pacing
  budget selection and statistics stamping. Keep the shipping trail observer
  behind a small history coordination interface, without changing its storage.
- Compile the actual stage executor under wgpu 30 using the existing mapped-view
  version boundary; keep shipping Bevy 0.19.1 and all public host APIs unchanged.
- Start with three compiler-authored persistent emitters and empty GPU state.
  Verify successive death-linked generations, supplied host-event positions,
  requested/dropped/accepted counts, output ticks/epochs and same-tick aggregation.
  Check exact opposite-camera permutations, ordinal ties and installed commands.
- Cover repeated-time idempotence, bounded preview catch-up, live/free counts,
  `PlaybackOnly` without snapshots and teardown. Readbacks/waits remain test-only.
- This qualifies coupled particle playback and lowered route records, not fluid
  stepping, joint trail history, full host scheduling/asynchronous output delivery,
  replay, async readiness, pipelined rendering, pixels or performance.
- **135** candidate checks (16 explicit serial native tests), **151** shipping
  renderer lib regressions and **36** hardware-required shipping conformance
  checks pass. Workspace/candidate strict Clippy, formatting, runner syntax and
  source/executable immutability gates pass.
- Validation and evidence: [coupled simulation report](../../benchmarks/bevy-upgrade/coupled-simulation-2026-10-10.md).

### B20-1 seventeenth slice — Fluid coupling and joint stateful trail history

- Path-import the actual stateful trail observer, GPU checkpoints, trail replay
  controller and historical pose uploads. Replace the observer's wildcard import
  with explicit dependencies; its algorithms and shipping behavior are unchanged.
- Compile the real fluid extension in the isolated candidate graph. A compiler-authored
  dense smoke domain drives persistent trail heads through Follow Field, using the
  shared production fixed-tick scheduler and stage executor.
- Verify fluid density/velocity and field-driven head motion, lockstep clocks,
  bounded skipped-frame catch-up, pause idempotence, dead-head tail retention and
  expiry, inline/paged history at 70/1,025 owners, source replacement and teardown.
  Installed per-view queues consume the actual GPU-written culling buffers.
- `PlaybackOnly` keeps particle/domain/trail checkpoint storage empty. Optional
  replay captures a common tick-60 checkpoint and restores bit-identical persistent
  history and fluid fields after an epoch change. Compare persistent aux metadata,
  not paged sort/scan scratch that is intentionally rebuilt on paused frames.
- Add a seventeenth explicit serial native gate and hash the full fluid extension,
  historical pose code and replay controller in immutable qualification evidence.
- **142** candidate checks (17 explicit serial native tests), **151** shipping
  renderer lib regressions and **36** hardware-required shipping conformance
  checks pass. Workspace/candidate strict Clippy, formatting, runner syntax and
  source/executable immutability gates pass.
- Validation and limitations: [fluid/joint-history report](../../benchmarks/bevy-upgrade/coupled-trails-2026-10-10.md).

### B20-1 eighteenth slice — Real domain births, trail queues and output rings

- Extend the shared headless renderer fixture with a compiler-authored dense smoke
  domain, Secondary Emission and Spawn From Domain. All particle and emission
  storage starts empty; the actual solver supplies every birth record.
- Exercise the production lockstep scheduler, domain spawner, persistent particle
  state, joint trail observer, compaction/culling and installed draw queues. Check
  the exact accepted emission prefix, inherited velocity and slot reuse.
- Saturate a 16-record emission list against ten destination slots. OnSpawn output
  aggregation reports only the ten accepted births and their lowest ordinals;
  rejected births and paused frames export nothing new. Test-only reads inspect
  the actual GPU ring, not a replacement host-delivery implementation.
- Playback-only has no checkpoints. Optional replay captures a common tick-60
  checkpoint and restores identical domain fields and persistent particle/trail
  history without duplicate output records. Add an eighteenth explicit serial gate.
- **143** candidate checks, **151** shipping renderer lib regressions and **40**
  hardware-required conformance checks pass. Workspace/candidate strict Clippy,
  formatting, runner syntax and immutable source/executable evidence pass.
- Validation and boundaries: [domain-birth report](../../benchmarks/bevy-upgrade/domain-spawn-2026-10-10.md).

### B20-1 nineteenth slice — Actual asynchronous particle-output delivery

- Move the existing presentation payload/material binding implementation and
  asynchronous counter/output observer into shared modules. Keep the public
  renderer exports and observer behavior; path-import these actual modules in
  the candidate instead of substituting host components or a delivery algorithm.
- Feed real dense-smoke domain births through Bevy's `Readback::buffer`, actual
  `ReadbackComplete` observer and `AestraOutputEvent` message stream. Delay the
  first read across ring wrap; verify retained ticks arrive once in tick order,
  with exact local payload/count and routed root/clip/epoch identity.
- Check source-tick inherited motion/root time, world placement, pause deduplication,
  stale epochs, a fresh restart cohort, seek-reconstruction suppression, resumed
  live output and removal/despawn handling. No manual completion triggers or
  synchronous readbacks drive delivery. Playback-only remains checkpoint-free.
- Add a nineteenth explicit serial native gate and hash all newly shared source.
  Full host clock scheduling is still fixture-controlled, and the existing
  32-tick output ring does not guarantee lossless delivery after arbitrary stalls.
- **155** candidate checks, **151** shipping renderer lib regressions and **40**
  hardware-required conformance checks pass. Workspace/candidate strict Clippy,
  formatting, runner syntax and immutable source/executable evidence pass.
- Validation and limits: [asynchronous output report](../../benchmarks/bevy-upgrade/async-host-outputs-2026-10-10.md).

### B20-1 twentieth slice — Asynchronous extension-stage outputs

- Share the actual stage-output callback wiring, mailbox receiver, latest-values
  component and finished watcher between shipping and the isolated candidate.
  Keep public exports and move the existing impact/finished unit coverage too.
- Fix a deferred-insertion bug exposed by first-completion batches: merge sibling
  stage values and share one pending edge tracker per owner. Cover repeated reads
  and independently missing components without resetting surviving tracker state.
- Add a twentieth serial native gate with actual compiled fluid pressure/force
  producers. Two-stage force values and impact messages reach the host through
  asynchronous executor callbacks; check owner isolation, output-less solvers,
  copy-and-clear, paused read gating, removal and checkpoint-free playback-only.
- Full host scheduling is still fixture-controlled. The legacy stage mailbox
  lacks artifact/epoch identity and out-of-order rejection; nested stage routing
  is not qualified. Keep these explicit in the full host lifecycle gate.
- **159** candidate checks, **152** shipping renderer lib regressions and **40**
  hardware-required conformance checks pass. Workspace/candidate strict Clippy,
  formatting, runner syntax and immutable source/executable evidence pass.
- Validation and boundaries: [stage-output report](../../benchmarks/bevy-upgrade/stage-outputs-2026-10-10.md).

### B20-1 twenty-first slice — Stage callback identity and ordering safety

- Stamp actual executor callbacks at submission with presentation/artifact identity,
  seed, playback epoch, history revision and host-input epoch. A replacement
  `PresentedEffect` gets a distinct token even when it reuses the same artifact;
  ordinary extraction clones preserve that token.
- Reject mismatched callbacks before decoding or changing tracker state. Clear
  previous-context latest values on the next receiver pass, including sibling
  stages without a fresh result. Reset edge/high-water state for the new context.
- Sort each drain by tick, then reject duplicate/older ticks independently per
  stage. Include context in the production read gate so a fresh context can read
  the same paused tick. Keep the earlier first-batch merge/repair behavior.
- Extend the existing serial native stage-output gate: hold actual queued fluid
  callbacks across restart and same-artifact presentation replacement; reject
  them, accept same-tick fresh reads and raise later live impacts again. No mock
  completions, production GPU waits or replay requirement are introduced.
- Shared regressions also cover seeks, revision invalidation, upper seed bits,
  independently changing extraction identity fields and reordered completions.
  Full stage reset/reconstruction scheduling, seek-event suppression and nested
  stage routing remain separate from this delivery contract.
- **162** candidate checks (including the existing **20** serial native gates),
  **155** shipping renderer lib regressions and **40** hardware-required
  conformance checks pass. Workspace/candidate strict Clippy, all-target workspace
  check, both formatting checks, runner syntax and immutable evidence pass.
- Validation and limits: [stage lifecycle report](../../benchmarks/bevy-upgrade/stage-output-lifecycle-2026-10-10.md).

### B20-1 twenty-second slice — Canonical root clock and stage reset bridge

- Share the actual host player clock/presentation systems and stage runtime
  preparer with the isolated candidate. Derive stage inputs through one production
  builder, including the history discontinuity boundary. Public host API exports
  stay stable; clock advancement no longer requires profiling/status components.
- Reset compatible stage timelines after restart, explicit simulation revision
  changes and presentation replacement. Detect restart even when its first live
  frame jumps beyond the old tick; preserve compatible seeks and constant-only
  edits. Host rebinds invalidate checkpoints on both compatible paths.
- Keep reconstructed latest force values, but silence gameplay impacts through
  the seek boundary and first mixed aggregate. Subsequent wholly live reads can
  raise events. Do not add GPU waits, extra readbacks or mandatory replay history.
- Add a twenty-first explicit serial native gate driven by the actual host clock,
  extraction and stage preparation. Verify bounded reconstruction, large-first-frame
  restart, forward/backward seek suppression, live resumption, context/presentation
  resets, zero playback-only checkpoints and teardown.
- This is the root clock-to-stage bridge, not the entire host plugin: nested clips,
  binding capture/forwarding, volume/profile preparation and the outer stage graph
  scheduler still need combined 0.20 qualification. Native pixels/performance,
  asynchronous readiness and pipelining remain separate acceptance gates.
- **165** candidate checks (including **21** serial native gates), **61** shipping
  host-library tests, **157** renderer-library regressions and **40** hardware-required
  conformance checks pass. Workspace/candidate strict Clippy, workspace all-target
  check, both formatting checks, runner syntax and final immutable evidence pass.
- Implementation and validation: [host scheduling report](../../benchmarks/bevy-upgrade/host-stage-scheduling-2026-10-10.md).

### B20-1 twenty-third slice — Nested project and live binding bridge

- Compile the canonical project reconciliation and host-binding systems against
  the isolated candidate, including their shared regressions. Reuse the actual
  public scheduling sets and input-before-playback/deferred ordering; preserve
  host API re-exports. Add the candidate's explicit `aestra-project` dev dependency.
- Reset surviving offset children on a root restart even when the first new frame
  requests a later tick than the preceding simulation. Preserve compatible child
  entities, positive-seek revisions and source-time output-suppression boundaries.
- Forward per-slot entity identity, not only snapshot values. Equal-value retargets
  now rebind each forwarded level; normal movement does not relatch snapshot-on-spawn
  slots or invalidate host-input history. Shared fixes also apply to shipping 0.19.
- Add a twenty-second serial native gate: two roots sharing a two-level project,
  composed source offsets, independent clocks, actual host-binding upload and GPU
  fluid-force consumption, seed/parameter/layer inheritance, nested cue root/path
  routing, seek suppression/live resumption, retarget/motion, restart and retirement.
  Playback-only keeps zero checkpoint bytes; no production waits are added.
- This qualifies combined project/live-binding scheduling, not the complete host
  plugin, volume/profile preparation, native trace-driven projects, input-event
  adapters or production outer graph scheduler. Nested stage impacts retain their
  legacy child-local contract; choreography routing is the root/path oracle here.
- **178** candidate checks (including **22** serial native gates), **63** shipping
  host-library tests, **157** renderer-library regressions and **40** hardware-required
  conformance checks pass. Workspace/candidate strict Clippy, all-target workspace
  check, both formatting checks, runner syntax and final immutable evidence pass.
- Implementation and validation: [project/host scheduling report](../../benchmarks/bevy-upgrade/project-host-scheduling-2026-10-10.md).

### B20-1 twenty-fourth slice — Async readiness and pipelined host outputs

- Add an opt-in threaded candidate feature and separate hashed native executable.
  Bevy's async cache needs `multi_threaded`; merely disabling synchronous
  compilation in the old default configuration would not qualify it. Retain
  all existing synchronous gates and serial GPU-process execution.
- Withhold the actual stateful pipeline descriptors' shader dependency for eight
  frames at requested tick 30. Verify no ticks, spawn carry or ordinals are
  consumed and allocations survive. Publish the real generated shader, observe
  asynchronous `Creating`, and compare resumed GPU particles with the CPU oracle.
  Reuse the complete stateful/alpha/draw integration assertions, including
  repeated-time, stop, burst-once, seed/capacity invalidation and retirement.
- Install Bevy's actual pipelined-rendering plugin: `RenderApp` moves off-thread,
  real channels exist and context-stamped test telemetry records a distinct
  graph thread. Reuse canonical nested host/project/binding scheduling and actual
  GPU stages/output transport for two independent roots. Check restart, nested
  cues, seek suppression, single live resumption, paused deduplication and teardown.
- These complementary gates do not qualify the complete host plugin or production
  outer graph. The particle gate retains local readback inspection; the threaded
  stage gate retains its bounded driver and raw-wgpu stage pipeline creation.
  No production synchronization, GPU waits, replay requirement or art changes.
- **180** candidate checks (including **24** serial native gates) pass with
  source-frozen evidence. Both candidate strict Clippy configurations, workspace
  all-target check/strict Clippy, formatting and runner syntax checks pass.
  Shipping host **63**, renderer **157** and hardware conformance **40** tests pass.
- Implementation and validation: [async/pipelined scheduling report](../../benchmarks/bevy-upgrade/async-pipelined-scheduling-2026-10-10.md).

Remaining B20-1: full-plugin/outer-graph integration (including combined async
readiness across coupled/volume/profiling branches),
B20-2's editor compatibility changes, coherent Bevy/wgpu/glam switch selecting the
prepared shader/storage/extraction adapters, renderer/cache identity review, and
native visual/performance parity. The shipping workspace still uses Bevy 0.19.1.

## B20-2 — Editor compatibility and theme

Required API migration is distinct from adopting new controls. The
[migration guide](https://bevy.org/learn/migration-guides/0-19-to-0-20/) changes pointer
events to flat types, lifecycle observer generics, BSN scene/list syntax, text-input
composition, text scrolling, cursor imports and interaction state. Escape now
propagates after text focus release; color-plane vertical values increase upward.
Contextual theme tokens replace the former theme-property layout.

Prioritize `feathers/scenes.rs`, `theme.rs`, `input.rs`, text/code editors, material
graph, docking, timeline and gizmos. Migrate deprecated interaction APIs rather than
suppressing warnings under strict Clippy.

Keep current colors and geometry initially. Then map Aestra's semantic surfaces to
the new contextual theme: panel, raised control, popup, selected, hovered, pressed,
disabled, warning and error. Preserve menu hover contrast, right-aligned shortcuts,
purple accents and transform-axis colors. Test dark surfaces and disabled text together.

Acceptance: project chooser → create project → create effect → edit → save → reopen;
settings/recovery compatibility; keyboard editing/shortcuts; popup focus/escape;
graph/timeline drags; docking restore; responsive layout and supported locales.

## B20-3 — Compatibility release gate

B20-0 through B20-2 form the minimum upgrade. Merge only with complete workspace
validation and comparable native evidence. Optional adoption below can ship separately.

Keep the current CI structure, pinned toolchain, shared dependency caches and sccache.
The new dependency graph will have an unavoidable cold compile. Preserve the common
GPU feature set instead of restoring duplicate workspace/benchmark test invocations.

Core commands, run from the repository root:

```powershell
cargo fmt --all -- --check
cargo check --workspace --all-targets --features aestra-bench/gpu --locked
cargo clippy --workspace --all-targets --features aestra-bench/gpu --locked -- -D warnings
cargo test --workspace --features aestra-bench/gpu --locked
cargo check -p aestra-bevy --examples --features fireworks-audio --locked
cargo check -p aestra-editor --features dev-dynamic-linking --locked
cargo check --release -p aestra-editor --locked
cargo test --locked -p aestra-gpu --test architecture --test shader_contract
```

Also run the opt-in native GPU/conformance suites and fireworks allocation/lifetime
runners explicitly, **sequentially** on suitable hardware. Ordinary workspace tests
do not execute every ignored GPU test. Exercise the showcase with and without audio,
PlaybackOnly and replay-enabled policy, asset loading failures and repeated teardown.
Retain the Linux engine-neutral checks. Do not use `--all-features` as a substitute
for intentional static/dynamic and capability configurations.

## B20-4 — Replace numeric interaction first

Upstream [number-input scrubbing](https://github.com/bevyengine/bevy/pull/24636) covers
dragging, numeric formats, limits and sensitivity. Programmatic value synchronization
also changes to `NumberInputValue` in 0.20. This is the best first adoption pilot.

Target `feathers/number_input.rs` and `number_input/arrows.rs`. Keep a thin Aestra
adapter for semantic changes, formatting and visual policy; remove duplicated scrub
observers only after upstream behavior passes the same tests. Never run both drag
implementations on one field.

Parity gate:

- Click-to-edit, keyboard commit/cancel, external value updates, precision modifiers,
  hard/soft limits, integer fidelity and f32/f64 behavior.
- Unit-aware values, degrees/radians, timing fields, constraints and `CustomNumberStep`.
- One undo operation per gesture; no-op/cancel does not dirty the document or restart
  the preview. Live preview updates remain distinct from final commits.
- Blender-style arrows, readable size and contrasting hover backgrounds; reserved
  space prevents layout movement, hover oscillation and property-panel flicker.
- No focus/selection loss when scrubbing or navigating between vector components.

Verify the **released** widget before deciding whether arrow/unit decoration is still
needed. Early development PR TODOs are not the final API contract. Avoid depending
on private upstream child hierarchy. Roll out from a simple settings field to
transforms, timeline and material properties, then remove redundant code.

## B20-5 — Menus, dropdowns and color controls

| Current boundary | Proposed upstream reuse | Aestra policy to retain |
| --- | --- | --- |
| `combo_box` | Dropdown selection | Localized labels, selected semantic value, disabled options and command dispatch. |
| `context_menu`, popup menu composition | Lazy menus | Pointer anchoring, viewport bounds, submenus, action availability, menu width and shortcut columns. |
| `color_picker` | Color input/popup and reusable color controls | Automatic/None reset, RGB/HSL/hex editing, alpha preview, live/final undo semantics and color-space conversion. |
| `text_input`, `search_field` | Continue upstream editing composition | Validation, clear action, accessibility labels and domain-specific commit behavior. |

The [new menu widgets](https://github.com/bevyengine/bevy/pull/24784) support creating
menu content on demand, rather than retaining closed popup content. The
[color-input work](https://github.com/bevyengine/bevy/pull/25446) provides a compact
entry point to a popup editor. These are opportunities to shrink wrappers, not reasons
to discard Aestra's application behavior.

Test keyboard navigation, focus restoration, click-away/Escape, disabled items and
stale actions after document changes. Audit sRGB versus linear values and HDR material
ranges explicitly before replacing color controls; a normalized swatch is not an HDR
emission editor. Preserve the recent menu contrast/alignment improvements.

## B20-6 — Lists, tabs and viewport: selective reuse

- Pilot the new scrollable list on recent projects or a simple settings selection.
  Do not assume it provides virtualization, asset grids, tree navigation or multi-select
  drag/drop. Keep persisted scroll behavior and overflow-only scrollbars.
- Use [headless tabs](https://github.com/bevyengine/bevy/pull/25515) for focus,
  keyboard navigation and selection where compatible. Keep Aestra's docking model,
  drag/reorder/close behavior, dirty indicators and layout persistence.
- Evaluate `PanOrbitCamera` behind the existing viewport boundary. Preserve the
  user's established RMB orbit, MMB pan and WASD fly without RMB, plus hover/focus
  gating and gizmo capture. No persistent navigation hints. Do not replace fly
  behavior simply because an orbit controller is available upstream.

These are follow-ups, not requirements for a working 0.20 release.

## Other new features: where they help, and where they do not

The following are proposed Aestra applications of features described in the
[release announcement](https://bevy.org/news/bevy-0-20/), not measured gains yet.

| Opportunity | Recommended use |
| --- | --- |
| Retained UI / change-tracking improvements | Profile inspector, graph and asset-browser CPU cost. Keep entities stable and avoid unnecessary whole-panel rebuilds to benefit. |
| FixedNode, em/rem, inline images | Prototype viewport-clamped popup roots, scalable control spacing and icon/text composition; preserve picking, clipping and DPI behavior. |
| BSN Ready | Initialize controls after child creation where useful; this does not signal shader compilation, asset completion or GPU readiness. |
| Sprite materials / extended 2D materials | Explore simpler 2D preview, fallback and material presentation paths. Do not replace GPU-resident particle indirect drawing with one Sprite per particle. |
| Texture compression | Benchmark smoke/flipbook loading and memory with capability-aware formats and quality checks; retain portable fallbacks and source assets. |
| Scheduler randomization / richer errors | Add reproducible dependency-order tests and actionable shader/asset diagnostics. Panic handling is not permission to continue from corrupted runtime state. |
| Bulk despawn | Measure project-switch and effect teardown; do not change ownership merely to use a faster primitive. |

### Fireworks smoke transparency: an experiment, not an automatic fix

0.20 adds [premultiplied-alpha OIT](https://github.com/bevyengine/bevy/pull/22821) and
[per-material opt-out](https://github.com/bevyengine/bevy/pull/24856).
Aestra's custom transparent draw path needs deliberate integration before it can
benefit. Run a bounded spike using overlapping smoke from different emitters/draws,
camera movement, opaque intersections, HDR and MSAA; compare ordering, compositing,
GPU time and memory against existing controls. Set a supported fallback and budget.
Do not mark the remaining fireworks transparency work complete merely by upgrading.
OIT also does not fix smoke emission, dispersion or texture/art direction.

Defer Solari, DLSS and mesh-shader adoption from the upgrade's acceptance criteria.
Solari's release description still lists point/spot/rect lights as future support;
it is not a drop-in solution for fireworks burst lighting. Optional high-end rendering
can be investigated later without making the flagship example hardware-dependent.

## Execution order and completion rules

1. B20-0: establish resolvability, patch strategy and baselines.
2. B20-1 and B20-2: migrate renderer and editor compatibility in reviewable chunks.
3. B20-3: complete native regression and workspace validation before merging the upgrade.
4. B20-4: numeric pilot, parity tests, then removal of duplicate interaction code.
5. B20-5: menu/dropdown/color pilots, independently reviewed.
6. B20-6 and optional rendering/performance experiments only with measured benefit.

For each milestone record implementation paths, exact versions, executed checks,
native evidence, unresolved limitations and removed custom code. No milestone is
complete solely because it compiles. Do not bundle unrelated fireworks art changes,
new host features or an authored-format redesign into this engine upgrade.

**Next action:** assemble and qualify B20-1's full production outer graph and
plugin integration beyond the accepted particle-readiness and pipelined
root/project/live-binding gates,
alongside B20-2's required editor API changes.
Activate the qualified storage adapter, WESL composition paths
and graph imports as the engine graph switches coherently to 0.20. Select the
qualified 0.20 patches when the root graph switches; retire the 0.19 copies only
after the full regression gate passes. Refresh visual/performance evidence rather
than approving changed references. Do not delete Feathers or adopt new controls
as part of the compatibility port.
