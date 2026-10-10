# B20-1: production pipeline specialization — 2026-10-09

Shipping remains Bevy **0.19.1**. The independent qualification graph is locked
to **0.20.0**. This slice qualifies the actual specializer and a bounded native
descriptor matrix; it does not switch the workspace engine or claim visual parity.

## Implementation

- Move the production sprite/mesh specializer, semantic keys, draw-material policy,
  scene-depth layouts and uniforms into `gpu/pipeline.rs`. The shipping renderer
  delegates initialization to its factory and uses its keys and helpers. Preserve
  blending, reverse-Z depth, culling, geometry ABI, shader selection and layouts.
- Move the exact portable material layout translator and its existing regression
  into `material_layout.rs`; retain the public `material::material_bind_group_layout`
  re-export. There is no authored or GPU ABI change.
- Default the extra per-stage pipeline-constant fields introduced in 0.20. A local
  `needless_update` allowance permits the same stage literals on 0.19, whose fields
  were already exhaustive; no other lint is suppressed.
- Import these actual production modules in `patches-020/extraction.rs`. Add key
  separation and scene-depth/deformed-wireframe policy contracts. The existing
  uniform-independent key test and layout translation test run on both graphs.
- Share the existing real ShaderCache/WESL library loader as `shader_support.rs`.
  Enable `bevy_sprite_render` only in the independent fixture, so DefaultPlugins
  initializes the actual 2D pipeline as well as the 3D pipeline. Refresh only that
  fixture's lockfile; the shipping lockfile is unchanged by this slice.
- Extend the provenance runner to hash both shared modules and material callsite,
  and execute a sixth, serial native check using the existing extraction binary.

## Native explicit-layout gate

`pipeline_native::native_020_production_pipeline_descriptors_validate_with_explicit_layouts`
creates real Bevy render resources and uses Aestra's actual `SpecializedRenderPipeline`
implementation plus Bevy's `SpecializedRenderPipelines` and synchronous `PipelineCache`.
Missing hardware or a non-OK cached pipeline fails the test.

The matrix validates **192 descriptor requests**, not 192 necessarily distinct
shader programs: two view types, two target formats, two MSAA counts, four material
cases, three draw blend keys and two render modes. Semantic materials deliberately
override the draw's blend setting with their validated material policy.

Cases cover legacy sprites; generated additive sprites; the authored, point-lit
fireworks smoke material with its real texture/uniform layout; and generated depth
fade with single-sample and multisampled depth bindings. Diagnostic cases exercise
both billboard wireframe and the actual 56-byte, five-attribute mesh line layout.
Assertions also check target format, sample count, topology, reverse-Z comparison,
depth layout selection and repeated-key cache reuse.

Shaders first compile through Bevy's actual ShaderCache and locked WESL libraries,
with the same storage-limit definitions used by PipelineCache. The resulting flat
WGSL is then registered in PipelineCache alongside the **unmodified explicit
production pipeline descriptors**. No inferred bind-group layouts or stub Bevy
view/cluster types are used. This is composition/layout/pipeline validation, not a
render submission, binding-content test, image comparison or performance claim.

## Executed validation

Pinned Rust `1.98.1-x86_64-pc-windows-msvc`, locked offline dependencies, Windows:

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --test extraction --locked --offline --target-dir target/bevy-020-qualification -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --all-targets --locked --offline --target-dir target/bevy-020-qualification -- -D warnings
# AESTRA_REQUIRE_GPU_CONFORMANCE=1; previous environment value restored afterward:
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --test gpu_conformance --locked --offline -- --test-threads=1 --nocapture
```

- Candidate runner: **113 passed** — 9 keyboard, 40 upstream focus, 4 cluster units,
  4 shader, 3 storage, 47 extraction/shared-module checks and 6 explicitly executed
  native checks. The 47 include one composition helper regression already exercised
  by the shader target; the new headless behavior contracts number two.
- Shipping renderer lib: **150 passed**, three pre-existing native tests ignored,
  no filtered tests. Moving the existing key and layout regressions retains the
  complete test count.
- Hardware-required GPU/CPU conformance: **3 passed**, including all seven bundled
  showcases, with no missing-device skip accepted.
- Workspace all-target check and strict Clippy, candidate all-target strict Clippy,
  both formatting checks, PowerShell AST parsing and whitespace validation pass.
  Configuration-read sandbox denials were retried with approved access; hard-link
  incremental-cache warnings use the existing copy fallback.

Accepted run: `target/bevy-020-qualification/runs/66c02d890dab42eca46d284115981409/`.
All native processes ran serially, without shipping GPU-test overlap. The new
pipeline and cluster checks used NVIDIA RTX 4070 SUPER / Vulkan / 616.92; the other
native checks used AMD Radeon Graphics / Vulkan / 26.3.1. Inputs and executables
remained unchanged during qualification.

| Artifact | SHA-256 |
| --- | --- |
| Accepted summary | `6431499e4643a16b5a57202d5255921b0999e99d76bd00d1801f5c225b6e74cb` |
| Accepted input manifest | `091d10d297931e4c8b70125fee2219c0d359cf9781f97a3f37225fb8169069d7` |
| Extraction / pipeline executable | `4f70049f8e9b5b24d9b1a8c4f3181953649d833b7be803ddae427c5187faa8f6` |
| Independent lockfile | `23b7862f310867ef34697ab57f15cf043c229c793183d4c5bbe35166bf3bafdb` |
| Unchanged shipping lockfile | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |

## Remaining

Custom draw-command bodies and actual mesh/prepass/view bind groups still need
0.20 qualification, including semantic triangle-list mesh rendering and deformed
mesh wireframe submission. Complete queue/schedule integration, B20-2 editor API
compatibility, the coherent engine graph switch, cache-identity review and native
visual/performance gates remain required. No production GPU wait, particle mirror,
per-particle ECS entity or replay requirement is introduced.
