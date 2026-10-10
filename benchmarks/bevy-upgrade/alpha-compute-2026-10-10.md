# B20-1: actual alpha compute to installed queues — 2026-10-10

Shipping remains Bevy **0.19.1**; the independent candidate remains locked to
**0.20.0**. This slice qualifies one real compute producer, not the entire
simulation/trail pipeline or a coherent workspace engine switch.

## Implementation

The candidate imports production `gpu/alpha_sort.rs`, including installation,
asset loading, preparation, allocation/rebinding, readiness check and all three
compute dispatch stages. Explicit imports remove coupling to the monolithic
shipping module. Shared `SimulateEffects` and `SortAlpha` sets preserve
simulation-before-sort-before-draw ordering in the shipping graph, with the sort
inside Bevy's render-graph diagnostic window. Production sorting algorithms,
buffer usage flags, statistics, allocation policy and public playback/replay APIs
are unchanged.

The existing sparse-pool shader contract now also compiles against the candidate.
Array-based vector conversion crosses the independent glam versions; a narrow
test-only map adapter handles wgpu 30's fallible mapped view. No new production
mapping or readback helper is installed.

## Native installed-producer contract

`alpha_sort_native::native_020_alpha_compute_feeds_installed_queues_and_gates_stale_indices`
uses normal `App::update`, actual extraction/render installers, two opposite 3D
cameras, real ShaderBuffer extraction/preparation and AssetServer/PipelineCache.
The production prepare and classify/merge/finish systems execute before actual
per-view effect bindings and registered transparent draw commands.

Checks include:

- Exact far-to-near permutations and deterministic depth ties for both cameras;
  nonzero alive-list offset and capacities **257/513** cross page/merge boundaries.
- Camera movement updates uniforms. Same-capacity particle-buffer replacement
  refreshes bindings without output allocation; growth creates a new merge plan.
- Zero, partial and full live populations, including padding sentinels. The
  unchanged-frame output identities match and allocation statistics report zero.
- Prepared-but-undispatched state skips both draws before first dispatch and
  after a successful frame. No stale or uninitialized sorted indices are drawn.
- Disabling sorting returns to the original unsorted binding; subsequent hiding
  and draw/view destruction leave no entries or submissions.
- Actual sorted per-view bind groups exist for each pair and two commands are
  submitted while sorting is active. A validation scope covers submitted work.

Dispatch gating is a test-only condition on the actual sort set. It tests the
consumer's fail-closed behavior and per-frame dispatch reset; **it does not prove
asynchronous shader-loading timing or the pipeline-not-ready branch itself**.
Pipeline compilation is synchronous for this bounded fixture. Simulation inputs
are seeded GPU records, not real simulated particles. The observer copies sorted
indices via a separate test-only compute pass because production sort outputs
intentionally lack COPY_SRC. All staging, readbacks and bounded waits are test-only.
This is not pixel, transparency-quality, performance or allocation-parity evidence.

The tenth serial native process runs
`alpha_sort::tests::native_alpha_sort_handles_large_sparse_pools_and_opposite_views`:
**42 cases**, capacities 1, 255, 256, 257, 4,097, 8,193 and **65,537**, opposite
view matrices and zero/partial/full populations. It checks shader output directly;
unlike the ninth gate, it manually constructs compute pipelines/bindings.

## Evidence integrity

The runner hashes the actual alpha shader and included sparse-pool test source,
as well as production/candidate inputs and executables. It excludes only the
fixture's generated nested `target/` directory from its fixture-source inventory.
A direct Cargo invocation had created that directory; the first full run passed
all GPU gates but was rejected when generated `.rustc_info.json` appeared during
validation. Its run `80495af4d6db4377a50e60bf78a198c4` is **not accepted evidence**.
The runner was corrected and rerun from a fresh report directory without relaxing
source/executable immutability or native hardware requirements.

## Validation

Pinned Rust `1.98.1-x86_64-pc-windows-msvc`, locked offline dependencies, Windows:

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib --locked --offline gpu::alpha_sort::tests::native_alpha_sort_handles_large_sparse_pools_and_opposite_views -- --exact --ignored --nocapture --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --locked --offline -- -D warnings
# With AESTRA_REQUIRE_GPU_CONFORMANCE=1; restore the previous value afterward:
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --test gpu_conformance --locked --offline -- --test-threads=1 --nocapture
```

- Candidate: **127 passed** — 9 keyboard, 40 upstream focus, 4 cluster units,
  4 shader, 3 storage, 57 extraction/shared tests and 10 explicit native tests.
  The existing two alpha headless contracts now run on both engine graphs.
- Shipping renderer lib: **151 passed**, three pre-existing ignored native tests,
  no filtering. Its ignored alpha test was then run explicitly: **1 passed**,
  including all 42 cases, on NVIDIA RTX 4070 SUPER / Vulkan / 616.92.
- Hardware-required shipping GPU/CPU conformance: **3 passed**, all seven
  showcases, with no missing-device skip. Native candidate and shipping processes
  ran serially, including the explicitly enabled sparse-pool test.
- Workspace all-target check and strict Clippy, candidate all-target strict Clippy,
  both formatting checks, PowerShell AST parsing and `git diff --check` pass.
  Sandbox configuration-read denials were retried with approved access. Existing
  incremental hard-link warnings use Cargo's copy fallback, not suppressed lints.

Accepted run: `target/bevy-020-qualification/runs/ae7bcdb9169a475ab18ad875e2c1f401/`.
Inputs/executable hashes stayed unchanged. Alpha, cluster, pipeline, command and
queue gates used NVIDIA RTX 4070 SUPER / Vulkan / 616.92; shader, storage and
device publication used AMD Radeon Graphics / Vulkan / 26.3.1. Both lockfiles
remain unchanged.

| Artifact | SHA-256 |
| --- | --- |
| Accepted summary | `25821d60c6063f073da299b1224b8c3c9ef2b128cba30fd1a194ee7ca0131e57` |
| Accepted input manifest | `5e97f6923ac2b2b5ea488027305791e8c3231742ea649003c1cc872e9ca91958` |
| Extraction / pipeline / command / queue / alpha executable | `be16453dd37c2704d969a4c55e2914d2702b128273176f16e094e278756841c1` |
| Independent lockfile | `23b7862f310867ef34697ab57f15cf043c229c793183d4c5bbe35166bf3bafdb` |
| Shipping lockfile | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |

## Remaining

Actual trail compaction and per-view culling, simulation integration, asynchronous
pipeline readiness and pipelined main/render scheduling remain B20-1 work.
The editor compatibility port, coherent engine graph selection, renderer/cache
identity and native visual/performance comparisons remain separate gates. No
custom widgets were replaced and no fireworks art changes were bundled here.
