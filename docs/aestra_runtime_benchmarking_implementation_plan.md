# Aestra Runtime Benchmarking & Profiling — Implementation Plan

Companion to [`aestra_runtime_benchmarking_profiling_plan.md`](aestra_runtime_benchmarking_profiling_plan.md)
(the strategy). This document is the *execution* plan: concrete deliverables, the
files they touch, exit criteria, and the order to build them in.

## Guiding rule

> Every optimization must correspond to a measured bottleneck. The harness and
> instrumentation come first; runtime redesign only follows a baseline number.

---

## 0. Codebase grounding (verified)

Before planning, the strategy's key claims were checked against the tree:

| Strategy claim | Status | Evidence |
| --- | --- | --- |
| GPU artifact reconstructed every frame (§2.1) | **Confirmed** | `GpuEffectArtifact::from_instance` runs per-frame per player in `update_gpu_inputs` — `crates/aestra-bevy-render/src/gpu.rs:686` (also once in `prepare_gpu_effects:297`) |
| Effect telemetry needs building (§5) | **Partly exists** | `EffectProfile`/`EmitterProfile` already cover alive/capacity/draws/dispatches/buffer bytes — `crates/aestra-runtime/src/profile.rs` |
| Measured-vs-estimated honesty (§13) | **Exists** | `ProfileValue::{Measured,Estimated,Unavailable}` — `crates/aestra-runtime/src/profile.rs:6` |
| Tracy / criterion / bench app | **Missing** | No `trace_tracy` in `Cargo.toml`; no `apps/aestra-bench` |
| Editor telemetry surface | **Partly exists** | `apps/aestra-editor/src/profiler.rs` (UI panel) |

**Implications for the plan**
- The *machine profiler* (per-stage CPU/GPU wall time) is the real instrumentation
  gap, not effect telemetry. Extend `profile.rs`, don't reinvent it.
- `update_gpu_inputs` (`gpu.rs:686`) is the confirmed first optimization target.
- Reuse the existing capture/manifest infrastructure in `apps/aestra-viewer` (deterministic
  60 Hz stepping, seed control, JSON/manifest writing) as the model for `aestra-bench`.

---

## Phase 1 — Harness + minimal instrumentation (strategy M0+M1)

**Goal:** *Be able to explain where one Aestra frame is spent, and record it deterministically.*

Merge the strategy's M0 and M1: a baseline is not meaningful without at least
whole-frame + per-stage timing, so build them together.

### 1a. Create `apps/aestra-bench`
- New workspace member (add to `Cargo.toml` members list).
- Headless-capable binary that: loads a deterministic scenario → warms up
  pipelines/GPU resources → runs a fixed capture interval → collects metrics →
  writes machine-readable JSON → prints a human summary. (Strategy §8.)
- Model the CLI and deterministic-frame stepping on `apps/aestra-viewer`
  (`--capture`, `--frames`, `--seed`, `--backend`). Share code where practical.
- Scenarios live under `benchmarks/scenarios/` as `.aestra.ron` + a small manifest
  describing the matrix axis they exercise.

### 1b. Machine profiler (per-stage timing)
- Add a `StageTimings` structure (new module in `aestra-runtime`, sibling to
  `profile.rs`) capturing the §10 CPU stages: whole frame, runtime advance,
  CPU/reference eval, render extraction, GPU input prep, artifact update, material
  prep, buffer uploads, bind-group prep.
- Keep the `ProfileValue<T>` honesty model — anything not yet wired reports
  `Unavailable`, never a fake zero.
- Instrument the real systems (`prepare_gpu_effects`, `update_gpu_inputs`,
  material prep in `crates/aestra-bevy-render`) with `bevy_utils` spans so both
  Tracy and the bench harness read the same measurement points.

### 1c. Tracy CPU + allocation instrumentation — **done (spans)**
- Named `tracing` spans are placed at the strategy §6 points: `aestra::runtime::advance`
  (aestra-bevy `play_effects`), and in aestra-bevy-render `aestra::gpu::{prepare_instance,
  artifact_update, buffer_upload, material_prepare, bind_groups, simulate}` plus
  `aestra::gpu::queue_sprites` for the CPU-side draw queue. The `buffer_upload` span
  isolates the per-frame emitter/renderer upload — the CPU cost Phase 4 targets.
- Spans use the `tracing` crate directly, so they need no extra dependency and are
  no-ops without a subscriber.
