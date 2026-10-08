# F8.1B2 — accepted GPU child-birth host outputs

2026-10-08, NVIDIA GeForce RTX 4070 SUPER / Vulkan,
`1.98.1-x86_64-pc-windows-msvc`. This closes the bounded output gap exposed by
[F8.1B](smoke-cohorts-2026-10-08.md), not full F8 or arbitrary-load event scalability.

## Implementation and contract

`DomainSpawnPipeline` is shared by domain emission lists, event links and input bursts.
When a live, unsuppressed `OnSpawn` host route exists, its one-thread allocation plan
reserves an event-buffer prefix for **accepted destination slots**. The spawn kernel
writes child ordinals and initial position/velocity into that prefix. Ordinary births
are preserved; rejected requests reserve nothing. Capture uses the existing event
buffer, not a new per-particle store or positional readback path. A reusable 16-byte
disabled buffer is bound when capture is off. Each new kernel uses seven storage
bindings, below the portable ceiling of eight; WGSL, SPIR-V and HLSL translation pass.

Internal kind 8 denotes an output-only external birth. Host `OnSpawn` aggregation
includes ordinary kind 1 and kind 8; link gathering still matches only 1/2/4. Therefore
enabling a host output cannot introduce new same-tick link cascades or change simulation.
The CPU stateful reference exposes this distinction through `output_events`, while
`events` retains its existing link-input semantics.

Output aggregation now follows input births as well as links/domain births. An input
stamped at boundary N allocates after that interval and exports at reached tick N+1;
this preserves the existing clock convention. Paused frames export nothing new,
reconstruction stays suppressed and restart epochs can export fresh births. Existing
asynchronous delivery rejects stale epochs and repeated/out-of-order observations.

The unchanged **1,024-record shared event capture ceiling per emitter/tick** still
applies. Reservation selects a deterministic accepted prefix; overflow increments the
existing overflow word and does not remove particles. `FirstPerTick`/`EachEvent`
magnitudes count captured matching records, not uncaptured demand. Their 32-tick rings
and at-most-16 stored positions/tick remain bounded and potentially lossy. This is a
visual/SFX observation API, not a gameplay-authority bus. No host voice-per-spark
policy, event-link recursion, fluid injection or unlimited event capacity is added.

## Qualification

- Native domain-spawn suite: four passed. Mixed ordinary/external births preserve
  existing records and exact simulation; ten accepted of forty requested produce
  ten child records, saturation adds no cue, both aggregations preserve lowest
  ordinals, and an `OnSpawn` link excludes the new output-only records.
- Native overflow: 1,100 allocated children, all alive, 1,024 captured in ordinal
  order and 76 overflowed. Two fresh runs match state/capture exactly.
- Native production coupled suite: fourteen passed, two pre-existing hardware-only
  tests remain ignored. A new test covers linked and input births, reached ticks,
  pause, reconstruction suppression, identical simulation after replay and fresh
  restart epochs. Separate duplicate/stale-output delivery tests: two passed.
- Native stateful conformance: thirty passed, including exact route/tick/count/origin parity against
  the CPU logical stream, position tolerance and matching final particles for death,
  collision, linked child births and input births.
- Runtime unit suite: 73 passed; portable stateful shader suite: six passed;
  SPIR-V/HLSL child-capture regression: one passed; audio-enabled public fireworks
  example contracts: thirteen passed.
- Workspace all-target check with `aestra-bench/gpu`, formatting and warnings-denied
  Clippy for runtime/GPU/renderer/viewer/public example pass.

The public `--smoke-cohorts` saved asset now binds representative flashes to accepted
child-star `OnSpawn`, replacing its historical parent-shell `OnDeath` qualification
route. The four real death links, smoke/stars populations, seed, budgets and image
thresholds are unchanged. Native high/medium/low lighting/lifecycle controls pass in
three fresh runs. All **84 PNGs** match between runs and the prior qualified parent-cue
images. Each cohort's `FirstPerTick` host message has magnitude 32 and child emitter
origin 1/2; a host reader also checks root identity, epoch, occurrence time, source
effect and effect-local-to-world position. Only smoke supplies receiver pixels;
representative admission is exactly one per break, with no invalid/budget drops.

Native fresh-app tests retain existing overlay/global-logger and shutdown closed-
readback-channel warnings. Accepted processes exit successfully; no WGPU validation
failure is accepted. During test development, a test buffer lacked `COPY_DST`, an
input expectation used the wrong boundary convention and a pause assertion compared
unrelated accumulating presentation counters. Those assertions/fixtures were corrected;
production simulation semantics and lighting thresholds were not changed.

## Reproduce

Run native GPU workloads separately, never concurrently:

```powershell
$env:AESTRA_REQUIRE_GPU_CONFORMANCE='1'
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --test domain_spawn --locked -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --test stateful_conformance --locked -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib gpu::coupled_tests:: --locked -- --test-threads=1
```

Set `AESTRA_SMOKE_COHORT_REPORTS` to an absolute fresh directory before each native
image run. Then run the existing smoke-cohort gate alone:

```powershell
cargo +1.98.1-x86_64-pc-windows-msvc test --locked -p aestra-viewer saved_shell_deaths_birth_overlapping_smoke_that_receives_later_break_light -- --ignored --test-threads=1
cargo run --release --locked -p aestra-bevy --example fireworks -- --smoke-cohorts --no-audio
```

See [reports and raw source/image hashes](child-birth-outputs-2026-10-08.json).
This proves a bounded child-cue/lighting contract, not audio listening acceptance,
instantaneous full-show particle census, cost/VRAM improvement or AAA smoke art.
Next: authored live overlap/full-show smoke costs before migrating saved show smoke.
