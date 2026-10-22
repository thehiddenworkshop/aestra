# Bevy 0.20 upgrade and adoption plan

Date: 2026-10-10. Updated: 2026-10-22. Status: the real workspace now selects
Bevy 0.20.0 and its qualified patches. Native editor build/launch, ordinary workspace
tests, feature/release checks and native cluster/particle-light proofs pass on Windows.
Final editor/showcase acceptance, executable packaging and Linux CI remain open;
compatibility is not yet declared complete.
Repository baseline inspected: `e8191982`, Bevy 0.19.1.

### Application cutover — 2026-10-21

Stop extending the isolated slice sequence as the upgrade's critical path. Finish
the real application cutover, then merge compatibility before adopting new widgets.
Earlier slice reports are historical boundary evidence, not proof that the editor
has launched or that the workspace has passed on 0.20.

- Root manifests/lockfile select Bevy 0.20.0, wgpu 30.0.1 and glam 0.33.12, with
  `vendor/bevy_input_focus_020` and `vendor/bevy_pbr_020`. Windows-filtered Cargo
  metadata confirms a single active Bevy engine graph. Physics remains explicitly
  suspended; the Aestra-owned SVG bridge remains active.
- The actual renderer/editor select the qualified 0.20 input, storage, extraction,
  material and volume boundaries. Production graph/grid shaders use WESL imports.
- Remaining application observers use native `PointerClick`/`PointerDrag*` events,
  typed component lifecycle events, relocated cursors and native headless buttons.
  A narrow read-only control-state bridge preserves existing application handlers
  from native Pressed/Hovered markers and queued Activate actions. It does not
  synthesize picking or dispatch actions. Replacing control compositions is deferred.
- Actual editor check passed with Rust 1.98.1:
  `cargo check -p aestra-editor --locked --offline`.
- Native build/launch has reached the editor and sample effects on NVIDIA RTX 4070
  SUPER / Vulkan, including semantic mesh materials and graph shaders. This is not
  final visual acceptance of every workflow or the later menu/numeric corrections.

### Current application acceptance — 2026-10-22

- Complete editor unit suite passes: **2,845 passed, zero failed, 10 existing opt-in
  tests ignored**. Command: `cargo test -p aestra-editor --bin aestra-editor --locked
  --offline -- --test-threads=1 --format terse`; local log:
  `target/bevy-020-editor-acceptance.log`.
- All three editor architecture integration tests pass, and the normal editor
  executable was rebuilt with the menu/numeric corrections. Local log:
  `target/bevy-020-editor-architecture.log`. The rebuilt executable still needs
  the final native interaction/visual pass; a successful build is not that pass.
- Workspace-wide all-target check and strict Clippy pass with the common
  `--features aestra-bench/gpu --locked --offline` configuration. Local logs:
  `target/bevy-020-workspace-check.log` and `target/bevy-020-workspace-clippy.log`.
- The full CI workspace test command passes with default parallel execution:
  `cargo test --workspace --features aestra-bench/gpu --locked --offline`.
  **4,149 passed, zero failed, 58 existing opt-in tests ignored**, across 117 unit,
  integration and documentation suite results. Local log:
  `target/bevy-020-workspace-tests-final.log`. Ignored tests are not counted as
  native lifetime/performance acceptance.
- The parallel run exposed a test-fixture readiness race after asynchronous
  arrangement: placement memory was installed before the next canvas measurement.
  The fixture now waits for stable measured geometry matching current node placement
  before making another request. Production stale-result rejection and all undo/no-op
  assertions remain unchanged. All eight arrangement qualifications also pass in
  five consecutive 16-thread runs (`target/bevy-020-arrange-parallel-stress.log`).
- Audio-enabled showcase examples type-check, and all **18 fireworks example tests**
  pass with `fireworks-audio`, including cue timing, duplicate suppression, bounded
  admission and restart/despawn cleanup. Local logs: `target/bevy-020-audio-check.log`
  and `target/bevy-020-fireworks-audio-tests.log`. These host tests do not require an
  audio device and do not constitute listening/visual acceptance of the show.
- `cargo check -p aestra-editor --features dev-dynamic-linking --locked --offline`
  and `cargo check --release -p aestra-editor --locked --offline` both pass. The
  release check is warning-free after retaining the existing debug-only control
  audits behind constant conditions rather than leaving unused release parameters.
  Local logs: `target/bevy-020-dynamic-check.log` and `target/bevy-020-release-check.log`.
  Dependency-tree inspection selects `bevy_dylib 0.20.0` only for the explicit dev
  feature, not the default editor graph. These are type-checks, not DLL deployment
  or packaged/static release-executable validation.
- Top-level menus now mirror native menu events instead of independently toggling
  on the same activation. Opening restores focus eligibility after restoring display;
  native-widget tests cover all four menus, release, idle frames, reopening, hover
  switching, focus loss and Escape.
- Numeric editable text remains a measured leaf with a sibling picking surface.
  Both fill the same grid cell, with centered text padding; real UI layout tests
  verify shaped glyphs and matching text/picking bounds at ordinary and narrow widths.
- Timeline/choreography activation survives native pointer release before Update.
  The regression sends a real native release, checks exactly one semantic edit and
  consumed activation, then exercises undo/redo. Press without Activate still cannot edit.

### Native cluster and particle-light acceptance — 2026-10-22

Nine opt-in native GPU tests pass, explicitly and sequentially, rather than being
inferred from the ordinary workspace suite's ignored tests:

- Two `cluster_buffer_reuse` tests verify public/private buffer generations,
  independent views, growth, CPU/GPU mode switches, overflow recovery and teardown.
  The undersized fixture recovers Z/index capacities from 32/64 to 512/8192 and
  retains generations over 240 updates. Observed staging peak is two per view
  against the eight-slot limit; all named cluster and staging allocations retire
  to zero, and weak readback owners expire. Log:
  `target/bevy-020-native-cluster-lifetime-final.log`.
- The cold first run exposed a probe-readiness problem: metadata could report zero
  before asynchronous clustering pipelines were usable. Both fixtures now wait for
  real pipeline readiness before their unchanged observation windows. Retryable
  shader loading/import states remain bounded by the deadline; actual shader errors
  fail immediately. Retirement is still compared with the measured empty baseline,
  not an invented zero or a relaxed allocation assertion. Production code is unchanged.
- One `particle_light_realization` test verifies moving stars illuminate a real
  StandardMaterial receiver and obey pooled-light restart/despawn cleanup without
  replay. Its captured receiver response moves and returns to the dark baseline.
  Log: `target/bevy-020-native-light-realization.log`.
- Three paced `particle_light_latency` tests pass for the asynchronous adapter,
  comparison-only direct GPU proof and production bounded GPU adapter. They retain
  the existing final-image registration limits at 25/75/150 m/s, require zero stale
  green light after disable/owner removal, and exercise production cluster retirement.
  Log: `target/bevy-020-native-light-latency-final.log`; CSV/image evidence remains
  under `target/fireworks-f7/particle-light-{latency,gpu-proof,gpu-adapter}/`.
  The previously unported comparison shader now uses WESL and Bevy's native
  clustered-light import. An ordinary, non-ignored shader parse/loader regression
  was added; that test and the existing image-analysis guard also pass.
- Two `particle_light_playback` tests cover forward PlaybackOnly selection,
  world transforms, global caps, nested/event-born identities and cleanup. The
  overlapping shell fixture admits 48 of 512 candidates; the multi-instance fixture
  admits five of 48. Log: `target/bevy-020-native-light-playback.log`.
- The timestamp selection profile passes four workloads, from 512 candidates/16
  selected slots to 262,144 candidates/64 slots. Log:
  `target/bevy-020-native-light-profile.log`. The Bevy-hosted probes use RTX 4070
  SUPER / Vulkan; the standalone profile selected AMD Radeon integrated graphics /
  Vulkan. Do not compare those timings or claim a same-hardware performance gain.

Strict workspace all-target Clippy and formatting pass after the test corrections
(`target/bevy-020-native-proof-clippy.log`). The qualified patch documentation now
correctly identifies both 0.20 patches as selected by the actual root workspace.

### Native editor workflow spot-check — 2026-10-22

The rebuilt editor was exercised with an isolated `AESTRA_CONFIG_DIR` and a scratch
project under `target/bevy-020-editor-workflow-77d96507893d45cab79d6bc0b309b44b/`.
The File menu remains open after pointer release and idle frames. New Project opens
the created workspace, Browse preserves the selected parent path, and New Effect
creates and selects a saved effect. Native numeric keyboard entry changed Capacity
from 128 to 2; Enter marked the effect unsaved, and Ctrl+S restored Saved status.
The resulting effect file contains `max_particles: 2`.

The saved editor session was closed and relaunched, but Computer Use was stopped
before restoration could be inspected. Reopen acceptance is therefore still open.
Narrow transform fields also need a final legibility check; the workflow spot-check
does not approve every numeric layout or the full showcase.

Remaining merge gates are native reopen and final UI legibility checks, plus full
showcase visual/audio/performance acceptance,
release/development executable packaging checks, and Linux CI validation.
Do not restart isolated compatibility slices or count optional widget adoption as a
prerequisite to merging the engine upgrade.

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

### B20-1 twenty-fifth slice — Production outer simulation graph

- Extract the actual shipping outer simulation system into a shared module,
  together with canonical particle world upload, domain/event coupling and
  this-frame presentation readiness. Compile that implementation on both versions
  rather than reproducing its orchestration in another candidate dispatcher.
- Add three serial native gates: independent persistent particles and linked
  event generations through the production graph into real alpha/draw consumers
  and hardware simulation timestamps; delayed shader recovery through that same
  graph with genuine asynchronous compilation. Preserve all previous gates.
- Fix successful independent presentation being mistaken for unavailable work:
  distinguish readiness from optional shared lockstep tick/checkpoint counters.
  Keep absent counters unknown and waiting trail dispatch unready; no new replay
  requirement, production readback/wait or authored API change.
- The entire system compiles, but this slice executes its independent/linked
  stateful branches, not its full mixed/analytic/joint-fluid-history combinations
  or complete plugin. Particle-light eligibility is tested, not full selection.
- Full source-frozen qualification: **183 checks**, **27 native gates** and
  **424 unchanged input hashes**. Shipping host **63**, renderer **157** and
  hardware conformance **40** tests,
  all-target workspace check, strict workspace/candidate Clippy, formatting,
  PowerShell AST and whitespace validation pass.
- Implementation and validation: [outer simulation graph report](../../benchmarks/bevy-upgrade/outer-simulation-graph-2026-10-11.md).

### B20-1 twenty-sixth slice — Production outer fluid and joint history

- Run the shared outer graph and canonical stage preparer with the real fluid
  extension: field-follow particles, inline/paged trails and domain-born particles
  into installed compaction/culling/draw queues and real output rings. Keep all
  existing narrow-dispatch gates as comparisons.
- Check playback-only retention, optional replay, exact joint buffer restoration
  and an actual ten-tick/ten-observation timestamp window for tick 60 → 70.
  Pacing uses the main-world setting and normal extraction; GPU waits are test-only.
- Fix missing required domains/resources silently falling back to link-only
  coupling. Wait before clearing counters or consuming ticks/births; cover missing
  extracted inputs, unavailable timelines/emission resources, then exact recovery.
- Full presentation installers, outer analytic/mixed combinations and combined
  pipelined host scheduling remain open; no shipping dependency switch or visual
  approval is implied.
- Source-frozen qualification: **185 checks / 29 native gates**, **424 unchanged
  input hashes** and five verified executables. Shipping host **63**, renderer
  **157** and hardware-required conformance **40** pass; workspace check, strict
  workspace/candidate Clippy, formatting, PowerShell AST and whitespace pass.
- Implementation and validation: [outer fluid/history report](../../benchmarks/bevy-upgrade/outer-fluid-history-2026-10-11.md).

### B20-1 twenty-seventh slice — Production particle-light installer

- Include the actual shipping light installer in the candidate: embedded shader
  registration, main-world collection, extraction, preparation and graph selection
  consume the production outer graph's separate analytic/stateful owners.
- Preserve production transform/simulation ordering through shared internal sets
  instead of monolithic function imports; infer the portable planner's identity
  matrix across the candidate glam boundary. No public API or playback change.
- Native gate covers global/per-output caps, exact pause, live world transforms,
  unavailable stateful-pipeline readiness, nested identities, artifact/seed
  replacement, memory-budget rejection, settings/backend/component retirement and
  teardown. Playback-only keeps no particle checkpoints; readbacks are test-only.
- Volume/extension-stage installers, host light realization, mixed emitters within
  one owner and complete/pipelined plugin integration remain open. Fixture inputs
  do not qualify the monolithic main-world particle preparation/transform sync.
- Validation: frozen runner **186 checks / 30 native gates**, **425 unchanged
  input hashes**; shipping host **63**, renderer **157**, light conformance **2**
  and explicit native light playback **2** pass. Workspace/candidate strict lint,
  all-target workspace check, formatting and runner syntax checks pass.
- Implementation and validation: [production light installer report](../../benchmarks/bevy-upgrade/production-light-installer-2026-10-11.md).

### B20-1 twenty-eighth slice — Production extension-stage/volume installers

- Include and call the actual production installers in the candidate: normal
  volume creation and stage inputs, extraction, stage execution, field copies
  and actual PBR material rendering. No substitute dispatcher or seeded textures.
- Share the unchanged public scheduling set and use the existing simulation set
  for ordering; select a narrow 0.19/0.20 volume shader-creation boundary. Shipping
  remains on WGSL, the candidate uses qualified WESL composition. No public API change.
- Native dense volume-only gate verifies real progress, live density/texture
  agreement, nonblack material pixels, exact pause/forward changes, look-edit
  reuse, layout rebuild, timestamps, debug fallback, camera re-entry and teardown.
  Playback-only keeps no domain checkpoints; all readbacks/waits are test-only.
- Sparse/coupled/transformed volume integration, host light realization, complete
  plugin/pipelined integration and visual/performance parity remain open.
- Validation: frozen runner **196 checks / 31 native gates**, **429 unchanged
  input hashes**; shipping renderer **157**, host **63**, GPU-required staged-fluid
  conformance **3** pass. Workspace/candidate strict lint, all-target workspace
  check, formatting and runner syntax checks pass.
- Implementation and validation: [production volume installer report](../../benchmarks/bevy-upgrade/production-volume-installer-2026-10-11.md).

### B20-1 twenty-ninth slice — Host particle-light realization

- Include the real host pooled/GPU light adapters, transport, representative-light
  adapter and quality policy in the candidate; call the actual light plugin with
  shared outer graph and production stage/volume installers.
- Isolate extraction and clustered-light shader construction in 0.19/0.20
  boundaries. Preserve public types, budgets and the shared shader body; use
  app-labeled extraction and WESL only in the candidate. Route transport through
  the existing fallible mapped-view boundary. Shipping remains on 0.19.
- Native stateful playback-only receiver gate checks both modes, exact settled
  GPU positions, motion/layers/pool reuse, clamp/transport/manifest admission,
  same-render-sequence injection without further position readbacks, actual PBR
  illumination, rejection/recovery and source/slot retirement.
- The stage/volume installers coexist in this fixture, but combined volume-light
  pixels, nested/pipelined host-light and complete-plugin integration remain open.
- Validation: frozen runner **218 checks / 32 native gates**, **437 unchanged
  input hashes**; shipping host **63**, renderer **157**, explicit portable
  receiver/lifecycle and bounded same-frame GPU/PBR regression **1 each** pass.
  Workspace/candidate strict lint,
  all-target workspace check, formatting and runner syntax checks pass.
- Implementation and validation: [host light realization report](../../benchmarks/bevy-upgrade/host-light-realization-2026-10-11.md).

### B20-1 thirtieth slice — Combined pipelined installers

- Combine the canonical binding/playback/project systems with the shared outer
  simulation graph and actual stage/volume/light installers plus host light plugin
  on Bevy's real pipelined RenderApp, with asynchronous pipeline compilation.
- Add a required native gate with two nested analytic-light/dense-fluid owners:
  independent root clocks, live binding consumption, stage/output/readiness
  convergence, restart/seek, current nested source identities, both host-light
  modes, same-frame selection/injection, real receiver pixels, profiling,
  admission disable/recovery and root/volume/light teardown.
- Fix a reproduced shared profiling race: paused empty timestamp batches could
  supersede real in-flight stage measurements. Start stage batches only when
  stages advance, preserving the last real measurement without a blocking wait.
- Monolithic analytic particle asset preparation remains a fixture adapter.
  This does not certify nested stateful/fluid lockstep, joint volume-light
  scattering pixels, sparse volumes or the full renderer/host plugin.
- Validation: frozen runner **219 checks / 33 native gates**, **438 unchanged
  input hashes**; shipping renderer **157**, host **63** pass. Strict candidate
  lint in both configurations, workspace strict lint/all-target check with
  `aestra-bench/gpu`, formatting, runner parsing and whitespace checks pass.
- Implementation and validation: [pipelined installers report](../../benchmarks/bevy-upgrade/pipelined-installers-2026-10-11.md).

### B20-1 thirty-first slice — Production clock and transform synchronization

- Extract the existing main-world synchronization and its schedule installer into
  shared production code. Shipping and candidate retain post-propagation,
  pre-visibility ordering, including the particle-light consumer dependency.
- Remove per-frame fixture clock/epoch/seed/matrix uploads from the combined
  pipelined gate. Exercise actual paused root placement changes through async
  light proxy positions; verify buffer reuse through playback, restart and seeks.
- Add drained 0.20 storage regressions for propagated parent/host-track matrices,
  context tokens, cutoffs, quality/history policy and repeated host updates.
  Keep the engine-neutral ABI independent from Bevy's newer glam version.
- Initial analytic allocation remains a fixture. This is not full particle/material
  preparation, draw-child wiring, a new stateful binding oracle or full-plugin acceptance.
- Validation: frozen runner **228 checks / 33 native gates**, **442 unchanged
  input hashes**; shipping renderer **158**, host **63** pass (three existing ignores).
  Strict workspace/candidate lint, all-target check and formatting pass.
- Implementation and validation: [production transform report](../../benchmarks/bevy-upgrade/production-transform-sync-2026-10-11.md).

### B20-1 thirty-second slice — Production particle and material preparation

- Share the existing initial allocation, live emitter/renderer/material updates,
  fallback textures, shader cache and trail-statistics setup through the production
  preparation module/installer. Preserve public reexports and schedule ordering.
- Remove manual particle allocation from the combined threaded native gate; wait
  for real capabilities before assigning its fixture backend status. Keep all
  existing host-clock/output/light/pixel/profiling/seek/reuse/teardown assertions.
- Keep shipping WGSL unchanged through a version-selected shader boundary; compile
  actual prepared 0.20 WESL assets for both sampling variants through ShaderCache.
- Run shared allocation/backend/capacity/trail/readback/material regressions on both
  engines. Include existing project-root, bounds, sampling and timing regressions;
  scalar math boundaries preserve the engine-neutral storage ABI.
- Full-plugin bootstrap and main-world draw-child/visibility wiring remain open;
  analytic particles plus dense stages are not a new combined coupled oracle.
- Correct the asynchronous host-stage thread probe to observe the actual schedule
  caller, not a stealable worker; retain the separate-thread assertion and all
  channel/output/seek/lifecycle checks.
- Validation: frozen runner **249 checks / 33 native gates**, **454 unchanged
  input hashes**; shipping renderer **163**, host **63**, engine-neutral ribbon
  bounds **2** pass. Strict workspace/candidate lint, all-target check, formatting
  and whitespace validation pass; the three existing shipping ignores remain.
- Implementation and validation: [production preparation report](../../benchmarks/bevy-upgrade/production-preparation-2026-10-11.md).

### B20-1 thirty-third slice — Production draw-child and visibility wiring

- Share the existing main-world visibility installer, host-motion draw-child local
  and exact global poses, mesh/ribbon bounds, wireframe and replay/sampling culling
  systems. Preserve all ordering around propagation, uploads and visibility.
- Run existing sampling/transform/ribbon regressions from shared source on both
  engines, retaining nested tracks, clip offsets, shear and repeated/backward seeks.
- Install this wiring in the combined real pipelined gate; inspect actual extracted
  draw centers and IDs through paused placement edits and inherited hide/show.
  Retain all host-clock/output/light/pixel/profiling/seek/buffer-reuse assertions.
- The candidate still assembles installers explicitly; full-plugin bootstrap and
  combined coupled readiness remain open. No root dependency or visual-reference
  changes are approved by this slice.
