# F8.1C4O — Public project pipeline preparation and playback encoding

This follows [C4N](fragment-list-public-host-2026-10-26.md). The public smoke-show
host prepares future clip pipelines before starting its clock. This is a loading
and registration change, not photographic smoke approval or default-show migration.

## Implementation

`EffectPipelinePreparation::for_project` belongs on the actual active `Camera3d`.
It includes the root and every compiled dependency, including inactive future clips.
Preparation uses the live renderer's material shader cache and actual HDR/MSAA/OIT
pipeline keys. Uniform/clip overrides retain their shader programs. Request status
is Pending, Ready with a unique-pipeline count, or Failed with a reason. Rendering
reevaluates the current view configuration; replacing the component starts a new
request. Separate camera requests have separate status. There is no hidden playback,
seek, dummy particle dispatch, clip spawning, cue delivery or particle-buffer allocation.

The first API deliberately supports rendered 3D sprite/trail/flipbook pipelines.
Mesh/ribbon/extension-stage requests fail explicitly. It does not guarantee texture
residency, arbitrary dynamic material replacement, simulation readiness or device
admission. The example retains its existing admission and global/compute loading gates.

The opt-in `GpuPlaybackDiagnostics` resource records only CPU command encoding:
prepared owners, requested simulation times, actual encoded times and blocked owners.
It retains one frame and performs no GPU readback, wait or timestamp query. A report
also checks child presentation times against the current project schedule. These are
not GPU completion fences, exact display timestamps or visible-pixel registration proofs.

## Retained failed attempt and rejected budget experiment

The first prepared high-tier OIT run eliminated late pipeline preparation but exposed
0.166668 seconds of requested-versus-encoded lag. Its report and log remain under
`target/fireworks-f8/prepared-host-2026-10-26-a` and
`target/fireworks-f8-prepared-pilot.log`. The binary SHA256 was
`AF7ABFE9DC5796EFA64CDBDDAA1EF0C211AA86E5DF91AAFBE4400B274FA3B4D4`.
It is not registration acceptance.

Particle-only event/trail coupling uses the same adaptive catch-up budget as simulation
domains, which can fall to two ticks after a hitch. An experimental change gave
particle-only coupling the full bounded particle allowance (300 exact/24 preview)
while retaining domain pacing. It reduced measured encoding lag to one tick, but
the four pacing runs had p95 45.56–62.68 ms, p99 325.31–347.39 ms and maxima up to
513.16 ms. Real and game clock totals diverged as Bevy clamped long updates, and
high-tier short flash admission fell from the pilot's 13 to 9. These runs are not
performance or full-show registration acceptance. The change was **reverted**;
the final implementation preserves the runtime's original catch-up policy.

Six experimental raw reports and ten images remain under
`target/fireworks-f8/prepared-host-2026-10-26-b`. The experimental binary SHA256 was
`FBC87D9C8E80F161E997AFBE52F5F5817CB4284D073C199EE8347F859A9F625A`.
All four no-image lanes reported zero late pipelines. Each image lane reported one
global pending update, including the lazy screenshot path; that global counter does
not identify it as an Aestra pipeline. Neither image lane is a cost lane.

## Native method

The final executable is built once with `fireworks-audio`; native runs use
`--no-audio`. Windows, RTX 4070 SUPER/Vulkan, pinned Rust 1.98.1, Bevy 0.20.0,
wgpu 30.0.1, development application opt-level 1/dependencies opt-level 3.
Each run keeps the actual 1280x720 public HDR/Tony/bloom/environment configuration,
PlaybackOnly, authored 37-second show and ten cleanup updates. Ordinary rendering
uses default Sample4 MSAA, experimental rendering uses Off and the same 128 MiB
node/head guard as C4N. These are host configurations, not isolated OIT overhead.

Cost lanes have no screenshots or GPU capacity scans. The optional CPU encoding
ledger and schedule comparison remain enabled equally. Image/capacity lanes are
separate. No compilation or other native GPU probes run during these windows.
All raw runs are retained, including poor pacing; no stable-60-FPS claim is made.

## Final restored-policy results

Final binary SHA256:
`EFF034446F9EB654A4A841F339934EE2700C673AE5801318F6DF3E69095673DD`.
All three final no-image runs complete 37 seconds with `Ready { pipelines: 4 }`,
zero late global pipeline updates, zero blocked prepared owners, zero missing
scheduled owners, exact child schedule times and 13 accepted flashes.

| Final high-tier case | Median host ms | p95 ms | Maximum ms | Max encoding lag ms |
| --- | ---: | ---: | ---: | ---: |
| Wispy ordinary | 16.651 | 31.897 | 92.224 | 50.001 |
| Wispy experimental OIT | 16.624 | 32.486 | 114.142 | 66.668 |
| Persistent experimental OIT | 16.599 | 28.191 | 88.857 | 49.999 |

The loading gate passes for these configurations. Full-show pacing and one-tick
encoding registration do **not** pass. The residual lag exceeds tick quantization;
the next task is to profile simulation/history/activation work and qualify a bounded
live catch-up policy against both latency and short flash delivery. Raising a budget
alone is not an accepted fix. No default migration or photographic approval follows.

The rejected experiment's two bright snapshots show the same broad three-burst
phase, dark broken haze and no currently active burst lights. Requests are at
17.550 versus 17.533 seconds; these are not exact-frame pixel comparisons. Its OIT
image lane has 1,368 capacity observations with zero drops/invalid links, which does
not rescue the failed pacing/registration gate or prove physical smoke realism.

Final checks: renderer library **176 passed, three ignored**; fireworks example
with audio **29 passed**; strict workspace Clippy with GPU/audio features, formatting
and diff checks pass. Full workspace tests were not rerun. The example never imports
viewer implementation. Existing authored effects, cues, capacities, tiers, renderer
defaults and runtime catch-up policy are unchanged.

[Machine-readable evidence](prepared-public-host-2026-10-26.json) includes final and
rejected reports, source inventories, binary/report/image/log hashes and limitations.
