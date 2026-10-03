# Fireworks F0 — reproducible validation baseline

This is the baseline for [the realistic-fireworks roadmap](../../docs/new/AESTRA_REALISTIC_FIREWORKS_ROADMAP_2026-10-03.md), not a production shell. The viewer flag `--fireworks-f0` builds one effect with rockets, a 48-star `OnDeath` burst, and collision glints. Its authored IDs and seed are fixed unless `--seed` overrides the seed. The same viewer supplies exact simulation-frame capture, native GPU timing and a fixed dark 3D scene with a ground plane and two geometry markers. Fixed inputs do not imply the current GPU event chain is bitwise repeatable; see the observation below.

## Capture protocol

Later bounded secondary-shell evidence: [F5A multi-break](secondary-shell-2026-10-03.md)
and [F5B particle-driven host cues](host-cues-2026-10-03.md), followed by
[F5C delayed crackle](crackle-2026-10-03.md) and
[F5D four-arm crossette](crossette-2026-10-03.md) and
[F5E phased strobe](strobe-2026-10-03.md) and
[F5F explicit tier budgets / overlapping volley](tier-volley-2026-10-03.md).
F6A adds a [26-second reusable clip show](show-composition-2026-10-03.md), with
per-clip admission and concurrent project reporting across all three tiers.
F6B validates [nested spatial particle host cues](spatial-host-cues-2026-10-03.md), including
all 13 launches/breaks, seek and restart at each tier.
These do not certify finale scale.

F7D1 adds [portable particle-light selection measurements](particle-light-selection-2026-10-03.md).
They cover per-output GPU selection only, not live realization or full-show lighting acceptance.
F7D2A adds [global admission and live GPU integration](particle-light-live-integration-2026-10-03.md).
F7D2B measures [authored hero/volley/show selection with normal rendering intact](particle-light-workloads-2026-10-04.md),
with counter/resource gates and explicit cap-zero baselines. Visible selected-light realization and
production-finale certification remain separate acceptance gates.

Run from the repository root on a native GPU. Use a clean output directory per run and record the Git revision, OS, GPU/driver, backend, resolution (the viewer is 960 × 540), quality tier, seed and camera in the run notes. Keep the same backend and hardware for comparisons; GPU timings across different adapters/backends are not interchangeable.

```powershell
New-Item -ItemType Directory -Force target/fireworks-f0 | Out-Null
cargo run --locked -p aestra-viewer -- --fireworks-f0 --camera audience --backend gpu --sample-frames 0,60,90,120,180,300 --capture target/fireworks-f0/audience
cargo run --locked -p aestra-viewer -- --fireworks-f0 --camera close --backend gpu --sample-frames 0,60,90,120,180,300 --capture target/fireworks-f0/close
cargo run --locked -p aestra-viewer -- --fireworks-f0 --camera wide --backend gpu --sample-frames 0,60,90,120,180,300 --capture target/fireworks-f0/wide
cargo run --locked -p aestra-viewer -- --fireworks-f0 --camera audience --backend gpu --gpu-bench target/fireworks-f0/gpu-bench-1.json
```

Captures use exact 60 Hz simulation frames and write a capture manifest with the active backend/adapter. The benchmark warms up for 120 frames and measures 600 frames; its JSON reports p50/p95/p99 for available GPU diagnostics. Run at least three times on an otherwise idle GPU. A missing timestamp is **unavailable**, not zero cost. Keep the raw JSON and selected frames with each baseline; do not approve a visual reference until the images have been inspected.

The built-in scene intentionally has no HDR/bloom, VFX lighting or smoke. The ground and markers are visual scale/light-response references, not proof of those later capabilities. The CLI camera presets are fixed: `close`, `audience` and `wide`; `--camera` is only valid with `--fireworks-f0`.

## Authored-shell review matrix — F4L

Use PowerShell 7 and a native GPU. This builds the viewer once, then sequentially captures
Peony, Chrysanthemum, Pistil and Willow from close/audience/wide cameras with two independent
presentation policies: `authored` (sprite/trail floors 0/0) and `sampled` (2/2). The latter is
an **opt-in comparison**, not a new runtime default. These are the bounded F3 prototypes,
not the production-density Test A workload.

```powershell
pwsh -File benchmarks/fireworks/capture-shell-review.ps1 -PlanOnly
pwsh -File benchmarks/fireworks/capture-shell-review.ps1 -OutputDirectory target/fireworks-f4/shell-review-new
# A single case, useful for reproducing a visual finding:
pwsh -File benchmarks/fireworks/capture-shell-review.ps1 -Shell willow -Camera wide -Response sampled -OutputDirectory target/fireworks-f4/willow-review-new
```