- Validation: frozen runner **251 checks / 33 native gates**, **456 unchanged input
  hashes** and five unchanged executable hashes; shipping renderer **163**, host
  **63** pass. Strict workspace/candidate lint, all-target check, formatting and
  whitespace validation pass; the three existing shipping ignores remain.
- Implementation and validation: [production visibility report](../../benchmarks/bevy-upgrade/production-visibility-2026-10-11.md).

### B20-1 thirty-fourth slice — Complete renderer and host plugin bootstrap

- Share the existing `AestraRenderPlugin`, GPU bootstrap and canonical
  `AestraPlugin` registration between shipping and the isolated candidate.
  Preserve public exports, production scheduling and shipping shader bytes.
- Include actual CPU fallback, input-event delivery, project profiling and
  geometry telemetry. Keep wgpu/Bevy API differences in narrow version-selected
  mapped-view and regression-setup boundaries; no production GPU wait is added.
- Replace the combined threaded gate's manual backend/resource/graph/host
  registration with the actual plugins. Retain all existing lifecycle, clock,
  visibility, output and light checks, and require actual particle pixels,
  submitted geometry and owner/project timing delivery. Isolate receiver
  screenshots from emissive draw overdraw without changing pixel thresholds.
- Combined full-plugin stateful/fluid/trail readiness remains a separate gate;
  independent coupled/history/queue/draw regressions are still required.
- Validation: frozen runner **258 checks / 33 native gates**, **462 unchanged input
  hashes** and five unchanged executable hashes. Strict workspace/candidate lint,
  all-target check, formatting and whitespace validation pass; shipping renderer
  **163**, host **63** pass, with three existing shipping ignores unchanged.
- Implementation and validation: [production plugin bootstrap report](../../benchmarks/bevy-upgrade/production-plugin-bootstrap-2026-10-11.md).

### B20-1 thirty-fifth slice — Full-plugin coupled readiness

- Add an explicit asynchronous native gate for persistent particles following a
  dense fluid domain and generating paged trails through the complete plugins.
  Share authored inputs and threaded app setup with existing gates; no alternate
  allocator, graph dispatch or clock synchronization is introduced.
- Withhold one production pipeline's shader dependency before any owner exists.
  Require at least eight allocated render observations without early readiness,
  joint ticks, births or fields. Asset insertion must resume the same allocation.
- Require actual GPU fields, fluid-following heads, trail candidates, host live
  counts and submitted trail primitives even after heads die. Paused frames,
  backward seek/restart, PlaybackOnly capture policy and teardown are covered.
- Fix the discovered shared tick-70 rounding bug: GPU presentation had rounded
  above the host clock, so strict future-time rejection discarded valid geometry
  and particle counts. Reuse canonical host clock conversion for presentation;
  do not loosen context/timestamp filtering or change fixed-dt integration.
- Add a shared ten-minute clock-boundary regression and update the bounded-trail
  oracle to require the exact host boundary. Validation: **260** candidate checks,
  including **34** explicit native gates, with **463** input hashes unchanged;
  shipping renderer **164** and host **63** tests pass. Strict workspace/candidate
  lint, workspace all-target check and formatting pass. See the
  [coupled readiness report](../../benchmarks/bevy-upgrade/full-plugin-coupled-readiness-2026-10-11.md).

Remaining minimum migration work: B20-2's editor compatibility changes,
coherent Bevy/wgpu/glam switch selecting the
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

Implemented slice 1 (2026-10-11): **version-selected theme storage and real
Feathers style regression gate**. `theme.rs` now selects a small 0.19 adapter;
the isolated candidate selects the 0.20 contextual adapter. Both share the
unchanged palette and regression tests. All 29 constants and 96 shipping token
assignments are preserved. Aestra overrides have distinct semantic identities
so upstream contextual fills cannot merge menu hover, button hover and selected
colors. All four contexts, merged group tokens, upstream defaults, actual button
hover/press/disable/re-enable, menu keyboard-focus contrast, inherited context
and resource replacement are covered by eight required candidate tests.
Shipping editor validation: **1,171 unit + 3 architecture tests pass**, ten
existing ignores unchanged; strict workspace/candidate lint, workspace all-target
checking and formatting pass. See the
[theme compatibility report](../../benchmarks/bevy-upgrade/editor-theme-2026-10-11.md).
The source-frozen qualification runner passes **268 checks**, including all
**34 explicit native gates**, with **469 input hashes** and five native executable
hashes unchanged. Shipping stays Bevy 0.19.1.

Implemented slice 2 (2026-10-11): **shared scene wrappers and actual single-line
text-input regression gate**. Sixteen production Feathers wrappers now compile
and instantiate on both graphs. Slider initial values use a shared explicit
`SliderValue` override instead of the removed 0.20 scene property. The unchanged
text composition/value bridge is installed by a shared plugin. Eight shared
tests cover Unicode editing, accessibility/identity, Enter/focus-loss submission,
separate fields, replacement text and wrapper/slider construction. Two additional
candidate tests check the new `TextInput` behavior marker and Escape reaching an
ancestor after focus release with exactly one final submission. The runner now
requires and source-freezes this gate. See the
[scene/text compatibility report](../../benchmarks/bevy-upgrade/editor-controls-2026-10-11.md).
Final validation (2026-10-12): **1,179 shipping editor unit + 3 architecture tests
pass**, ten existing ignores unchanged. Workspace/candidate strict lint, workspace
all-target checking and both format checks pass. The accepted source-frozen run
passes **278 checks**, including **34 explicit native gates**; **475 input hashes**
and five native executable hashes are unchanged. Shipping stays Bevy 0.19.1.

Implemented slice 3 (2026-10-12): **shared numeric scrub core and version-selected
pointer/cursor boundary**. The production numeric plugin now runs on both graphs,
retaining the existing Sync scheduling and scrub behavior. Aliases bridge flat
0.20 pointer events and cursor types moving from Feathers to picking. Two existing
policy tests and six new interaction tests cover real pointer event dispatch,
editable text updates, modifiers, clamping, owner identity, propagation, exactly
one final submission and cursor restoration. The required editor-controls gate
now has 18 tests and freezes the additional production files. See the
[numeric scrub compatibility report](../../benchmarks/bevy-upgrade/editor-scrub-2026-10-12.md).
This is not complete numeric scene/arrow acceptance: 0.20's built-in scrub/step
handlers must be reconciled with Aestra's handlers before composing them.
Validation: **1,185 shipping editor unit + 3 architecture tests pass**, ten
existing ignores unchanged; strict workspace/candidate lint, workspace checking
and formatting pass. The accepted source-frozen **headless** run passes **252
checks** with **480 input hashes** unchanged. Native renderer gates were not
rerun for this UI-only slice; previous native evidence remains separately dated.

Implemented slice 4 (2026-10-12): **controlled numeric composition, value bridge
and hover arrows shared across both graphs**. Feathers text primitives retain
the editor's label/sigil/filter/step presentation without combining upstream
0.20 private numeric drag/arrow handlers with Aestra's handlers. An app-owned
surface separates click-to-edit from drag-to-scrub. The app-owned typed update
contract preserves document ownership and focused edits; changed text alone
emits live values, Enter release does not double-submit, and invalid/non-finite
typed input is rejected. Ten shared composition regressions join production
arrow/scrub/text/SVG/localization checks in a required 46-check editor-numbers
gate. See the [numeric composition report](../../benchmarks/bevy-upgrade/editor-numbers-2026-10-12.md).
Native component-based numeric synchronization/adoption remains B20-4 work;
native hit-testing, OS cursor and visual acceptance are not claimed here.
Validation: **1,195 shipping editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. The accepted source-frozen headless run executes
**298 checks**, all **540 input hashes** unchanged. Strict workspace/candidate
lint, workspace checking and both formatting checks pass. Shipping manifests
and lockfile remain unchanged; no new renderer-native acceptance is claimed.

Implemented slice 5 (2026-10-12): **complete shared color-picker composition and
color-plane event compatibility**. The candidate now imports the production
picker, numeric channels, hex field, tabs and Automatic control. Bevy 0.20 fixes
pointer/thumb y to agree with the existing HSL gradient; Aestra retains channel
values without introducing another inversion. Click commits no longer compete
with native DragEnd, cancel/disabled interactions cannot produce stray commits,
and programmatic hex display writes cannot feed back as color edits. Twelve new
shared regressions and existing picker/dependency checks form the required
63-check editor-colors gate. See the
[color compatibility report](../../benchmarks/bevy-upgrade/editor-colors-2026-10-12.md).
Native hit-testing, gradient pixel/visual acceptance and full editor workflows
remain open; no new renderer-native acceptance is claimed.
Validation: **1,207 shipping editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. The accepted source-frozen headless run executes
**361 checks**, with all **545 input hashes** unchanged. Strict workspace and
candidate lint, both formatting checks and whitespace validation pass. Shipping
manifests and lockfile remain unchanged. All-target workspace checking with
`aestra-bench/gpu` also passes.

Implemented slice 6 (2026-10-12): **custom code/search compositions and shared
context menus** now compile and execute against both graphs. Shared real-plugin
regressions cover Unicode/multiline edits, batched logical-key undo/redo and
selection, active-editor focus retention, buffered focus-loss submission,
context-menu actions/navigation/hover/click containment and search clear/retyping.
Search clearing no longer duplicates live changes or mutates empty/disabled
fields. Pointer focus changes retain the pending focus-event buffer. Candidate
action controls use the native pressed marker through a narrow boundary; the
surrounding application dispatcher still needs its own migration. See the
[custom text compatibility report](../../benchmarks/bevy-upgrade/editor-text-2026-10-12.md).
Native hit-testing/visual and OS clipboard/IME acceptance remain open.
Validation: **1,217 shipping editor unit + 3 architecture tests pass**, with
ten existing ignores unchanged. The accepted source-frozen headless run passes
**393 executed checks**, with all **556 input hashes** unchanged. Strict
workspace/candidate lint, both formatting checks and whitespace validation pass.
Shipping manifests and lockfile remain unchanged.

All-target workspace checking with `aestra-bench/gpu` also passes for slice 6.

Implemented slice 7 (2026-10-12): **tooltip pointer and layout compatibility**.
The candidate imports the production tooltip and shared regressions through the
existing picking boundary. Cursor anchors now convert window/viewport coordinates
to UI units correctly at different UI/display scales and retain inherited or
default UI cameras. Nine new shared lifecycle regressions and six existing
contract/font-backed wrapping tests form the required editor-tooltips gate.
The candidate editor feature set includes the default font so layout acceptance
cannot silently measure empty text. See the
[tooltip compatibility report](../../benchmarks/bevy-upgrade/editor-tooltips-2026-10-12.md).
Native tooltip/popover visual acceptance and full graph interaction remain open.

Validation: **1,226 shipping editor unit + 3 architecture tests pass**, with
ten existing ignores unchanged. The accepted source-frozen headless run passes
**408 executed checks**, with all **559 input hashes** unchanged. Strict
workspace/candidate lint, all-target workspace checking with `aestra-bench/gpu`,
both formatting checks and whitespace validation pass. Shipping manifests and
lockfile remain unchanged.

Implemented slice 8 (2026-10-12): **graph navigation, materials and bounded solvers**.
Production viewport navigation and wire/grid material implementations now have
shared importable modules, retaining their shipping plugin scheduling and public
call sites. Narrow cursor exports select the upstream 0.19/0.20 types. Pan motion
now respects UI scale without double-applying display scale; foreign-window wheel
input and dead viewport cursor capture are rejected. The required editor-graph
gate executes 33 checks, with one existing manual timing probe ignored, covering
real ECS navigation, material contracts and existing/new bounded snapping,
resize and insertion regressions. The isolated editor feature set includes
`bevy_ui_render`; no package versions change. See the
[graph navigation compatibility report](../../benchmarks/bevy-upgrade/editor-graph-navigation-2026-10-12.md).
Node/marquee observers, measured geometry ownership and native graph visuals remain open.

Validation: **1,240 shipping editor unit + 3 architecture tests pass**, with
ten existing ignores unchanged. The accepted source-frozen headless run passes
**441 executed checks**, with all **587 input hashes** unchanged. Strict
workspace/candidate lint, both formatting checks and whitespace validation pass.
Shipping manifests and lockfile remain unchanged.

All-target workspace checking with `aestra-bench/gpu` also passes for slice 8.

Implemented slice 9 (2026-10-12): **marquee and shared node ownership**.
The actual marquee observers, node presentation state/events, ancestor resolver
and fresh-press guard now compile and run on both graphs through narrow pointer
type/metadata exports. Existing panel call sites and shipping control filters
remain intact. Marquee coordinates now account for UI/display scale and final
release movement; gestures retain pointer/target ownership and cancel safely on
Escape, pointer cancellation or viewport/overlay removal. Invalid geometry is
rejected. Fourteen new shared interaction regressions extend the required
editor-graph gate to **47 executed checks**, with its existing timing ignore
unchanged. A separate shipping regression covers all six actual control filters.
See the [marquee compatibility report](../../benchmarks/bevy-upgrade/editor-graph-marquee-2026-10-12.md).
Full node drag/modified-drag transactions, measured geometry/document authority
and native graph workflow acceptance remain open; they are not fixture substitutes.

Validation: **1,255 shipping editor unit + 3 architecture tests pass**, with
ten existing ignores unchanged. The accepted source-frozen headless run passes
**455 executed checks**, with all **592 input hashes** unchanged. Strict
workspace/candidate lint, all-target workspace checking with `aestra-bench/gpu`,
both formatting checks and whitespace validation pass. Shipping manifests and
lockfile remain unchanged; no new native graph or full-editor acceptance is claimed.

Implemented slice 10 (2026-10-12): **node drag transactions and geometry dependencies**.
The candidate now imports the production begin/move/end observers, real placement
memory, shared document/view identities, stable geometry registry, snapping and
wire-insertion controllers. Host modules retain their existing public paths through
reexports. The candidate adds only local `aestra-authoring` and direct serde
dependency edges; root manifests and lockfile remain unchanged. Eleven new shared
transaction tests plus five existing registry and three existing spacing tests
cover group/modifier movement, authored undo-before positions, invalid inputs,
alignment invalidation and wire-drop ownership without fake document/geometry types.
See the [drag compatibility report](../../benchmarks/bevy-upgrade/editor-graph-drag-2026-10-12.md).
Host UI geometry collection/document-view authority, semantic action dispatch and
full native editing remain open. Legacy delta scaling and drag pointer/cancellation
behavior are preserved and require explicit native acceptance; this is not complete
editor qualification.

Validation: **1,266 shipping editor unit + 3 architecture tests pass**, with
ten existing ignores unchanged. The accepted frozen headless run passes **474
executed checks**, including **66 graph checks** (one existing timing ignore),
with all **602 input hashes** unchanged. Strict workspace/candidate lint,
all-target workspace checking with `aestra-bench/gpu`, both formatting checks
and whitespace validation pass. No new native graph/editor evidence is claimed.

Implemented slice 11 (2026-10-12): **post-layout collection and document/view authority**.
The candidate imports the actual UI geometry collector and real document/view
registries, including the production open/focus paths. Host catalog scope is a
direct system-pipeline input, not a mirrored resource. A partially removed
registry cannot reinterpret a docked view as a tool panel; mismatched active
document/view state cannot take ownership of split-view geometry. Eleven new
shared tests include real UI layout at three display scales, stale/duplicate
identities, project switches, close/reopen, preview extents and view ownership.
Four existing document lifecycle tests and the existing invalid-measurement
collector test now run in the candidate; a new shipping test checks current
catalog projection. Existing host consumer paths remain through reexports.
See the [collection compatibility report](../../benchmarks/bevy-upgrade/editor-graph-collection-2026-10-12.md).
Full panel composition, semantic dispatch/rewiring/history and native editor
workflows remain open; no fixture stand-in is substituted for those paths.

Validation: **1,278 shipping editor unit + 3 architecture tests pass**, with
ten existing ignores unchanged. Existing material/function camera-persistence
fixtures now resolve restored split views through real registries and matching
active documents, retaining all focus/fallback/idle assertions. Strict
workspace/candidate lint, all-target workspace checking with `aestra-bench/gpu`,
both formatting checks and whitespace validation pass. The accepted frozen
headless run executes **490 checks**, including **82 graph checks**, with all
**607 input hashes** unchanged. Shipping manifests/toolchain/lockfile remain
unchanged; no new native full-editor acceptance is claimed.

Implemented slice 12 (2026-10-12): **semantic preparation and material history boundary**.
The graph gate imports the editor's actual detached command preparation and
material-program history resource/stack transitions. Shipping uses the same
modules, retaining host catalog publication/rollback, session routing and
operation ordering. Twelve shared tests cover create/connect/disconnect,
duplicate/delete/extract, invalid commands, reversible transactions, failed
undo/redo, history bounds and project/asset isolation. Full UI dispatch and
function/ordered/semantic-layout history integration remain open; this is not
a mock-host acceptance claim. See the
[command/history compatibility report](../../benchmarks/bevy-upgrade/editor-graph-commands-2026-10-12.md).

Validation: **1,290 shipping editor unit + 3 architecture tests pass**, with
ten existing ignores unchanged. Strict workspace/candidate all-target lint,
workspace all-target checking with `aestra-bench/gpu`, both formatting checks
and whitespace validation pass. The accepted frozen headless run executes
**502 checks**, including **94 graph checks**, with all **613 input hashes**
unchanged. Root manifests/toolchain/lockfile remain unchanged; no new native
editor acceptance is claimed.

Implemented slice 13 (2026-10-12): **activation dispatch and document-target routing**.
The candidate imports the production Feathers activation queue, history action
identity/event dispatcher, material target model and view/document target
resolution/reconciliation. Shipping uses the same modules and retains host menu,
session and catalog behavior. Fourteen shared regressions cover activation guards,
consumption, disabled actions, undo/redo payloads and focused/stale/split/WESL
document routing. A shipping-only test exercises the real button-to-history
execution path and menu dismissal. Full application handlers, history publication
and semantic/layout integration remain open. See the
[routing compatibility report](../../benchmarks/bevy-upgrade/editor-graph-routing-2026-10-12.md).

The accepted frozen headless run executes **522 checks**, including **108 graph
checks** and **38 text checks**, with all **620 input hashes** unchanged. Strict
candidate all-target Clippy passes. Root dependencies remain unchanged; no new
native editor acceptance is claimed.

Shipping validation: **1,305 editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. Strict workspace all-target Clippy, workspace checking
with `aestra-bench/gpu`, both formatting checks and whitespace validation pass.

Implemented slice 14 (2026-10-12): **presentation transactions and document edit ordering**.
The candidate imports the real graph snapshot/transaction and per-document edit-order
model. Shipping delegates validation/apply and chronology to the same sources,
retaining authoritative semantic lookup, catalog/session publication and filesystem
history adapters. Thirteen shared tests cover bounded/context-scoped ordering,
compound semantic/layout attachment, preview round trips, placement/pin conflicts
and stale-identity rejection. Full host semantic undo and publication are not claimed.
See the [history compatibility report](../../benchmarks/bevy-upgrade/editor-graph-history-2026-10-12.md).

The accepted frozen headless run executes **535 checks**, including **121 graph
checks**, with all **628 input hashes** unchanged. Strict candidate all-target
Clippy and both formatting checks pass. Root dependencies remain unchanged;
no new native editor acceptance is claimed.

Shipping regression for slice 14: **1,318 editor unit + 3 architecture tests pass**,
with ten existing ignores unchanged, including original semantic/layout host tests.
Strict workspace all-target Clippy, all-target checking with `aestra-bench/gpu`,
formatting and whitespace validation pass. Root manifests/toolchain/lockfile
remain unchanged.

Implemented slice 15 (2026-10-12): **semantic snapshot resolution and function history**.
The candidate imports the production semantic resolver, persisted graph identities
and actual function-history resource/transitions. Shipping supplies authoritative
catalog inventories/readiness and publication callbacks, retaining existing draft
overlay/tombstone, built-in, call-site validation and session chronology policy.
Twelve shared regressions cover real compiler projection parity, semantic stamp
invalidation, source/root/identity guards, bounded scoped history, failed publication
and undo/redo retry. This is not a full catalog or host execution port.
See the [semantic/history compatibility report](../../benchmarks/bevy-upgrade/editor-graph-semantics-2026-10-12.md).

The accepted frozen headless run executes **547 checks**, including **133 graph
checks**, with all **652 input hashes** unchanged. Strict candidate all-target
Clippy and both formatting checks pass. Root dependencies remain unchanged;
no new native editor acceptance is claimed.

Shipping regression for slice 15: **1,330 editor unit + 3 architecture tests pass**,
with ten existing ignores unchanged. Strict workspace all-target Clippy, all-target
checking with `aestra-bench/gpu`, formatting and whitespace validation pass.
Root manifest/toolchain/lockfile diffs remain empty.

Implemented slice 16 (2026-10-12): **catalog-backed semantic publication and rollback**.
The candidate imports the actual editor catalog resource and unchanged draft store,
alongside production material/function publication, real project compilation and
extraction/undo compensation. Shipping uses the same sources, preserving startup
defaults, watcher/I/O setup, session preview installation and chronology adapters.
Ten shared regressions exercise actual indexed sources/drafts/compiler results,
stale/invalid edits, failed preview installation, retryable history, cleanup/restore
failure and cross-catalog function call-site checks. Five existing draft-save tests
now also execute on 0.20. Full session/action execution and native workflows remain open.
See the [publication compatibility report](../../benchmarks/bevy-upgrade/editor-publication-2026-10-12.md).

The accepted frozen headless run executes **562 checks**, including **148 graph
checks**, with all **661 input hashes** unchanged. Strict candidate all-target
Clippy and both formatting checks pass. No new native editor acceptance is claimed.

Shipping regression for slice 16: **1,340 editor unit + 3 architecture tests pass**,
with ten existing ignores unchanged. Strict workspace all-target Clippy, all-target
checking with `aestra-bench/gpu`, formatting and whitespace validation pass.
Root manifest/toolchain/lockfile diffs remain empty.

Implemented slice 17 (2026-10-12): **real session preview and scoped semantic history**.
The candidate imports the actual editor session, preview installer, graph authoring
context, session-backed material/function execution, semantic ledger/action-domain
executor and session context selection. Shipping calls the same modules. Twelve
new shared tests cover installer state/failure preservation, pending/interaction/solo
policy, project-scoped undo/redo, retryable publication and real extraction/save history.
Existing session, fixture and preview-mock regressions also execute on 0.20; no
candidate session, catalog or playback surrogate is used. Complete application
focus/modal/I/O/filesystem/presentation routing and native workflows remain open.
See the [session/history report](../../benchmarks/bevy-upgrade/editor-session-history-2026-10-12.md).

The accepted frozen headless run executes **613 checks**, including **199 graph/session
checks**, with all **675 input hashes** unchanged. Strict candidate all-target Clippy
and both formatting checks pass. No new native editor acceptance is claimed.

Shipping regression for slice 17: **1,352 editor unit + 3 architecture tests pass**,
with ten existing ignores unchanged. Strict workspace all-target Clippy, all-target
checking with `aestra-bench/gpu`, formatting and whitespace validation pass.
Root manifest/toolchain/lockfile diffs remain empty.

Implemented slice 18 (2026-10-12): **project watch/background I/O lifecycle**.
The candidate imports actual project I/O and filesystem refresh sources, the existing
serialized/idle-gated lifecycle schedule, and production Fluent localization. The
shipping plugin uses the same service module while retaining native texture-root
integration. Thirteen new shared tests cover queue draining/busy/rejection semantics,
project/document/authoring guards, live draft preservation, actual asynchronous
refresh, project switching, malformed-source recovery and I/O/watch ordering.
Eleven existing refresh tests and eight localization tests also run on 0.20.
The graph/session gate enables the real multithreaded I/O pool through the existing
`async-qualification` feature. See the
[project lifecycle report](../../benchmarks/bevy-upgrade/editor-project-lifecycle-2026-10-12.md).
Complete application action integration and native workflow acceptance remain open.

The accepted frozen headless run executes **645 checks**, including **231
graph/session/lifecycle checks**, with all **680 input hashes** unchanged. Strict
candidate all-target Clippy with the editor/asynchronous feature combination and
both formatting checks pass. No new native editor acceptance is claimed.

Shipping regression for slice 18: **1,365 editor unit + 3 architecture tests pass**,
with ten existing ignores unchanged. Strict workspace all-target Clippy, all-target
checking with `aestra-bench/gpu`, formatting and whitespace validation pass.
Root manifest/toolchain/lockfile diffs remain empty.

Implemented slice 19 (2026-10-13): **focus/modal/code-editor history ownership**.
The shipping action observer and menu availability share the actual early ownership
gate with the 0.20 candidate. Pointer/keyboard focus policy, the real bounded code
buffer history and complete modal protection resource are shared production sources.
Small 0.19/0.20 pointer-event boundaries preserve native dispatch without translating
behavior. Sixteen new regressions cover Unicode/bounded/coalesced buffer history,
focus traversal/neutral ownership, active/stale/empty code histories, every modal
branch and busy-until-applied I/O. One existing focus regression also now runs on
0.20, and the real code-widget gate retains its existing interaction coverage.
See the [history ownership report](../../benchmarks/bevy-upgrade/editor-history-ownership-2026-10-13.md).
Filesystem/presentation-aware history, complete shortcut dispatch and native workflow
acceptance remain open; this does not mark the complete action observer qualified.

The accepted frozen headless run executes **667 checks**, including **248
graph/session/history checks** and **43 code/search/context-menu widget checks**,
with all **690 input hashes** unchanged. Strict candidate all-target Clippy with
the editor/asynchronous feature combination and both formatting checks pass.
No new native editor acceptance is claimed.

Shipping regression for slice 19: **1,381 editor unit + 3 architecture tests pass**,
with ten existing ignores unchanged. Strict workspace all-target Clippy, all-target
checking with `aestra-bench/gpu`, formatting and whitespace validation pass.
Root manifest/toolchain/lockfile diffs remain empty.

Implemented slice 20 (2026-10-13): **filesystem chronology and redo synchronization**.
The candidate imports the complete production `AssetOrder` bridge and its real
session-backed context types, plus existing effect-ledger synchronization, scoped
presentation-redo clearing and document-order retirement. Shipping calls the same
code. Seventeen new regressions use real temporary-project deletion/recovery journals
and actual material/function edits to check ordering, retryable conflicts, stale
revisions, project/document resets, bounded history and scoped/global redo forks.
Recovery records remain independent of the bounded session-local undo history.
See the [filesystem history report](../../benchmarks/bevy-upgrade/editor-filesystem-history-2026-10-13.md).
Complete GUI deletion composition and compound presentation-aware action execution
remain open; this slice does not claim full observer or native workflow parity.

The accepted frozen headless run executes **684 checks**, including **265
graph/session/history checks**, with all **693 input hashes** unchanged. Strict
candidate all-target Clippy with the editor/asynchronous feature combination and
both formatting checks pass. No new native editor acceptance is claimed.

Shipping regression for slice 20: **1,398 editor unit + 3 architecture tests pass**,
with ten existing ignores unchanged. Strict workspace all-target Clippy, all-target
checking with `aestra-bench/gpu`, formatting and whitespace validation pass.
Root manifest/toolchain/lockfile diffs remain empty.

Implemented slice 21 (2026-10-13): **compound semantic/presentation history execution**.
The shipping action observer and 0.20 candidate share the actual validation-first
undo/redo block, project/session snapshot resolution and optional ECS presentation
state. Real preview visibility/cache data and project-wide graph inventory are
shared with the existing raster and placement consumers. Fourteen new regressions
cover exact node/visibility restoration, layout-only state preservation, missing or
stale state rejection, no-op attachment, and retry after semantic failure.
Effect/material/function history uses actual publication and session resources.
See the [compound history report](../../benchmarks/bevy-upgrade/editor-compound-history-2026-10-13.md).
The complete application action observer, filesystem/UI composition, shortcut
dispatch and native acceptance remain open; no native parity is claimed here.

The accepted frozen headless run executes **698 checks**, including **279
graph/session/history checks**, with all **700 input hashes** unchanged. Strict
candidate all-target Clippy with the editor/asynchronous feature combination and
both formatting checks pass. No new native editor acceptance is claimed.

Shipping regression for slice 21: **1,412 editor unit + 3 architecture tests pass**,
with ten existing ignores unchanged. Strict workspace all-target Clippy, all-target
checking with `aestra-bench/gpu`, formatting and whitespace validation pass.
Root manifest/toolchain/lockfile diffs remain empty.

Implemented slice 22 (2026-10-13): **application undo/redo routing**.
The real application router and ECS borrows are now shared with the 0.20 candidate.
Shipping keeps its existing observer contract and supplies the unchanged guarded
filesystem recovery callback through deferred commands. Fourteen new regressions
combine ownership, actual focused view/document registries, global ordering,
effect/material/function history, compound validation and failure-safe retry.
Filesystem dispatch is checked without pretending that a callback recorder
qualifies the complete native recovery worker/UI lifecycle. See the
[application history routing report](../../benchmarks/bevy-upgrade/editor-history-routing-2026-10-13.md).

The accepted frozen headless run executes **712 checks**, including **293
graph/session/history checks**, with all **702 input hashes** unchanged. Strict
candidate all-target Clippy with the editor/asynchronous feature combination and
both formatting checks pass. No new native editor acceptance is claimed.
Shipping regression for slice 22: **1,426 editor unit + 3 architecture tests pass**,
with ten existing ignores unchanged. Strict workspace all-target Clippy, all-target
checking with `aestra-bench/gpu`, formatting and whitespace validation pass.
Root manifest/toolchain/lockfile diffs remain empty.

Implemented slice 23 (2026-10-13): **guarded filesystem history execution**.
The shipping worker and 0.20 candidate now share real serialized restore/redo,
closed-document recovery, draft/reference checks and catalog publication. Existing
browser/dialog callbacks retain their implementation and publication ordering.
Seventeen new regressions perform actual filesystem operations, test cancellation
and conflict guards, restore exact drafts/targets/source paths, and connect the real
application router to the real worker. The candidate adds only its already-transitive
`ron 0.12` development dependency; root dependency inputs remain unchanged.
See the [filesystem execution report](../../benchmarks/bevy-upgrade/editor-filesystem-execution-2026-10-13.md).
The accepted frozen headless run executes **729 checks**, including **310
graph/session/history checks**, with all **708 input hashes** unchanged. Strict
candidate all-target Clippy and both formatting checks pass.
Shipping regression for slice 23: **1,443 editor unit + 3 architecture tests pass**,
with ten existing ignores unchanged. Strict workspace all-target Clippy, all-target
checking with `aestra-bench/gpu`, formatting and whitespace validation pass.
Root manifest/toolchain/lockfile diffs remain empty.
Initial delete/restore modal and browser UI composition remain open; no new native
editor acceptance is claimed.

Implemented slice 24 (2026-10-13): **complete delete/restore dialog and browser publication**.
Shipping and the 0.20 candidate share the complete action coordinator, actual
Feathers dialog, focus return, localized controls and real browser/bookmark
publication callbacks. The browser snapshot model, bounded bookmark model and
scrollbar composition/visibility are production sources, not substitutes.
Fifteen new shared regressions cover guarded initial deletion/bin restoration,
retry and stale actions, pagination, real layout/scrolling at two DPI/locale
combinations, Escape/Close focus restoration, busy controls, idle entity stability,
bookmark revival through history and exact unsaved-material recovery.
See the [deletion dialog report](../../benchmarks/bevy-upgrade/editor-deletion-dialog-2026-10-13.md).
The accepted final frozen headless run executes **744 checks**, including **325
graph/session/history checks**, with all **715 input hashes** unchanged. One existing
graph timing probe remains ignored. Shipping regression: **1,458 editor unit + 3
architecture tests pass**, with ten existing ignores unchanged. Strict all-target
Clippy passes on both engines, as do workspace checking with `aestra-bench/gpu`,
both formatting checks and whitespace validation. Root dependency inputs remain unchanged.
This is real headless dialog composition/layout qualification, not new interactive
native-window acceptance or full asset-browser/plugin parity.

Implemented slice 25 (2026-10-13): **history controls, availability and keyboard ingress**.
Shipping and the candidate share actual button handling, availability, control
classification, event-time key collection and history shortcut dispatch. Shipping
retains its original Interaction query; the 0.20 adapter uses real Pressed/Hovered
markers and removal transitions. Actual menu state/markers and panel identities
are shared production sources, with existing defaults and serialization preserved.
Twenty-two new regressions cover focused histories/code ownership, global chronology,
layout-only undo, modal/I/O blocking, one-shot activation, native hover/release,
transient menus and real layout-aware keyboard snapshots/chords.
See the [history controls report](../../benchmarks/bevy-upgrade/editor-history-controls-2026-10-13.md).
The final frozen headless run executes **766 checks**, including **347 graph/session/
history checks**, with all **725 input hashes** unchanged. One existing graph timing
probe remains ignored. Shipping: **1,480 editor unit + 3 architecture tests pass**,
with ten existing ignores unchanged. Strict all-target Clippy passes on both engines,
as do workspace checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency inputs remain unchanged.
The native wrapper still supplies actual ShortcutContext/palette ownership; candidate
dispatcher tests use an explicit blocking signal and do not qualify that complete
context. Full modal/window/focus shortcut ownership, tab-navigation and other
application keyboard consumers remain open, as does native workflow acceptance.

Implemented slice 26 (2026-10-13): **actual shortcut ownership and tab navigation**.
The candidate imports the complete production keyboard plugin/ShortcutContext,
including real modal, extraction-dialog, graph-menu, browser-surface, window and
focus state. Actual module-palette state and the application history keyboard wrapper
replace the previous explicit blocking-signal boundary. State-model extraction
preserves payloads/defaults and existing native consumers; it does not qualify their
complete UI handlers. The actual retained-popup tab eligibility/restoration observer,
root scene and window floor constants are shared. Tiny version adapters select
lifecycle observer and upstream text-input API spellings without changing policy.
Twenty-two new shared tests plus eight existing input/tab regressions cover all
30 input cases on both engines, including real keyboard history dispatch, modal/
menu/window/focus guards, browser isolation, no stale-chord replay, ancestor changes
and real Tab/Shift+Tab with same-frame eligibility restoration.
See the [shortcut ownership report](../../benchmarks/bevy-upgrade/editor-shortcut-ownership-2026-10-13.md).
The final frozen headless run executes **796 checks**, including **377 graph/session/
history checks**, with all **740 input hashes** unchanged. One existing graph timing
probe remains ignored. Shipping: **1,502 editor unit + 3 architecture tests pass**,
with ten existing ignores unchanged. Strict all-target Clippy passes on both engines,
as do workspace checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency inputs remain unchanged.
Remaining keyboard/action consumers, full panel/plugin composition and native
workflow acceptance remain open; this is not complete editor parity or a root switch.

Implemented slice 27 (2026-10-13): **transport keyboard, actions and playback**.
Shipping and the candidate share actual keyboard/repeat dispatch, action execution,
playback advancement and registration in the existing transport sets. Native toolbar
activation/handlers/icons/composition and main application's ordering stay unchanged.
Nineteen new real-input/session integration regressions plus seven existing transport
tests cover all 26 shared transport cases, including event-time Alt, repeat suppression,
actual ownership states, fresh-chord recovery, stop/restart/loop semantics and real
Bevy-time advancement. The port adds no checkpoint/replay work to live playback.
See the [transport playback report](../../benchmarks/bevy-upgrade/editor-transport-playback-2026-10-13.md).
The final frozen headless run executes **822 checks**, including **403 graph/session/
history checks**, with all **744 input hashes** unchanged. One existing graph timing
probe remains ignored. Shipping: **1,521 editor unit + 3 architecture tests pass**,
with ten existing ignores unchanged. Strict all-target Clippy passes on both engines,
as do workspace checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency inputs remain unchanged.
Transport toolbar composition, remaining keyboard/action
consumers, full panel composition and native workflows remain open.

Implemented slice 28 (2026-10-14): **complete transport toolbar composition**.
The candidate imports the complete production transport plugin: real Feathers
controls, SVG loader, tooltip/accessibility content, button activation/handling,
icons and exclusive loop-mode selection. Version adapters preserve shipping's
Interaction filter and read actual Pressed/Hovered transitions/removals on 0.20.
Layout, scene composition and icon/mode synchronization remain unchanged.
Nineteen new shared regressions plus existing toolbar/playback tests provide all
47 passing transport checks, including localized 1x/2x DPI layout, real keyboard/
headless click activation, disabled behavior, idle focus/image identity, shared SVG
raster teardown and selection following undo. The tests exposed and fixed duplicate
Space activation and accepted actions lost during visual release on both engines.
See the [transport toolbar report](../../benchmarks/bevy-upgrade/editor-transport-toolbar-2026-10-14.md).
The final frozen headless run executes **864 checks**, including **445 graph/session/
history checks**, with all **747 input hashes** unchanged. One existing graph timing
probe remains ignored. Shipping: **1,540 editor unit + 3 architecture tests pass**,
with ten existing ignores unchanged. Strict all-target Clippy passes on both engines,
as do workspace checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency inputs remain unchanged.
Full headless toolbar composition is qualified, not native
OS/window/GPU acceptance or complete editor parity.

Implemented slice 29 (2026-10-14): **discrete viewport shortcut consumers**.
The candidate imports the production shortcut system/registration and actual canvas,
action and system-set identities, using native transform-gizmo modes. Shipping calls
that same registration; the shortcut body is unchanged apart from visibility.
Eighteen new shared tests qualify global grid/maximize, hovered framing/gizmo modes,
real modal/focus/window ownership, event-time modifiers, non-QWERTY/AZERTY input,
ordered same-frame taps, repeat/stale replay suppression, canvas teardown and no
interference with held WASD. See the
[viewport shortcuts report](../../benchmarks/bevy-upgrade/editor-viewport-shortcuts-2026-10-14.md).
The accepted frozen headless run executes **882 checks**, including **463 graph/
session/history/consumer checks**, with all **751 input hashes** unchanged. One
existing graph timing probe remains ignored. Shipping passes **1,558 editor unit +
3 architecture tests**, with ten existing ignores unchanged. Strict all-target
Clippy passes on both engines, as do workspace checking with `aestra-bench/gpu`,
both formatting checks and whitespace validation. Root dependency inputs
remain unchanged. Rendered viewport action execution, toolbar composition and
continuous camera/pointer navigation remain separate; no new native acceptance
is claimed.

Implemented slice 30 (2026-10-14): **shared clipboard payload and function graph
consumers**. The candidate imports production immutable snapshots/remapping, actual
per-view material/function selection, function graph identity/activation and complete
function clipboard keyboard/execution. Shipping calls the same registration once;
the extracted model, selection and execution bodies are unchanged apart from
visibility/imports. Twenty-one new regressions plus two relocated existing tests
form 23 shared checks, including atomic rejection, scoped targeting, pointer paste
placement, real keyboard ownership and compound semantic/presentation undo/redo.
Existing native material/function menu tests are retained. See the
[function clipboard report](../../benchmarks/bevy-upgrade/editor-function-clipboard-2026-10-14.md).
The accepted frozen headless run executes **905 checks**, including **486 graph/
session/history/consumer checks**, with all **761 input hashes** unchanged. One
existing graph timing probe remains ignored. Shipping passes **1,579 editor unit +
3 architecture tests**, with ten existing ignores unchanged. Strict all-target
Clippy passes on both engines, as do workspace checking with `aestra-bench/gpu`,
both formatting checks and whitespace validation. Root dependency inputs remain
unchanged. Full material selection-edit/keyboard, menu composition and native
workflow acceptance are not claimed by this function-consumer gate.

Implemented slice 31 (2026-10-14): **material graph keyboard and selection-edit
consumers**. The candidate imports the production material clipboard adapters,
selection-edit keyboard/execution, semantic preparation/activation, actual graph
view/node identities and inspector selection. Shipping calls the same registration
once. Source audits confirm unchanged function bodies apart from imports/visibility;
native menu tests are retained. Fifteen new shared regressions cover real extraction,
typed connection reset, clipboard/delete/duplicate, compound semantic/layout/preview
undo/redo, live-node placement, scoped targeting/ownership and atomic rejection.
See the [material keyboard report](../../benchmarks/bevy-upgrade/editor-material-keyboard-2026-10-14.md).
The accepted frozen headless run executes **920 checks**, including **501 graph/
session/history/consumer checks**, with all **767 input hashes** unchanged. One
existing graph timing probe remains ignored. Shipping passes **1,594 editor unit +
3 architecture tests**, with ten existing ignores unchanged. Strict all-target
Clippy passes on both engines, as do workspace checking with `aestra-bench/gpu`,
both formatting checks and whitespace validation. Root dependency inputs remain
unchanged. Palette-opening keyboard/menu action handling, full graph composition
and native workflows remain separate.

