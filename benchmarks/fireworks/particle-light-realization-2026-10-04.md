# F7E selected-light portable baseline — 2026-10-04

Implemented against base commit `0a12cfe9`. Native RTX 4070 SUPER/Vulkan; the report identifies
driver vendor NVIDIA, not its detailed version. This is the same development host as the F7D2B
workload study, not a controlled production-game performance certification.

## Result

GPU-selected particles now illuminate ordinary Bevy geometry through an explicit, capped,
asynchronous adapter. No source-particle map, current-frame GPU wait, CPU event fanout, replay
requirement or entity per source particle. The independent representative flash pool still works.

`AestraParticleLightPlugin` is opt-in after `AestraPlugin`; the global selector still defaults
to zero. The authored output cap, global selection cap and transport-prefix cap are independent.
Main-world updates check current root/owner epochs, seed/revision, originating compiled artifact,
source output/context/backend/visibility and exact originating host budgets. This prevents an old
packet from reviving a replaced effect or an old budget during pipelined extraction.

Storage: three reusable async staging slots by default, up to 1 MiB total GPU staging and 1 MiB
estimated source-manifest payload per snapshot. One pending-result mailbox and one retained
main-world snapshot; selected identities share source metadata within each pool update. Referenced
compiled artifacts are retained, not copied; their original allocation is not part of the estimated
manifest byte count. Default expiry is 100 ms or eight main frames. Busy slots drain before resizing;
no new work is admitted over a reduced budget. Global cap zero removes selected jobs, copies and
proxy entities. Idle coordinating systems stay installed.

Copy happens after selection/drawing, before diagnostic resolve and submission, through Bevy's
ordered render context. `aestra::gpu::particle_light_copy` measures the selected-record/counter copy,
not map completion. No extra per-frame queue submit is needed.

## Authored full-show measurements

F7D2B's explicit light-output fixture, 1500-lumen/12-unit-range plans on non-smoke emitters;
normal materials, trails, event births, transforms and choreography unchanged. F6 show:
13 clips, 26 seconds, fixed forward 60 Hz playback-only, 120 warm-up and 1680 measured ticks,
960×540 offscreen HDR/exposure 0, audience camera, fast transparency and unpaced catch-up.
Independent authored flashes enabled, bounded to 8/4/2. Pipelined rendering remains enabled.
The final retained runs were sequential, with no overlapping native GPU tests or benchmarks.

| Tier | Peak active/allocated | Peak staging bytes | Accepted-set age p95 / max ms | Pool CPU p95 ms | Copy GPU p95 ms | Render-window GPU p95 ms |
|---|---:|---:|---:|---:|---:|---:|
| high | 96 | 13872 | 19.805 / 79.873 | 0.0719 | 0.009216 | 6.669 |
| medium | 48 | 6960 | 20.939 / 82.396 | 0.0394 | 0.009216 | 6.826 |
| low | 24 | 3504 | 19.358 / 77.372 | 0.0257 | 0.008192 | 6.566 |

All tiers reached their selected cap; all accepted sets were at most three main frames old.
Zero failed/rejected copies, expired sets or busy-slot skips. Superseded mailbox results were
replaced; one old-generation callback per run was rejected. Flash and particle pools coexisted
for approximately 200 measured samples. All thirteen clips retained complete event admission,
no overflow/truncation/eviction, and final active lights, pending maps and staging bytes were zero.
Idle scene pools retained 96/48/24 hidden proxies for reuse; explicit disable removes those too.

The global-zero control generated no selected set, submitted no selected-record copies, allocated
no particle-light proxies/staging and emitted no copy timing span. Representative flashes remained
independently enabled.

Timing distributions are not frame-paired with counters or each other. Pool samples observe the
prior PostUpdate; age/lag values refer to the last newly accepted set and are deduplicated by sequence.
Render-window timings are not whole-app/game timings. Do not subtract this study from F7D2B's
separate runs to infer incremental light cost. Shader/host scheduling startup and unpaced clocks
are present; readback tails around 80 ms need paced/perceptual follow-up.

Full values, raw-report SHA-256 and receiver hashes: [measurements](particle-light-realization-2026-10-04.json).
Raw reports are generated under `target/fireworks-f7/realization/`, not checked in.

## Visible receiver and lifecycle proof

`bevy/aestra-bevy/tests/particle_light_realization.rs` runs a headless native scene with a
rough lit plane, 256×256 camera, zero ambient light, no rendered particles, bloom or representative
flash in its images. Three generic light-only source particles use 40,000 lumens/range 8.
Global cap two, transport cap one: exactly one pooled light, 64 bytes copied, no more than 192 bytes
staging. Shadows remain off.

At forward source times 0.5 and 1.5 seconds, the already-transformed selected positions are
(-1,1,0) and (1,1,0). The same pool entity is reused. Mean RGB8 response is 19.578 versus 0.540
disabled; the weighted x centroid shifts from 107.104 to 147.897 pixels. These are image-response
checks, not radiometric lux or artistic brightness approval. PBR pipeline readiness is established
by actual lit receiver response, not merely compute-selector readiness.

The probe separately tests flash coexistence, first-update restart rejection, global disable and
staging drain, re-enable, zero-byte-budget rejection/recovery, resizing and owner removal. Unit
tests additionally cover bad wire data/counters, duplicate identities, out-of-order callbacks,
configuration-qualified packets, compiled replacement, seed/revision/backend changes, layer
refresh, clamps, stale age/frame expiry and externally deleted proxies.

Generated proof images: `target/fireworks-f7/particle-light-receiver/on-left.png`,
`on-right.png`, `off.png`. Hashes and native observations are in the measurement JSON.

## Reproduce

```powershell
New-Item -ItemType Directory -Path target/fireworks-f7/realization -Force
foreach ($tier in @('high', 'medium', 'low')) {
    cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f6-show --camera audience --backend gpu --history playback-only --hdr --exposure 0 --tier $tier --particle-light-bench --particle-light-realization --transient-lights --headless-bench --gpu-bench "target/fireworks-f7/realization/show-$tier.json"
    if ($LASTEXITCODE -ne 0) { throw "Failed $tier" }
}
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f6-show --camera audience --backend gpu --history playback-only --hdr --exposure 0 --tier high --particle-light-bench --particle-light-realization --particle-light-cap 0 --transient-lights --headless-bench --gpu-bench target/fireworks-f7/realization/show-disabled.json
./benchmarks/fireworks/validate-particle-light-realization.ps1 -ReportsDirectory target/fireworks-f7/realization
cargo test --locked -p aestra-bevy --test particle_light_realization -- --ignored --nocapture
cargo test --locked -p aestra-bevy-render --test particle_light_playback -- --ignored --test-threads=1
```

The read-only validator preserves the existing per-clip show-admission gate, then requires caps,
staging/latency bounds, flash coexistence, final cleanup and the global-zero control. Failures and
missing evidence fail closed, never become zero. Rebuild first and avoid concurrent host builds
or GPU workloads when comparing timings.

## Still open

This completes **F7E1's portable implementation/evidence**, not all F7 or AAA acceptance.
The receiver is a slow generic moving source, not a paced, continuously fast firework.
Three source/main frames of lag at the fixed simulation cadence can mean about 50 ms of simulation
displacement; that differs from the measured unpaced wall-clock age and precedes final display.
F7E2 must measure HDR-star/receiver spatial offset at real-time cadence and approve visual lag.
No extrapolation or direct render-world clustered-light integration has been implemented.

Production finale/game-frame certification, editor light controls, and lit Aestra particle/volume
smoke remain open. Ordinary lit meshes responding is not proof that current unlit smoke responds.
