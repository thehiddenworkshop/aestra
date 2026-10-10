# B20-0 — Bevy 0.20 preflight, 2026-10-09

Historical audit of the original 17-member graph. The later approved
[dependency isolation](dependency-isolation-2026-10-09.md) supersedes the SVG/physics
blocking decision below, not the keyboard/cluster patch findings.

Baseline: `e8191982ef0fa71ee69f583a92f1f5b365a5a079`. Engine migration readiness:
**blocked**. This audit does not change shipping dependencies or remove any patch.

## Dependency result

Crates.io queries confirm Bevy 0.20.0 is published and requires Rust 1.97.1. Keep the
repository's Rust 1.98.1. The isolated resolution experiment copies **all 17 workspace
member manifests**, keeps SVG and both physics adapters, migrates shared engine/GPU/math
version requirements, removes only the copied patch table and resolves all workspace
features for Windows MSVC. Sources in this main copy are placeholders; no Aestra 0.20
source-compatibility claim is made.

Latest published ecosystem versions returned by `cargo search --limit 1`:

| Crate | Published version | Published Bevy requirement | Decision |
| --- | --- | --- | --- |
| [bevy_resvg](https://crates.io/crates/bevy_resvg/2.5.0) | 2.5.0 | 0.19 | Editor SVG integration blocks a clean 0.20 graph. |
| [avian3d](https://crates.io/crates/avian3d/0.7.0) | 0.7.0 | 0.19.0 | Physics adapter blocks; interpolation and other Bevy-coupled helpers also need review. |
| [bevy_rapier3d](https://crates.io/crates/bevy_rapier3d/0.36.0) | 0.36.0 | 0.19.0 | Physics adapter blocks. |

These requirements were inspected in downloaded, published Cargo manifests, not
inferred from README badges. This establishes published-version blockers at the audit
date; it does not establish that no unreleased upstream migration branch exists.

The resolver succeeds **but the result is rejected** as an upgrade candidate:

- Bevy, bevy_app, bevy_ecs, bevy_render and bevy_input_focus: both 0.19.1 and 0.20.0.
- wgpu: 29.0.4 and 30.0.1; Naga: 29.0.4 and 30.0.1.
- New shared math resolves to glam 0.33.12, satisfying the 0.33.2 requirement; older
  math versions remain in the ecosystem graph. Not every math-conversion version is
  an ECS compatibility bug; the Bevy/ECS/render splits are the decisive problem.
- WESL resolves to 0.6.0, encase to 0.12.1, and winit to 0.30.13. The published 0.20
  bevy_winit manifest still requests winit 0.30, so no winit major bump is needed here.

The old engine's direct reverse paths are:

```text
bevy 0.19.1
  avian3d 0.7.0 -> aestra-bevy-avian
  bevy_transform_interpolation 0.5.0 -> avian3d
  bevy_rapier3d 0.36.0 -> aestra-bevy-rapier
  bevy_resvg 2.5.0 -> aestra-editor
```

This is not solved by forcing a Cargo patch with a changed version number. Plugin
source must actually migrate, and their components/resources must belong to the same
engine graph as Aestra's application.

## Keyboard patch: runtime failure confirmed upstream

The published `bevy_input_focus` 0.20.0 still dispatches queued keyboard messages after
`InputSystems` without reconstructing per-event modifier state. Its published
`bevy_ui_widgets` text input reads `ButtonInput<Key>` when handling focused input.
There is no upstream `KeyboardInputSnapshot` equivalent in the inspected focus crate.

The separate real [keyboard fixture](keyboard-020.rs), compiled against exact published
0.20.0 crates with Rust 1.98.1 MSVC, confirms:

| Required behavior | Result on unpatched 0.20.0 |
| --- | --- |
| Ctrl held from the prior frame is visible at A-down | Pass |
| Ctrl-down / A-down / A-up / Ctrl-up in one PreUpdate retains Ctrl at A-down | Fail: observer records false, expected true |

The second test deliberately keeps its desired-behavior assertion and exits 101; it
is negative qualification evidence, not a passing regression gate. The fixture uses
a focused observer with the same modifier resource as upstream text editing; it does
not claim to run the complete Feathers text widget or OS clipboard.

Decision: do not remove the keyboard compatibility fix or its deferred-shortcut
snapshot adapter. Port the minimal behavior to real 0.20 APIs when the engine migration
starts, then require both fixture tests and all existing editor regressions to pass.

## Cluster patch: released source still needs the fix

Inspection of published `bevy_pbr` 0.20.0 `src/cluster/gpu.rs` finds:

- `prepare_clusters_for_gpu_clustering` constructs new `ViewClusterBindings` and
  `ViewGpuClusteringBuffers` inside each view's preparation loop.
- Staging acquisition pushes a cloned owner into `metadata_staging_pending_buffers`;
  successful completion recycles into the free list without removing that pending owner.
- Map/decode error paths return without retiring the pending owner; acquisition has
  no equivalent of Aestra's eight-slot bound.

This is a **source audit**, not a native rendering run of unpatched 0.20. Decision:
port retained containers, logical resets, bounded staging ownership and mode/view
cleanup to real 0.20 source. Preserve/regenerate test instrumentation; the existing
native tests import `AestraClusterReadbackProbe` and `aestra_cluster_diagnostics`, which
are Aestra-only exports, not upstream APIs. A performance or memory-regression claim
about 0.20 still requires a same-hardware native comparison after the port.

SHA-256 of the downloaded source files inspected:

| Published source | SHA-256 |
| --- | --- |
| bevy_input_focus 0.20.0 `src/lib.rs` | `fcf4494b0b5f5109935abbda51e487fdb1b363a07f99c6494b6299e2dbf5eaef` |
| bevy_ui_widgets 0.20.0 `src/text_input.rs` | `54271989ac6fa30f79501b3acf49d051cd23e55ffaf59a1a1005b74df6fd302d` |
| bevy_pbr 0.20.0 `src/cluster/gpu.rs` | `dd42ff7110b3b38ba20b50c604258d3eee3f6e3440651af791ddfcd372633c64` |

## Current 0.19.1 baseline checks

Fresh execution during this audit:

| Check | Result |
| --- | --- |
| Portable architecture and shader contracts | 11 passed |
| Editor `input::tests::` filter | 8 passed: six keyboard regressions and two numeric precision tests |
| Vendored PBR `aestra_reuse_tests` | 4 passed |
| Sequential native cluster buffer and private lifetime tests | 2 passed; runner accepted both |
| Workspace formatting | Pass |

Native lifetime evidence observes stable steady-state generations, overflow recovery
to 512 Z slices / 8,192 indices over 240 steady updates, a staging peak of two per
view within the eight-slot bound, and zero final named private allocations. These are
the runner's bounded ownership/cleanup gates, not total VRAM or production performance.
The runner does not emit adapter identity; do not use this run for cross-adapter timing.

Commands used, from the repository root:

```powershell
cargo test --locked --offline -p aestra-gpu --test architecture --test shader_contract
cargo +1.98.1-x86_64-pc-windows-msvc test --locked --offline -p aestra-editor input::tests:: -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc test --locked --offline -p bevy_pbr --lib aestra_reuse_tests
$env:RUSTUP_TOOLCHAIN = '1.98.1-x86_64-pc-windows-msvc'
./benchmarks/fireworks/run-cluster-lifetimes.ps1 -ReportsDirectory '<fresh output directory>'
cargo fmt --all -- --check
```

The initial editor test attempt under the active GNU 1.98.1 toolchain failed because
`dlltool.exe` is absent. It was rerun successfully under the already installed MSVC
1.98.1 toolchain; no toolchain pin/configuration was changed. The portable tests passed
under GNU. Existing hard-link-cache warnings are environmental, not test failures.

Existing visual/behavior evidence is indexed, not newly recertified by this audit:
[rocket smoke deposition](../fireworks/distance-smoke-2026-10-09.md),
[smoke transparency/art](../fireworks/smoke-transparency-art-2026-10-09.md), and
[native cluster lifetimes](../fireworks/cluster-private-lifetime-2026-10-05.md).
Select and refresh the broader editor/material/volume/fireworks image and performance
baseline before implementing the renderer port. No fresh AAA art, first-user-journey
or full-workspace-Clippy certification is claimed here.

## Evidence and next decision

Local raw artifacts are retained under
`target/bevy-0.20-preflight/60c327b76ce141bd9b7523b4469e6644/`:
input hashes, generated manifest copy/lockfile, metadata, resolution summary, keyboard
failure log, and accepted native lifetime manifest/logs. These ignored artifacts are
machine-local; the [summary JSON](preflight-2026-10-09.json) preserves key results and
digests. The [README](README.md) describes regeneration.

Real root Cargo manifest and lockfile remain unchanged:

- Cargo.toml: `0b4b47d5b24111d2cd017b1c73a94dc0ec748379519042513840c2301d1d4f1a`
- Cargo.lock: `58ba265c196a1318c65f8398d1b802319c5409058885cecabf9e2491ddd6f11c`

**Recommended decision:** retain working 0.19.1 while compatible ecosystem releases
land. If an immediate migration is required, explicitly approve pinned, narrowly
scoped SVG/physics ports (including their Bevy-coupled helpers), preserve licenses and
record an upstream-removal strategy. Do not disable adapters or tests to make the
workspace look compatible. Re-run the mixed-engine gate before starting B20-1/B20-2.