Implemented slice 32 (2026-10-14): **timeline marker/event document actions**.
The candidate imports the production timeline action/snap contracts and six
session-backed edit branches. Shipping delegates from its existing observer while
retaining native TimelineState selection cleanup in the original order. Source
audits confirm unchanged edit bodies apart from the optional-localizer boundary;
duration, view and region execution remain native-owned. Fourteen shared checks
cover playhead creation, localized marker labels, selection, identity/payload
restoration, stale-ID rejection and mixed global undo/redo. Three shipping-only
regressions execute the actual native observer's state cleanup. See the
[timeline document-action report](../../benchmarks/bevy-upgrade/editor-timeline-document-actions-2026-10-14.md).
The accepted frozen headless run executes **934 checks**, including **515 graph/
session/history/consumer checks**, with all **772 input hashes** unchanged. One
existing graph timing probe remains ignored. Shipping passes **1,611 editor unit +
3 architecture tests**, with ten existing ignores unchanged. Strict all-target
Clippy passes on both engines, as do workspace all-target checking with
`aestra-bench/gpu`, both formatting checks and whitespace validation. Dependency
inputs are unchanged. Full timeline state/keyboard/choreography
and panel composition are not claimed by this narrow document-branch gate.

Implemented slice 33 (2026-10-14): **production timeline state and keyboard
consumer**. The candidate imports the full TimelineState and actual choreography
keyboard system with production canvas/set identities, transient drag/automation/
nested-inspection types, host-motion keyboard ownership and automation projection.
Shipping keeps the same chained input registration; body audits confirm unchanged
behavior apart from visibility/imports and calling the exact value method previously
wrapped by a one-line helper. Nineteen new shared checks cover ordered/logical/
event-time shortcuts, delete priority, nested inspection, host-motion ownership,
focus/hover/modal guards, real marker/event delete/undo, curve/gradient/vector-channel
insertion and navigation/order state. Four existing state and six existing
automation-curve checks also enter the candidate gate. See the
[timeline keyboard report](../../benchmarks/bevy-upgrade/editor-timeline-keyboard-2026-10-14.md).
The accepted frozen headless run executes **963 checks**, including **544 graph/
session/history/consumer checks**, with all **785 input hashes** unchanged. One
existing graph timing probe remains ignored. Shipping passes **1,630 editor unit +
3 architecture tests**, with ten existing ignores unchanged. Strict all-target
Clippy passes on both engines, as do workspace all-target checking with
`aestra-bench/gpu`, both formatting checks and whitespace validation. Dependency
inputs remain unchanged. Complete timeline/choreography/pose semantic execution,
panel composition and native workflow acceptance remain separate from this real
keyboard/state gate.

Implemented slice 34 (2026-10-14): **host-motion and automation action execution**.
The candidate executes production motion action/numeric observers and the complete
automation action branch, command construction and stored-curve sampling with the
full production CurvesState. Source audits cover all 23 moved contracts/functions
and unchanged native motion registration/activation; only visibility, formatting
and internal state type paths differ. Twenty-three new shared regressions execute
keyboard-driven semantic edits, pose actions, numeric preview/final/cancel behavior,
loop seams, curve/gradient/vector automation, parameter bindings, source-offset
insertion, rejection and mixed global undo/redo/forks. See the
[timeline execution report](../../benchmarks/bevy-upgrade/editor-timeline-execution-2026-10-14.md).
The accepted frozen headless run executes **986 checks**, including **567 graph/
session/history/consumer checks**, with all **791 input hashes** unchanged. One
existing graph timing probe remains ignored. Shipping passes **1,653 editor unit +
3 architecture tests**, with ten existing ignores unchanged. Strict all-target
Clippy passes on both engines, as do workspace all-target checking with
`aestra-bench/gpu`, both formatting checks and whitespace validation. Dependency
inputs remain unchanged. Remaining top-level timeline/choreography/emitter/region/referenced-
effect execution, pointer/numeric cancellation controllers, full Curves panel,
panel/plugin composition and native workflow acceptance remain separate.

Implemented slice 35 (2026-10-14): **complete timeline/choreography action
observers**. The candidate registers both actual production observers with shared
region split/join/duplicate/delete and nested catalog-backed selection/resolution.
Native registration/order stays unchanged. Audits cover all 13 moved contracts;
only visibility/formatting, the same shared motion helper path and removal of an
unused layout argument differ. Twenty-six new shared checks cover emitter edits,
deletion preview/confirm/cancel and minimum protection, modifier selection,
region edits and exact undo/redo, clip preview/inspection cleanup, source-opening
intents, full automation delegation and real keyboard-to-observer execution.
See the [timeline choreography report](../../benchmarks/bevy-upgrade/editor-timeline-choreography-2026-10-14.md).
The accepted frozen headless run executes **1,012 checks**, including **593 graph/
session/history/consumer checks**, with all **795 input hashes** unchanged. One
existing graph timing probe remains ignored. Shipping passes **1,679 editor unit +
3 architecture tests**, with ten existing ignores unchanged. Strict all-target
Clippy passes on both engines, as do workspace all-target checking with
`aestra-bench/gpu`, both formatting checks and whitespace validation. Dependency
inputs remain unchanged. Pointer/navigation, popup/numeric cancellation controllers,
full timeline/Curves panel/plugin composition and complete document-opening/native
workflow acceptance remain separate from this complete action-observer gate.

Implemented slice 36 (2026-10-15): **timeline navigation and cancellation
controllers**. The candidate executes actual wheel/pan, popup dismissal/focus
restoration and host-motion drag/numeric cancellation systems. Native composition
uses the same pane/header/popup identities; TimelinePlugin and host-motion system
registration/order stay unchanged. A tiny cursor boundary handles the upstream
Feathers-to-picking namespace move. Audits verify all 18 moved system/helper/type
contracts; no behavior changes are bundled. Twenty-four new shared checks cover
scroll units/bounds, input ownership, anchored zoom, middle-button pan, popup
protection/dismissal, deferred focus, drag/cursor cleanup and stale/cancelled
numeric previews with late-final suppression.
See the [timeline navigation report](../../benchmarks/bevy-upgrade/editor-timeline-navigation-2026-10-15.md).
The accepted frozen headless run executes **1,036 checks**, including **617 graph/
session/history/consumer checks**, with all **800 input hashes** unchanged.
Shipping passes **1,703 editor unit + 3 architecture tests**, with existing ignores
unchanged. Strict all-target Clippy passes on both engines, along with workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Dependency inputs remain unchanged. Explicit fixture geometry and
drag payloads do not qualify pointer gesture initiation/movement/final commit,
native picking/layout, complete TimelinePlugin or full timeline/Curves composition.

Implemented slice 37 (2026-10-15): **timeline seeking and region/marker/event
pointer gestures**. The candidate runs actual seek, control propagation stoppers
and drag start/move/end observers with shared snapping, distance/cursor helpers
and real component identities. Pointer boundaries select equivalent upstream
event names without behavior translation. Audits verify 28 moved observer/helper/
type contracts; native TimelinePlugin registration/order remains unchanged during
this slice. Twenty-eight new shared checks cover hierarchy propagation, seek
ownership, preview-only motion, one release commit, exact undo/redo, trim/source
offsets, grouped movement/clamping, multi-selection, scaled/bounded motion, snap
modes, stale/mismatched targets, Escape/late release and anchored marker/event edits.
See the [timeline gesture report](../../benchmarks/bevy-upgrade/editor-timeline-gestures-2026-10-15.md).
The accepted frozen headless run executes **1,064 checks**, including **645 graph/
session/history/consumer checks**, with all **806 input hashes** unchanged.
Shipping passes **1,731 editor unit + 3 architecture tests**, with existing ignores
unchanged. Strict all-target Clippy passes on both engines, along with workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Dependency inputs remain unchanged. Targeted upstream events and real
ECS hierarchy propagation do not qualify native picking/layout/window input.
Effect-clip, automation and host-motion pointer gestures, click/context-menu
consumers and complete timeline/Curves composition remain separate.

Implemented slice 38 (2026-10-15): **source-aware effect-clip pointer gestures**.
The candidate runs actual clip start/move/end observers, source-aware trim preview,
snapping and final timing command with the real catalog/session/history consumers
and control identity. Audits verify eight moved observer/helper/type contracts;
only visibility, formatting and equivalent pointer aliases differ. Native
TimelinePlugin registration/order and control attachment names remain intact.
Twenty-three new shared checks cover catalog source policy/fallback, preview-only
motion, one-shot source bounds, looping wrapping/expansion, minimum duration,
scaled/bounded movement, one release commit, exact global undo/redo, snapping,
stale/mismatched targets, Escape/late release, mid-drag deletion and anchored clips.
Referenced source bytes remain unchanged.
See the [effect-clip gesture report](../../benchmarks/bevy-upgrade/editor-effect-clip-gestures-2026-10-15.md).
The accepted frozen headless run executes **1,087 checks**, including **668 graph/
session/history/consumer checks**, with all **808 input hashes** unchanged.
Shipping passes **1,754 editor unit + 3 architecture tests**, with existing ignores
unchanged. Strict all-target Clippy passes on both engines, along with workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Dependency inputs remain unchanged. Native picking/layout/window input,
automation/host-motion pointer gestures, click/context-menu consumers and complete
timeline/Curves composition remain separate.

Implemented slice 39 (2026-10-15): **automation gestures and live presentation**.
The candidate runs actual automation selection/insertion, key and lane gestures,
source/snap projection and graph/key/raster preview systems with the real session,
catalog, choreography observer, CurvesState and history router. Native metadata/
lane discovery stays separate; real identities and control attachment names stay
intact. Audits verify 19 moved function/impl/type contracts; only visibility,
formatting and equivalent upstream pointer aliases differ. Native TimelinePlugin
registration/order is unchanged. Twenty-seven new shared checks cover scalar/
vector/gradient and parameter-backed edits, global undo/redo, source offsets,
neighbor bounds, scaled motion, snapping, stale/mismatched keys, click/drag
propagation, live previews and non-semantic lane resizing.
See the [automation gesture report](../../benchmarks/bevy-upgrade/editor-automation-gestures-2026-10-15.md).
The accepted frozen headless run executes **1,114 checks**, including **695 graph/
session/history/consumer checks**, with all **810 input hashes** unchanged.
Shipping passes **1,781 editor unit + 3 architecture tests**, with existing ignores
unchanged. Strict all-target Clippy passes on both engines, along with workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Dependency inputs remain unchanged. Existing automation Escape
cancellation and mismatched resize-release cleanup limitations are documented
as separate behavior-hardening follow-ups, not fixed by the compatibility port.
Host-motion pointer gestures, remaining click/context-menu consumers, native
picking/layout/window input and complete timeline/Curves composition remain open.

Implemented slice 40 (2026-10-15): **host-motion gestures and in-place key visuals**.
The candidate runs actual host-pose click/start/move/end observers, displacement
calculation and stable key presentation with real previews, motion/marker action
execution and global history. Native composition/attachment names stay intact.
Audits verify seven moved function/type contracts; only visibility, formatting,
equivalent upstream pointer aliases and the actual pointer-location accessor
adapter differ. Native TimelinePlugin and host-motion install registration/order
are unchanged. Twenty-seven shared checks cover preview-only motion, one release
commit, undo/redo, cross-key retiming/trailing-click suppression, cancellation and
stale canonical changes, anchor/loop constraints, snapping, floating-window/canvas
routing, hierarchy propagation and live/idle visuals without rebuilding targets.
See the [host-motion gesture report](../../benchmarks/bevy-upgrade/editor-host-motion-gestures-2026-10-15.md).
The accepted frozen headless run executes **1,141 checks**, including **722 graph/
session/history/consumer checks**, with all **812 input hashes** unchanged.
Shipping passes **1,808 editor unit + 3 architecture tests**, with existing ignores
unchanged. Strict all-target Clippy passes on both engines, along with workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Dependency inputs remain unchanged. Host-motion keeps existing active
drag/effect routing without capturing pointer/target ownership; foreign-pointer
hardening and automation cancellation/resize cleanup remain separate follow-ups.
Remaining timeline click/context-menu consumers, native picking/layout/window
input and complete timeline/Curves composition stay open.

Implemented slice 41 (2026-10-15): **timeline clicks and context-menu intent**.
The candidate runs actual marker/event/region/effect-clip selection, source-opening
double clicks, referenced-emitter click timing, pointer/keyboard popup opening,
native header action dispatch and emitter-group collapse with real session,
catalog/source resolution, CurvesState, document intents and global history.
Native composition keeps controller attachment names and real identities. Audits
verify 25 moved function/type/constant contracts; only visibility, formatting,
equivalent upstream event/marker aliases and the pointer-location accessor differ.
Native TimelinePlugin registration/order is unchanged. Thirty-two shared checks
cover real consumer routing, timing/row isolation, group/region selection, popup
exclusivity/positioning, hierarchy propagation, keyboard focus/shortcut policy,
native action classification and undo. Source bytes remain unchanged.
See the [timeline click/menu report](../../benchmarks/bevy-upgrade/editor-timeline-clicks-2026-10-15.md).
The accepted frozen headless run executes **1,173 checks**, including **754 graph/
session/history/consumer checks**, with all **815 input hashes** unchanged.
Shipping passes **1,840 editor unit + 3 architecture tests**, with existing ignores
unchanged. Strict all-target Clippy passes on both engines, along with workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Dependency inputs remain unchanged. Existing stale-header menu
identities and bubbled reorder-handle group collapse are documented separately,
not silently fixed by this compatibility port. Native picking/layout/window
input, filesystem source-opening execution and full popup/panel composition
remain open, along with remaining timeline action/drop/reorder/presentation
consumers and complete timeline/Curves integration.

Implemented slice 42 (2026-10-15): **timeline action dispatch and track-color controls**.
The candidate runs actual activation queue/dispatch, list-selection routing,
control classification audit and track-color preview/commit using real session,
MenuState, CurvesState, catalog/referenced resolution and global history consumers.
Shipping retains Changed<Interaction>; the candidate uses native Changed<Pressed>
queue/clear semantics. Audits verify 14 moved function/type contracts with explicit
query/queue/clear adapter substitutions. Shared action/menu/color policies, real
identities and native TimelinePlugin registration/order remain intact. Twenty-six
shared checks cover raw-versus-explicit activation, coalescing/repeated dispatch,
transition gating, menu cleanup/revisions, invalid controls, emitter/effect/
referenced list selection, live swatch/region previews, targeted final/default
color edits, English/French status, global undo/redo and debug control audits.
See the [timeline action/color report](../../benchmarks/bevy-upgrade/editor-timeline-action-controls-2026-10-15.md).
The accepted frozen headless run executes **1,199 checks**, including **780 graph/
session/history/consumer checks**, with all **821 input hashes** unchanged.
Shipping passes **1,866 editor unit + 3 architecture tests**, with existing ignores
unchanged. Strict all-target Clippy passes on both engines, along with workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Dependency inputs remain unchanged. Upstream disabled-control gating
and existing None-color live-fill policy are preserved; no new injected-Activate
guard or preview cancellation is claimed. Timeline asset-drop/reorder, remaining
presentation, native widget/input acceptance and complete timeline/Curves panel/
plugin composition remain open.

Implemented slice 43 (2026-10-15): **timeline asset placement and track reorder**.
The candidate runs actual asset-drop preview/enter/leave, canvas/header placement,
compound clip/order transaction, rejection feedback/expiry and emitter/effect-clip
reorder lifecycle with the complete production AssetPayload, AuthoringDropGuard,
ancestry/snapshot helpers, catalog/session and global history. Native component
identities and controller attachments remain intact. Audits verify 36 moved
function/type/impl/alias contracts; only visibility, formatting, namespace
resolution and equivalent pointer aliases differ. TimelinePlugin order and the
entire native root timeline test module compare unchanged. Twenty-five shared
checks cover placement clamping/insertion, Undo routing without closing material
targets, snapshot/modal/proposal/Escape guards, duplicate/self/cyclic/incompatible
sources, preview cleanup/feedback expiry, mixed-track reorder/cursor transitions,
exact undo/redo and no-op/stale/missing targets.
See the [timeline asset-drop/reorder report](../../benchmarks/bevy-upgrade/editor-timeline-drop-reorder-2026-10-15.md).
The accepted frozen headless run executes **1,224 checks**, including **805 graph/
session/history/consumer checks**, with all **830 input hashes** unchanged.
Shipping passes **1,891 editor unit + 3 architecture tests**, with existing ignores
unchanged. Strict all-target Clippy passes on both engines, along with workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Dependency inputs remain unchanged. Existing secondary reorder and
uncaptured pointer/target ownership policy, plus single-canvas placement, remain
documented hardening follow-ups. Native input/layout/feedback rendering, timeline
drop-gap/reorder hints, remaining geometry/scroll presentation and full timeline/
Curves panel/plugin composition remain open.

Implemented slice 44 (2026-10-15): **timeline presentation and scrolling**.
The candidate executes production region/marker/event/effect-clip/referenced-child
geometry, ruler/playhead/snap guide and transport labels, synchronized scrolling,
emitter reveal/focus, hover insertion/drop gaps/preview and reorder borders.
Catalog-backed nested-track projections and visible automation lane validation/
height aggregation are shared with shipping, not replaced by candidate fixtures.
Sixteen presentation identities and four projection data types retain the native
UI contract. Audits verify 53 moved function/type contracts; only visibility,
formatting and explicit imports differ. Native TimelinePlugin ordering and the
entire root timeline test module compare unchanged. Twenty-seven shared checks
cover live/stale/clipped geometry, scroll input and clamping/priority, delayed
focus, insertion cleanup/grid restoration, preview placement/labels, reorder
borders, nested projection metadata and visible/custom-height automation lanes.
See the [timeline presentation report](../../benchmarks/bevy-upgrade/editor-timeline-presentation-2026-10-15.md).
The accepted frozen headless run executes **1,251 checks**, including **832 graph/
session/history/consumer checks**, with all **833 input hashes** unchanged.
Shipping passes **1,918 editor unit + 3 architecture tests**, with existing ignores
unchanged. Strict all-target Clippy passes on both engines, along with workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Dependency inputs remain unchanged. Explicit computed geometry is not
native UI/layout/picking acceptance. Single-canvas/viewport query policy, largest-
canvas ruler spacing and previously documented interaction hardening remain open.
Next: timeline/Curves lane/track and full panel/plugin composition, then remaining
viewport/application consumers and native workflow acceptance.

Implemented slice 45 (2026-10-15): **automation lane composition**.
The candidate builds production automation headers, graphs, scalar/gradient/vector
key controls, resize handles and real Feathers icon buttons with their attached
controllers. It uses actual registry-driven lane discovery, public binding/source-
bank resolution, localized labels, tooltip/accessibility metadata and CPU curve
rasters. The linked-extension module registry/default and parameter/label helpers
move to shared properties/module_data.rs; existing properties exports remain
intact. Seven extracted contracts and six newly candidate-qualified automation
definitions preserve bodies/fields; only imports, visibility, formatting and
equivalent Button/cursor namespaces differ. Native plugin order and existing root
timeline, automation and properties tests compare unchanged. Twenty new checks
cover both locales, metadata/value/channel resolution, construction geometry and
classification, selection, source offsets/offscreen identity, raster reuse,
attached pointer controls, single add/delete activation, reversible edits and
non-semantic resizing. Two existing automation metadata tests also join the
candidate graph. See the [automation lane composition report](../../benchmarks/bevy-upgrade/editor-timeline-lane-composition-2026-10-15.md).
The accepted frozen headless run executes **1,273 checks**, including **854 graph/
session/history/consumer checks**, with all **836 input hashes** unchanged.
Shipping passes **1,938 editor unit + 3 architecture tests**, with existing ignores
unchanged. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Dependency inputs remain unchanged. Real scene/observer and CPU raster
checks do not qualify full panel/plugin composition or native layout/picking/
rendering. Prior interaction-hardening policies stay unchanged. Next: remaining
emitter/effect-clip/referenced-track construction and complete timeline/Curves
integration, then remaining viewport/application consumers and native workflows.

Implemented slice 46 (2026-10-15): **referenced-track composition**.
The candidate builds actual referenced-emitter/effect-clip headers and read-only
bars from production catalog projections, with native list/control identity,
localized badges, selected/muted/disabled styling, disclosure icons and attached
selection/source-opening/press observers. Track label/color helpers and the real
tool-button composition are shared with shipping; its public button export and
application alias stay intact. Eight function contracts retain their bodies with
only import/visibility/formatting/equivalent namespace differences. Native plugin
order, existing root timeline/referenced-effect tests and remaining button bodies
compare unchanged. Twenty new shared checks cover nested/reused source paths,
grid/depth/trim geometry, missing/offscreen timing, selection and source intents,
disclosure behavior, both locales and live root-clip trim compatibility. Four
existing referenced checks and one tool-button API probe also join candidate gates.
See the [referenced-track composition report](../../benchmarks/bevy-upgrade/editor-referenced-track-composition-2026-10-15.md).
The accepted frozen headless run executes **1,298 checks**, including **878 graph/
session/history/consumer checks**, with all **839 input hashes** unchanged.
Shipping passes **1,958 editor unit + 3 architecture tests**, with existing ignores
unchanged. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Dependency inputs remain unchanged. These scene/observer/intent checks
are not native layout/picking/rendering, external source navigation or full panel/
plugin acceptance. Earlier hardening and nested disclosure labeling remain open.
Next: outer emitter/root effect-clip headers and actual menus/popovers, remaining
track/toolbar construction and full timeline/Curves integration, then remaining
viewport/application consumers and native workflows.

Implemented slice 47 (2026-10-15): **outer effect-clip headers and popup composition**.
The candidate builds actual root effect-clip headers, reorder handles and five-item
menus, sharing complete production pointer-popup constructors/observers and the
real AssetAction identity. Native exports and host operations remain intact;
the explode item is checked for its exact action payload, not filesystem execution.
Fifteen function contracts and the action enum retain their behavior, with native
timeline/asset plugin wiring and complete existing root timeline/context-menu
test modules unchanged. Four files join the frozen inventory.
Twenty shared tests cover native list/control identity, geometry, selected styling,
localized inverse labels, inline click/Activate separation, secondary-click menu
routing, popup action identities and propagation, single activation consumption,
source/list intents, deletion cleanup/history and mixed-order reorder history.
Existing deletion policy retains removed IDs in the authored order vector; actual
normalized projection filters them out. No behavior change is bundled here.
See [implementation, scope and evidence](../../benchmarks/bevy-upgrade/editor-effect-clip-header-composition-2026-10-15.md).
The accepted frozen headless run executes **1,318 checks**, including **898 graph/
session/history/consumer checks**, with all **843 hashes** unchanged. Shipping
passes **1,978 editor unit + 3 architecture tests**, with existing ignores unchanged.
Strict all-target Clippy passes on both engines, alongside workspace all-target
checking with `aestra-bench/gpu`, both formatting checks and whitespace validation.
Dependency inputs remain unchanged. Scene/observer tests with explicit geometry
do not qualify native picking/layout/clamping/rendering, external source/explode
operations or full timeline/Curves plugin acceptance. Prior hardening remains open.
Next: outer emitter headers and actual color/automation/context popovers, remaining
track/toolbar and full timeline/Curves integration, then viewport/application
consumers and native workflows.

Implemented slice 48 (2026-10-15): **emitter headers and their popovers**.
The candidate builds actual production emitter headers and context/color/automation
visibility popovers, using complete production color/numeric/text controllers and
the existing 0.20 pointer/scene boundaries. Three header markers join shared
interaction state; eight extracted function contracts, native TimelinePlugin
wiring and its complete existing root tests are unchanged. Two production files
and a candidate import facade join the frozen inventory; dependency inputs stay
unchanged. No optional widgets or fake color/automation behavior are introduced.
Twenty shared tests cover native list/action identity, grid and multi-selection
policy, enabled/solo/diagnostic and localized labels, inline click versus Activate
routing, context and region-sensitive payloads, semantic mute/duplicate history,
actual color live preview/final commit/Automatic undo, registry lane visibility,
empty spacers and mixed-order reorder history. Forty-nine existing control tests
also execute in the integrated graph gate, not as new shipping tests.
See [implementation, scope and evidence](../../benchmarks/bevy-upgrade/editor-emitter-header-composition-2026-10-15.md).
The accepted frozen headless run executes **1,387 checks**, including **967 graph/
session/history/consumer checks**, with all **846 hashes** unchanged. Shipping
passes **1,998 editor unit + 3 architecture tests**, with existing ignores unchanged.
Strict all-target Clippy passes on both engines, alongside workspace all-target
checking with `aestra-bench/gpu`, both formatting checks and whitespace validation.
Explicit-geometry scene/observer checks do not qualify native picking/layout/
clamping/rendering, floating windows, external extraction/deletion workflows or
full timeline/Curves plugin composition. Existing interaction-hardening policies
are preserved. Next: remaining ruler/event-lane, track and toolbar construction,
full timeline/Curves integration, then viewport/application consumers and native
workflows. Shipping remains 0.19.1.

Implemented slice 49 (2026-10-15): **ruler and choreography-event lane composition**.
The candidate builds actual production ruler ticks, marker controls, choreography
event lane/controls and the hidden effect-drop spacer. Three extracted constructor
contracts retain their behavior, with complete existing root timeline tests and
TimelinePlugin wiring unchanged. Two production files join the frozen inventory;
dependency versions, features, toolchain and authored formats remain unchanged.
Eighteen shared integration tests cover retained tick/cue identities, native
action/cursor classification, both catalogs, selected styling and lane/spacer
geometry, real visual updates/zoom/offscreen and missing-cue policy, click/Activate
separation and press isolation, attached drag previews, payload-preserving commits,
global undo/redo, two DPI scales, bounds and no-motion release/cursor restoration.
See [implementation, scope and evidence](../../benchmarks/bevy-upgrade/editor-ruler-cue-composition-2026-10-15.md).
The accepted frozen headless run executes **1,405 checks**, including **985 graph/
session/history/consumer checks**, with all **848 hashes** unchanged. Strict
all-target Clippy passes on both engines, alongside workspace all-target checking
with `aestra-bench/gpu`, both formatting checks and whitespace validation.
Shipping passes **2,016 editor unit + 3 architecture tests**, with existing ignores
unchanged. Explicit-geometry/observer checks do not
qualify native layout/picking/rendering, floating-window routing or full plugin
acceptance; earlier pointer/stale-action/cancellation hardening stays separate.
Next: remaining track and toolbar construction/full timeline/Curves integration,
then viewport/application consumers and native workflows. Shipping remains 0.19.1.

Implemented slice 50 (2026-10-15): **timeline toolbar and selection-combo composition**.
The candidate builds the actual time/frame-step, view/marker/event, region,
snap and duration toolbar controls. The original toolbar expression and three
helpers retain their behavior. The real generic combo option/selection builders
and default shell EditorAction identity are shared without synthetic host actions;
native APIs remain reexports of the same types/functions. The region-tool marker
joins shared state. Combo idle initialization is explicit by version: shipping
keeps Interaction::None; the candidate uses Hovered(false) with real Feathers
Pressed/Activate behavior. Native timeline/shell plugin wiring, existing timeline/
searchable-menu tests and eight remaining combo/menu helper bodies are unchanged.
Twenty shared toolbar tests cover retained controls/SVGs, localized labels, real
snap/menu metadata, single dispatch, transport stepping, view-only actions,
semantic marker/event/duration edits, split/merge and global history, invalid
region requests and time-label presentation. Three shared combo tests cover the
public/default API, real option classification and empty menus; each engine also
runs its own idle-state contract. No native menu/keyboard acceptance is claimed.
See [implementation, scope and evidence](../../benchmarks/bevy-upgrade/editor-timeline-toolbar-composition-2026-10-15.md).
The accepted frozen headless run executes **1,429 checks**, including **1,009 graph/
session/history/consumer checks**, with all **857 hashes** unchanged. Shipping
passes **2,040 editor unit + 3 architecture tests**, with existing ignores unchanged.
Strict all-target Clippy passes on both engines, alongside workspace all-target
checking with `aestra-bench/gpu`, both formatting checks and whitespace validation.
Root dependency/toolchain inputs are unchanged. Typed activation/scene tests do
not qualify native input, menu focus/lifecycle, layout/rendering, floating windows
or full timeline/Curves integration; earlier hardening remains separate.
Next: remaining track construction and full timeline/Curves panel/plugin
integration, then viewport/application consumers and native workflows.

Implemented slice 51 (2026-10-18): **choreography track assembly**.
The candidate now assembles actual header/body lists for root and nested effect
clips, implicit/explicit emitter regions and visible automation lanes, including
both drop spacers. Two original assembly expressions retain their contracts;
native timeline plugin wiring and the complete existing root tests are unchanged.
Eighteen shared tests cover exact mixed/nested row alignment and heights, lane
visibility, reused source paths, control identities, clipping/styling and attached
selection/context/press consumers. Actual move/trim controls preserve exact timing
at two DPI scales, non-mutating previews and single globally reversible commits.
Source/project files remain byte-identical.
See [implementation, scope and evidence](../../benchmarks/bevy-upgrade/editor-track-composition-2026-10-18.md).
The accepted frozen headless run executes **1,447 checks**, including **1,027
graph/session/history/consumer checks**, with all **859 hashes** unchanged.
Shipping passes **2,058 editor unit + 3 architecture tests**, with existing ignores
unchanged. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain inputs remain unchanged. Explicit-geometry
scene/observer tests do not qualify native input/layout/rendering, menu lifecycle,
floating windows or full plugins. Earlier interaction-hardening stays separate.
Next: remaining full timeline/Curves panel/plugin integration, including host-motion
construction and scrolling, then viewport/application consumers and native workflows.
Shipping remains 0.19.1.

Implemented slice 52 (2026-10-18): **host-motion lane and timeline scrollbar composition**.
The candidate builds the production host-motion header/pose lane and horizontal/
vertical timeline gutters with real Feathers bars. Native constructors and the
actual host activation queue/consumer are shared through the existing input
boundary: Interaction on 0.19; upstream Pressed insertion/removal on 0.20.
Five constructor/helper expressions and two bridge contracts retain their behavior;
native host installation, remaining inspector helpers, TimelinePlugin wiring and
both complete existing timeline/host test blocks remain unchanged.
Fourteen shared tests exercise real action classification/consumption, empty/
existing/new keys, attached selection/retiming at two scales, global history,
anchor protection, trailing clicks and stable visuals. Ten shared scrolling tests
cover real bar targets/geometry, paired panes, view mapping, overflow/clamping,
content shrink, half-pixel thresholds and attached gesture isolation.
See [implementation, scope and evidence](../../benchmarks/bevy-upgrade/editor-host-motion-scroll-composition-2026-10-18.md).
The accepted frozen headless run executes **1,471 checks**, including **1,051
graph/session/history/consumer checks**, with all **863 hashes** unchanged.
Shipping passes **2,082 editor unit + 3 architecture tests**, with existing ignores
unchanged. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain inputs remain unchanged. These explicit-
geometry scene/observer tests do not qualify native input/layout/rendering,
scrollbar thumb input, persisted scroll memory, floating windows or full plugins.
Earlier hardening remains separate. Next: remaining full timeline/Curves panel/
plugin integration and host-motion inspector construction, then viewport/
application consumers and native workflows. Shipping remains 0.19.1.

Implemented slice 53 (2026-10-18): **host-motion inspector composition**.
The candidate builds the real inspector, controlled numeric fields, motion actions,
concrete Curves link and scroll area. Four original helpers, complete scroll-memory/
Curves action identities, generic button composition and actual field-row/scroll
wrappers are shared, preserving native APIs. Host installation, Timeline/Curves/
Shell plugin wiring and all four existing root test blocks retain their contracts.
Twenty-two shared tests cover inspector states, exact initialization/ranges/steps,
real text editing and keyboard Enter, non-mutating previews, validation/rollback,
time/pose edits, Escape/late release, constructed motion actions and global history,
scroll targets and field layout. The candidate also runs the existing field-row
contract and a new native-button-reexport API probe. Authored files stay unchanged.
See [implementation, scope and evidence](../../benchmarks/bevy-upgrade/editor-host-motion-inspector-2026-10-18.md).
The accepted frozen headless run executes **1,495 checks**, including **1,074
graph/session/history/consumer checks**, with all **869 hashes** unchanged.
Shipping passes **2,104 editor unit + 3 architecture tests**, with existing ignores
unchanged. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain inputs remain unchanged. Headless Feathers/
text/input checks do not qualify native visual/pointer acceptance, persisted scroll
capture/restore, floating windows or complete plugins. The Curves link's dock-opening
consumer and earlier interaction-hardening remain separate. Next: remaining full
timeline panel composition and Curves action/panel/plugin integration, then
viewport/application consumers and native workflows. Shipping remains 0.19.1.

Implemented slice 54 (2026-10-18): **complete timeline panel composition**.
The candidate builds the entire production timeline tree: toolbar, heading/track
list, nested choreography rows, ruler/cue/motion lanes, scroll viewports/gutters,
playhead/snap guide and drop feedback. The original constructor and five panel
marker contracts are shared; native TimelinePlugin wiring and the existing root
timeline test block remain unchanged. Twenty-two new shared tests exercise
complete/empty/localized panels, exact paired grid rows, retained controls,
attached seek/drop/drag observers, actual toolbar/header/transport actions,
global history, preview/rejection cleanup, paired scrolling, idle stability and
nested teardown. See
[`editor-timeline-panel-composition-2026-10-18.md`](../../benchmarks/bevy-upgrade/editor-timeline-panel-composition-2026-10-18.md).
The accepted frozen headless run executes **1,517 checks**, including **1,096
graph/session/history/consumer checks**, with all **871 input hashes** unchanged.
Shipping passes **2,126 editor unit + 3 architecture tests**, with existing ignores
unchanged. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain inputs remain unchanged. These explicit-
geometry checks do not qualify native layout/picking/rendering, floating windows
or complete plugin schedules. Next: TimelinePlugin schedule integration, then
remaining Curves action/panel/plugin integration, viewport/application consumers
and native workflows. Earlier interaction-hardening remains separate. Shipping
remains 0.19.1.

Implemented slice 55 (2026-10-18): **TimelinePlugin schedule integration**.
The candidate installs the original production plugin, with its nine timeline
observers, three chained `Update` groups, and real host-motion observer/system
wiring shared with shipping. Two shared tests verify one real button activation
is consumed exactly once without dirtying history, and check the executable
input/action/visual and host-motion ordering by registered system keys. See
[`editor-timeline-plugin-schedule-2026-10-18.md`](../../benchmarks/bevy-upgrade/editor-timeline-plugin-schedule-2026-10-18.md).
The accepted frozen headless run executes **1,521 checks**, including **1,100
editor graph/session/history/consumer checks**, with all **873 input hashes**
unchanged. Shipping passes **2,128 editor unit + 3 architecture tests**, with
ten existing ignores; strict Clippy on shipping and candidate targets, both
formatting checks and whitespace validation pass. Native layout, pointer
picking, rendering and complete application workflow acceptance remain open.
Next: Curves action/panel/plugin integration,
then remaining viewport/application consumers and native workflows. Shipping
remains on 0.19.1.

Implemented slice 56 (2026-10-18): **Curves property-editing core**.
Nineteen original scalar/vector curve and gradient helpers and the key-edit enum
are shared with the candidate: active/bound value resolution, key mutations,
pointer insertion, bounds/sampling, projection, labels and preview math. Source
audits preserve their contracts, all fourteen remaining root functions, native
Curves plugin wiring and the complete existing root test block. Twenty-seven
shared tests cover local/bound values, channel isolation, normalized ranges,
gradient RGBA edits, pointer coordinates, stale/invalid inputs and minimum-key
guards. Accepted edits use real global undo/redo with one history entry and leave
source files byte-identical. See
[`editor-curves-property-editing-2026-10-18.md`](../../benchmarks/bevy-upgrade/editor-curves-property-editing-2026-10-18.md).
The accepted frozen run executes **1,548 checks**, including **1,127 editor graph/
session/history/consumer checks**, with all **876 input hashes** unchanged.
Shipping passes **2,155 editor unit + 3 architecture tests**, with ten existing
ignores. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain inputs remain unchanged. This qualifies
the editing prerequisites, not the outer activation/keyboard/dock-opening,
host-curve consumers, full panel/plugin or native workflows. Next: those actual
Curves consumers, then complete composition/plugin integration. Shipping remains
on 0.19.1.

Implemented slice 57 (2026-10-18): **Curves activation, keyboard and action routing**.
The candidate executes the actual activation observer, Insert/Delete keyboard
consumer and full Curves dispatcher. Original semantic action branches are
unchanged; a version-specific boundary retains the shipping Interaction query
and maps 0.20 marker transitions/removals to the same phases. Fifteen host-curve
helpers, the complete persisted workspace model and actual panel reveal function
are shared production dependencies. Native plugin installation, host construction
and four root test blocks retain their contracts. Twenty-three shared tests cover
activation consumption/idle guards, styling, property/vector/gradient commands,
host-transform actions, keyboard focus/hover guards, production dock transitions
and globally reversible edits. Action fixtures keep Curves active to avoid saving
user settings; closed-panel reveal persistence and native floating windows remain
separate. See
[`editor-curves-action-routing-2026-10-18.md`](../../benchmarks/bevy-upgrade/editor-curves-action-routing-2026-10-18.md).
The accepted frozen run executes **1,571 checks**, including **1,150 editor graph/
session/history/consumer checks**, with all **884 input hashes** unchanged.
Shipping passes **2,178 editor unit + 3 architecture tests**, with ten existing
ignores. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain inputs remain unchanged. Next: complete
Curves panel composition and host graph drag observers, then full plugin
integration and native workflows. Shipping remains on 0.19.1.

Implemented slice 58 (2026-10-18): **complete Curves panel and host graph composition**.
The candidate builds the actual property/host-transform scenes and attached graph
observers. Eleven original scene helpers, the complete host constructor, labels/
markers and properties stack-button helper are shared. Timeline and Curves reuse
one version-specific pointer/cursor boundary, with visibility-only changes and a
typed test drag-end builder. Original construction/observer contracts, native
Curves plugin wiring, host installation and existing root test blocks are retained.
Eighteen shared tests cover scalar/vector/gradient/host controls, both locales and
empty/error states, actual commands, temporary raster/node/label previews,
single-edit history, quaternion retiming, Escape/late-release and secondary-button
guards, two scales, idle identity and nested teardown. See
[`editor-curves-panel-composition-2026-10-18.md`](../../benchmarks/bevy-upgrade/editor-curves-panel-composition-2026-10-18.md).
Accepted frozen qualification executes **1,589 checks**, including **1,168 editor
graph/session/history/consumer checks**, with all **889 input hashes** unchanged.
Shipping passes **2,196 editor unit + 3 architecture tests**, with ten existing
ignores. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain inputs remain unchanged. Explicit-geometry
typed-event checks do not establish native layout/picking/rendering, application
UI rebuilding, persisted scroll/floating-window lifecycle or full plugin schedules.
Earlier interaction hardening remains separate. Next: full Curves plugin
integration, then remaining viewport/application consumers and native workflows.
Shipping remains on 0.19.1.

Implemented slice 59 (2026-10-18): **full Curves plugin integration**.
The candidate installs the actual production plugin, system set and host
cancellation installer. State initialization, activation observer, keyboard/action
chain and host cancellation ordering retain their original bodies. Native Curves
and host root test blocks remain unchanged. All 41 existing action/panel tests now
install the real plugin instead of duplicating its wiring. Three new shared tests
verify missing/existing state ownership, exact executable system/set membership
and ordering, and gated activation delivered once without idle document changes.
All **71 focused Curves tests** pass. See
[`editor-curves-plugin-integration-2026-10-18.md`](../../benchmarks/bevy-upgrade/editor-curves-plugin-integration-2026-10-18.md).
Accepted frozen qualification executes **1,592 checks**, including **1,171 editor
graph/session/history/consumer checks**, with all **892 input hashes** unchanged.
Shipping passes **2,199 editor unit + 3 architecture tests**, with ten existing
ignores. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain inputs remain unchanged. This establishes
the Curves plugin's own headless schedule, not the entire application schedule,
native layout/picking/rendering, application rebuilding, persisted scrolling or
floating-window acceptance. Next: remaining viewport execution/navigation
consumers, then graph palette/menu and application handlers before native
workflows. Shipping remains on 0.19.1.

Implemented slice 60 (2026-10-18): **production viewport camera navigation**.
The candidate executes the original navigation consumer, complete camera
controller/state, transform helper and existing camera/proxy markers. Native
ViewportPlugin registration, all 70 remaining root functions and existing root
test block are unchanged. Eighteen shared tests (ten mirrored native regressions
and eight additional edge cases) preserve RMB orbit, MMB pan, WASD fly without
RMB, wheel zoom, framing, capture/release priority, focus/modal/Assets gating,
modifier sides, real-time motion and idle transform stability. No orbit-controller
replacement is included. See
[`editor-viewport-navigation-2026-10-18.md`](../../benchmarks/bevy-upgrade/editor-viewport-navigation-2026-10-18.md).
Accepted frozen qualification executes **1,610 checks**, including **1,189 editor
graph/session/history/consumer checks**, with all **895 input hashes** unchanged.
Shipping passes **2,217 editor unit + 3 architecture tests**, with ten existing
ignores. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain inputs are unchanged. These real Bevy
message/state consumer checks do not establish OS input delivery, rendered camera/
viewport lifecycle, full plugin/application scheduling or native editor workflows.
Next: viewport action activation/execution and camera viewport lifecycle, then
remaining graph palette/menu and application consumers. Shipping remains 0.19.1.

Implemented slice 61 (2026-10-18): **viewport action activation and execution**.
The candidate runs the actual action queue, dispatcher, complete executor,
display state and control audit. Native registration and Interaction filter are
preserved; the candidate maps changed native Pressed markers without altering
the existing pressed-plus-pending gate. Real settings persistence/localized status,
complete settings/photographic profile sources and transient maximized-panel state
are shared dependencies. Seventeen new shared tests cover real keyboard dispatch,
toolbar activation/consumption, navigation framing, all action payloads, localized
settings success/failure, malformed-file protection, set gating and audits. Effect
and semantic history remain unchanged; all test writes use temporary paths.
Fourteen existing settings and three photographic-profile tests also execute on
the candidate. Native plugin wiring, existing viewport/persistence/docking root
tests and remaining root functions are unchanged. See
[`editor-viewport-actions-2026-10-18.md`](../../benchmarks/bevy-upgrade/editor-viewport-actions-2026-10-18.md).
Accepted frozen qualification executes **1,644 checks**, including **1,223 editor
graph/session/history/consumer checks**, with all **903 input hashes** unchanged.
Shipping passes **2,234 editor unit + 3 architecture tests**, with ten existing
ignores. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain inputs remain unchanged.
These checks do not establish native toolbar composition, OS/pointer input,
camera viewport lifecycle, full plugin/application scheduling or editor workflows.
Next: camera viewport lifecycle, then remaining preview/render, graph palette/menu
and application consumers. Shipping remains 0.19.1.

Implemented slice 62 (2026-10-18): **camera viewport lifecycle**.
The candidate executes the three original viewport synchronization, activation
helper and gizmo overlay configuration functions unchanged. Native startup/
PostUpdate wiring, all 63 remaining root functions and existing viewport root
tests remain intact. Sixteen shared tests cover physical bounds/conversion,
valid resize, invalid and missing/ambiguous canvases, zero-size surface restore,
one-valid-update activation delay, exact gizmo-layer isolation, preserved camera
composition, nine explicit scale combinations and primary-window ownership. See
[`editor-viewport-camera-lifecycle-2026-10-18.md`](../../benchmarks/bevy-upgrade/editor-viewport-camera-lifecycle-2026-10-18.md).
Accepted frozen qualification executes **1,660 checks**, including **1,239 editor
graph/session/history/consumer checks**, with all **905 input hashes** unchanged.
Shipping passes **2,250 editor unit + 3 architecture tests**, with ten existing
ignores. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain inputs remain unchanged. These explicit
headless transitions do not establish native layout/DPI, surface restore, GPU
cluster/render-stage ordering, full application scheduling or floating windows.
Next: photographic preview/overlay composition, then remaining grid/display,
runtime preview and gizmo consumers. Shipping remains 0.19.1.

Implemented slice 63 (2026-10-18): **photographic preview and overlay composition**.
The candidate executes the two original consumers and applied-state caches.
Native plugin wiring, all 61 remaining viewport root functions and existing root
tests remain unchanged. Real settings and renderer photographic profile sources
are reused. Eighteen shared tests cover live normalized profiles, HDR/bloom/
tonemapping and legacy restoration, exact overlay blending/ownership, camera
recreation, idle change suppression, normalized cache equivalence, lifecycle
cooperation and unchanged effect/history. No files are written. See
[`editor-viewport-photographic-2026-10-18.md`](../../benchmarks/bevy-upgrade/editor-viewport-photographic-2026-10-18.md).
Accepted frozen qualification executes **1,678 checks**, including **1,257 editor
graph/session/history/consumer checks**, with all **907 input hashes** unchanged.
Shipping passes **2,268 editor unit + 3 architecture tests**, with ten existing
ignores. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain inputs remain unchanged.
These checks do not establish native HDR image quality, actual surface behavior,
GPU render-stage ordering or full viewport/application scheduling. Next:
grid/display presentation consumers, then runtime preview/gizmo integration
and native workflows. Shipping remains 0.19.1.

Implemented slice 64 (2026-10-18): **grid/display presentation consumers**.
The candidate executes six original grid/display/gizmo-feedback functions and the
original grid material/uniform definitions, using the complete production
PresentedEffect payload and real material binding/layout dependencies. Native
plugin wiring, all 55 remaining viewport root functions and existing root tests
remain unchanged. Twenty shared tests cover anchoring/spacing/fade, ownership,
missing/replaced assets, actual presentation mode routing, SVG/text feedback and
real viewport actions without authored edits. Eight existing renderer dependency
tests also run in the editor graph gate. A new required shader test validates the
unchanged production grid shader through Bevy 0.20's composition path and Naga,
including its actual material binding index and 96-byte uniform layout. See
[`editor-viewport-presentation-2026-10-18.md`](../../benchmarks/bevy-upgrade/editor-viewport-presentation-2026-10-18.md).
Accepted frozen qualification executes **1,707 checks**, including **1,285 editor
graph/session/history/consumer checks**, with all **910 input hashes** unchanged.
Shipping passes **2,288 editor unit + 3 architecture tests**, with ten existing
ignores. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain inputs remain unchanged.
Native material pipeline execution, image quality, full viewport/application
scheduling and runtime/gizmo workflows remain separate gates. Retained entity/
material identity does not imply idle-write optimization. Next: runtime preview/
player setup and project-instance reconciliation, then gizmo/host-motion
integration and remaining graph/application consumers. Shipping remains 0.19.1.

Implemented slice 65 (2026-10-18): **runtime preview/player reconciliation**.
The candidate executes eight unchanged production functions for root/child player
construction, project resolution, desired-instance collection, reconciliation and
live-edit classification, with their three original records. Actual session,
catalog, timeline and complete renderer payloads are used. Native plugin wiring,
all 47 remaining viewport root functions and existing root tests remain unchanged.
Twenty-four shared tests (nine adapted shipping regressions and fifteen additional
tests) cover clocks, seeds, parameter overrides, nested ancestry, catalog/material
refresh, invalid/missing source recovery, clip retirement/reentry, retained identity,
structural/live changes, seek fidelity and half-tick tolerance. Initial project
installation still invalidates checkpoints through the existing runtime revision;
authored state and semantic history remain unchanged. Filesystem fixtures use
temporary catalogs. See
[`editor-viewport-runtime-2026-10-18.md`](../../benchmarks/bevy-upgrade/editor-viewport-runtime-2026-10-18.md).
Accepted frozen qualification executes **1,731 checks**, including **1,309 editor
graph/session/history/consumer checks**, with all **912 input hashes** unchanged.
Shipping passes **2,312 editor unit + 3 architecture tests**, with ten existing
ignores. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain inputs remain unchanged.
Native simulation/rendering, full viewport scheduling, remaining mock binding/
output/profiling consumers and gizmo/host-motion integration remain separate gates.
Shipping remains 0.19.1.

Implemented slice 66 (2026-10-18): **preview feedback and profiler ingestion**.
Four unchanged production mock/input/output/drop consumers and complete profiler
aggregation/history state now execute in the candidate. Actual renderer output
messages and event-link statistics are used. Renderer statistics records and the
accumulator move unchanged behind existing public re-exports; native readback
production, both native plugin registrations, all 43 remaining viewport functions,
remaining profiler functions and existing root tests remain unchanged.
Nineteen shared feedback tests and eight shared profiler-ingestion tests (six
adapted shipping regressions) cover recording reuse/ownership, root output
filtering and bounds, dropped-link UI refresh policy, invalid/missing observations,
semantic-state preservation, measurement provenance, history/project identity and
peak reset. Two existing output-context routing tests also run in the candidate.
See
[`editor-viewport-feedback-2026-10-18.md`](../../benchmarks/bevy-upgrade/editor-viewport-feedback-2026-10-18.md).
Accepted frozen qualification executes **1,760 checks**, including **1,338 editor
graph/session/history/consumer checks**, with all **918 input hashes** unchanged.
Shipping passes **2,339 editor unit + 3 architecture tests**, with ten existing
ignores. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain inputs remain unchanged.
Existing output filtering remains root/path-only, without new epoch validation,
sorting or deduplication. GPU-observation-to-profile integration, native profiler
presentation, gizmo drawing and full application workflows remain separate gates.
Shipping remains 0.19.1.

Implemented slice 67 (2026-10-19): **GPU observations to preview profiles**.
The unchanged production `update_preview` consumer executes with actual session,
project, timeline, presentation and profiler types. Five small renderer modules
isolate existing observation records/read methods while preserving public namespace
re-exports. Eight record layouts, four model implementations, remaining producer
functions, native plugin registration, all 42 remaining viewport functions and the
existing test body remain unchanged after required parent visibility normalization.
Ten shared tests and nine candidate-only observation tests cover decoded counts,
geometry/timing/field/trail validity, stale contexts, backend provenance, nested
aggregation, unknown versus known-zero metrics, estimated peaks and unchanged
semantic history/clock/seed. Sixteen existing capability/statistics tests also run
in the editor candidate. No new public renderer test or constructor API is shipped.
See
[`editor-viewport-telemetry-2026-10-19.md`](../../benchmarks/bevy-upgrade/editor-viewport-telemetry-2026-10-19.md).
Accepted frozen qualification executes **1,795 checks**, including **1,373 editor
graph/session/history/consumer checks**, with all **927 input hashes** unchanged.
Shipping passes **2,349 editor unit + 3 architecture tests**, with ten existing
ignores, plus eight focused renderer timing regressions. Strict all-target Clippy
passes on both engines, alongside workspace all-target checking with
`aestra-bench/gpu`, both formatting checks and whitespace validation. Root dependency/
toolchain inputs remain unchanged. Native asynchronous delivery, full GPU workload,
visual/performance acceptance and complete application scheduling remain separate
gates. Next: profiler presentation/action/plugin consumers, then remaining viewport
gizmo/host-motion and graph/application integration. Shipping remains 0.19.1.

Implemented slice 68 (2026-10-19): **Profiler workspace, actions and plugin**.
The actual workspace, formatters, live labels, reset consumer and plugin execute
on both engines. All 20 presentation function bodies, the plugin implementation,
existing root tests and state/aggregation code remain unchanged. A narrow native
input boundary preserves Interaction/pending gating on 0.19 and uses Pressed on
0.20. Existing schedule membership and ordering remain unchanged; shared panel
chrome and real Feathers scenes are used.

23 shared cases include two mirrored existing regressions and 21 new tests for
metric provenance/formatting, both locales, real workspace/history/scroll/control
composition, retained updates, nested project text, invalid pending previews,
single-consumption resets, unchanged semantic history and plugin/set registration.
The focused candidate gate passes 31 cases including eight existing state tests.
See
[`editor-profiler-workspace-2026-10-19.md`](../../benchmarks/bevy-upgrade/editor-profiler-workspace-2026-10-19.md).
Accepted frozen qualification executes **1,818 checks**, including **1,396 editor
graph/session/history/consumer checks**, with all **935 input hashes** unchanged.
Shipping passes **2,372 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines, alongside
workspace GPU-feature all-target checking, both formatting checks and whitespace
validation. Root dependency/toolchain inputs remain unchanged; post-documentation
input drift is zero. This is headless composition/consumer/schedule evidence,
not native visual/GPU-measurement or complete application
acceptance. Next: remaining viewport gizmo/host-motion, then graph/application
consumers. Shipping remains 0.19.1.

Implemented slice 69 (2026-10-19): **Transform gizmos and host-motion viewport
consumers**. Twenty actual transform-gizmo functions and the complete host-motion
viewport module execute in the candidate with original state and real session,
timeline, registry, modal protection, camera and command/history implementations.
All extracted/remaining root function bodies, native plugin implementation,
existing root/host-motion test bodies and eleven record definitions are unchanged
after required parent visibility normalization. Existing crate re-exports remain.

36 shared cases include 13 mirrored regressions and 23 new tests; six existing
host-motion cases also execute unchanged in the candidate. They cover TRS
preview/commit/single-step undo, modifier/snapping transitions, stale/cancelled
drags, modal/focus ownership, fluid handles, actual readout/visual-root updates,
projected pose selection and real CPU gizmo-asset output. Pose selection retains
the playback clock. See
[`editor-viewport-transform-gizmo-2026-10-19.md`](../../benchmarks/bevy-upgrade/editor-viewport-transform-gizmo-2026-10-19.md).
The focused viewport gate passes 193 tests. Accepted frozen qualification executes
**1,860 checks**, including **1,438 editor graph/session/history/consumer checks**,
with all **940 input hashes** unchanged. Shipping passes **2,408 editor unit + 3
architecture tests**, with ten existing ignores. Both strict all-target Clippy
gates, workspace GPU-feature all-target checking, both formatting checks and
whitespace validation pass. Root dependency/toolchain inputs remain unchanged;
post-documentation input drift is zero. Native visual/rendering/picking acceptance
and complete viewport plugin installation/scheduling remain separate. Next:
shape-gizmo interactions/visualization, then preview composition/full-plugin
integration and graph/application workflows. Shipping remains 0.19.1.

Implemented slice 70 (2026-10-19): **Shape-gizmo interaction and visualization**.
The actual module shares 13 functions for target selection, projected handles,
lock gates, scalar drag/shape edits, transactional interaction, cursor/readouts,
geometry and overlay setup. All function bodies, native plugin implementation,
existing root tests and two extracted record definitions remain unchanged after
parent visibility normalization. Actual module-parameter projection is reused;
the candidate's shape-state dead-code allowance is removed. No new semantic,
cancellation, ownership or playback policy is introduced.

22 shared tests comprise three mirrored regressions and nineteen new cases:
all eight shape geometries/handles, dimensional independence, projection,
transformed scalar dragging, preview/release/undo, Escape and late release,
locks/menu/selection gates, both locales, live shape/handle drawing and actual
line-list/line-strip gizmo buffers. See
[`editor-viewport-shape-gizmo-2026-10-19.md`](../../benchmarks/bevy-upgrade/editor-viewport-shape-gizmo-2026-10-19.md).
Accepted frozen qualification executes **1,882 checks**, including **1,460 editor
graph/session/history/consumer checks**, with all **942 input hashes** unchanged.
Shipping passes **2,430 editor unit + 3 architecture tests**, with ten existing
ignores. Both strict all-target Clippy gates, workspace GPU-feature all-target
checking, both formatting gates and whitespace validation pass. Root dependency/
toolchain inputs remain unchanged; post-documentation input drift is zero.
This remains headless consumer/projection/CPU geometry evidence, not native visual,
rendering/picking or complete application acceptance. Next: remaining preview
setup/composition/status/stand-in drawing and full viewport-plugin integration,
then graph/application workflows. Shipping remains 0.19.1.

Implemented slice 71 (2026-10-19): **Preview scene, workspace and status consumers**.
Two actual viewport modules share the eight remaining production functions:
camera/grid/player/proxy startup, stand-in drawing, panel and seven-control
Feathers toolbar construction, SVG children and runtime status synchronization.
The original viewport-pane scene has its own shared module. Extracted bodies,
remaining root helpers, native plugin registration and original root tests remain
unchanged; the crate-level spawn_preview path and original status change gate
are retained. No dependency, format, semantic or playback policy changes.

24 new shared cases cover actual startup assets/payload and camera settings,
empty effects and nonduplicating updates, Still/Orbit/FlyBy stand-in geometry,
missing/orphan/removal cases, panel layout and native button classification,
both locales, accessible labels/tooltips, SVG paths and drag readouts, real
mode/display/frame/maximize activation, retained controls and all runtime backend/
error/change-gate states. See
[`editor-viewport-workspace-2026-10-19.md`](../../benchmarks/bevy-upgrade/editor-viewport-workspace-2026-10-19.md).
Accepted frozen qualification executes **1,906 checks**, including **1,484 editor
graph/session/history/consumer checks**, with all **947 input hashes** unchanged.
Shipping passes **2,454 editor unit + 3 architecture tests**, with ten existing
ignores. Both strict all-target Clippy gates, workspace GPU-feature all-target
checking, both formatting gates and whitespace validation pass. Root dependency/
toolchain inputs remain unchanged; post-documentation input drift is zero.
This remains headless composition/consumer/CPU geometry evidence, not native rendering,
complete viewport-plugin installation/scheduling or full application acceptance.
Next: complete viewport-plugin integration, then graph/application workflows.
Shipping remains 0.19.1.

Implemented slice 72 (2026-10-19): **Installed viewport-plugin integration**.
The candidate installs the actual complete main-world ViewportPlugin through a
shared module, retaining the native re-export path and unchanged plugin body,
root test helpers and existing root tests. It uses the actual renderer
AestraRenderSet and removes the obsolete runtime-preview/shortcut dead-code
allowances. No system ordering, run-condition, semantic or playback policy changes.

15 new shared integration tests install the real upstream gizmo logic/mesh plugins
and actual Feathers panel: startup/resources/retained scene identities, post-startup
overlay/material configuration, real keyboard and toolbar delivery to upstream
mesh mode and player display mode, modal/application-set gates, supplied layout
before gizmo execution, camera activation/resize/restore, seek/seed synchronization,
live transform edits and undo, empty documents and mock/preparation/telemetry
ordering. See
[`editor-viewport-plugin-2026-10-19.md`](../../benchmarks/bevy-upgrade/editor-viewport-plugin-2026-10-19.md).
Accepted frozen qualification executes **1,921 checks**, including **1,499 editor
graph/session/history/consumer checks**, with all **949 input hashes** unchanged.
Shipping passes **2,469 editor unit + 3 architecture tests**, with ten existing
ignores. Both strict all-target Clippy gates, workspace GPU-feature all-target
checking, both formatting gates and whitespace validation pass. Root dependency/
toolchain inputs remain unchanged; post-documentation input drift is zero.
This is headless installed
main-world plugin/scheduling evidence: computed layout and preparation observations
are supplied by test producers, not native UI layout or GPU dispatch. Native
renderer/viewport integration, full application workflows, visual/DPI and performance
acceptance remain separate. Shipping remains 0.19.1.

Implemented slice 73 (2026-10-19): **Material Add Node palette consumers**.
The actual palette module shares ten lifecycle/catalog/filtering/composition
functions and nine original records; the original input/edge mapping helpers
have a shared module with the crate-level input_target path retained. All twelve
extracted bodies, 105 remaining top-level graph functions, plugin registration
and original root tests are unchanged. Record changes are visibility-only. No
new semantic, selection, keyboard ownership, dismissal or playback policy.

21 shared palette cases comprise one mirrored category regression and twenty
new tests: hover/Tab coordinates and target scope, menu reset/dismissal, search/
category/no-match/clear/focus behavior, actual compiler/planner-compatible typed
options and stale identities, standalone restored targets, edge mappings, actual
Feathers popup/search/scroll/menu-item composition and complete action payloads,
both locales and real keyboard/clear input through the native text bridge.
The actual search module also brings its six existing cases into this editor gate.
See [`editor-material-palette-2026-10-19.md`](../../benchmarks/bevy-upgrade/editor-material-palette-2026-10-19.md).
Accepted frozen qualification executes **1,948 checks**, including **1,526 editor
graph/session/history/consumer checks**, with all **952 input hashes** unchanged.
Shipping passes **2,490 editor unit + 3 architecture tests**, with ten existing
ignores. Both strict all-target Clippy gates, workspace GPU-feature all-target
checking, both formatting gates and whitespace validation pass. Root dependency/
toolchain inputs remain unchanged; post-documentation input drift is zero.
This is headless consumer/
planner/real-Feathers composition evidence, not full graph plugin/application or
native visual acceptance. Semantic action dispatch and pointer-opening handlers
remain next. Shipping remains 0.19.1.

Implemented slice 74 (2026-10-19): **Material palette/context semantic dispatch**.
Actual Activate ownership, Add Node and all context-menu commands now share
production consumers across both engines. Only the changed/consumed input state
adapts from Interaction to Pressed; semantic mutation/history/selection and
native plugin registration stay unchanged. Original action records, arrangement
request, chrome dimensions, material node metrics and bounded placement source
are shared, without a candidate-only semantic implementation.

18 new shared consumer tests cover activation coalescing/ownership, pointer-only
rejection, failure paths, target/view isolation, creation/wiring/placement,
clipboard, duplicate/delete, extraction, connection reset, undo/redo and all four
actual arrangement request payloads. Six shared placement algorithm tests
include five existing regressions. The original measured shipping UI-layout
test remains intact. See
[`editor-material-menu-actions-2026-10-19.md`](../../benchmarks/bevy-upgrade/editor-material-menu-actions-2026-10-19.md)
for final evidence and limitations. Native pointer opening, asynchronous
arrangement completion, full graph/application workflows and visual/DPI acceptance
remain separate. Shipping remains 0.19.1.

Accepted frozen qualification passes **1,972 checks**, including **1,550 editor
checks**, with all **961 input hashes** unchanged. Shipping passes **2,509 editor
unit + 3 architecture tests**, with ten existing ignores. Both strict all-target
Clippy gates, workspace GPU-feature all-target checking, both formatting gates
and whitespace validation pass. Root dependency/toolchain inputs remain unchanged;
post-documentation input drift is zero.

Implemented slice 75 (2026-10-19): **Material canvas/node/pin pointer opening**.
Four actual menu-opening/canvas-selection consumers now share native click
type/location adapters, original socket/wire identities, measured anchor collection,
compact proxy geometry and actual Bézier hit-testing. Native registration,
semantic content and interaction policy remain unchanged. All 89 remaining
top-level graph function bodies and original root tests are unchanged.

23 shared consumer cases cover owning-view coordinates/target identity, node
and connection selection, connected/unconnected pins, wire picks, invalid targets,
scope isolation and propagation; two further tests cover actual compact proxy
state and dimensions. Single-delivery guards and global ancestor-fallback parity
are tested separately: a marker-only fixture is not full popup blocker integration.
See [`editor-material-menu-opening-2026-10-19.md`](../../benchmarks/bevy-upgrade/editor-material-menu-opening-2026-10-19.md)
for evidence and the retained propagation limitation. Full native picking/menu
composition and complete graph/application acceptance remain separate.

Accepted frozen qualification passes **1,997 checks**, including **1,575 editor
checks**, with all **967 input hashes** unchanged. Shipping passes **2,534 editor
unit + 3 architecture tests**, with ten existing ignores. Both strict all-target
Clippy gates, workspace GPU-feature all-target checking, both formatting gates
and whitespace validation pass. Root dependency/toolchain inputs remain unchanged;
post-documentation input drift is zero. Shipping remains 0.19.1.

Implemented slice 76 (2026-10-19): **Material connection drag/snap/finish**.
Twelve original lifecycle/semantic/wire-asset consumers and helpers share native
drag type/location adapters, original gesture/ghost/layer records, socket cache,
actual semantic planner and publication/history. All extracted bodies (apart
from API spelling), 77 remaining top-level graph functions, original root tests
and native plugin registration remain unchanged. The existing uniform-update
API remains re-exported for function graphs and insertion tests.

22 shared regressions cover start/motion/end gates, actual ghost assets,
forward/reverse and nearest-valid planner snapping, radius/cycle/missing-target
rejection, typed palette fallback, publication/undo/redo/no-op/failure, cached
offsets, selection/detach styling, two-view projection and real AssetEvent
idle-write behavior. See
[`editor-material-connection-drag-2026-10-19.md`](../../benchmarks/bevy-upgrade/editor-material-connection-drag-2026-10-19.md).
Original pointer ownership/cancellation, stale-snap and global ghost-retirement
policies are preserved, not hardened. Native PostLayout/render acceptance and
complete graph/application integration remain separate.

Final frozen qualification passes **2,019 checks**, including **1,597 editor
checks**, with all **973 input hashes** unchanged. The final-source shipping
rerun passes **2,556 editor unit + 3 architecture tests**, with ten existing
ignores. Both strict all-target Clippy gates, workspace GPU-feature all-target
checking, both formatting gates and whitespace validation pass. Root dependency/
toolchain inputs remain unchanged; post-documentation input drift is zero.
Shipping remains 0.19.1.

Implemented slice 77 (2026-10-19): **Material node selection/opening/modified drags**.
Five original node click/function-open/marquee/modified-drag and branch traversal
functions share native click boundaries, original scope/inline-control records
and the actual browser function-open intent. The extracted bodies, 72 remaining
graph functions, original root tests, graph plugin registration and browser
action bodies stay unchanged. Duplicate drag executes real semantic clipboard,
publication and history; branch drag executes real compiler projection/placement
and emits the actual presentation batch event.

22 shared cases cover modifiers, descendant gates/consumption, release suppression,
scoped marquee selection, retained-target function-open intent and stale rejection,
duplicate remapping/placement/undo/redo and failure, transitive branch/output/
collapse/seed-delta behavior, missing placements and stale drag identities. See
[`editor-material-node-actions-2026-10-19.md`](../../benchmarks/bevy-upgrade/editor-material-node-actions-2026-10-19.md).
Targeted actual-event tests do not install the unchanged global plugin routing;
function document opening and branch presentation-history consumption remain
separate. Native picking/rendering and complete graph/application integration
are not claimed by this headless gate.

Frozen qualification passes **2,041 checks**, including **1,619 editor checks**,
with all **978 input hashes** unchanged. Shipping passes **2,578 editor unit +
3 architecture tests**, with the same ten existing ignores. Both strict all-target
Clippy gates, workspace GPU-feature all-target checking, both formatting gates
and whitespace validation pass. Root dependency/toolchain inputs remain unchanged;
post-documentation input drift is zero. Shipping remains 0.19.1.

Implemented slice 78 (2026-10-19): **Material toolbar, preview and inline controls**.
Eleven original toolbar/preview/numeric/toggle/focus/synchronization functions
share native pressed/press-event boundaries, retaining Activate-required dispatch
and the original preview-toggle filter. The original preview transaction/context
recorder now executes real presentation history on both engines. Actual locate
intent and node-toggle marker identities retain their existing native paths.
The removed upstream TextScroll component is isolated at the version boundary.

25 shared cases cover scoped retained-target Add Node coordinates, arrangement/
locate intents and effect-context return, preview visibility with actual
presentation undo/redo, scalar/vector/bool semantic commits and undo/redo,
live/no-op/invalid rejection, focus routing and Added-only numeric bridge/width/
clip synchronization. See
[`editor-material-controls-2026-10-19.md`](../../benchmarks/bevy-upgrade/editor-material-controls-2026-10-19.md).
Original graph plugin registration, root tests, browser action bodies and all
61 remaining top-level graph functions stay unchanged. Existing pointer, stale-
action and numeric snapshot policies are preserved, not hardened.

Frozen qualification passes **2,066 checks**, including **1,644 editor checks**,
with all **983 input hashes** unchanged. Shipping passes **2,603 editor unit +
3 architecture tests**, with the same ten existing ignores. Both strict all-target
Clippy gates, workspace GPU-feature all-target checking, both formatting gates
and whitespace validation pass. Root dependencies/toolchain remain unchanged;
post-documentation input drift is zero. Shipping remains 0.19.1. Native picking, graph-zoom/DPI
rendering, complete numeric widget/graph composition and application acceptance
remain separate; no new visual reference is approved.

Implemented slice 79 (2026-10-19): **Material header, appearance and bootstrap layout**.
The actual material header/camera-key helpers, bootstrap layout/depth traversal,
node background policy, graph toolbar/frame composition and frame/snapping
controls now share production code across engines. Native interaction/pressed
boundaries preserve colors, Activate gating and consumption. Real Feathers
scenes and the SVG plugin are installed in the composed-control tests, without
candidate-only toolbar/layout/snapping implementations.

23 shared cases cover header child order/dimensions/action and scope/frame
identity, standalone/empty projections and localized preview labels, frame
dispatch, snapping defaults/toggles/locale/idle tooltips, node appearance/scope/
idle changes and deterministic bootstrap/depth behavior. See
[`editor-material-header-2026-10-19.md`](../../benchmarks/bevy-upgrade/editor-material-header-2026-10-19.md).
All remaining material graph bodies, original root tests and both native graph
plugin registrations stay unchanged. This is header composition, not the entire
canvas or full plugin schedule; native picking/rendering and graph zoom/DPI
acceptance remain separate. No visual reference is approved.

Frozen qualification passes **2,089 checks**, including **1,667 editor checks**,
with all **996 input hashes** unchanged. Shipping passes **2,626 editor unit +
3 architecture tests**, with the same ten existing ignores. Both strict all-target
Clippy gates, workspace GPU-feature all-target checking, both formatting gates
and whitespace validation pass. Root dependencies/toolchain remain unchanged;
post-documentation input drift is zero. Shipping remains 0.19.1.

Implemented slice 80 (2026-10-20): **Material canvas, nodes, sockets and wires**.
The shipping material workspace now delegates to the shared production canvas
builder. Generic viewport/node/socket/preview chrome, material node/output/wire
assembly, typed port presentation and immediate completed-preview cache mounting
are compiled directly by both engines. Narrow native Button aliases preserve
the original hit targets. Raster worker scheduling remains separate; function/
code-inspector branches, asset-drop and popup mounting stay native-host-owned.

16 shared real-Feathers/SVG cases cover layer geometry/order, document/view and
semantic identity, camera fallback/priority, scoped selection and shared pins,
wire endpoints/insertion metadata, typed sockets and hit dimensions, previews
and cache isolation, constant/inline controls, warning styling and empty graphs.
Both native graph plugin registrations and original root tests remain unchanged.
See [`editor-material-canvas-2026-10-20.md`](../../benchmarks/bevy-upgrade/editor-material-canvas-2026-10-20.md).
Frozen qualification passes **2,105 checks**, including **1,683 editor checks**,
with all **1,003 input hashes** unchanged. This is headless canvas composition,
not full plugin scheduling, native picking/rendering or rendered zoom/DPI
acceptance; no visual reference is approved. Shipping remains 0.19.1.
Shipping passes **2,642 editor unit + 3 architecture tests**, with the same ten
existing ignores. Both strict all-target Clippy gates, workspace GPU-feature
all-target checking, both formatting gates and whitespace validation pass;
post-documentation input drift is zero. Root dependencies/toolchain are unchanged.

Implemented slice 81 (2026-10-20): **Generic graph pin/collapse and socket visuals**.
Nine actual production restoration/memory-sync, pin/collapse activation/action,
body/icon and socket visual functions now run on both engines. Existing Feathers
activation classification preserves Activate-required, one-shot edits; native
pressed/hover boundaries include explicit marker-removal handling on 0.20.
Authored bases, temporary offsets, edit payloads, labels and colors are unchanged.

15 shared real-Feathers/SVG cases cover pin/collapse round trips and exact events,
bootstrap anchors, temporary displacement, activation gates/coalescing and stale
nodes, rebuilds, split-view/document isolation, idle change ticks and socket
hover/press/release/leave behavior, marker removal and unrelated entities.
Controlled socket states run the retained Update schedule, without claiming OS
pointer delivery. All six remaining generic graph functions, original root tests
and native graph plugin registration remain unchanged. See
[`editor-graph-node-controls-2026-10-20.md`](../../benchmarks/bevy-upgrade/editor-graph-node-controls-2026-10-20.md).
Frozen qualification passes **2,120 checks**, including **1,698 editor checks**,
with all **1,007 input hashes** unchanged. Both strict all-target Clippy gates
pass. Shipping passes **2,657 editor unit + 3 architecture tests**, with the same
ten existing ignores. Workspace GPU-feature all-target checking, both formatting
gates and whitespace validation pass; post-documentation input drift is zero.
Root dependencies/toolchain remain unchanged; shipping remains 0.19.1.

Implemented slice 82 (2026-10-20): **Viewport camera, grid and selection presentation**.
The six actual camera remembrance/restoration, grid attachment/uniform and
selection-outline functions now share unchanged production bodies on both
engines. Original graph plugin registration/order and root tests remain unchanged;
no top-level generic graph presentation functions remain in the native root.
The candidate composition's previous grid/material dead-code allowance is removed.

16 shared cases use actual Feathers/SVG graph composition and material assets to
cover Added-only restoration/order, seeded/uncached/split/rebuilt cameras, idle
memory, per-grid asset reuse and uniforms, missing owners/assets, same-frame
canvas/grid updates, outline zoom/ancestor policy, geometry isolation and idle
outlines. Existing per-frame grid writes and deselection-width policy are
preserved. See
[`editor-graph-viewport-presentation-2026-10-20.md`](../../benchmarks/bevy-upgrade/editor-graph-viewport-presentation-2026-10-20.md).
Frozen qualification passes **2,136 checks**, including **1,714 editor checks**,
with all **1,009 input hashes** unchanged. Both strict all-target Clippy gates
pass. Shipping passes **2,673 editor unit + 3 architecture tests**, with the same
ten existing ignores. Workspace GPU-feature all-target checking, both formatting
gates and whitespace validation pass; post-documentation input drift is zero.
Root dependencies/toolchain remain unchanged; shipping remains 0.19.1.
This is headless asset/component/schedule-subset evidence, not native material
shader rendering, zoom/DPI visuals or complete graph-plugin acceptance.

Implemented slice 83 (2026-10-20): **Generic graph registration and scheduling**.
The shipping plugin and isolated candidate now share the production resources,
observers, embedded-control filter and exact Update/PostUpdate chain through
`feathers/node_graph/integration.rs`. Geometry collection, measured framing,
session overlays and drag guides now use the complete native modules on 0.20,
including 24 previously unqualified existing cases. Shader embedding stays in
the native wrapper to preserve its asset namespace; the old geometry facade is removed.

Ten new integration cases cover deferred host rebuild/readback, real material
registration and canvas/grid ordering, action consumption, split-view cameras,
drag teardown, pending and measured framing, and embedded controls through native
event propagation. Composition exposed and fixes a real bug on both engines:
node press/drag-start and marquee classification retain the original pointer
target, so bubbling cannot reinterpret an excluded control as a parent-node drag.
The registration pipeline and original root test bodies are preserved. See
[`editor-graph-integration-2026-10-20.md`](../../benchmarks/bevy-upgrade/editor-graph-integration-2026-10-20.md).
Frozen qualification passes **2,185 checks**, including **1,753 editor graph
checks**, with all **1,011 input hashes** unchanged. Shipping passes **2,688
editor unit + 3 architecture tests** with default parallel execution and the
same ten existing ignores. Both strict all-target Clippy gates, workspace GPU-feature
all-target checking, both formatting gates and whitespace validation pass.
Root dependencies/toolchain remain unchanged; shipping remains 0.19.1.
This is generic graph headless integration, not full MaterialGraphPlugin,
native picking/shader rendering or complete editor workflow acceptance.

Implemented slice 84 (2026-10-20): **Material graph host lifecycle and wire scheduling**.
`material_graph/host_integration.rs` now shares the native gesture/palette/selection/
function-menu resource setup, document-transient cleanup, four connection observers
and post-layout wire chain. The shipping plugin delegates this setup. The 22
existing connection cases use the real registrar instead of a selected test-only
chain, with their assertions unchanged. EditorSet's unchanged identity is shared
from `shell/sets.rs`.

Ten new cases cover initial/stable/changed identities, project and effect-history
boundaries, optional-resource cleanup, placement/history preservation, queued
cleanup, same-frame identity/rebuild/graph ordering, and deferred wire attachment
after transform and UI layout. A missing ordering edge is fixed: cleanup now
observes MainUpdate's target changes before UiRebuild. The cleanup bodies and
original material-root/connection assertions are unchanged. See
[`editor-material-host-lifecycle-2026-10-20.md`](../../benchmarks/bevy-upgrade/editor-material-host-lifecycle-2026-10-20.md).
Frozen qualification passes **2,195 checks**, including **1,763 editor graph
checks**, with all **1,014 input hashes** unchanged. The full candidate graph
suite passes with default parallel execution too. Shipping passes **2,698 editor
unit + 3 architecture tests**, also parallel, with the same ten existing ignores.
Both strict all-target Clippy gates, both formatting gates and whitespace checks
pass. Root dependencies/toolchain remain unchanged; shipping remains 0.19.1.
This remains headless host-lifecycle/connection evidence, not full material-plugin
or native editor acceptance.
Workspace GPU-feature all-target checking passes; post-documentation source drift is zero.

Implemented slice 85 (2026-10-20): **Project-bound material graph layout persistence**.
`material_graph/layout_persistence.rs` shares production loading/restoration,
debounced writing, exit flush and registration. The native plugin delegates
this setup. PersistenceSet's unchanged identity is shared from `persistence/sets.rs`.
The candidate directly imports the real layout lifecycle and function-layout
modules, retaining the disk schema, 300 ms delay, corruption protection,
project namespaces, base placements and focused visible camera ownership.

Nine new complete-registrar cases cover startup, delayed/exit writes, corruption,
unsupported files, idle ticks, clean-exit byte preservation, late catalog changes
and queued project-switch cleanup before rebuild. Six existing lifecycle and five
function-layout cases now run on both engines. The existing full function-canvas
restart test remains unchanged in the shipping suite until the complete function
panel is ported; no assertion is replaced or ignored. All extracted persistence,
lifecycle and camera bodies and original material-root tests are unchanged. See
[`editor-material-layout-persistence-2026-10-20.md`](../../benchmarks/bevy-upgrade/editor-material-layout-persistence-2026-10-20.md).
Frozen qualification passes **2,215 checks**, including **1,783 editor graph
checks**, with all **1,021 input hashes** unchanged. The candidate graph suite
also passes in parallel. Both strict all-target Clippy gates, both formatting
gates and whitespace validation pass. Root dependencies/toolchain remain unchanged;
shipping remains 0.19.1. This is headless persistence/scheduling evidence, not
complete material-plugin or native editor acceptance.
Shipping passes **2,707 editor unit + 3 architecture tests** with default parallel
execution and the same ten existing ignores. Workspace GPU-feature all-target
checking passes; post-documentation input drift is zero.

Implemented slice 86 (2026-10-20): **Material preview-worker orchestration**.
The native plugin now delegates the actual `node_previews::register` setup.
The candidate imports the real worker and shared `preview_pixels.rs` evaluator,
not a replacement raster implementation. Worker limits, dependency-closure
matching, project/target/instance keys, cached-image reuse, cancellation and retry
behavior remain unchanged, as do all 25 extracted evaluator/helper bodies.

Nine new orchestration cases exercise the actual registrar, duplicate views,
independent instance/target keys, bounded slots and refilling, close/drop
cancellation, stale completed results, cached replacement, same-ID project switches,
failed-job retry and preserved semantic state. Two further pixel cases check
scanline cancellation and strict-versus-permissive fallback. Three existing worker
cases and three unchanged pixel/basis tests now execute on both engines. Remaining
native material-root tests are unchanged. See
[`editor-material-preview-workers-2026-10-20.md`](../../benchmarks/bevy-upgrade/editor-material-preview-workers-2026-10-20.md).
This is headless CPU worker/image-publication evidence, not native GPU preview or
complete material-plugin/editor acceptance. Preset and asset-thumbnail orchestration
remain separate candidate integration work. Shipping remains 0.19.1.
Frozen qualification passes **2,232 checks**, including **1,800 editor graph
checks**, with all **1,024 input hashes** unchanged. The graph suite also passes
in parallel. Both strict all-target Clippy gates, both formatting gates and
whitespace validation pass. Root dependencies/toolchain remain unchanged.
Shipping passes **2,718 editor unit + 3 architecture tests** with default parallel
execution and the same ten existing ignores. Workspace GPU-feature all-target
checking passes; post-documentation input drift is zero.

Implemented slice 87 (2026-10-20): **Material-preset preview pipeline**.
`material_graph/preset_previews.rs` shares actual composition, cache/status state,
Update registration, rasterization, recipe fingerprinting, program resolution and
material seed. The native plugin delegates registration. All seven extracted
function bodies, recipe-only caching, publication-version observation, fallback,
status presentation and synchronous raster behavior remain unchanged.

