# F8.1C2 — exact live clocks and natural final-tick cleanup

2026-10-08, NVIDIA GeForce RTX 4070 SUPER / Vulkan, MSVC Rust 1.98.1,
repository optimized **test** profile. Bounded native cleanup qualifies; saved-show
migration, smoke art/cross-draw transparency and full F8 acceptance do **not**.

## Corrections and rejected partial fixes

Three independent rounding problems affected the same endpoint:

1. Stateful hosts repeatedly added an f32 tick while their playback clocks advanced
   integer frames. Frame 1080 could leave instance time 17.999897. The runtime driver,
   public Bevy player and editor now advance consecutive exact clock snapshots.
   This preserves per-tick bindings, cue windows, restart-loop epochs and playback-only
   checkpoint policy; frame-addressed stateful seeks after looping rebuild the first cycle.
2. GPU target conversion divided by a rounded f32 reciprocal: even exact 18 seconds
   mapped to tick 1079. Both stateful GPU dispatch paths, coupled extension progress and
   binding traces now share `trace_tick`. Canonical f32 clock boundaries round-trip;
   other times floor, including the immediately preceding representable float.
   A one-hour boundary/next-down/next-up/half-tick regression avoids a broad epsilon.
3. CPU/GPU particle ages accumulated float drift, delaying parent death, child birth
   and final expiry. Ages now reconstruct their fixed-grid tick on each update.
   Lifetimes and death comparisons are unchanged; no end-of-effect force-kill or
   grace interval was introduced. CPU lifetimes of 2, 14 and 240 seconds expire on
   the exact tick. GPU conformance reference kernels use the same fixed-grid integration.

The host-only attempt at `target/fireworks-f8/clock-final-tick-2026-10-08` and
host-plus-backend attempt at `target/fireworks-f8/clock-final-tick-qualified-2026-10-08`
both fail the stronger first-case population gate with `[1,0,0,0,0]`.
Their `accepted.json` remains false. The historical
[C1 report](smoke-live-costs-2026-10-08.md) remains unchanged and does not claim cleanup.

## Native qualification

The final matrix at `target/fireworks-f8/clock-smoke-qualified-2026-10-08` passes
all **30 fresh-app runs** (226.68 seconds): two repetitions × high/medium/low × five cases.
The 24 cohort runs contain **60 root observations**, all at player frame 1080,
instance time **18**, GPU-observed time **18**, and final populations
**[0,0,0,0,0]**. These checks occur before owner removal. No corrective seek,
replay, particle clear, lifetime-threshold relaxation or image-only drain claim is used.
The final paused transport pump advances no time and only receives async observations.

One root reaches 64 smoke / 128 total particles; four coincident roots reach 256 smoke /
512 total. Every root accepts exactly 32 children on each of four links, with zero source
overflow, omitted expansion or rejected destination. Draw/control admission stays identical.
Stable-capacity sort ownership peaks at 3/12 pairs and 28,368/113,472 bytes with smoke drawn,
or 2/8 pairs and 18,912/75,648 bytes in no-smoke-draw controls (launch markers remain).
There are no late sort allocations after update 60; owner retirement returns owned bytes
and pairs to zero. Observed replay-checkpoint capture bytes remain zero. Native selected
positions never travel through a host readback/copy/allocation fallback.

Separate final all-tier cohort-image runs at `clock-smoke-final-2026-10-08` and
`clock-smoke-final-repeat-2026-10-08` pass the existing lighting, output, density,
lifecycle and retirement controls (40.29 / 45.20 seconds). All **84 PNGs match exactly**
between repeats. **66 differ** from `child-birth-outputs`: corrected birth/death timing
changes the previous pictures. Unchanged art or photo approval is not claimed.
The [evidence JSON](smoke-final-tick-2026-10-08.json) records raw-report/source hashes,
all run summaries, image hashes, rejected partial fixes and qualification limits.

## Corrected live cost observations

GPU render-graph p95 milliseconds, repetition 1 / repetition 2:

| Tier | One pool | One, no smoke draw | Four pools | Four, no smoke draw | Existing unlit show |
| --- | --- | --- | --- | --- | --- |
| high | 0.812 / 0.957 | 0.935 / 0.911 | 2.264 / 2.296 | 2.172 / 2.146 | 5.238 / 5.234 |
| medium | 0.941 / 0.923 | 0.913 / 0.885 | 2.443 / 2.275 | 2.247 / 2.156 | 5.385 / 5.487 |
| low | 0.934 / 0.969 | 0.902 / 0.928 | 2.288 / 2.282 | 2.142 / 2.150 | 5.253 / 5.304 |

Same setup and controls as C1: 960×540 LDR, seed `0xf83b000000000001`,
unpaced 60 Hz playback-only, shader-ready paused capture followed by 120 warmup updates,
1,080 cohort live updates and 1,680 show updates. Native GPU workloads run sequentially
after builds/lints finish. Tooling sleeps outside measured `app.update`.
Fresh GPU arrivals and per-owner simulation sequences are deduplicated.

These are separate distributions, **not causal paired-frame incremental deltas**.
Do not subtract independent p95s or sum pass/per-source percentiles. Render-graph work
excludes preparation, CPU/game, audio and presentation; app-update wall time is not GPU
completion time. Async arrivals can cross measurement boundaries.
The existing unlit 26-second show still has six active clips, peaks of 1,680/648/234
particles and terminal frame 1560. Its assets/camera/order/light policy are unchanged;
HDR/bloom/environment/audio are excluded. Four coincident roots remain synthetic
cross-draw cost stress, not authored-show or inter-draw sorting acceptance.

## Reproduction and validation

Use a fresh absolute output directory; run the native matrix alone:

```powershell
$env:AESTRA_SMOKE_LIVE_REPORTS = 'C:\path\to\aestra\target\fireworks-f8\fresh-clock-run'
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-viewer live_smoke_overlap_costs_and_authored_show_baseline --locked -- --ignored --test-threads=1
```

Run the separate native image gate twice, setting `AESTRA_SMOKE_COHORT_REPORTS`
to fresh absolute directories:

```powershell
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-viewer saved_shell_deaths_birth_overlapping_smoke_that_receives_later_break_light --locked -- --ignored --test-threads=1
```

Validation: runtime 76; public Bevy host 61; editor session 34; audio-enabled public
fireworks example 13; viewer 97 (24 explicit native tests ignored by the ordinary suite);
WGSL validation 6; required native CPU/GPU conformance 30; production coupled GPU
regressions 14 (2 separate native visual tests ignored). Strict Clippy for all targets of
the six changed packages passes with `-D warnings`; formatting and diff checks pass.
Windows sandbox-denied temp-file tests were rerun with access and passed.
Vulkan GalaxyOverlay-layer warnings and repeated Bevy logger messages occurred;
no GPU validation errors were reported. Incremental hard-link warnings copy cache files
and do not change qualification.

**Next:** opt-in saved-show migration to lit persistent particle smoke, with authored
overlap controls and matched costs through the public Bevy example. The normal show stays
unchanged/unlit for this milestone. Natural smoke art, cross-draw transparency, large-pool/
finale costs, generic fluid injection and full F8 acceptance remain separate gates.

