# B20-1 renderer control extraction — 2026-10-09

Status: sixth compatibility slice. Shipping remains Bevy 0.19.1; the independent
candidate builds against Bevy 0.20.0. This is not the full engine/editor upgrade.

## Implementation

- `render_settings.rs` shares the actual public renderer settings and presentation/
  transparent-order enums. Existing public paths, defaults and budgets are retained.
  The two extraction adapters implement their respective resource traits; the
  candidate uses explicit `RenderApp` labels and `bevy::extract` imports.
- `gpu/catchup_pacing.rs` shares the actual adaptive pacing algorithm, host setting
  and existing regression. `gpu/extraction_systems.rs` contains the actual custom
  pacing extraction system, registered by each adapter. Removing the optional host
  pacing setting restores paced behavior without resetting learned frame history.
- `gpu/capability_publication.rs` contains the actual device-limit/downlevel
  discovery and render-to-main publication system. Backend selection uses the
  existing production policy. Unchanged decisions do not dirty host resources;
  mode/device changes still publish. The engine-specific `Extract` and `MainWorld`
  paths are selected through the adapter, including optional-resource cleanup.
- The independent fixture imports these production modules and the existing
  capability policy verbatim. No substitute settings, pacer or detection types are
  used. The runner hashes their sources and executes the additional native test
  after the four existing native processes have exited.

No authored format, shader/storage ABI, playback/replay policy, production GPU wait,
CPU particle mirror or per-particle ECS entity is introduced. This is a compatibility
refactor, not a pacing optimization or a change to backend fallback rules.

## Contracts

Three new shared tests run through both engines' real extraction/render schedules
or production publication helper:

- Settings arrive before render preparation across all presentation/order modes,
  including zero budgets. Extraction does not mutate host values and restores a
  removed render-side copy even when the host resource has not changed.
- Optional pacing removal/reinsertion and toggling retain adaptive history, honor
  existing exact/preview limits while unpaced, and restore the learned paced budget.
- Supplied capability metadata exercises actual publication/selection policy:
  requested modes, unchanged-frame change tracking, device loss and recovery.

The additional hardware-required 0.20 test creates a real Vulkan device and Bevy
renderer resources, then executes the actual publication system through the real
extraction schedule. It verifies detected identity/limits, initial publication,
unchanged resource change ticks, mode updates and render-side settings. It does
not submit a draw, execute a full 0.20 renderer or qualify pipelined thread behavior.

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

- Candidate runner: **93 passed**: 9 keyboard, 40 upstream focus, 4 cluster units,
  4 shader, 3 storage, 28 extraction/capability/pacer/mesh checks and 5 explicitly
  executed native checks. Native processes ran serially, without overlapping
  shipping GPU tests.
- Shipping renderer lib: **146 passed**, three pre-existing native tests ignored;
  no lib tests filtered. Includes the new shared contracts and unchanged policy tests.
- Hardware-required GPU/CPU conformance: **3 passed**, including all seven bundled
  showcases; no missing-device skip accepted.
- Workspace all-target check/strict Clippy, candidate extraction strict Clippy,
  both formatting checks, PowerShell AST parsing and whitespace validation pass.
  Configuration-read sandbox denials were retried with approved access; existing
  hard-link cache warnings used the copy fallback.

Accepted run: `target/bevy-020-qualification/runs/75a9698897664659894158fce549e8e7/`.
Inputs/executables remained unchanged during the run; exactly Bevy 0.20.0 and the
expected candidate patches were selected. Cluster tests used NVIDIA RTX 4070
SUPER/Vulkan (616.92); shader/storage/device-publication tests used AMD Radeon
Graphics/Vulkan (26.3.1).

| Artifact | SHA-256 |
| --- | --- |
| Accepted summary | `7bc3bbfa77d7145c5c42cc77612bfd228d278a613e58769a3bb92999f06f84bb` |
| Accepted input manifest | `4ace7b6a417381ace6bcff5e396ff3b1519bb99ebccfc062c1f5c7324e8230d1` |
| Extraction executable | `764fd195ff6e9dd94e0727414d3a37add50b21be1f88caa7cf83479b6c0efcfd` |
| Independent lockfile | `06b75595dc9f7365be47f23df2ec6bda4dd65a07c43a0a978f7180e3ba4b70a6` |
| Unchanged shipping lockfile | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |

## Remaining

Custom render commands/pipelines/view APIs, editor compatibility and the coherent
root Bevy/wgpu/glam switch remain. Select the prepared shader/storage/extraction
adapters with that switch, then complete native integration, visual/performance and
cache-identity gates. These control tests do not justify retiring the shipping
adapters, approving changed references or claiming a complete 0.20 port.
