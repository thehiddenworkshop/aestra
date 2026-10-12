# Bevy 0.20 complete renderer and host plugin bootstrap

## Implementation

Shipping and the isolated candidate now compile the same `AestraRenderPlugin`,
GPU bootstrap and canonical `AestraPlugin`, rather than reconstructing their
registration in the candidate fixture. Existing startup code moves into
`render_plugin.rs`, `gpu/bootstrap.rs` and `host_plugin.rs`; public plugin and
profiler exports remain unchanged. Production system ordering and shipping
shader bytes are preserved. The candidate also includes the actual CPU fallback,
input-event delivery and project profiler modules.

Geometry telemetry uses the established version-selected mapped-view boundary:
0.19 retains its wrapper/infallible view, while 0.20 uses the raw buffer/fallible
view. Failed views invalidate telemetry instead of publishing partial counts.
Only regression setup adapts the changed `GpuShaderBuffer` fields and pending
command-buffer finishing API; playback still never blocks on GPU polling.

The qualification runner hashes the six newly included production files.

## Qualification

The combined real pipelined native gate now adds `AestraPlugin` and the opt-in
host particle-light plugin. Manual backend assignment, resource registration,
shader installation, graph scheduling, timing consumption, and host playback/
binding system assembly are removed. Only authored scene setup, deterministic
time/pacing, screenshot capture and read-only render instrumentation remain
fixture-owned.

The gate requires automatic hardware capability publication and GPU backend
assignment. It retains the existing two-root nested clock/binding, stage output,
readiness, paused placement, inherited hide/show, buffer reuse, restart/seek,
both light modes, exact-frame injection and teardown assertions. It additionally
checks actual particle pixels, nonzero submitted geometry through the real
project profiler, simulation timings delivered to main-world owners, and
per-project profile aggregation.

The full plugin now renders particles, unlike the previous installer fixture.
Receiver screenshots therefore temporarily hide only particle draw children;
source owners, light authorization and simulation stay unchanged. Normal draw
visibility is restored between captures. Particle pixels and geometry are
checked separately with draws visible. The existing dark/lit receiver thresholds
are unchanged, so emissive particle overdraw cannot masquerade as illumination.

## Scope and next step

This qualifies complete plugin startup with analytic particles and dense fluid
stages on the real render thread. Independent stateful/fluid/trail gates remain
required; this is **not** a new full-plugin coupled readiness or volume-light
scattering oracle. Next, exercise a jointly coupled stateful particle/fluid/trail
effect through these actual plugins, including delayed shader readiness and
restart/seek transitions.

The editor compatibility port, coherent root Bevy/wgpu/glam switch and final
native visual/performance parity remain open. Shipping stays **Bevy 0.19.1**;
the isolated candidate stays **Bevy 0.20.0 / wgpu 30.0.1**. No dependency or visual
reference changes, widget replacements or new ignored tests are introduced.

## Validation

Accepted source-frozen run:
`target/bevy-020-qualification/runs/600b1f4de02141689c0154985082b9d0/`.
Pinned MSVC Rust **1.98.1**, locked/offline dependencies: **258 checks** pass
(9 keyboard, 40 upstream focus, 4 cluster units, 4 shader, 3 storage,
165 shared/candidate checks and **33 explicit native gates**).
All **462 input hashes and five executable hashes** remain unchanged during the
run. The complete-plugin pipelined gate passes in **6.20 seconds** on NVIDIA
RTX 4070 SUPER / Vulkan 616.92. Low-level native device/shader/storage/timestamp
gates also use AMD integrated Vulkan 26.3.1. These are correctness checks, not a
new performance or image-parity baseline.

Shipping renderer library: **163 pass**, three existing ignored tests unchanged.
Host library: **63 pass**. Existing geometry, CPU transform, input-event and nested
project profiler regressions execute from shared source on both engine graphs.

Strict workspace Clippy/all-target check with `aestra-bench/gpu`, strict candidate
Clippy in both configurations, both formatting checks and whitespace validation
pass. Sandbox-denied lint/format checks pass when rerun with approved access.
Existing incremental-cache hard-link warnings fall back to copying. No lint
suppression, assertion removal or new ignored test is added. GPU suites run
serially.

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy --lib --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --features async-qualification --locked --offline -- -D warnings
```

SHA-256 evidence:

| Artifact | SHA-256 |
| --- | --- |
| Synchronous extraction executable | `8669e8fa7514248e75024750b0b8c49cc4a6ab377a73e5148d33638625c7f805` |
| Asynchronous extraction executable | `608d4e7d27aa3a1bded1293d2b566f900be8d1b5905fa4039cc0a082a2c5242b` |
| Shipping lockfile | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |
