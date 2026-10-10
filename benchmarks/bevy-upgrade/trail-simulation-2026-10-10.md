# B20-1: actual trail-history simulation to installed queues — 2026-10-10

Shipping remains Bevy **0.19.1** / wgpu **29.0.4**. The independently locked
candidate remains Bevy **0.20.0** / wgpu **30.0.1**; no dependency switch is made.

## Implementation

The production history-recording block now lives in
`gpu/simulation_pipeline.rs::record_trails`. Shipping calls it at the same
observation boundary: update history, optional paged passes, ribbon linking and
the final timing marker retain their order. Diagnostics, timestamp writes and
the missing-globals early continue are preserved. `gpu/paged_trails.rs` now has
explicit imports, so the fixture imports the exact production dispatcher too.

The new native test installs actual pipeline startup/bind-group preparation,
effect/draw extraction, trail compaction/culling and rendering. A test controller
supplies times/poses and calls the shared production particle/history dispatch
before the installed producers and transparent commands. Inputs come from
`EffectCompiler` → `EffectInstance` → `GpuEffectArtifact`; particle/history/aux
storage starts zeroed. There are no seeded history records or duplicate GPU
simulation, page/merge, compaction or culling implementations.

The native contract checks three source generations: 70 owners with seed 7,
same capacity with seed 11, then 1,025 owners with seed 13. Each has ten distinct
observations plus repeated identical-time updates:

- Four live observations append exact time/position samples for three heads.
  Owner identity, sample count, counters and bounds validity are read back.
- Emitter translation changes new world-space samples without moving recorded
  history; further translation after death cannot move the retained tails.
- Three retired-head observations preserve old history while exact compacted
  segment membership progressively shrinks; complete expiry clears owners,
  produces known-empty bounds and zero-instance commands.
- An epoch discontinuity rebuilds from one observation without replay or
  checkpoint storage; the next observation produces a fresh segment.
- Installed culling produces the exact near/far view counts, and installed draw
  commands consume those precise GPU-written indirect buffers at offset zero.
- Same-capacity replacement rebinds sources and reuses compact output; crossing
  1,024 owners uses the actual paged path and grows output. Final effect/draw
  teardown retires producer entries and submitted draws.

All cases explicitly use `PlaybackOnly`. Only bounded test observation/readback
waits for the GPU; no production wait, readback, CPU particle mirror, shader
change, per-particle ECS entity or new public API is introduced.

## Scope and next gate

This qualifies actual analytic history dispatch feeding production producers and
queues, **not the complete host scheduler**. The controller supplies time/pose
and uses synchronous headless rendering. Stateful/routed/coupled simulation,
replay/checkpoints, async pipeline readiness, pipelined rendering, lighting and
the ribbon-link branch still need dedicated integration evidence. These tests
inspect records and submitted commands, not pixels or performance. The next
simulation slice is actual stateful dispatch and host scheduling, followed by
async/pipelined operation; B20-1 remains open.

## Validation

Pinned Rust `1.98.1-x86_64-pc-windows-msvc`, locked offline dependencies, Windows:

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib --locked --offline -- --test-threads=1
# With AESTRA_REQUIRE_GPU_CONFORMANCE=1, restoring its previous value afterward:
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --test gpu_conformance --test trail_compaction_conformance --test trail_culling_conformance --locked --offline -- --test-threads=1 --nocapture
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --locked --offline -- -D warnings
```

- Candidate: **133 passed** — 9 keyboard, 40 upstream focus, 4 cluster units,
  4 shader, 3 storage, 59 extraction/shared and **14 explicit serial native tests**.
- Shipping renderer lib: **151 passed**, three pre-existing ignored native tests,
  no filtering. Hardware-required shipping GPU/CPU/trail conformance: **5 passed**,
  including all seven showcase effects.
- Workspace all-target check and strict Clippy, candidate all-target strict
  Clippy, both formatting checks, PowerShell AST parsing and whitespace checks pass.
- Trail-history integration ran on NVIDIA RTX 4070 SUPER / Vulkan / driver 616.92.
  Other candidate gates also used AMD Radeon Graphics / Vulkan / driver 26.3.1.
  Native GPU processes were serial, including the shipping renderer tests.

Accepted immutable runner evidence:
`target/bevy-020-qualification/runs/b956dcaa0a6d4a92810ca341583522ed/`.
The runner hashes the shared history/paged modules and rejects changed source
inputs/executables or logged native errors/panics.

| Evidence | SHA-256 |
| --- | --- |
| `summary.json` | `0fd8964791625757fb03dbe10e8a7271c25fa9d7dbb4a416aa91cf326077f0b3` |
| `inputs-after.json` | `acf0f20ff57cb984e532c615a308989dee2e24d33f523f57b5fcd075b6aaa69f` |
| Extraction/native executable | `485ec0fe6d12753edd7f443379fbf1804f09b54ff225b10d03b7138e01fa9cd4` |
| Candidate lock | `23b7862f310867ef34697ab57f15cf043c229c793183d4c5bbe35166bf3bafdb` |
| Shipping lock | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |

Both lockfiles are unchanged. Earlier focused runs are exploratory; the accepted
evidence above is the final-source full runner execution.
