# F8.1C3 — opt-in saved persistent lit-smoke show candidate

2026-10-08, NVIDIA GeForce RTX 4070 SUPER / Vulkan, MSVC Rust 1.98.1,
repository optimized **test** profile. This qualifies a bounded saved candidate,
not final AAA smoke art, cross-draw transparency or production-finale performance.

## Public integration and saved assets

The canonical host remains `bevy/aestra-bevy/examples/fireworks.rs`:

```powershell
cargo run --release --locked -p aestra-bevy --example fireworks -- --persistent-smoke-show
cargo run --release --locked -p aestra-bevy --example fireworks --features fireworks-audio -- --persistent-smoke-show --tier medium
```

`assets/test/effects/fireworks_show_persistent_smoke.aestra.ron` references four separate
saved multi-break/crackle/crossette/strobe variants. The original show and four sources
remain untouched. All 13 clip IDs, launch times, transforms, seeds and star-color overrides
are retained. Event links, capacities, non-burst-smoke emitters, parameters and representative
light/sound routes are unchanged. The existing shared persistent lit-billow material serves
both short launch smoke and longer burst smoke. Burst smoke has 12–14-second lifetimes,
slower drift/drag and larger size keys; its existing opacity envelope is retained.

Each child window lasts 18 seconds. The root lasts 37 seconds, including two quiet seconds
after the last clip ends at 35. The longer windows intentionally raise peak retained clips
from six to thirteen. No host timer injects births. These are editable saved assets, not
viewer-generated effects; the schema currently has no effect-inheritance mechanism.

The public host keeps its normal scale-one audience camera, HDR/bloom, environment,
optional audio and playback-only default. Representative caps are 8/4/2, with selected-star
lighting unauthored/disabled. Alpha smoke opts into GPU back-to-front ordering **within
each draw**, not between draws. Smoke labs are mutually exclusive with this flag; an
explicit `--effect` still overrides the default asset path. This is not yet a replacement
for the default show.

## Native image and lifecycle qualification

Two fresh all-tier runs at `persistent-show-qualified-images-2026-10-08` and
`persistent-show-qualified-repeat-2026-10-08` pass in 93.69 / 91.59 seconds. All **39 PNGs
match exactly**. They use a 480×360 LDR target with the public audience camera, but no
public HDR/environment/audio. At 9.3 seconds older smoke overlaps three simultaneous
breaks; at 18.4 seconds a late break illuminates accumulated smoke. Reversible light
muting uses zero lumens without deleting requests. Density-zero bindings hide smoke,
then restoring authored defaults recovers the exact image.

| Tier | Light-gain pixels at 9.3s / 18.4s | Visible tail pixels at 30s | Tail particles observed |
| --- | --- | --- | --- |
| high | 2,779 / 2,017 | 1,802 | 137 |
| medium | 2,582 / 1,518 | 1,095 | 67 |
| low | 1,882 / 1,028 | 744 | 35 |

All light/density restorations and frozen repeats have zero pixel delta. At 33 seconds
four late child effects are still retained until their authored 34/35-second ends, and
their actual GPU-observed particle arrays are all zero. Empty/missing observations may
not satisfy this gate. Smoke disappears naturally without seek, replay, forced clearing
or owner removal. Sort pairs/owned bytes are zero at the 37-second root endpoint.

The first attempt, `target/fireworks-f8/persistent-show-images-2026-10-08`, remains
**rejected**. Its 18.3-second sample landed on the asynchronous birth/cue-delivery boundary:
zero active lights and zero light-gain pixels. Other lifecycle controls passed. Moving
the sample inside the **unchanged** pulse at 18.4 seconds passes unchanged thresholds;
this is not exact cue-delivery latency certification. The tail is faint on black, not
photographically approved. Exact image repeats do not establish physically correct
inter-draw transparency.

A final fresh all-tier run with the strengthened nonempty/four-retained-child population
gate passes in **89.67 seconds** and matches all 39 earlier PNGs. Its report hashes are
recorded separately; no missing-owner observation can vacuously certify natural cleanup.

## Live matched rendering costs

All **12 fresh-app runs pass** (334.40 seconds). GPU render-graph p95 milliseconds,
repetition 1 / repetition 2:

| Tier | Smoke drawn, GPU p95 | No-smoke-draw, GPU p95 | Smoke drawn, app-update wall p95 |
| --- | --- | --- | --- |
| high | 9.112 / 9.631 | 8.654 / 8.283 | 21.076 / 22.829 |
| medium | 9.226 / 9.665 | 8.307 / 8.339 | 20.785 / 23.248 |
| low | 9.511 / 8.933 | 8.477 / 8.481 | 21.833 / 20.352 |