- **To capture with Tracy:** build a binary with Bevy's `trace_tracy` feature enabled,
  e.g. `cargo run -p aestra-editor --features bevy/trace_tracy` (requires network to
  fetch `tracy-client`; deliberately not wired as a committed cargo feature so the
  offline workspace gate stays resolvable). `trace_tracy_memory` adds allocation
  tracking. Remaining: `aestra::runtime::evaluate_cpu` on the CPU backend path.

### 1d. GPU timestamps — **wired (compile-verified); validate on GPU lane**
- Uses Bevy's built-in render diagnostics (`RecordDiagnostics::time_span`) rather
  than a hand-rolled `TIMESTAMP_QUERY` pass. The `run_simulation` compute pass in
  `crates/aestra-bevy-render/src/gpu.rs` is wrapped in an `aestra::gpu::simulate`
  GPU time span; it is a no-op unless the host app adds `RenderDiagnosticsPlugin`.
- `aestra-viewer` adds `RenderDiagnosticsPlugin`, so the capture tool records GPU
  time on Vulkan/DX12. Timings surface through Tracy (`--features bevy/trace_tracy`)
  or a diagnostics logger.
- **Validate on the self-hosted GPU lane:** timestamp queries only produce real
  numbers on GPU hardware (Vulkan/DX12), so the values must be confirmed there, not
  in the headless CPU environment. This is why the code is written against Bevy's
  supported API and compile-checked here, but not runtime-verified.
- The sprite draw is **not** a separable Aestra pass to wrap: Aestra sprites are
  phase items added to Bevy's `Transparent2d`/`Transparent3d` passes via
  `add_render_command` (`gpu/render.rs`), so Aestra never begins its own render pass.
  Their GPU render time is already recorded by `RenderDiagnosticsPlugin` as
  `main_transparent_pass_2d` / `main_transparent_pass_3d`; in the viewer those passes
  contain essentially only Aestra sprites, so that timing is the Aestra render cost
  in practice. A dedicated `aestra::gpu::render` span would require fragile per-draw
  timestamps and is deliberately not added.
- Remaining: on the GPU lane, read the resolved GPU timings (sim-reset / sim /
  transparent-pass render) from the diagnostics store into the bench/capture JSON
  (strategy §7, §10 GPU block).

### 1e. Statistical output + JSON format
- Per timing metric record median/p95/p99/max/stddev (strategy §10); optional
  min/mean/MAD. **Never average FPS.**
- Emit the strategy §15 JSON schema (scenario, commit, hardware, content, cpu, gpu).
- Add normalized metrics (strategy §11): ns/1k slots, ns/1k alive, bytes/slot,
  draws/emitter, dispatches/effect, occupancy.

**Exit criteria:** `cargo run -p aestra-bench -- --scenario B002` produces valid
JSON with real (non-`Unavailable`) whole-frame CPU timing and a printed summary; a
Tracy capture shows the named spans.

---

## Phase 2 — Canonical scenarios + frozen baseline (strategy M0)

- Author the B001–B010 baseline scenarios (strategy §16): empty, single-small,
  single-dense, sparse-large, many-small-effects, many-emitters, loop-pressure,
  curve-stress, material-stress, overdraw-stress.
- Capture baseline JSON on current `main`, archive under
  `benchmarks/baselines/<commit>/` with hardware + driver + Bevy/wgpu version.
- Capture representative Tracy traces for the dense, sparse, and many-effects cases.

**Exit criteria:** archived, reproducible baseline JSON for all ten scenarios +
recorded hardware. This is the reference every later phase is measured against.

---

## Phase 3 — Remove per-frame CPU waste (strategy M2) — highest-value win

Target the confirmed hotspot: `update_gpu_inputs` (`gpu.rs:686`) rebuilds the full
`GpuEffectArtifact` every frame just to refresh emitter/renderer/globals buffers.

- Split immutable compiled data from mutable runtime state (strategy §2.1 target):
  introduce a `GpuEffectPrototype` (immutable: emitter/renderer layout, material
  variants, capacity, buffer layouts) built once at prepare time, and a
  `GpuEffectInstance` (time, seed, transform, runtime params, dirty flags,
  persistent buffers).
- Persistent particle buffers allocated once at create/resize — not per frame.
- The per-frame path uploads only dynamic emitter/renderer/globals data; no
  particle-vector construction.
- Precompute per-emitter invariants (transforms, quaternion normalization, static
  matrices) outside per-particle loops (strategy §4).

**Exit criteria:** re-run B003 (dense) and B004 (sparse) — CPU `artifact_update`
stage time drops materially and no longer scales with capacity on static effects.
Before/after numbers recorded against the Phase 2 baseline.

---

## Phase 4 — Dirty-state GPU updates (strategy M3)

- Add explicit invalidation flags on the GPU instance: `DirtyTime`,
  `DirtyTransform`, `DirtyParameters`, `DirtyEmitterData`, `DirtyMaterial`,
  `DirtyPrototype`, `DirtyBuffers` (strategy §2.8, M3).
- Per-frame update uploads/rebuilds only what changed. A static effect with only
  `time` advancing touches globals only.

**Exit criteria:** B002/B005 — a static effect shows near-zero CPU prep beyond the
time-globals write; measured against baseline.

---

## Phase 5 — Cache immutable GPU resources (strategy M4)

- Bind groups, pipelines, immutable material resources, and static emitter data
  are recreated only when their dependencies change (dirty flags from Phase 4).
- Verify shared materials actually share GPU resources (strategy §2.9).

**Exit criteria:** re-run instance-count scaling (B005, 100–1000 effects) — CPU
prep and bind-group time per effect drop; recorded.

---

## Phase 6 — Analytical kernel scaling study (strategy M5) — decision gate

Run the **full benchmark matrix** (strategy §9): capacity × occupancy × emitters ×
instances × spawn mode × curve complexity × loop pressure × renderers × materials ×
param mutation × nesting × screen coverage.

Determine measured relationships between capacity / occupancy / emitter count /
loop pressure / curve complexity and GPU simulation time.

**This phase decides** whether the analytical simulation is adequate as the main
gameplay runtime, and which §2.x risks are real bottlenecks vs. acceptable. Output
is a written scaling report, not code.

### GPU scaling result — measured (RTX 4070 SUPER / Vulkan)

A GPU-timestamp sweep (`aestra-viewer --gpu-bench`, archived under
`benchmarks/gpu-baselines/20d5785…/` and `…/a6e6103…/`) answers the strategy's four
§2.x GPU-simulation risks with real distributions. **Three confirmed, one refuted:**

| Risk | Verdict | Evidence (sim p50) |
| --- | --- | --- |
| §2.2 capacity-bound | **REFUTED** | b004 500k/5k-alive = 0.001 ms vs b003 100k/100k = 0.362 ms |
| §2.3 emitter lookup | **CONFIRMED** | b006 64-emitter/100k = 0.965 ms = **2.7×** b003 (1 emitter, same count) |
| §2.4 loop pressure | **overstated** | b007 LP=32 = 0.096 ms — but see correction below |
| §2.5 curve emission | **CONFIRMED** | b008 curve/~2k = 0.389 ms = **~8×/particle** vs b007 constant rate |

> **Correction (fe7ab75).** The "~100×/particle" attributed to loop pressure was a
> **measurement confound**: the b004 baseline dispatches ~7,800 workgroups vs b007's
> ~64, so b004's cheapness was mostly GPU-saturation latency-hiding, not loop pressure.
> Implementing the O(1) cycle lookup (Phase 7 #1) moved b007 only 0.096 → 0.080 ms
> (~17%), proving the cycle search was a *minor* share of the cost, not a ~100× cliff.
> The dominant per-slot cost is the per-particle simulation itself, plus GPU
> under-utilization at low workgroup counts. **Lesson: never compare across scenarios
> with very different workgroup counts.**

Implications:
- The capacity-scaling optimizations (dead-slot dispatch elimination, packed slot
  ranges) are **not** justified for the GPU kernel — dead slots are already free.
  The real capacity cost was CPU-side artifact allocation, fixed in Phase 3.
- Confirmed, still-open GPU bottleneck: curve-driven emission (§2.5, ~8×/particle,
  isolated by b007-vs-b008 which share workgroup counts, so credible). Emitter
  lookup (§2.3) bites only at high counts (64 ≈ 3×). Loop-pressure cycle search
  (§2.4) is addressed and was minor.
- Sweep completed (`a6e6103…/` manifest): emitter count 1/4/16/64 shows §2.3 bites
  only at high counts (64 ≈ 3–5×; 1–16 within noise); occupancy at fixed 10k alive
  across 10k/100k/1M capacity reconfirms §2.2 refutation (1M cap is *cheapest*).

---

## Phase 7 — Optimize measured analytical bottlenecks (strategy M6)

1. **Historical-cycle reconstruction (§2.4) — DONE (`fe7ab75`).** O(1) cycle lookup
   for constant per-cycle emission replaces the O(lifetime/duration) walk. Measured
   b007 0.096 → 0.080 ms (~17%); bit-identical output (prism_bloom visual regression
   RMSE 0.0000). Smaller than expected — see the correction above. Correct and
   zero-cost, so it stays, but it was not the big lever it appeared to be.
2. **Curve→time inverse table for emission (§2.5) — DONE (`14c20f3`).** Replaced the
   per-particle 12-iteration `curve_spawn_time` binary search with a 32-entry
   inverse-emission table (built CPU-side in `from_instance`, uploaded in
   `GpuEmitter`) that seeds a tight bracket, then 8 refinement bisections. GPU-only;
   the conformance tolerance absorbs it (as it did the prior 12-vs-20 gap). Measured
   b008 simulate p50 **0.389 → 0.204 ms (1.9×)**; the inversion portion alone (vs the
   0.081 ms constant-rate control) dropped **0.308 → 0.123 ms (~2.5×)**. Verified:
   curve GPU/CPU conformance passes, prism_bloom visual regression bit-clean, snapshot
   + full suite green. Bigger than Phase 7 #1 — and, unlike it, not confounded (the
   b008-vs-control comparison shares workgroup counts).
   - The confirming control lives at `benchmarks/sweep-scenarios/curve_control.ron`.
3. **Eliminate linear per-slot emitter search (§2.3)** — precomputed slot→emitter
   ranges or per-emitter dispatch. Only ~3× at 64 emitters (b006 = 0.965 ms), flat
   below, so still gated on the many-emitter path mattering. Note the
   [architecture survey](aestra_gpu_architecture_comparison.md) elevates the
   **per-emitter dispatch** form specifically: it removes the search *and* sizes each
   dispatch to its emitter's occupancy, subsuming the (separately refuted)
   analytically-sized single-dispatch idea. This is the measurably-motivated way to
   capture the professional engines' indirect-dispatch benefit.

### Per-particle floor — shader ablation (b003, 100k @ 100%, matched workgroups)

Ablating shader sections (measuring the `simulate` p50 delta on b003, in-session
variance ~1%) attributes the 0.418 ms baseline:

| Component | Cost | Share | Lever |
| --- | ---: | ---: | --- |
| Reconstruction + memory (particle write, emitter read, spawn/age recompute per slot) | ~0.282 ms | **67%** | inherent to the analytical model |
| Force / shape / gradient / curve / quaternion compute | ~0.116 ms | 28% | micro-opt (broad, no single hotspot) |
| Atomic bookkeeping (§2.6) | ~0.020 ms | 5% | not worth it |

Findings that redirect the roadmap:
- **Turbulence (3 `sin` + 3 hashes) is ~0 cost** — transcendentals run on GPU SFUs;
  chasing them is pointless.
- **§2.6 (atomic contention) is a non-issue** at ~5% — do not remove the dead list
  / atomics for performance.
- **67% is inherent per-slot reconstruction + memory**, which no micro-optimization
  touches — every slot recomputes its particle from scratch each frame. The ceiling
  for micro-optimizing the analytical kernel is therefore only ~33% (28% compute +
  5% atomics), and most of that is spread thinly.

**Conclusion:** the analytical GPU kernel is already near its practical floor for
dense effects. Substantial further gains require the **incremental backend
(strategy M7 / §3)** — persistent particles updated in place instead of full
per-frame reconstruction — not more kernel micro-opts. That is now a measurement-
backed decision, not a hunch.

Explicitly **not** pursued (measurement did not justify): dead-slot dispatch
elimination and packed slot ranges for capacity (§2.2 refuted); dead-list /
atomics (§2.6, measured ~5%); transcendental force math (turbulence ~0).

**Exit criteria:** each landed optimization cites the scenario and before/after
delta (re-run its `--gpu-bench` capture) that justified it.

---

## Phase 8 — Performance CI (strategy §14)

- `.github/workflows/performance.yml`, two lanes:
  - **PR CPU lane** on hosted runners: run deterministic microbenchmarks, upload
    artifacts, compare to baseline, report (advisory first; later fail on stable
    >5–10% CPU regression).
  - **Native GPU lane** on the existing self-hosted `gpu` runner (reuse the weekly
    GPU CI infra): record GPU/driver/OS/backend/commit/Bevy-wgpu version, compare
    to accepted `main` baseline; advisory until GPU-clock variance is characterized,
    then threshold ~10–15%.
- Baseline update workflow: accepting a new `main` baseline is an explicit,
  reviewed step.

**Exit criteria:** a PR shows a benchmark comparison comment; a deliberate
regression is flagged.

---

## Phase 9 — Editor performance feedback (strategy M9)

- Surface trustworthy metrics in `apps/aestra-editor/src/profiler.rs`: capacity,
  alive, occupancy, CPU cost, GPU sim/render cost, VRAM, draws, dispatches.
- Warnings: low occupancy, high loop pressure, too many unique materials, high
  emitter count, excessive overdraw, param updates forcing repacks (strategy §9 list).
