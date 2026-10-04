# F7E3 — same-frame selected GPU lighting proof

Measured 2026-10-04 on top of F7E2 (`55a3f3a9`), RTX 4070 SUPER / Vulkan.
The adapter reports driver `NVIDIA`, not a numeric version. Two sequential native
runs passed; the second is the retained [measurement](particle-light-gpu-proof-2026-10-04.json).

**Result: the narrow registration proof passes. This is not a shipping adapter.**
The test-only bridge removes selected-light position readback and writes one
reserved slot in Bevy's native clustered-light buffer before camera clustering.
An unchanged `StandardMaterial` plane receives the light; no custom receiver
shader, CPU particle sampling, extrapolation, GPU wait or replay is used.

| Star speed | Same-frame p95 equivalent offset | Same-frame p95 world offset | Async comparison |
| --- | --- | --- | --- |
| 25 m/s | 0.0066 frames | 0.0028 m | 2.035 frames |
| 75 m/s | 0.0092 frames | 0.0115 m | 2.061 frames |
| 150 m/s | 0.0150 frames | 0.0375 m | 2.052 frames |

The control residual is at most 0.112 pixel. GPU residuals are of the same
subpixel order: the table is **equivalent spatial offset**, not measured
sub-millisecond execution/scanout latency. The unchanged gate is p95 at most
one 60 Hz frame **and** one-quarter of the eight-metre light range. All GPU
speeds pass; the simultaneously remeasured async comparison still fails.

## Implementation and scope

The fixture extends the [F7E2 method](particle-light-latency-2026-10-04.md):
control, async and GPU phases use separate forward-only intervals at each
speed, 60 moving ticks and ten final-image observations per phase. Bevy
pipelining remains enabled, history remains playback-only, and external
deadline pacing targets 60 Hz. No screenshot callback time is used as a
captured star position. Missing/black signals fail rather than imply zero lag.

`bevy/aestra-bevy/tests/support/particle_light_gpu_proof.rs` is explicitly
test-only. The production change is just `ParticleLightSelectionSet`, a
render-graph ordering point after current-frame selection and before cameras.

- Reserve one ordinary zero-lumen, shadowless `PointLight`. Resolve its current
  render entity through Bevy's `entity_to_index`; do not assume it occupies
  buffer slot zero. Import Bevy's own `ClusteredLight` WGSL definition rather
  than duplicating private Rust field offsets.
- After selection, write current world position, radius, range and RGB candela
  into that slot. Native Bevy GPU clustering consumes it before normal PBR
  shading. The compute shader clears its destination before writing a valid
  selected prefix; Bevy also recreates the zero-lumen placeholder each frame.
- Global selection cap is one. During the GPU phase the portable pool and
  selected-light readback cap are zero: allocated/active proxies, pending maps
  and staging bytes remain zero, and submitted-readback count does not advance.
  This does not claim that all of Bevy's unrelated diagnostics are readback-free.
- Reject unsupported storage/GPU clustering, multiple cameras, non-default
  camera/source layers, multiple selected slots/outputs, nested clips and other
  source identities. These are **proof restrictions**, not new production caps.
  The light is clamped to the portable baseline's one-million-lumen/200-metre
  limits. No new host API or engine-neutral rendering dependency is introduced.
- Native assertions verify pipeline readiness and advancing dispatches. The
  light re-enables after global disable. Six dark captures verify global disable
  and owner removal at all speeds **while the reserved slot remains alive**;
  green receiver energy is zero in each. Cleanup does not rely on hiding a
  stale light by despawning the placeholder.

The GPU phase's main-update p95 is 1.193–1.366 ms, not total GPU frame time or
bridge-only cost. Uniform/bind-group construction is intentionally simple for
the proof. These timings do not certify a bounded multi-light adapter, a hero
shell or a finale. Perspective/bloom artistic acceptance, additional hardware,
multiple views/layers, host-light coexistence and lit smoke remain open.

## Reproduction and gate

Run native work alone; do not run the two ignored probes concurrently:

```powershell
cargo test --locked -p aestra-bevy --test particle_light_latency same_frame_gpu -- --ignored --nocapture
./benchmarks/fireworks/validate-particle-light-latency.ps1 -ReportsDirectory target/fireworks-f7/particle-light-gpu-proof -GpuProof
```

The test generates 90 registration PNGs, six dark lifecycle PNGs and four CSVs
under `target/fireworks-f7/particle-light-gpu-proof`. The read-only validator
checks exact sample identities, moving sprite trajectories, control/image
signals, cadence, bounded slots, disabled selected-light transport, advancing
dispatches and cleanup. `-GpuProof` accepts only the GPU path, retaining the
async baseline separately; without it the original F7E2 gate remains unchanged.
The committed JSON retains CSV and representative PNG hashes. Generated raw
captures are evidence, not committed reference artwork.

## Next: F7E4 — bounded production adapter

Generalize only after this proof: an explicit opt-in mode, configurable bounded
slot pool, stable render-index mapping, zeroed unused entries and current-frame
source/layer validation. Cover multiple roots, source replacement/removal,
camera visibility, host lights/representative flashes, unsupported adapters,
buffer/cluster limits and warmup/error diagnostics without blocking readback.
Preserve the portable async mode for latency-tolerant workloads. Then remeasure
hero, overlapping volley and full show at every quality tier, including final
image registration and CPU/GPU/byte costs. Do not promote the one-slot fixture
to an unbounded or silently restricted shipping implementation.
