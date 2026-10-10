# B20-1: installed queue/schedule integration — 2026-10-10

Shipping remains Bevy **0.19.1**; the independent candidate is locked to **0.20.0**.
This slice qualifies actual render installation and queue execution, not a coherent
workspace engine switch or completed B20-1.

## Implementation and reproduced defects

The candidate imports production `gpu/render.rs` directly. Ordering sets live
with their shared producer records; shipping alpha-sort and trail-compaction
producers reexport the same sets, preserving their existing schedule edges.
Public shader paths retain their exact values and public API through reexports.

The native regression reproduced three lifecycle failures before their fixes:

- A sprite changed to a scene-depth material retained its old unsupported 2D draw.
  Missing/unsupported mesh rejection had the same early-return cleanup gap.
- Hidden draws stopped submitting but remained in retained phase membership.
  Per-view retirement now removes only Aestra command entries absent from that
  view's visible class. Other renderer commands remain untouched. A headless
  contract verifies exact render/main identity, including stale owner identity.
- Changing a mesh renderer to a sprite retained `PreparedMeshDraw`, including its
  old geometry/layout. Preparation now removes it when the mesh handle disappears.

These fixes run in both shipping and candidate graphs. Visibility retirement is
linear in phase/visible renderer entries, not particle count. No production wait,
GPU readback, particle mirror, per-particle ECS entity or history-policy change.

## Actual installed render gate

`queue_native::native_020_installed_queues_render_and_retire_through_real_schedule`
uses DefaultPlugins with headless image targets and synchronous test-only pipeline
compilation. It installs Aestra's real extraction adapter and render installer,
then runs normal `App::update` schedules. ShaderBuffer, Image and Mesh assets use
normal extraction/preparation, rather than injected RenderAssets. Embedded shader
paths use AssetServer; real WESL semantic shaders enter PipelineCache without
flattening. Camera visibility classes, per-view RenderLayers, ViewKeyCache,
MeshAllocator, depth prepasses, actual queue systems and registered transparent
commands execute through Bevy's render graph.

The initial frame has **10 submissions** across 2D, 3D 1x and isolated 3D 4x MSAA
views. It covers legacy/semantic sprites, depth fade, deformed indexed geometry
and deformed wireframe. The isolated MSAA sprite then switches to depth fade.
Each lifecycle transition checks exact phase membership and the submitted owner
multiset, not merely a nonzero count. It exercises unsupported 2D materials,
missing mesh preparation, restoration, hide/show, per-view layer reassignment,
mesh-to-sprite-to-mesh changes, draw despawn and view despawn. A validation error
scope covers the whole run and submitted GPU work; hardware absence is a failure,
not a skip. Native processes remain serial.

Fixture markers in the shared sort/compaction preparation sets audit their order
before actual effect bind-group preparation. Test-only RenderQueue writes seed
mesh indirect instance counts after preparation. **They are not the production
compute producers.** The fixture does not manually specialize pipelines, invoke
RunSystemOnce or call DrawFunctions.draw. Bounded GPU waits occur only in tests.

## Validation

Pinned Rust `1.98.1-x86_64-pc-windows-msvc`, locked offline dependencies, Windows:

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --all-targets --locked --offline --target-dir target/bevy-020-qualification -- -D warnings
# With AESTRA_REQUIRE_GPU_CONFORMANCE=1; restore the previous value afterward:
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --test gpu_conformance --locked --offline -- --test-threads=1 --nocapture
```

- Candidate: **123 passed** — 9 keyboard, 40 upstream focus, 4 cluster units,
  4 shader, 3 storage, 55 extraction/shared checks and 8 explicit serial native
  tests. The two additional headless checks are visibility retirement and the
  storage encoding contract reused by this integrated fixture.
- Shipping renderer lib: **151 passed**, three pre-existing native tests ignored,
  no filtered tests. The new shared visibility-retirement contract also runs on
  shipping 0.19.1.
- Hardware-required GPU/CPU conformance: **3 passed**, including all seven
  showcases, with no missing-device skip accepted. Shipping GPU tests ran only
  after the full candidate native runner completed.
- Workspace all-target check and strict Clippy, candidate all-target strict
  Clippy, both formatting checks and PowerShell AST parsing pass. Sandbox
  configuration-read denials were retried with approved access. Existing Cargo
  incremental hard-link warnings use its copy fallback.

Accepted run: `target/bevy-020-qualification/runs/7c022dda6fb149bc82ead4ba463055a7/`.
Inputs and executable hashes remained unchanged throughout. Cluster, pipeline,
command and queue gates used NVIDIA RTX 4070 SUPER / Vulkan / 616.92; shader,
storage and device-publication gates used AMD Radeon Graphics / Vulkan / 26.3.1.
Shipping and independent lockfiles are unchanged from the preceding slice.

| Artifact | SHA-256 |
| --- | --- |
| Accepted summary | `d7eb12d4ae797578d577b74791131889cb03febe40542313e7c9562ad8558ffa` |
| Accepted input manifest | `bf163d78ed6af220e5e9a7bf94262623bf4c7765525c8f82bff67b2b23f56d66` |
| Extraction / pipeline / command / queue executable | `550d8ef179f2f52a834c870efc32b11ca8c63b2fb4cb00f5535fe54ca14ce1ce` |
| Independent lockfile | `23b7862f310867ef34697ab57f15cf043c229c793183d4c5bbe35166bf3bafdb` |
| Unchanged shipping lockfile | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |

## Remaining

Port and qualify actual simulation, sorting, trail compaction and view-culling
dispatch feeding this installed consumer path. Check their readiness/fallbacks
and pipelined main/render scheduling. Volume rendering and full editor migration,
coherent root graph selection and native visual/performance comparisons remain
separate gates. This test proves queued commands and validated submissions, not
pixel correctness, smoke realism, equivalent allocations or performance.
