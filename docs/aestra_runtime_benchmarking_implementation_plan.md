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

### 1d. GPU timestamps — **pending (needs GPU lane)**
- Prefer Bevy's built-in render diagnostics (`RenderDiagnosticsPlugin` +
  `RecordDiagnostics::time_span`) over a hand-rolled `TIMESTAMP_QUERY` pass.
- Integration point identified: the `run_simulation` compute pass
  (`crates/aestra-bevy-render/src/gpu.rs`, `begin_compute_pass("aestra simulation")`)
  and the sprite draw in `gpu/render.rs`. Wrap these with GPU `time_span`s.
- Deferred deliberately: GPU timestamps can only be verified on the self-hosted GPU
  lane, so this is implemented and validated there rather than blind in a headless
  environment. Record GPU sim-reset / sim / render timings into the bench JSON once
  available (strategy §7, §10 GPU block).

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

---

## Phase 7 — Optimize measured analytical bottlenecks (strategy M6)

Only address what Phase 6 flagged. Candidate levers (each tied to a measured number):
eliminate linear emitter search (§2.3), drop unused dead-list bookkeeping (§2.6),
reduce global atomics, simplify spawn-time reconstruction, precompute curve lookup
data (§2.5), reorganize emitter slot ranges, specialize simple emitters, reduce
branch divergence.

**Exit criteria:** each landed optimization cites the scenario and before/after
delta that justified it.

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
