# Bevy 0.20 combined pipelined installers

## Implementation

The isolated candidate now combines the canonical host binding/playback/project
systems, shared outer simulation graph, production extension-stage/volume/light
installers and actual host light plugin in one application. DefaultPlugins keeps
Bevy's real PipelinedRenderingPlugin and asynchronous pipeline compilation enabled.
RenderApp is removed from the host and operated through its real bounded channels;
the fixture does not emulate a second world or manually submit the simulation.

`patches-020/pipelined_installers_native.rs` supplies only a main-world analytic
particle asset allocator/uploader in place of monolithic `prepare_gpu_effects`.
It consumes the actual PresentedEffect clock, seed and epoch after the canonical
host systems. No particle simulation, stage dispatcher, selector, host proxy pool
or GPU light injection is substituted. The existing nested project fixture adds
an optional analytic point-light output; its original tests retain their inputs.
The offscreen receiver helpers are reused without changing existing assertions.

Read-only telemetry observes the real render resources. An exclusive cleanup
probe identifies the render-app schedule caller: ordinary sendable systems can
be stolen by pool workers, including the waiting main thread. The production
graph executor is not changed to force the test to pass.

## Profiling fix

The combined test reproduced missing stage measurements after ordinary forward
playback. `run_extension_stages` created timestamp batches even for paused frames;
their newer empty publication could supersede a real in-flight timing callback.
The documented last-real-measurement behavior was therefore unreliable when the
render thread continued while GPU callbacks were pending.

The shared installer now starts a timestamp batch lazily, only when at least one
stage needs to advance. Paused frames neither reserve timestamp slots nor publish
an empty replacement. The transport's bounded slots, sequence rejection and
simulation timing behavior remain unchanged. This fix applies to shipping 0.19
as well as the 0.20 candidate; it does not add a GPU wait or readback.

## Required native gate

`pipelined_installers_native::native_020_pipelined_installers_share_nested_clock_outputs_and_lights`
is required explicitly by the runner's asynchronous native section. It checks:

- Two independent paused roots, each with two nested source offsets and a live
  forwarded binding; only the source inside the actual fluid grid produces force.
- Matching extracted stage identities, completed stage ticks, volume targets,
  actual progress/output mailboxes and outer-graph particle readiness. Advancing
  one root leaves the other paused. Restart uses the changed live source; forward
  and backward seeks converge under the same production schedule.
- Portable async proxies and completed bounded selected-light transport for
  nested occurrences; source artifact/seed/revision and root/owner epochs match
  the current main-world presentations after convergence.
- SameFrameGpu mode consumes this exact render frame's selection sequence and
  illuminates a real StandardMaterial receiver without further selected-light
  readbacks. Disabling selection makes the receiver black; restoring it relights
  the receiver. At that point both fluid sources are outside their grids, so
  volume self-illumination cannot satisfy the light assertion.
- Actual asynchronous outer simulation and positive stage timestamps for both
  owners. The paused stage timelines retain zero checkpoint bytes throughout.
- Removing one root preserves only the other root's stage/light identities.
  Removing both retires volume draws and light injection, leaving a black receiver.
  Validation waits occur only after application/thread shutdown, in the test.

The manifest includes unready sources with zero GPU input counts; membership
alone is not readiness. The test checks outer-graph readiness separately.

## Limits and next boundary

This is combined **analytic particles + dense volumes + nested host-light**
pipelining, not a new nested stateful/fluid lockstep oracle. Existing stateful,
joint-history and dense-volume field/texture/pixel gates remain independently
required. This test does not certify joint volume-light scattering pixels, sparse
volume rendering, full monolithic particle preparation/transform sync, the complete
AestraPlugin, editor compatibility or visual/performance parity.

Next: remove the remaining monolithic renderer preparation/transform wiring
boundary so the complete renderer/host plugin can be qualified, then finish the
editor compatibility port and coherent root dependency switch. Shipping remains
Bevy 0.19.1; no dependency graph or approved visual references are changed here.

## Validation

Accepted source-frozen run:
`target/bevy-020-qualification/runs/d4ce8223c21a40c1ae055b4eafb3be20/`.
Pinned MSVC Rust **1.98.1**, locked/offline dependencies: **219 checks** pass
(9 keyboard, 40 upstream focus, 4 cluster units, 4 shader, 3 storage,
126 extraction/shared, **33 explicitly executed native gates**). All previous
gates remain required. All **438 input hashes** and five native executable
hashes match after validation.

The new combined gate passes on NVIDIA RTX 4070 SUPER / Vulkan, driver 616.92,
in **5.27 s** in that frozen run; both positive stage timing callbacks arrive.
Some lower-level gates use AMD Radeon integrated graphics / Vulkan, driver 26.3.1.
These durations are test evidence, not rendering-performance claims.

Shipping renderer library: **157 passed**, three pre-existing ignored tests.
Shipping host library: **63 passed**. Strict workspace Clippy and all-target
check with `aestra-bench/gpu`, strict candidate Clippy in both configurations,
both formatting checks, runner parsing and whitespace validation pass. GPU
suites execute serially. Incremental hard-link warnings fall back to copying.

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy --lib --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --features async-qualification --locked --offline -- -D warnings
```

SHA-256 evidence:

| Artifact | SHA-256 |
| --- | --- |
| Synchronous extraction executable | `8af2f05ac45249992fd67a148b63b8eaf931844956c77619aee0d4e686f6401d` |
| Asynchronous extraction executable | `87e14a412cc92e3869c2204abdd67a8ba58cd81831d317ccc462141054936eae` |
| Candidate lockfile | `fd2d39d93f6811f031a4678e0ff5ff89c3050538eda1a6395b76bce05fddef78` |
| Shipping lockfile | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |
