# Fireworks F0 — reproducible validation baseline

This is the baseline for [the realistic-fireworks roadmap](../../docs/new/AESTRA_REALISTIC_FIREWORKS_ROADMAP.md), not a production shell. The viewer flag `--fireworks-f0` builds one effect with rockets, a 48-star `OnDeath` burst, and collision glints. Its authored IDs and seed are fixed unless `--seed` overrides the seed. The same viewer supplies exact simulation-frame capture, native GPU timing and a fixed dark 3D scene with a ground plane and two geometry markers. Fixed inputs do not imply the current GPU event chain is bitwise repeatable; see the observation below.

## Capture protocol

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

## First measured baseline — 2026-09-29

Worktree based on roadmap commit `3fa3805`, plus the uncommitted F0 fixture/viewer changes. The [machine-readable baseline](baseline-2026-09-29.json) records the run settings and values. Windows, NVIDIA GeForce RTX 4070 SUPER, Vulkan, driver reported as NVIDIA; 960 × 540, GPU backend, high tier, default seed `0xf1e0000000000001`, audience camera. The viewer reported a physical capacity of 4,194,240 particles and an effective configured budget of 262,144. The effect's compiled capacity is 6,208 particles. Three runs each used 120 warm-up and 600 measured frames; values below are milliseconds from the available GPU timestamp diagnostics:

| Run | Simulation p50 / p95 / p99 | Main 3D transparent pass p50 / p95 |
| --- | --- | --- |
| 1 | 0.392 / 0.554 / 0.698 | 0.034 / 0.041 |
| 2 | 0.403 / 0.458 / 0.555 | 0.034 / 0.041 |
| 3 | 0.403 / 0.454 / 0.558 | 0.034 / 0.042 |

These are a **small, trail-free baseline**, not Test A timings or an AAA performance claim. The capture report observed 502 live particles at frame 300 (4 rockets, 397 stars, 101 glints) and estimated 397,368 bytes of effect buffers; neither figure is a total requested/emitted count or total GPU memory. Per-pass event gather, source/list overflow and seek latency are not yet reported independently. The GPU benchmark originally wrote `warmup: 0` after consuming warm-up; F0 corrects that report field to retain the configured 120.

Two fresh audience-camera captures with identical IDs, seed and exact frames produced identical PNG hashes at frames 0, 60, 90 and 120, but **different hashes at frames 180 and 300**, when the collision/glint chain is active. The visible differences are small, but the deterministic-playback F0 exit criterion is **not met** for the full effect. Keep both runs under `target/fireworks-f0/` for local investigation; do not approve the late frames as exact visual references. F1 must verify whether event capture/expansion, target allocation or another stateful step causes the divergence and add a replay regression.

To repeat the comparison, capture again to `target/fireworks-f0/audience-repeat` with the same audience command, then compare each `frame-*.png` pair using `Get-FileHash`. Do not compare contact sheets alone: individual frame mismatches identify when divergence begins.

## Current scale-limit probes

Run `cargo test --locked -p aestra-viewer --bin aestra-viewer fireworks_f0` to check the fixture and the present compilation boundaries. These tests make the current limits explicit, not desirable: one link compiles at count 64 but not 65 or 800; one trail emitter compiles at 256 parents but not 257 or 800. F1/F1B must replace these expected-rejection assertions with passing production-density cases. Existing trail tests in `crates/aestra-gpu/tests/trail_contract.rs` cover history details; these F0 probes keep the proposed firework workload in view.

Two GPU probes complement those compiler checks:

```powershell
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event --camera audience --backend gpu --sample-frames 0,15,30,45,60 --capture target/fireworks-f0/event-probe
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe trail --camera close --backend gpu --sample-frames 0,120,240,270,300 --capture target/fireworks-f0/trail-probe
```

The event probe fills 64 rocket slots on one 60 Hz tick, gives them a fixed 0.5-second lifetime, and requests 64 children per death: **4,096 requested children** from one supported link. The original F0 frame-60 GPU report measured **1,024 live stars** against 4,096 destination slots and had **no platform warning**. F1's diagnostic slice then identified **3,072 dropped children per cohort** from the bounded link list. The per-link list now scales with captured source capacity and authored fan-out; a new frame-60 RTX 4070 SUPER/Vulkan capture measures **4,096 live stars**, zero platform warnings, and the GPU conformance test proves an intentionally undersized list still reports the exact loss. The original baseline JSON remains historical. Source demand/capture, destination acceptance, aggregate list-memory accounting and profiler exposure still need counters and budgets.

The trail probe derives its material and emitter behavior from the bundled Trail Lab fixture, then raises one emitter to 256 parents/owners and 32 records per trail. A frame-120 capture measured **256 occupied trails**, 256 live particles, 27,588 submitted vertices and **183 truncated trails** with no platform warning. A separate later capture, after parent deaths, measured **127 live particles, 209 occupied trails, 82 retired trails, 175 truncated trails and zero evictions** at frame 300. The trails were out of this camera's view then, so that capture proves reported retained history, not visible tail quality. The supported-side probe visibly runs earlier; it does not imply the desired 800-parent hero burst is supported or that record truncation is acceptable. F1B must establish the meaning and acceptability of the truncation count under the hero workload. Probe captures and raw reports remain in ignored `target/fireworks-f0/` locally.

GPU benchmark runs use the same 120-frame warm-up and 600-frame sampling as the baseline. The event probe measured simulation p50/p95/p99 **0.251/0.278/0.528 ms** and main transparent pass p50/p95 **0.117/0.157 ms**. This is the cost of the **truncated** workload, not the requested 4,096 children. The final six-second, 256-owner trail probe measured simulation p50/p95/p99 **8.267/8.557/49.277 ms** and **8.376/8.589/53.026 ms** in two runs; its transparent pass was roughly 0.01/0.05 ms. Earlier three-second probe runs had even larger outliers and are not directly comparable. These large simulation outliers are real report values, but their stage/cause is not isolated. Neither is an acceptable hero-performance result. Raw timing JSON is under `target/fireworks-f0/`.

### Candidate target budgets and instrumentation inventory

These are **targets to test**, not measurements or claims of support. Development reference: RTX 4070 SUPER/Vulkan, high tier, 960 × 540, 60 Hz (16.67 ms total frame). The VFX GPU budget is for simulation **plus** its transparent pass, leaving the rest of the frame to the host. Buffer budgets cover the effect's particles, trails, event queues and checkpoints, not all application VRAM. Seek is a wall-clock p95 target for an exact-frame replay from a cold effect. Recalibrate at 1920 × 1080 and on lower tiers before making release claims.

| Workload | VFX GPU p95 target | Effect buffer target | Cold seek p95 target | Current status |
| --- | ---: | ---: | ---: | --- |
| A — hero | ≤2 ms | ≤64 MiB | ≤0.5 s | Blocked by compile limits and silent event truncation; 256-owner trail probe alone far exceeds GPU target |
| B — volley | ≤3 ms | ≤128 MiB | ≤1 s | Not executable at authored density |
| C — finale | ≤4 ms | ≤256 MiB | ≤2 s | Not executable at authored density |

Available now: compiled particle/trail capacities, live particles, occupied and retired trail owners, trail eviction/truncation counts when their readback is available, submitted vertices/primitives, estimated effect buffer bytes, aggregate GPU simulation and transparent-pass timings. Missing or unavailable: event demand/captured/expanded/spawned/dropped by link, stage-specific event timing, measured effect GPU allocation, and isolated cold-seek latency. The F0 exit gate remains **partial**: late chained-event frames differ across launches, the larger workloads reject compilation, and missing event counters prevent loss attribution. F1/F1B own those runtime correctness and scale gates.

## Test ladder and gates

| Workload | Requested scene | F0 status | Required future gate |
| --- | --- | --- | --- |
| Baseline | 48 stars per rocket, no history trails | Captured and timed; late chained-event frames differ across launches | Exact-frame replay regression before approving references |
| A — hero | One 300–800-star burst, a trail per star | Authored link count above 64 and production-density trails still reject compilation; the supported 64 × 64 fan-out probe now yields all 4,096 children | F1/F1B compile and execute with exact counts, replay and seek |
| B — volley | 8–12 overlapping shells, thousands of trails | Not yet executable at authored density | Measured concurrent event/trail budget |
| C — finale | 20–40 shells, 20k–100k visible particles and thousands/tens of thousands of trail histories | Not yet executable at authored density | Tier-specific frame/memory budget with no silent drops |

Do not split Test A into many links or emitters to bypass the present limits. First establish A, then B and C. Choose numerical GPU-time, memory and seek-latency budgets for named target hardware after the baseline captures; do not invent universal budgets before measuring. Each future result must state both requested and produced work, including retained dead-parent tails.