The output folder must not exist; there is no overwrite/resume or automatic golden-reference
approval. `-PlanOnly` prints the exact commands without building, launching or writing files.
Camera/response/shell filters are deduplicated. Captures default to high tier;
`-Tier medium|low` selects explicit profiles for the F5 shells/secondary volley
(older F3/F4 fixtures retain authored budgets). They use seed
`0xf1e0000000000001`, GPU/playback-only history, opt-in stable transparency, 960×540,
60 Hz, HDR/Tony/0-stop exposure/0.15 bloom. Stable ordering is for these small visual probes,
**not** the fast unsorted live-performance path.

| Shell | Exact lifecycle frames, in contact-sheet reading order |
| --- | --- |
| Peony, Chrysanthemum, Pistil | 45, 80, 110, 150, 210, 300, 390, 420 |
| Willow | 45, 80, 110, 150, 240, 330, 450, 540 |

Each case writes eight frame PNGs, a contact sheet, capture manifest and preview report.
The runner checks native-GPU success/compatibility, resolution/seed/frame/response metadata,
the observed 256-star main cohort (plus Pistil's 96-star inner cohort), and measured zero
final live particles, occupied/retired trails, evictions and truncation. These endpoint/peak
checks do not establish per-frame production, never-dropped events or real-time performance.
Failures stop the run and preserve partial evidence. `review-manifest.json` records the plan,
Git revision/dirty paths, viewer/asset source hashes, adapter, endpoint telemetry and each
frame's SHA-256; artistic acceptance remains explicitly pending.

Inspect the individual PNGs at native resolution, not only the downscaled contact sheet.
Compare launch/flash hierarchy, expansion, color separation, arc shape, tail continuity and
decay; note camera crops and smoke/scene limitations separately from rendering defects.
Stills cannot certify animated shimmer or AAA realism. Select real footage with an agreed
camera/exposure target before artistic approval; do not run `--approve-visual-reference`
merely because the runner passes. See [the first review](shell-review-2026-10-02.md).

## Reference-driven hero — F4M

`assets/test/effects/fireworks_reference_hero.aestra.ron` is a separate, editable
Cannes-inspired pink/gold shell. The four F3 assets and runtime sampling defaults are
unchanged. It uses the existing project material, appearance, death-event and trail APIs;
there is no fireworks-specific simulation or private viewer material injection.

```powershell
pwsh -File benchmarks/fireworks/capture-shell-review.ps1 -Shell reference-hero -PlanOnly
pwsh -File benchmarks/fireworks/capture-shell-review.ps1 -Shell reference-hero -OutputDirectory target/fireworks-f4/hero-review-new
# Interactive native-GPU playback, explicitly without replay history:
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f4-reference-hero --camera close --semantic-materials --backend gpu --history playback-only --hdr --exposure 0 --tonemapping tony --bloom 0.15 --sprite-min-pixels 2 --trail-min-pixels 2
```

The runner still defaults to the original four-shell/24-case matrix. Selecting
`reference-hero` produces six cases (three cameras × two floor policies), frames
45, 80, 110, 150, 210, 270, 360, 480. It checks observed main/inner/ember/smoke peaks of
384/96/128/48 and the same measured-zero final cleanup gates. The hero has independent
whole-shell camera presets; their positions are not interchangeable with F0's baseline
detail/distance cameras. See [reference choices and findings](reference-hero-2026-10-02.md).

## F5A — secondary particle-driven shell

The editable `fireworks_multi_break.aestra.ron` fixture uses two death-event generations:
one rocket → 64 main stars → eight secondary sparks each. Use viewer probe `f5-multi-break`
or the review runner's `-Shell multi-break`. The runner checks bounded observed cohorts and
cleanup; total admitted work is verified separately in uninterrupted playback-only mode.
F5 bench mode records fixed 60 Hz ticks to cover the seven-second lifecycle, not to certify
real-time finale throughput. Interactive mode still uses wall-clock time.

See [implementation, GPU admission evidence and reproduction](secondary-shell-2026-10-03.md).
Host cues, other secondary-shell styles and artistic acceptance remain open.

## First measured baseline — 2026-09-29

Worktree based on roadmap commit `3fa3805`, plus the uncommitted F0 fixture/viewer changes. The [machine-readable baseline](baseline-2026-09-29.json) records the run settings and values. Windows, NVIDIA GeForce RTX 4070 SUPER, Vulkan, driver reported as NVIDIA; 960 × 540, GPU backend, high tier, default seed `0xf1e0000000000001`, audience camera. The viewer reported a physical capacity of 4,194,240 particles and an effective configured budget of 262,144. The effect's compiled capacity is 6,208 particles. Three runs each used 120 warm-up and 600 measured frames; values below are milliseconds from the available GPU timestamp diagnostics:

| Run | Simulation p50 / p95 / p99 | Main 3D transparent pass p50 / p95 |
| --- | --- | --- |
| 1 | 0.392 / 0.554 / 0.698 | 0.034 / 0.041 |
| 2 | 0.403 / 0.458 / 0.555 | 0.034 / 0.041 |
| 3 | 0.403 / 0.454 / 0.558 | 0.034 / 0.042 |

These are a **small, trail-free baseline**, not Test A timings or an AAA performance claim. The capture report observed 502 live particles at frame 300 (4 rockets, 397 stars, 101 glints) and estimated 397,368 bytes of effect buffers; neither figure is a total requested/emitted count or total GPU memory. Per-pass event gather, source/list overflow and seek latency are not yet reported independently. The GPU benchmark originally wrote `warmup: 0` after consuming warm-up; F0 corrects that report field to retain the configured 120.

Two fresh audience-camera captures with identical IDs, seed and exact frames produced identical PNG hashes at frames 0, 60, 90 and 120, but **different hashes at frames 180 and 300**, when the collision/glint chain is active. The visible differences are small, but the deterministic-playback F0 exit criterion is **not met** for the full effect. Keep both runs under `target/fireworks-f0/` for local investigation; do not approve the late frames as exact visual references. F1 must verify whether event capture/expansion, target allocation or another stateful step causes the divergence and add a replay regression.

F1's production lockstep GPU regression compares canonical per-ordinal particle records, captured event records, spawn counts and transparent draw ordinals at frames 120, 180, 240 and 300 across two fresh runs. It also seeks back to frame 90 and replays through those frames; all records match bit for bit, with both event links active. Before the presentation fix, two fresh viewer captures differed at frames 180 and 300 by 99 and 68 pixels respectively, with a maximum summed RGB-channel delta of 3 per pixel. Particle slots can differ across runs even when per-ordinal state is identical; atomic live-slot compaction had made transparent draw order nondeterministic. Exact transparent ordering is now an **opt-in capture mode** (`--stable-transparency`) that sorts up to 4,096 live particles per emitter by spawn ordinal without changing simulation slots. Two fresh opt-in audience captures have identical PNG hashes at frames 120, 180 and 300. Normal runtime playback defaults to unsorted GPU compaction: it avoids the bounded sort and its frame cost, but exact transparent pixels across runs are not promised. Above 4,096 live particles per emitter, even the capture mode leaves draw order unordered pending a scalable sort.

To repeat the exact-image comparison, include `--stable-transparency` in both audience capture commands and compare each `frame-*.png` pair using `Get-FileHash`. Do not compare contact sheets alone: individual frame mismatches identify when divergence begins.

## Current scale-limit probes

Run `cargo test --locked -p aestra-viewer --bin aestra-viewer fireworks_f0` to check the fixture and current boundaries. One event link now compiles at count 800 but not 801; five worst-case 800-child links fit the conservative 128 MiB per-effect list budget, while six do not. One trail emitter still compiles at 256 parents but not 257 or 800. Existing trail tests in `crates/aestra-gpu/tests/trail_contract.rs` cover history details; these probes keep the proposed firework workload in view.

Two GPU probes complement those compiler checks:

```powershell
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event --camera audience --backend gpu --sample-frames 0,15,30,45,60 --capture target/fireworks-f0/event-probe
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-hero --camera audience --backend gpu --sample-frames 60 --capture target/fireworks-f1/hero-800
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe trail --camera close --backend gpu --sample-frames 0,120,240,270,300 --capture target/fireworks-f0/trail-probe
```

The event probe fills 64 rocket slots on one 60 Hz tick, gives them a fixed 0.5-second lifetime, and requests 64 children per death: **4,096 requested children** from one supported link. The original F0 frame-60 GPU report measured **1,024 live stars** against 4,096 destination slots and had **no platform warning**. F1's diagnostic slice then identified **3,072 dropped children per cohort** from the bounded link list. The per-link list now scales with captured source capacity and authored fan-out; the frame-60 RTX 4070 SUPER/Vulkan capture measures **4,096 live stars**, zero platform warnings. The separate single-link hero probe authors **800 children from one rocket death** and measures **800 live stars** at frame 60. A deterministic GPU ordering pass replaces the quadratic source scan; GPU counters now distinguish captured child demand, list drops and destination accepts. A continuing 64-rocket probe can later fill its 4,096-slot destination and logs newly rejected cohorts rather than silently losing them. The original F0 baseline JSON remains historical. Source-event capture remains capped at 1,024, and event counters are not yet in the profiler or capture report.

The trail probe derives its material and emitter behavior from the bundled Trail Lab fixture, then raises one emitter to 256 parents/owners and 32 records per trail. A frame-120 capture measured **256 occupied trails**, 256 live particles, 27,588 submitted vertices and **183 truncated trails** with no platform warning. A separate later capture, after parent deaths, measured **127 live particles, 209 occupied trails, 82 retired trails, 175 truncated trails and zero evictions** at frame 300. The trails were out of this camera's view then, so that capture proves reported retained history, not visible tail quality. The supported-side probe visibly runs earlier; it does not imply the desired 800-parent hero burst is supported or that record truncation is acceptable. F1B must establish the meaning and acceptability of the truncation count under the hero workload. Probe captures and raw reports remain in ignored `target/fireworks-f0/` locally.

GPU benchmark runs use the same 120-frame warm-up and 600-frame sampling as the baseline. The original, truncated event probe measured simulation p50/p95/p99 **0.251/0.278/0.528 ms** and main transparent pass p50/p95 **0.117/0.157 ms**. A later full-cohort run before presentation ordering measured simulation p50/p95/p99 **0.292/0.325/0.497 ms**. With the portable 16 KiB presentation sort, the same probe measured **1.499/1.543/1.758 ms**. After making the sort opt-in, the default live-playback run measured **0.344/0.376/0.601 ms** on the same named GPU. These are aggregate simulation measurements, not stage-isolated sort costs, and normal run-to-run variance applies. The capture sort's extra time is a reason to keep it off the live path; optimizing or replacing it matters only if exact high-density captures become a requirement. The final six-second, 256-owner trail probe measured simulation p50/p95/p99 **8.267/8.557/49.277 ms** and **8.376/8.589/53.026 ms** in two runs; a new default-fast run measured **8.820/9.160/182.990 ms**, confirming that presentation sorting was not the trail bottleneck. Its transparent pass was roughly 0.01/0.05 ms in the earlier runs. The trail-update shader currently processes each emitter on one invocation and scans owner chunks for every live head, which is a likely scaling cause but not yet stage-timed. Earlier three-second probe runs had even larger outliers and are not directly comparable. Raw timing JSON is under ignored `target/fireworks-f0/` and `target/fireworks-f1/`.

### Candidate target budgets and instrumentation inventory

These are **targets to test**, not measurements or claims of support. Development reference: RTX 4070 SUPER/Vulkan, high tier, 960 × 540, 60 Hz (16.67 ms total frame). The VFX GPU budget is for simulation **plus** its transparent pass, leaving the rest of the frame to the host. Buffer budgets cover the effect's particles, trails, event queues and checkpoints, not all application VRAM. Seek is a wall-clock p95 target for an exact-frame replay from a cold effect. Recalibrate at 1920 × 1080 and on lower tiers before making release claims.

| Workload | VFX GPU p95 target | Effect buffer target | Cold seek p95 target | Current status |
| --- | ---: | ---: | ---: | --- |
| A — hero | ≤2 ms | ≤64 MiB | ≤0.5 s | Single 800-child link executes; production trails, source limits, replay proof and full-scene budget remain open |
| B — volley | ≤3 ms | ≤128 MiB | ≤1 s | Not executable at authored density |
| C — finale | ≤4 ms | ≤256 MiB | ≤2 s | Not executable at authored density |

Available now: compiled particle/trail capacities, live particles, occupied and retired trail owners, trail eviction/truncation counts when their readback is available, submitted vertices/primitives, estimated effect buffer bytes, aggregate GPU simulation and transparent-pass timings. Persistent GPU counters and warnings cover captured per-link child demand, list drops and accepted destination spawns. Missing or unavailable: full source-event demand beyond the 1,024-entry capture buffer, per-link counters in the profiler/capture report, stage-specific event timing, measured effect GPU allocation, and isolated cold-seek latency. The original F0 exit gate remains **partial**: F1 fixed the observed late chained-event image mismatch for opt-in baseline captures, but production-density trails still reject compilation. F1/F1B own the remaining live-runtime correctness and scale gates; exact-image ordering above 4,096 is an optional capture feature.

## Test ladder and gates

| Workload | Requested scene | F0 status | Required future gate |
| --- | --- | --- | --- |
| Baseline | 48 stars per rocket, no history trails | Captured and timed; late chained-event frames differ across launches | Exact-frame replay regression before approving references |
| A — hero | One 300–800-star burst, a trail per star | A single 800-child link now compiles and yields 800 live GPU stars; production-density trails and full-scene replay/seek gates remain open | F1/F1B compile and execute with exact counts, replay and seek |
| B — volley | 8–12 overlapping shells, thousands of trails | Not yet executable at authored density | Measured concurrent event/trail budget |
| C — finale | 20–40 shells, 20k–100k visible particles and thousands/tens of thousands of trail histories | Not yet executable at authored density | Tier-specific frame/memory budget with no silent drops |

Do not split Test A into many links or emitters to bypass the present limits. First establish A, then B and C. Choose numerical GPU-time, memory and seek-latency budgets for named target hardware after the baseline captures; do not invent universal budgets before measuring. Each future result must state both requested and produced work, including retained dead-parent tails.
