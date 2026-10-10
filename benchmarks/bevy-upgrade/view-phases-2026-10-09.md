# B20-1 per-view visibility and retained phases — 2026-10-09

Status: seventh compatibility slice. Shipping remains Bevy 0.19.1; the independent
candidate uses Bevy 0.20.0. Pipeline specialization and custom draw commands are
not yet qualified by this slice.

## Implementation

- `gpu/view_phases.rs` shares the actual production visibility-class selection,
  sampled-sprite rejection/removal and transparent sorting policy. The shipping
  queue systems call this module; existing visibility consumers retain their path
  through a reexport. Three existing regressions move with the implementation.
- `PhaseDraw` centralizes the existing 2D/3D phase-item construction, used by both
  shipping queue systems and the candidate tests. Entity pair, pipeline/draw IDs,
  indexed mode, batch range, extra index, 2D extracted-index sentinel and 3D sorting
  metadata are preserved. Shader selection and pipeline specialization stay in
  the existing queue systems; this is not an alternate test-only queue.
- Each extraction adapter reexports its engine's actual `MainEntity` type. The
  candidate uses `bevy::extract::sync_world`, not the old render-module location.
  No main/render entity IDs or Bevy types cross between the independent graphs.
- The fixture imports the complete production `sprite_culling.rs`, including all
  seven existing conservative-culling contracts. Shared tests use Bevy's actual
  `CameraProjection` API for perspective and reverse-Z orthographic matrices,
  avoiding deprecated glam constructors under 0.20 without lint suppression.
  Production culling math and fail-open policy are unchanged.
- The qualification runner hashes the shared phase module and real queue call
  sites alongside the already tracked culling and adapter sources.

Four additional contracts execute unchanged under both engines:

1. Phase items preserve draw/owner identity, command IDs, geometry mode and
   GPU-instanced metadata; ordering fields remain unchanged.
2. Requeueing replaces a retained entry rather than duplicating it. Culling removes
   the exact render/main pair from one view without touching another owner or view;
   reentry restores the entry.
3. Real sorted phases recalculate 3D order after camera rotation and retain the
   renderer-order depth bias; 2D phases sort by renderer order.
4. Visibility includes both CPU-visible and GPU-culling lists, but not unrelated
   visibility classes or removed entities. Empty classes remain safe.

These are real Bevy phase/view API tests, not rendered images. They do not execute
the full queue systems, compile Aestra's custom draw-command bodies against 0.20,
qualify mesh/prepass bindings, or establish pipelined/visual/performance parity.
No particle mirror, per-particle ECS entity, production GPU wait, replay requirement
or authored-format change is added.

## Executed validation

Pinned Rust `1.98.1-x86_64-pc-windows-msvc`, locked offline dependencies, Windows:

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --test extraction --locked --offline --target-dir target/bevy-020-qualification -- -D warnings
# AESTRA_REQUIRE_GPU_CONFORMANCE=1; previous environment value restored afterward:
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --test gpu_conformance --locked --offline -- --test-threads=1 --nocapture
```

- Candidate runner: **107 passed**: 9 keyboard, 40 upstream focus, 4 cluster units,
  4 shader, 3 storage, 42 extraction/control/mesh/view checks and 5 explicitly
  executed native checks. The view slice adds 14 candidate checks: 7 existing
  culling, 3 moved phase regressions and 4 new contracts.
- Shipping renderer lib: **150 passed**, three pre-existing native tests ignored,
  no filtered lib tests. Existing tests are retained; four new contracts are added.
- Hardware-required GPU/CPU conformance: **3 passed**, including all seven bundled
  showcases; no missing-device skip accepted.
- Workspace all-target check/strict Clippy, candidate extraction strict Clippy,
  both formatting checks, PowerShell AST parsing and whitespace validation pass.
  Configuration-read sandbox denials were retried with approved access; existing
  hard-link cache warnings use the copy fallback.

Accepted run: `target/bevy-020-qualification/runs/9e68ced584b949b089807661f6a494a4/`.
All native processes ran serially without overlap with shipping GPU tests. Existing
native checks cover cluster lifetime, shader validation, storage and capability
publication, not rendered parity of the newly shared view code. Cluster checks used
NVIDIA RTX 4070 SUPER/Vulkan (616.92); the other native checks used AMD Radeon
Graphics/Vulkan (26.3.1). Sources and executables remained unchanged during the run.

| Artifact | SHA-256 |
| --- | --- |
| Accepted summary | `d8d25a9623b89c8d2df737dbb1698c911da17dd75dc8b44458bbf0d600affb62` |
| Accepted input manifest | `d0d7650cb474239657d616daaba3571565c900bdbefd051726b8be81ba214a33` |
| Extraction executable | `12703a7db74f69db2599907478a196f7f3ff75a2e6b73bfc4cbe3406ad8142ec` |
| Independent lockfile | `06b75595dc9f7365be47f23df2ec6bda4dd65a07c43a0a978f7180e3ba4b70a6` |
| Unchanged shipping lockfile | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |

## Remaining

Next: production pipeline specialization and custom draw commands, followed by
mesh/prepass/view bind groups and complete queue/schedule integration. B20-2 editor
compatibility, the coherent Bevy/wgpu/glam switch, cache-identity review and native
visual/performance gates remain required before retiring the shipping adapters.