Eleven new cases cover real composition and registration, deduplication, idle ticks,
late-added views, metadata/recipe publication, removal/recovery, same-ID project
switches, malformed-file indexing, actual catalog-error fallback, status transitions,
missing labels and resolved-program pixel parity. Three existing preset checks
now run on both engines with unchanged assertions; the bundled fixture locator
finds the same assets from both manifests. Frozen inputs include all bundled
preset recipes. Remaining native material-root tests are unchanged. See
[`editor-material-preset-previews-2026-10-20.md`](../../benchmarks/bevy-upgrade/editor-material-preset-previews-2026-10-20.md).
This is headless CPU preset/image/status evidence, not browser GPU thumbnail
or full material-plugin/editor acceptance. Shipping remains 0.19.1.
Frozen qualification passes **2,246 checks**, including **1,814 editor graph
checks**, with all **1,035 input hashes** unchanged. The graph suite also passes
in parallel. Both strict all-target Clippy gates, both formatting gates and
whitespace checks pass. Root dependencies/toolchain remain unchanged.
Shipping passes **2,729 editor unit + 3 architecture tests** with default parallel
execution and the same ten existing ignores. Workspace GPU-feature all-target
checking passes; post-documentation input drift is zero.

Implemented slice 88 (2026-10-20): **Saved-material asset-preview validation**.
The candidate imports the actual `material_graph/asset_preview.rs` entry point,
with explicit production dependencies and canonical preset/evaluator paths.
The validator and recursive traversal bodies, all three original tests and
remaining native material-root tests are unchanged. Eleven new cases qualify
exact/one-over bounds, memoized shared-subgraph depth, scene displacement,
disconnected duplicate IDs, every unsupported form, unselected branches,
disabled-node bypass, saved defaults, project function libraries, nonfinite
outputs and cancellation precedence. The existing thumbnail limits and strict
CPU fallback policy are not runtime limits and are not changed by this port.
See
[`editor-material-asset-previews-2026-10-20.md`](../../benchmarks/bevy-upgrade/editor-material-asset-previews-2026-10-20.md).
This is headless saved-material CPU validation, not browser GPU thumbnail
or full editor acceptance. Function-library validation and remaining browser/
workspace/plugin integration are separate responsibilities. Shipping remains 0.19.1.
Frozen qualification passes **2,260 checks**, including **1,828 editor graph
checks**, with all **1,037 input hashes** unchanged. The graph suite also passes
in parallel. Both strict all-target Clippy gates, both formatting gates and
whitespace checks pass. Root dependencies/toolchain remain unchanged.
Shipping passes **2,740 editor unit + 3 architecture tests** with default parallel
execution and the same ten existing ignores. Workspace GPU-feature all-target
checking passes; post-documentation input drift is zero.

Implemented slice 89 (2026-10-20): **Material/effect workspace host composition**.
The native material/effect branch now delegates to the same `workspace.rs` imported
by the 0.20 candidate, using actual header/canvas/palette/drop components. Explicit
target projection and drop identity/checks are shared. The native function-editor
branch and complete drop execution stay unchanged and are not claimed as ported.
Seven workspace and three drop-identity cases cover restored targets before focus,
instance binding, split cameras/shared placement, English/French empty states,
invalid drafts, palette routing and stale identity. A regression reproduced
cross-view node-menu leakage; node and connection menus now require their owning
view as well as program. No action or semantic edit policy changes.
Projection/drop methods, menu constructors, native function composition, original
material-root tests and remaining drop business logic pass unchanged-source audits.
See
[`editor-material-workspace-2026-10-20.md`](../../benchmarks/bevy-upgrade/editor-material-workspace-2026-10-20.md).
Frozen qualification passes **2,270 checks**, including **1,838 editor graph checks**,
with all **1,044 input hashes** unchanged; the parallel graph suite also passes.
Both strict all-target Clippy gates, both formatting gates and whitespace validation
pass. Shipping passes **2,750 editor unit + 3 architecture tests**, with the same
ten existing ignores. Workspace GPU-feature all-target checking passes;
post-documentation input drift is zero.
This is headless material/effect composition, not function-graph composition,
browser drop/GPU capture, full material-plugin or complete editor acceptance.
Shipping remains 0.19.1; dependencies/toolchain and visual references are unchanged.

Implemented slice 90 (2026-10-20): **Function-native canvas composition**.
Both engines now compile the actual function toolbar/canvas/context menus from
`material_function_editor/graph/composition.rs`, with shared real component types.
No surrogate material program or test-only canvas is created. The native input
and wire-handler bodies (31), remaining insertion business logic and original
function tests are unchanged. Actual compact/icon/searchable action menus and
function-wire identity are shared as dependencies, with their behavior preserved.
Eight new cases cover explicit unfocused targets, socket/wire/output identities,
split cameras and scoped toolbar actions, missing/custom-WESL targets, view-owned
menus, typed constant controls, actual search filtering, invalid drafts and
bootstrap/semantic target mapping. The three original menu tests also run on 0.20;
all 12 assertions remain, with complete headless UI/visibility/window prerequisites
and PostUpdate focus acquisition added to their fixtures.
See
[`editor-function-canvas-2026-10-20.md`](../../benchmarks/bevy-upgrade/editor-function-canvas-2026-10-20.md).
Frozen qualification passes **2,281 checks**, including **1,849 editor graph checks**,
with all **1,050 input hashes** unchanged. The graph suite also passes in parallel.
Shipping passes **2,758 editor unit + 3 architecture tests**, with the same ten
existing ignores. Both strict all-target Clippy gates, both formatting gates,
whitespace and workspace GPU-feature all-target checks pass. Post-documentation
input drift is zero. Function signature fallback,
complete workspace host composition and action/input registration remain separate
qualification boundaries. Shipping remains 0.19.1; no visual reference is approved.

Implemented slice 91 (2026-10-20): **Complete graph workspace host and function signatures**.
The candidate and shipping host now compose the same full material/effect/function
workspace dispatch from `workspace_host.rs`, including real inspection header and
scroll fallback. The shipping entry point remains a canonical re-export with its
original signature. Actual function signature controls/formatting and component
identities are shared; custom-WESL signatures remain read-only. All four composition/
formatting bodies, eight native signature handler/helper bodies and original
signature/material-root tests are unchanged.
Two real-host regressions reproduced fallback reads of the active graph function
inside an unfocused code-function tab and unavailable/wrong-project tab. Both
inspection text and signature controls now resolve the explicit tab target.
With no override, the tool still follows the current session target. Five host
and four signature cases cover routing, source/control isolation, scroll/drop/canvas
identity, localized Back/empty states, typed port/actions and saved/required defaults.
See
[`editor-graph-workspace-host-2026-10-20.md`](../../benchmarks/bevy-upgrade/editor-graph-workspace-host-2026-10-20.md).
Frozen qualification passes **2,290 checks**, including **1,858 editor graph checks**,
with all **1,055 input hashes** unchanged; the parallel graph suite also passes.
Shipping passes **2,767 editor unit + 3 architecture tests**, with the same ten
existing ignores. Both strict all-target Clippy gates, both formatting gates,
whitespace and workspace GPU-feature all-target checks pass. Post-documentation
input drift is zero. This is complete workspace
composition, not native signature/function observer dispatch, rendered wires or
full editor acceptance. Shipping remains 0.19.1; no visual reference is approved.

Implemented slice 92 (2026-10-21): **Function signature events and semantic placement**.
The candidate now registers the actual signature text/number/action observers and
uses production publication, semantic placement and the global history observer.
The immutable layout contract and native/partial engines are shared directly;
all signature handler/helper, placement, node-size and history observer bodies are
unchanged. Shipping registration order and existing validation policy are retained.
Only the private qualification manifest/lockfile adds the already-pinned
`elkrs = "=0.1.1"` dev dependency for unchanged reference layout tests; shipping
dependencies, lockfile, toolchain and CI are unchanged.
Ten new composed-control cases cover final/live edits, typed defaults, no-op/error
history, stale owners, real indexed caller validation, Add Input/Output placement,
pin preservation, keyboard Enter and global compound undo/redo. Another 52 existing
placement/layout checks now run on the candidate without substitute engines.
See
[`editor-function-signature-events-2026-10-21.md`](../../benchmarks/bevy-upgrade/editor-function-signature-events-2026-10-21.md).
Frozen qualification passes **2,352 checks**, including **1,920 editor graph checks**,
with all **1,069 input hashes** unchanged; the parallel graph suite also passes.
Shipping passes **2,777 editor unit + 3 architecture tests**, with the same ten
existing ignores. Both strict all-target Clippy gates, both formatting gates,
whitespace and workspace GPU-feature all-target checks pass. Post-documentation
input drift is zero. This qualifies headless signature event integration, not
function-graph pointer consumers, rendered wires or complete native acceptance.
Shipping remains 0.19.1; no visual reference is approved.

Implemented slice 93 (2026-10-21): **Function body actions and constant editing**.
The candidate now uses the production node-create/remove, input and Boolean action
observer plus constant text/number observers and their actual semantic helpers.
Native and candidate controls retain the same component identities, target
activation, publication, placement and global compound history. All six moved
bodies and 29 remaining graph handler/helper/test bodies are unchanged; all 16
native graph observers preserve their order through shared subregistrars.
Ten real-composition cases cover creation, removal, typed and live/final/no-op/error
edits, pinned placement, explicit owner activation, scoped toolbar requests, stale
project controls, keyboard Enter and creation without viewport memory.
See
[`editor-function-body-actions-2026-10-21.md`](../../benchmarks/bevy-upgrade/editor-function-body-actions-2026-10-21.md).
Frozen qualification passes **2,362 checks**, including **1,930 editor graph checks**,
with all **1,071 input hashes** unchanged; the parallel graph suite also passes.
Shipping passes **2,787 editor unit + 3 architecture tests**, with the same ten
existing ignores. Both strict all-target Clippy gates, both formatting gates,
whitespace and workspace GPU-feature all-target checks pass. Post-documentation
input drift is zero.
This is headless semantic action/value integration, not function-graph pointer,
context-menu/wire or full native acceptance. Shipping remains 0.19.1; dependencies,
toolchain and visual references are unchanged.

Implemented slice 94 (2026-10-21): **Function socket and selection pointer events**.
Actual function socket drag/drop lifecycle, preview state, node/canvas/marquee
selection and nested-call navigation are shared with the candidate. The existing
native drag/click boundaries are reused within the crate, with native DragDrop
aliases/test construction added. All 16 graph observers keep their order.
The real compatible-node palette is compiled directly; all 12 production bodies,
eight existing tests and 35 assertions are preserved. Ten new native-event cases
exercise composed descendants, view/owner guards, release-time snapping at three
DPI/zoom combinations, single commit/undo, cancellation and scoped selection.
A real constant-field regression reproduced selection changes during bubbling.
Click handlers now resolve from the originally picked descendant, retaining
control/socket guards as the current event target walks ancestors on both engines.
See
[`editor-function-pointer-events-2026-10-21.md`](../../benchmarks/bevy-upgrade/editor-function-pointer-events-2026-10-21.md).
The candidate graph suite passes **1,948 checks** in parallel, with the existing
ignored timing probe. Shipping passes **2,797 editor unit + 3 architecture tests**,
with the same ten existing ignores. Both strict all-target Clippy gates, both
formatting gates, whitespace and workspace GPU-feature all-target checks pass.
Frozen qualification passes **2,380 checks**, including **1,948 editor graph checks**,
with all **1,076 input hashes** unchanged. Post-documentation input drift is zero.
This is headless native-event/UI-layout
qualification, not OS picking, GPU wire or complete native acceptance. Shipping
remains 0.19.1; dependencies, toolchain and visual references are unchanged.

Implemented slice 95 (2026-10-21): **Function context menus and modified drags**.
The candidate compiles actual node/pin/canvas context-menu hit testing, opening,
action dispatch and dismissal, together with duplication and modified-node drags.
The real presentation batch observer/context resolver is shared so branch movement
reaches actual global history. Seven context and three modified-drag bodies, four
batch/context bodies, eight remaining graph wire/test bodies and the original
arrangement-context test are preserved apart from native location/module access.
All 16 graph observers keep their order. Ten new real-composition cases exercise
grouped duplicate/delete, clipboard paste, required-source rejection, scoped
arrangement requests, hover/Escape, unknown targets and positioned-graph drag
undo/redo. Explicit placement fixtures retain the existing bootstrap-only policy.
See
[`editor-function-context-drags-2026-10-21.md`](../../benchmarks/bevy-upgrade/editor-function-context-drags-2026-10-21.md).
Frozen qualification passes **2,391 checks**, including **1,959 editor graph checks**,
with all **1,081 input hashes** unchanged; the parallel graph suite also passes.
Both strict all-target Clippy gates, both formatting gates and whitespace checks pass.
Shipping passes **2,807 editor unit + 3 architecture tests**, with the same ten
existing ignores. Workspace GPU-feature all-target checking passes;
post-documentation input drift is zero. This is headless menu/event
and positioned-graph history qualification, not OS picking, GPU wire or complete
native acceptance. Shipping remains 0.19.1; dependencies, toolchain and visual
references are unchanged.

Implemented slice 96 (2026-10-21): **Function wires and complete registration**.
Both original wire systems and the complete graph/function-editor registrars now
compile on the candidate. Shipping uses re-exported entry points; all 16 graph
observers retain their order, root graph-before-signature registration is unchanged,
and wire updates remain after PostLayout. Source comparison preserves both wire
bodies, the expanded registrar and all six original graph test bodies. Eight new
shared cases use actual composition, assets, layout, native events and the complete
production registrar: resource ownership, optional-asset recovery, stable handles,
saved-camera rebuilds at multiple pans/zooms/DPI, split-view endpoints/ghosts,
logical snapping/release undo, input-direction ghosts, stale-origin cleanup and
simultaneous signature/body/selection/clipboard/context consumers. No production
policy or dependency changes. See
[`editor-function-wires-registration-2026-10-21.md`](../../benchmarks/bevy-upgrade/editor-function-wires-registration-2026-10-21.md).
The parallel candidate graph suite passes **1,967 tests**, with one existing ignore.
Frozen qualification passes **2,399 checks**, including **1,967 editor graph checks**,
with all **1,085 input hashes** unchanged; post-documentation input drift is zero.
Shipping passes **2,815 editor unit + 3 architecture tests**, with the same ten
existing ignores. Both strict all-target Clippy gates, both formatting gates,
whitespace checks and workspace GPU-feature all-target checking pass. This remains
headless layout/event/wire-uniform qualification, not OS picking, GPU wire
appearance or full native acceptance. Shipping stays 0.19.1.

Implemented slice 97 (2026-10-21): **Material presentation consumers and arrangement
installation**. The original node/pin observers and atomic arrangement-result
installer now compile on the candidate through a shared production source. A shared
node/batch/pin registrar preserves the shipping plugin's exact registration point
and observer order. Source comparison preserves all three production bodies and the
expanded plugin build. All 16 existing history cases run on the candidate, retaining
all 65 assertions; two pressed fixtures use the existing native input boundary.
Eight new shared cases cover scoped program/function move-pin-batch chronology,
unfocused origin routing, no-op/unknown-event redo preservation, malformed/nonfinite
arrangement rejection, semantic/pin/project staleness, function arrangement
undo/redo, temporary-offset handling and preservation of camera, other graphs and
saved bytes.
See
[`editor-material-presentation-edits-2026-10-21.md`](../../benchmarks/bevy-upgrade/editor-material-presentation-edits-2026-10-21.md).
The parallel candidate graph suite passes **1,991 tests**, with one existing ignore.
Frozen qualification passes **2,423 checks**, including **1,991 editor graph checks**,
with all **1,087 input hashes** unchanged; post-documentation input drift is zero.
Shipping passes **2,823 editor unit + 3 architecture tests**, with the same ten
existing ignores. Both strict all-target Clippy gates, both formatting gates,
whitespace checks and workspace GPU-feature all-target checking pass. This remains
CPU ECS event/history and arrangement-result qualification, not asynchronous
solver completion or full native acceptance. Shipping stays 0.19.1.

Implemented slice 98 (2026-10-21): **Asynchronous arrangement coordinator and layout
adapter**. The candidate now compiles the actual complete coordinator and adapter,
using one canonical request type and unchanged registration, preparation, task,
polling, stale-result and installation bodies. Source comparison preserves nine
production coordinator/helper bodies, the existing restored-tab test and all five
adapter method bodies. The three original adapter cases also run on the candidate.
Eight new shared cases use actual composed/measured canvases, production geometry,
real CPU tasks and global undo: unfocused full layout, partial scope/frozen nodes,
pinned anchors, busy/idle behavior, invalid requests, stale movement/pinning/drafts,
and retired/remeasured views. The fixture uses the production vertical function
panel layout; it does not manufacture measurements or bypass readiness.
See
[`editor-material-async-arrangement-2026-10-21.md`](../../benchmarks/bevy-upgrade/editor-material-async-arrangement-2026-10-21.md).
Frozen qualification passes **2,435 checks**, including **2,003 editor graph checks**,
with all **1,090 input hashes** unchanged; the parallel graph suite also passes.
Post-documentation input drift is zero. Shipping passes **2,831 editor unit + 3
architecture tests**, with the same ten existing ignores. Both strict all-target
Clippy gates, both formatting gates, whitespace checks and workspace GPU-feature
all-target checking pass. This is real CPU task/geometry/history qualification,
not full native editor acceptance. Shipping stays 0.19.1.

Implemented slice 99 (2026-10-21): **Semantic drag-on-wire insertion adapter**.
The candidate compiles the complete production adapter and exposes it through the
canonical material-graph facade; the function-wire helper now comes from its actual
child module, not a standalone qualification module. All six production handler
bodies and all five original test bodies with their 30 assertions are unchanged.
Five new shared cases exercise actual registration/probes/drop/global history:
unfocused source routing, feedback rejection, retired/retargeted views, rollback
after a missing optional function editor, and literal-input replacement with
deduplicated choices while retaining the sixteen-choice bound. Native root imports
are cleaned up without changing production policy; the widget's obsolete candidate
dead-code allowance is removed.
See
[`editor-material-insertion-2026-10-21.md`](../../benchmarks/bevy-upgrade/editor-material-insertion-2026-10-21.md).
Frozen qualification passes **2,445 checks**, including **2,013 editor graph checks**,
with all **1,092 input hashes** unchanged; the parallel graph suite also passes.
Post-documentation input drift is zero. Shipping passes **2,836 editor unit + 3
architecture tests**, with the same ten existing ignores. Both strict all-target
Clippy gates, both formatting gates, whitespace checks and workspace GPU-feature
all-target checking pass. Candidate events/viewport metadata qualify semantic
consumption, placement and history, not full rendered picking or native acceptance.
Shipping stays 0.19.1.

Implemented slice 100 (2026-10-21): **Complete material graph plugin registration**.
Both roots compile the actual plugin build from `material_graph/registration.rs`,
with its registrar/observer order, Update consumers and keyboard registration
unchanged. The candidate now compiles the complete browser function-drop adapter
and common owner-scoped feedback, not only target identity. Native pointer event
types/location accessors are the only engine boundary; routing, validation, history
and edit policy stay shared. Existing browser-drop and feedback regressions run on
the candidate; all 26 browser-drop assertions remain. Three new shared cases run
the actual material and function registrations together with real input, workspace
composition and measured layout: idle/source preservation, scoped node click/menu
and Escape, and single-commit browser drop with global undo/redo and unchanged files.
See
[`editor-material-plugin-registration-2026-10-21.md`](../../benchmarks/bevy-upgrade/editor-material-plugin-registration-2026-10-21.md).
Frozen qualification passes **2,459 checks**, including **2,027 editor graph checks**,
with all **1,098 input hashes** unchanged; the parallel graph suite also passes.
Post-documentation input drift is zero. Shipping passes **2,839 editor unit + 3
architecture tests**, with the same ten existing ignores. Both strict all-target
Clippy gates, both formatting gates, whitespace checks and workspace GPU-feature
all-target checking pass. The existing modal-state test factory keeps the older
protection fixture lint-clean without exposing native private fields.
This is combined headless ECS/UI qualification, not OS picking or native GPU/editor
acceptance. Shipping stays 0.19.1.

B20-2 remains in progress: remaining application action
dispatch and full workflow acceptance are not yet ported. The merged
0.20 group body tokens do not
by themselves preserve the legacy darker settings header; retain that styling
explicitly during the scene port. No optional control replacement is included.

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

**Next action:** build and launch the actual 0.20 editor, fix startup/runtime errors,
then run complete workspace tests/lints and native editor workflow acceptance.
The root graph and qualified adapters are now switched; do not add more isolated
slices or make optional widget adoption a prerequisite for the compatibility merge.
Keep current controls, geometry, action routing and undo behavior. Retire retained
0.19 boundary copies only after the full regression gate passes. Refresh visual and
performance evidence rather than approving changed references.