- Reuse `ProfileValue` so the UI never shows an estimate as measured (strategy §13).

---

## Phase 10 — Longer-horizon, gated on measurement

Kept deliberately light; each is a decision gated by earlier phases, not committed work.

> **Prior art.** How professional VFX engines (bevy_hanabi, Niagara, Unity VFX
> Graph, Wicked Engine) solve this — and which of their techniques transfer to
> Aestra's analytical model — is surveyed in
> [`aestra_gpu_architecture_comparison.md`](aestra_gpu_architecture_comparison.md).
> Its measured conclusion: the seductive **analytically-sized single dispatch**
> (size the simulate dispatch to the CPU-computed alive count) is **not** worth
> building — b004's `simulate` is already at the GPU timer floor (p50 0.001 ms,
> ~40× below its own render pass), so §2.2 extends to the sparse case; and the
> packed slot layout means a single global dispatch saves <2% for multi-emitter
> effects anyway. What survives the survey: attack the 67% memory floor with
> **SoA/FP16 attribute packing**, and reach for **per-emitter dispatch (Phase 7 #3)**
> only when the §2.3 emitter search (b006 = 0.965 ms) actually hurts — it subsumes
> the sparse-dispatch idea. M7 remains the big lever, gated on a workload the
> analytical kernel provably cannot serve.

- **Analytically-sized single dispatch — investigated, not pursued.** Sizing the one
  `simulate` dispatch to the CPU-known alive count (reusing Phase 7 #2 inversion)
  looked like a cheap, determinism-preserving way to capture the professional
  engines' indirect-dispatch win. Grounding it in the archived baseline refuted it:
  b004 `simulate` p50 = 0.001 ms (one timer tick; the dead-slot early-return is
  already free on the RTX 4070 SUPER), and the packed slot layout limits a single
  global high-water dispatch to <2% savings at 64 emitters. Superseded by per-emitter
  dispatch below. See the comparison doc §4.
- **SoA + FP16 particle attributes (kernel win):** structure-of-arrays output and
  half-precision on tolerant attributes attack the measured 67% memory floor
  (Phase 7) without changing the analytical model. Bounded and measurable.
- **Incremental gameplay backend (M7):** prototype persistent particles + alive/dead
  lists + spawn/update/compaction only *if* Phase 6 shows the analytical kernel is
  inadequate for sparse/long-lived/high-instance workloads. Analytical execution is
  preserved for editor seeking/scrubbing/reference/tests. Both backends share the
  semantic effect IR.
- **Batch effect execution (M8):** global particle arena / effect+emitter tables /
  batched compute / multi-draw — only if Phase 5 instance-count benchmarks show
  submission overhead dominates.
- **AAA performance gates (M10):** hardware tiers + permanent baselines once the
  above stabilize.

---

## Budgets (strategy §12, for reference)

For a 60 Hz frame (16.67 ms): Aestra CPU ~1.0 ms typical / ~2.0 ms ceiling; Aestra
GPU ~2.0 ms typical / ~4.0 ms ceiling. Engineering targets, not guarantees.

---

## Open decisions (resolve during Phase 1)

1. **GPU timing source:** Bevy 0.19 built-in render diagnostics vs. custom
   `TIMESTAMP_QUERY` passes — confirm coverage of Aestra compute nodes first.
2. **Bench harness reuse:** how much of `aestra-viewer`'s capture path to factor
   into a shared crate vs. copy.
3. **Allocation profiling backend:** Tracy memory tracking vs. a custom global
   allocator shim for the CPU lane.
4. **Scenario authoring:** hand-written `.ron` vs. programmatic scenario generation
   for the large matrix (§9 has ~12 axes; full cartesian is impractical — pick a
   representative fractional set).
5. **`GpuEffectPrototype`/`GpuEffectInstance` split (Phase 3):** whether the split
   lives in `aestra-gpu` (artifact) or `aestra-bevy-render` (presentation). Leaning
   `aestra-gpu` so it stays engine-neutral.

---

## Immediate next actions (first PR)

1. Scaffold `apps/aestra-bench` + add to workspace members.
2. Author scenarios B001–B003 first (empty, single-small, single-dense).
3. Add whole-frame + `artifact_update` stage timing to the bench harness via
   `profile.rs` extension and spans in `update_gpu_inputs`.
4. Emit the §15 JSON with median/p95/p99 for those three scenarios.
5. Land it, then capture the first archived baseline (start of Phase 2).

This first PR is intentionally scoped to prove the measurement loop end-to-end on
three scenarios before authoring the full matrix.
