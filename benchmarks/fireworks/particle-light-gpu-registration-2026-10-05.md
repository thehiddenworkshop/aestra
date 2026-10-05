# F7E4B2B — paced registration under authored overlap, 2026-10-05

Base commit `5c4045e4` (F7E4B2A). Native RTX 4070 SUPER/Vulkan, optimized
development build. This is a **synthetic calibration tracer under authored load**,
not natural authored-star art acceptance, display/scanout latency, a repeated
work-matched cost study, native cluster-memory certification or a production-finale claim.

## Method

The viewer's normal prepared high-tier thirteen-clip show retains its compiled
choreography, transforms, materials, trails, event chains and F7D2B light-output
fixture (1500 lumens, 12 m range, per-output cap 32). Global selection and the native
GPU adapter cap stay at 96 throughout. A one-particle diagnostic emitter reserves
at most one slot at maximum priority, leaving the other 95 available to authored
sources. Representative flashes are disabled to isolate red/green classification:
this does **not** re-certify flash coexistence (covered separately by B2A).

The fresh show advances forward, externally clocked at sequential 60 Hz simulation
ticks in playback-only mode, with native pipelined rendering. Warmup to frame 1020
(17 s) is unpaced; pipeline readiness is awaited before measuring. Each case has
three consecutive 72-tick phases with the same show root and **no rewind**:

- Analytic control, show frames 1021–1092: a red diagnostic sprite and ordinary
  shadowless green Bevy light share a known analytic position.
- GPU, frames 1093–1164: a fresh red diagnostic sprite emits a green particle light
  via the canonical GPU selection/realization adapter; no CPU position readback.
- Negative control, frames 1165–1236: an ordinary green light deliberately trails
  a fresh red sprite by three simulation ticks.

Each tracer travels along +X at 25, 75 or 150 m/s. Its size is 0.4 m, fixed HDR red
radiance 8; its diagnostic light is 40,000 lumens, 8 m range, radius 0.1 m and green.
These stronger diagnostic values are **not edits to the authored show's lights**.
The isolated geometric measurement is not a comparative rendering-cost benchmark;
the show phase/time and ordinary-light realization differ between controls.

The show audience camera remains perspective: eye (0,48,180), target (0,28,0).
The 960×540 HDR final target uses Tony tonemapping, exposure 0, disabled deband dither,
and bloom 0 or 0.15. A rough nonemissive gray receiver, sRGB 0.7, metallic 0, size
220×24×1, center (0,38,90), isolates the foreground tracer at z=92. It occludes
background fireworks in the ROI; these are diagnostic images, not unobstructed
hero/show reference images. The existing wall/ground remain in the scene.

## Gate and reproducibility

Ten final images per phase are requested at ticks 12,16,20,24,28,32,36,40,44,48.
The last twelve paced ticks drain callbacks **without pausing either clock**.
Request labels are not claimed to be GPU capture ticks. Both centroids come from
the **same saved final image**, never a GPU position assigned at screenshot-callback
time. Screenshot transport is test instrumentation, not selected-record readback.

A projected, inset receiver ROI weights red minus max(green,blue) and green minus
max(red,blue), ignoring differences of three channel levels or less. Each color
must have more than 200 summed weighted levels. The ordinary-light control defines
the perspective/receiver centroid bias as a function of the visible sprite's X,
with interpolation and only a one-pixel endpoint tolerance (no unrestricted
extrapolation). The calibrated two-dimensional residual is converted using the
projected horizontal metres-to-pixels scale.

Every GPU sample must remain within `min(speed / 60, 2 m)`: 0.4167, 1.25 or 2 m.
The known three-tick delayed control must exceed the same gate at p95; this tests
detector sensitivity rather than accepting a blank receiver. Residuals are spatial
registration equivalents, **not measured display latency**.

Each phase records 60 raw wall-clock interval/update timings and deadline misses;
interval p95 must be ≤25 ms at the requested 60 Hz cadence. Missed deadlines are
counted and the deadline is rebased, not hidden. Each phase must observe at least
500 authored live particles and two active source instances at its peak. All 72
asynchronous population/source observations are retained, with unavailable values
as null; these are not frame-aligned GPU population certificates. In the GPU phase,
every observation must meet the 500-particle/two-source minima (controls need only
meet them at their peak because they use different sequential show times). Both show and
tracer must report native GPU presentation, cap bounds hold, dispatch advances,
and invalid sources, adapter rejection, selected-record transport and portable
proxy allocation remain zero.

The read-only test reloads all 180 PNGs, validates ordered case/phase/request labels,
dimensions and metadata, recomputes centroids/calibration/residuals, and checks
cadence, population observations and positive/negative gates against the report.
Native reruns first write `accepted:false`; quantitative gate failures now save a
full failing report before the test fails. Fatal setup/capture failures cannot
leave an old accepted flag.

## Native results and limitations

See the adjacent JSON for the retained report and aggregate image-manifest hashes,
all eighteen phase summaries and the earlier failed cadence attempt. The first
native run passed spatial/cadence gates; an intermediate run passed the first
spatial case but aborted in the 25 m/s bloom-on analytic control at cadence p95
**25.1734 ms**, just over the unchanged 25 ms limit. Its printed intervals are
retained, not silently discarded or used to relax the threshold. A later complete
run with the failure-reporting/observation harness is retained separately. Its raw
observations also pass the subsequently strengthened sustained-load predicate in
the read-only validator.

The retained run passes all 180 image checks. GPU phases observe **550–1121**
authored particles and **4–6** source instances throughout all 72 ticks. All eighteen
phases have zero unavailable population observations and zero deadline misses.
The largest phase interval p95 is 22.9061 ms; no universal cadence guarantee is inferred.

| Speed | Spatial gate | GPU p95, bloom off / on | Delayed control p95, off / on |
|---|---|---|---|
| 25 m/s | 0.4167 m | 0 / 0.0061 m | 1.2467 / 1.2808 m |
| 75 m/s | 1.25 m | 0.0003 / 0.0051 m | 3.7071 / 3.8117 m |
| 150 m/s | 2 m | 0.000022 / 0.0056 m | 7.3918 / 7.4528 m |

These are calibrated centroid residuals, not millimetre-accurate world-space
position measurements. All GPU samples (not merely p95) pass the gate. Native
GPU presentation, advancing adapter dispatch, both cap bounds and zero
selected-record readback/proxies/rejections are checked while measuring.

This evidence establishes registration for this calibrated high-tier load on this
GPU, not a universally stable 60 Hz budget. It does not measure actual native
cluster allocated/in-flight bytes or growth/overflow. Initial cluster capacities
remain 4096 slices / 524288 indices; they are initial allocations, not hard limits.
Medium/low receiver images, natural authored-star visual approval, additional
hardware/cadences, non-default per-view layers/multi-view, editor lighting, lit smoke
and production-finale certification remain open.

## Reproduce

Run the native test **alone**, after building; ordinary `cargo test` ignores it.
The environment path is repository-relative (or absolute), not crate-cwd-relative.
Use a fresh directory for each run when comparing cadence. No GUI window opens.

```powershell
$env:AESTRA_GPU_LIGHT_REGISTRATION_REPORTS = 'target/fireworks-f7/gpu-authored-registration-2026-10-05-run3'
cargo test --locked -p aestra-viewer paced_gpu_tracer_registers_under_authored_show_overlap -- --ignored --nocapture
cargo test --locked -p aestra-viewer retained_paced_registration_images_pass_the_gate -- --ignored
```

## Next

Repeat **work-matched** authored GPU adapter-on/off cost runs, and qualify native
cluster actual allocation/growth/overflow. Keep the failed cadence observation
visible when defining performance budgets; do not infer cost or universal real-time
performance from this registration experiment. Preserve portable async mode for
latency-tolerant workloads and the explicit default-layer adapter restriction.