App-update wall p95 exceeds a 16.67-ms budget in this unpaced harness. Neither the lower
GPU p95 nor successful lifecycle assertions certify a paced 60-fps game/show. A public-host
paced profile must separate preparation, retained-child simulation and rendering before
deciding whether runtime optimization is needed; whole-host responsiveness remains open.

All runs retain thirteen peak clips and finish at root frame 2220. Draw/control peak
async total populations match at 1,872 / 744 / 312 for high/medium/low, respectively.
Representative admission also matches: high/medium accept 13, low accepts 11 and drops
two at its intentional two-light ceiling. Invalid/binding-invalid requests are zero.
Observed checkpoint capture bytes stay zero. Drawn smoke owns up to 26 view/draw pairs
and 245,856 sort-buffer bytes; no-smoke-draw controls own none. Late sort-buffer allocation
counts are 48/72 (high repetitions), 72/72 (medium), and 144/144 (low), not zero. New clips
and visible draws acquire ownership during the show, so this is not the fixed-cohort
no-churn gate. All runs retire ownership to zero. These bytes are sort scratch, not total
runtime or driver VRAM. No tier-speedup or zero-allocation claim is made.

The explicit native suite runs two repetitions × three tiers × smoke drawn/not drawn,
in fresh apps **sequentially**, with no competing compilation or native GPU workloads.
The no-smoke-draw control removes only the two compiled smoke renderers per shell after
normal validation/compilation. Ordinary contracts verify identical simulation, links,
outputs, capacities, material instances and non-smoke renderers. It is deliberately not
a saved renderer-less asset (which would be invalid).

Each case measures 2,400 live updates at fixed 60 Hz through the 37-second Once endpoint
and a quiet tail: scale one, audience camera, 960×540 LDR, seed `0xf83b000000000001`,
playback-only history, public representative caps and selected cap zero. The initial
paused readiness capture and 120 paused warmup updates precede measurements. Later
first-use child shader/preparation costs remain included; this does not prewarm all clips.
Fresh GPU diagnostic arrivals and per-owner simulation sequences are deduplicated.

These are separate distributions, **not paired-frame incremental costs**. Do not subtract
independent p95s or sum pass/per-source percentiles. Render-graph GPU time excludes
preparation, CPU/game, audio and presentation. App-update wall time excludes the tooling
sleep and is not GPU completion time. Async arrivals can cross sampling boundaries.
The 37-second/13-overlap candidate must not be compared as work-matched with the historical
26-second/six-overlap unlit baseline. Raw `peak_async_smoke: 0` and empty `final_populations`
in these show cost reports mean those cohort-only instruments are not populated, not
that the show contains no smoke; the separate image gate supplies actual smoke counts.

## Reproduction and validation

Use fresh absolute output directories. Run each native command alone:

```powershell
$env:AESTRA_PERSISTENT_SHOW_IMAGES = 'C:\path\to\aestra\target\fireworks-f8\fresh-show-images'
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-viewer authored_persistent_smoke_show_lights_overlaps_and_drains --locked -- --ignored --test-threads=1
$env:AESTRA_PERSISTENT_SHOW_COSTS = 'C:\path\to\aestra\target\fireworks-f8\fresh-show-costs'
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-viewer persistent_smoke_show_live_matched_draw_costs --locked -- --ignored --test-threads=1
```

The [evidence JSON](persistent-smoke-show-2026-10-08.json) records source/raw-report/image
hashes, rejection provenance, visual controls and per-run cost distributions. No native
selected-position CPU readback/copy fallback or replay checkpoint capture is permitted.
Ordinary validation: audio-enabled public example **16 passed**; viewer **98 passed,
26 explicit native/export tests ignored**. Strict Clippy for both changed packages, all
targets and public audio feature passes with `-D warnings`; formatting/diff checks pass.
Vulkan GalaxyOverlay layer warnings and repeated Bevy logger initialization messages
are expected harness noise, not GPU validation errors.

**Next:** cross-draw transparency qualification, then public-host HDR/environment smoke-art
review against reference images. Continuous selected-star lighting, generic fluid injection,
large-pool/finale costs and complete F8 acceptance remain separate. Preserve playback-only
defaults and do not replace the normal show until those visual gates pass.
