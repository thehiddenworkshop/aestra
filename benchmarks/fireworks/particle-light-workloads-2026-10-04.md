# F7D2B — authored particle-light selection workloads

Implemented and measured against F7D2A commit `eaf4c76a` plus this benchmark change.
This is **selection-only** evidence, not visible particle lighting, lit smoke, a production
finale or an end-to-end game frame budget. The normal materials, sprite/trail renderers,
history budgets, emission/event chains, placements and clip choreography remain intact.

## Fixture and capture contract

`--particle-light-bench` explicitly adds one generic point-light output to each non-smoke
emitter that has no scene outputs. Existing outputs are preserved. Stable emitter-derived
output/curve IDs make fixture preparation repeatable. Outputs use particle RGB, an independent
1500 -> 600 -> 0 lumen lifetime curve, 12-unit range, flash priority 1 and per-output caps
32/16/8. Global host caps are 96/48/24, with a 64 MiB selection-buffer budget. Nothing is
written into the saved effects. The hero retains the same particle counts at all tiers;
the volley/show also use their existing authored particle quality budgets.

The player remains paused until the initially queued render pipelines are ready, then
restarts once and advances forward at 60 Hz, with benchmark catch-up pacing disabled.
No seeks or replay history are used. This avoids consuming the show while initial shaders
compile, or reporting a nominal host time as though the simulation had reached it.
Later clip-specific pipeline work can still appear in the measured render window.

There are 120 warm-up ticks and 600 measured ticks for the hero/volley, or 1680 for the
show (30 forward seconds including warm-up, reaching cleanup). The headless path draws
the ordinary HDR scene into a 960x540 image; it does not disable rendering. Native GPU
backend observations and nonzero transparent fragment work are required by the validator.

GPU measurements:

- `aestra::gpu::particle_lights`: the existing complete local/global selection span.
- `aestra::bench::full_frame`: one outer GPU timestamp window across simulation, selection
  and ordinary render-graph work. It is **not** application/OS frame time, and excludes
  the later diagnostic/counter resolve/readback work. Do not sum stage percentiles.
- Independent fresh diagnostic observations supply p50/p95/p99; they are not frame-paired
  with the counter readbacks. A slow diagnostics mailbox can omit intermediate observations.

Only the 16-byte requested/candidate/selected/dropped counters are copied asynchronously
to the CPU. There are at most three reusable staging buffers and four pending mailbox
results; a busy slot is skipped, never waited on. Each observation retains its original
benchmark tick and selection sequence. Late warm-up results are excluded. Final in-flight
results may be absent. Null counters/memory mean no prepared selected frame, not a measured
zero. `tick.host_elapsed_seconds` includes paused startup; use benchmark tick index for
the forward capture window, not this application clock as a particle simulation time.

## Native measurements

Windows 11 Home build 26200, Ryzen 7 7800X3D, NVIDIA GeForce RTX 4070 SUPER, Vulkan,
NVIDIA driver 616.92. Optimized **dev** build, HDR exposure 0 / Tony tonemapping /
bloom 0.15, audience camera, fast transparency, seed `0xf1e0000000000001`.
GPU runs were sequential. This was not an isolated production host; development/check
work ran during portions of the repeated captures. Retain the observed variation.

[Machine-readable summaries and raw-report hashes](particle-light-workloads-2026-10-04.json)
retain sample counts, complete statistics and the two additional show runs per tier.
Raw reports are in `target/fireworks-f7/` and are regenerable with the commands below.

First sequential selection/baseline pair, GPU milliseconds:

| Workload / tier | Selection p50 / p95 / p99 | Render-window p50 / p95 / p99 | Selection-disabled window p50 / p95 / p99 |
| --- | --- | --- | --- |
| Hero high | 0.161 / 0.212 / 0.221 | 0.862 / 1.069 / 1.232 | 0.859 / 0.974 / 1.032 |
| Hero medium | 0.162 / 0.200 / 0.216 | 0.982 / 1.288 / 1.325 | 0.701 / 0.914 / 0.924 |
| Hero low | 0.168 / 0.209 / 0.222 | 1.042 / 1.249 / 1.270 | 0.851 / 1.156 / 1.200 |
| Volley high | 0.142 / 0.201 / 0.212 | 0.944 / 1.356 / 1.370 | 0.844 / 1.017 / 1.041 |
| Volley medium | 0.137 / 0.168 / 0.177 | 0.967 / 1.163 / 1.181 | 0.701 / 0.969 / 1.020 |
| Volley low | 0.105 / 0.156 / 0.163 | 0.744 / 1.133 / 1.151 | 0.654 / 0.851 / 0.864 |
| Show high | 0.680 / 1.107 / 1.245 | 3.029 / 6.929 / 8.999 | 2.260 / 5.802 / 7.792 |
| Show medium | 0.727 / 1.030 / 1.543 | 2.948 / 7.791 / 8.858 | 2.897 / 8.572 / 9.854 |
| Show low | 0.944 / 1.857 / 1.953 | 3.822 / 9.926 / 11.649 | 2.124 / 6.852 / 10.978 |

Baseline retains the same output fixture but sets the host global cap to zero. It therefore
includes the same instrumentation/window, while selection has no published set. These
independent distributions are **not paired incremental cost**; the medium baseline even
has a higher p95 than its selection-enabled run. They cannot establish a tier speed ordering.

Across all three show runs, selection p50/p95/p99 ranges:

| Tier | p50 range | p95 range | p99 range |
| --- | --- | --- | --- |
| High | 0.680–0.702 | 1.096–1.107 | 1.217–1.245 |
| Medium | 0.648–0.987 | 0.952–1.953 | 1.115–2.100 |
| Low | 0.565–0.944 | 0.967–1.857 | 1.046–1.953 |

Counts/memory remained stable. Fixed per-output dispatch/merge overhead is still material:
lowering the selected cap alone does not eliminate the 31-source show work. Measure an
isolated, frame-correlated cost before claiming a optimization benefit or game-frame budget.

## Counter and resource gate

Counters below are from **one observation at peak candidate count**, not independent maxima.
`reserved` covers local/global selection buffers, plans, keys, uniforms and counters;
`scratch` covers only selection ping-pong buffers. Neither is total runtime/render memory,
and staging adds at most 48 bytes. Source-run and byte maxima can come from different ticks.

| Workload / tier | Requested / candidates / selected / dropped | Max source runs | Max scratch bytes | Max reserved bytes |
| --- | --- | ---: | ---: | ---: |
| Hero high | 608 / 608 / 96 / 512 | 5 | 49,344 | 51,600 |
| Hero medium | 608 / 608 / 48 / 560 | 5 | 24,768 | 27,024 |
| Hero low | 608 / 608 / 24 / 584 | 5 | 12,480 | 14,736 |
| Volley high | 1820 / 1820 / 56 / 1764 | 4 | 123,648 | 125,552 |
| Volley medium | 670 / 670 / 30 / 640 | 4 | 28,416 | 30,256 |
| Volley low | 227 / 227 / 15 / 212 | 4 | 7,680 | 9,424 |
| Show high | 1536 / 1536 / 96 / 1440 | 31 | 302,208 | 314,960 |
| Show medium | 576 / 576 / 48 / 528 | 31 | 99,456 | 111,888 |
| Show low | 192 / 192 / 24 / 168 | 31 | 41,856 | 54,064 |

The volley can select less than its global cap because the per-output cap still applies;
it is not evidence of missing candidate evaluation. All observed selected counts satisfy
both the frame capacity and host cap; candidate minus selected equals budget-dropped.
All matrix runs had zero rejected outputs, skipped slots and overwritten mailbox results.
One medium repeated show skipped one busy slot, explicitly recorded, without blocking.

Both selection-enabled and disabled show reports pass the existing strict per-clip gate:
13 independent clip owners, six peak concurrent presentations, exact event admissions
8413/3317/1217, no source overflow, omitted expansions, destination rejection, trail evictions
or truncation, no replay checkpoints, and final particle/history/clip cleanup. Both additional
show runs per tier pass the same gate. This is the current bounded show, not the roadmap's
production-density finale target.

An additional hero run with `--particle-light-memory-mib 0` produced 598 explicit
`particle-light selection rejected: ... configured device storage limits` observations,
no selected frame/counters and no selection timing path. It did not retain an old selected set.

## Reproduction and regression checks

Use PowerShell 7 from the repository root; do not run native GPU probes concurrently.

```powershell
cargo build --locked -p aestra-viewer
New-Item -ItemType Directory -Force target/fireworks-f7 | Out-Null
$probes = @{ hero='f4-reference-hero'; volley='f5-secondary-volley'; show='f6-show' }
foreach ($name in @('hero','volley','show')) {
    foreach ($tier in @('high','medium','low')) {
        foreach ($mode in @('selection','baseline')) {
            $extra = if ($mode -eq 'baseline') { @('--particle-light-cap','0') } else { @() }
            & target/debug/aestra-viewer.exe --fireworks-f0 --fireworks-f0-probe $probes[$name] `
                --camera audience --backend gpu --history playback-only --hdr --particle-light-bench `
                --headless-bench --tier $tier --gpu-bench "target/fireworks-f7/$name-$mode-$tier.json" @extra
            if ($LASTEXITCODE -ne 0) { throw 'native benchmark failed' }
        }
    }
}
pwsh -File benchmarks/fireworks/validate-particle-lights.ps1 -ReportsDirectory target/fireworks-f7
# Repeat the show three times for performance conclusions. Additional show reports use
# show-selection-<tier>-repeat2.json / repeat3.json; validate admission/cleanup with:
pwsh -File benchmarks/fireworks/validate-show.ps1 -ReportsDirectory target/fireworks-f7 `
    -ReportPrefix show-selection -ReportSuffix '-repeat2'
cargo test --locked -p aestra-viewer --bin aestra-viewer
cargo clippy --locked -p aestra-viewer -p aestra-bevy-render -p aestra-gpu --all-targets -- -D warnings
cargo check --locked --workspace --all-targets
```

Viewer tests cover deterministic fixture preparation, preservation of normal emitter/material/event
data, shared show dependencies at every tier, paused startup, bounded mailbox/origin tags and CLI
guards. Native matrix validation requires actual GPU timing/fragment work, correct counter algebra,
unique sequences, bounded memory, no silent fallback, and the full existing show admission gate.

## Decision

F7D2B's current authored-workload measurement gate is implemented. Proceed to **F7E's bounded
asynchronous selected-record realization**, measuring latency, stale-epoch rejection, pool update
cost and moving-star receiver lighting. Keep the GPU selector opt-in, shadows off and representative
flash lights separate. Do not replace it with one entity per particle, or claim AAA/finale, editor
lighting or lit-smoke acceptance from these selection-only measurements.
