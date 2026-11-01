# Particle sampling

The **Particle Sampling** module in Particle Spawn controls launch layouts
independently of **Shape** and Initialize's **Velocity distribution**. It is
optional. Effects without it retain their historical sampling and random streams.
The same saved settings compile for the editor, public Bevy host and offline bake.
It does not require stateful simulation, introduce a clock, or allocate an owner
table, rejection loop or additional particle storage.

Open **Particle Sampling Lab** in `sample-project` for a 64-star sphere with short
trails. Select the Particle Sampling module, compare Directions `uniform` and
`stratified`, then change Seed and Jitter. Initialize's speed and lifetime ranges
remain separate controls. The lab restarts every six seconds.

## Settings

| Setting | Contract |
| --- | --- |
| Positions | `legacy` retains historical density; `uniform` samples area/volume; `stratified` samples a bounded low-discrepancy layout. |
| Directions | Same choices, independent of position sampling. Use an explicit Initialize velocity mode. Sphere produces a seed-rotated Fibonacci layout; cone/hemisphere retain their solid-angle support, disk retains planar area, ring retains unit planar directions. |
| Sequence count | 1–16,777,216. Pattern length for direct/continuous emission, **not a particle or owner limit**. Match the initial burst for a complete sphere. Linked and host-input bursts use requested fan-out per source instead. |
| Jitter | 0–1. Moves samples within height strata and perturbs azimuth by a bounded fraction of angular spacing. Zero produces the regular layout. It never adds, rejects or retries births. |
| Seed | Full u32 layout seed, combined with effect seed and first destination ordinal of the cohort. |

Default Positions is `uniform`, Directions is `legacy`, Sequence count is 64,
Jitter is 0.12 and Seed is 17. A small jitter and narrow speed/lifetime ranges are
a starting point for readable shell spokes; their final curves still need art
review against the footage. Jitter is not a physical turbulence force.

Analytic position sampling supports Point, Circle, Ring, Sphere, Hemisphere, Box,
Cylinder and Cone. Stateful position sampling currently supports Point, Sphere
and Box. Unsupported enabled combinations produce a compiler diagnostic rather
than silently spawning at the origin. Event/domain/input births retain the source
position and inherited velocity; this module changes only their launch direction.
Position shapes do not relocate event-born particles.

Changing Directions from `legacy` requires an explicit Initialize velocity mode
(for example Sphere or Cone). Legacy Cone has a historical backend-specific
spread-factor contract and is deliberately not reinterpreted. Invalid settings,
duplicate modules and dynamic parameter/host bindings are diagnosed. V1 settings
are authored constants; edits use the normal editor recompile/undo path.

## Counts and identity

An event link with count 64 produces one 64-direction layout per source event,
including when several source events arrive together. Changing the fan-out to
3 or 129 changes the requested layout automatically. If capacity admits only a
prefix, that prefix retains its original requested count: missing slots do not
stretch the surviving directions into a different sphere. Existing admission,
overflow and destination-rejection counters remain authoritative.

Direct/continuous emission maps `ordinal % sequence_count` to its pattern and
rotates each new pattern using its first ordinal. It does not infer an arbitrary
multi-tick burst boundary. A short partial pattern is a prefix, not a rebalanced
smaller sphere. Match Sequence count to a known direct burst when balance matters;
use event/input bursts for variable-size shells. It is never based on storage
slot or parallel invocation order. Existing backend ordinal-width and capacity
contracts still apply.

Position and direction have independent hash streams. Speed, lifetime, angular
velocity, source events, shape extents and renderer settings retain their original
streams/values. Stateful simulation samples at birth and stores the result; normal
checkpoint restore reproduces it. PlaybackOnly needs no additional history.

The scalar coordinates use an integer avalanche hash and 24-bit uniforms. The
stratified height is `(index + 0.5 + bounded_jitter) / requested_count`, with
fixed-point golden-ratio azimuth and a seed-derived uniform sphere rotation.
Uniform volumes use cube-root radius (including cone height), disks use square-root
radius. This is not blue-noise/Poisson sampling or density-mask rejection. Angular
spacing is low-discrepancy rather than an exact equal-distance packing, and float
parity has a stated tolerance rather than a cross-device bit-identity promise.

## Host API

```rust
use aestra_bevy::{ParticleSampling, SamplingMode};
use aestra_core::ModuleInstance;

emitter.modules.push(ModuleInstance::particle_sampling(ParticleSampling {
    positions: SamplingMode::Legacy,
    directions: SamplingMode::Stratified,
    sequence_count: 64,
    jitter: 0.12,
    seed: 17,
}));
```

Choose Initialize's Sphere velocity mode independently. Gameplay still owns rocket
position, velocity and fuse. The CPU reference exposes
`StatefulSimulation::set_particle_sampling` and pure
`aestra_runtime::particle_sampling::{direction, position, SamplingCohort}` for
engine-independent testing. Set valid settings before playback; callers supplying
explicit cohorts must supply `index < count` and a nonzero requested count.

## Qualification boundary

The analytic shape/playback oracle uses 512 slots to avoid admission pressure.
An initial 128-slot continuous test exposed CPU/GPU live-count disagreement at
2.55 seconds (80 versus 70), because raw per-cycle births occupy the GPU's slot
mapping while the CPU admits surviving particles. Sampling does not change that
admission path. Treat continuous analytic capacity-pressure parity as a separate
runtime follow-up; unconstrained layout parity is not a fix for it. Event-born
stateful shells use their normal measured admission and loss counters.

## Reproduce

```powershell
cargo run --locked -p aestra-demo-fireworks --bin author_particle_sampling_study
cargo test --locked -p aestra-demo-fireworks --bin author_particle_sampling_study
cargo test --locked -p aestra-bevy-render --test particle_sampling_gpu -- --ignored --nocapture
cargo test --locked -p aestra-bevy-render --lib particle_sampling_native -- --ignored --nocapture
cargo test --locked -p aestra-bevy-render --test gpu_conformance particle_sampling_analytic -- --ignored --nocapture
```

The gold-shell control/candidate are `assets/test/effects/fireworks_sampling_control.aestra.ron`
and `fireworks_sampling.aestra.ron`. Both retain the filament study's materials,
ballistics, ranges, source/event settings, capacities and secondary emitters. Only
main-star Uniform versus Stratified direction sampling differs. Changed directions
can change later distance-trigger demand through gravity and drag; measure those
counters rather than assuming all secondary work is identical. These assets are
an opt-in study, not automatic repertoire adoption or final realism approval.

The [native review](../../benchmarks/fireworks/n3-particle-sampling-2026-11-01.md) retains matched shell images, counters and sampler cost measurements.
