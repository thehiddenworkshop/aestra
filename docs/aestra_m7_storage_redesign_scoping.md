# Aestra M7 — Particle Storage Redesign (Scoping)

A design-scoping document, not committed work. It reframes the strategy's **M7
("incremental gameplay backend")** around what the runtime benchmarking effort
actually measured, and defines the storage redesign that unblocks the remaining GPU
optimizations.

Companions:
[`aestra_runtime_benchmarking_implementation_plan.md`](aestra_runtime_benchmarking_implementation_plan.md)
(Phase 6 AAA-scale sweep, Phase 7 floor) ·
[`aestra_gpu_architecture_comparison.md`](aestra_gpu_architecture_comparison.md)
(professional-engine survey, §5 roadmap) ·
[`aestra_runtime_benchmarking_profiling_plan.md`](aestra_runtime_benchmarking_profiling_plan.md)
(strategy M7 / §3).

> **Status:** scoping only. No code. Ends with phased options and open decisions for
> a dedicated implementation session.

---

## 1. Why this document reframes M7

Strategy M7 was written as an **incremental/stateful simulation backend** (persistent
particles, dead/alive lists, update-in-place) whose motivation was *throughput past
the analytical floor*. This session's measurements changed that premise:

| Measured finding (this session) | Consequence for M7 |
| --- | --- |
| Analytical sim scales to **4M dense in ~2.2 ms** (no throughput wall on a 4070); marginal cost 1M→4M ≈ 0.05 ns/particle | The incremental **sim** is **not** needed for dense throughput on capable GPUs. Its remaining sim motivation is weaker GPUs + long-lived/persistent-state semantics. |
| **Render dominates** (~2.4–3× sim); it is **vertex/particle-fetch bound**, not overdraw | The near-term win is on the **read side** of the particle buffer (SoA/compact), not the sim. |
| The SoA/compact record — the roadmap's "#1 lever" — is **blocked**: `GpuParticle` is an overloaded union the trail/ribbon system depends on | The blocker *is* a storage-model problem. Fixing it is the real, measured driver for M7. |

**Reframed M7 = a storage redesign** whose primary, measured goal is to **untangle
the shared particle buffer** so that (a) SoA/compact packing becomes possible and (b)
trails/ribbons stop overloading the particle struct. A stateful/incremental *sim*
backend becomes an **optional** later layer on that storage, gated on a workload the
analytical kernel provably cannot serve — which the sweep did **not** find at dense
scale.

---

## 2. Current storage model — the overloaded union

One `GpuParticle` record (64 B) serves every purpose, distinguished by context. From
the shaders (`crates/aestra-gpu/src/shaders/`):

| Field | Sprite meaning | Trail/ribbon reuse |
| --- | --- | --- |
| `color: vec4` | tint | trail sample color |
| `position, size` | billboard center/extent | trail sample position/size |
| `rotation: f32` | sprite spin | **timestamp** of a trail sample (precision-critical) |
| `normalized_age` | curve lookup | (age) |
| `emitter_index` | ownership check | — |
| `alive: u32` | 0/1 | **tri-state** 0/1/2 (trail eviction states) |
| `particle_index: u32` | flipbook identity | trail **owner id / epoch** |
| `_padding_0/1/2: u32×3` | *unused* | ring head, count/evictions, tick — **or** ribbon prev/next/uv links |

Additional coupling:
- **Trail history samples are stored in Particle-sized slots** adjacent to a head
  record (`particles[owner + 1 + head._padding_0] = sample` in
  `aestra_trail_history.wesl`). The particle buffer physically holds both live
  particles and trail history.
- `GpuEffectDynamics.storage_records` already distinguishes live capacity from trail
  storage, so the record count is understood; the *record layout* is what is fused.
- `GpuParticle` is documented as a **"Stable storage/readback ABI shared with the GPU
  simulation shader"** (`crates/aestra-gpu/src/lib.rs`), consumed by the CPU-readback
  backend and the conformance tests.

**Implication.** You cannot compact, FP16, or SoA-split the particle record without
first giving trails/ribbons **their own record types and buffers**. That separation is
the core of M7.

---

## 3. Goals and non-goals

### Goals (measured priority order)
1. **Separate storage records.** A live-particle record, a trail-history record, and a
   ribbon-link record become distinct types in distinct buffers — no more field
   overloading.
2. **Compact / SoA the live-particle read path.** Once separated, pack the render-read
   fields (position, size, rotation, color, age) tightly (or as SoA arrays) to cut the
   scattered vertex-shader gathers that bound the render pass, and the sim write
   bandwidth that bounds the ~2 ms plateau. *This is the payoff the SoA blocker points
   at.*
3. **Preserve the analytical backend unchanged** as the authoring/reference path
   (deterministic seeking, bit-exact snapshots, CPU conformance). Non-negotiable.

### Non-goals (explicitly, grounded in measurement)
- **Not** a stateful/incremental *sim* backend for dense throughput — the sweep showed
  the analytical kernel already does 4M in ~2.2 ms. Do not build dead/alive-list
  update-in-place *to go faster at dense scale*; that motivation was refuted.
- **Not** FP16 on trail-timing fields — `rotation`-as-timestamp and bitcast ring state
  are precision/bit critical.
- **Not** overdraw/fill work — the fill ablation showed it is nearly free on capable
  GPUs at ≤1M.

### Optional, later, gated
- A stateful incremental backend as a **second playback backend** for **weaker GPUs**
  or **long-lived/persistent-state** effects the analytical model cannot express. Gate
  on a concrete such workload; share the semantic IR; keep analytical for authoring.

---

## 4. Proposed architecture (sketch)

- **`GpuParticleCore`** — the compact live record the render path reads: `position`,
  `size`, `rotation`, `color`, `normalized_age`, `emitter_index`, `alive` (0/1), and a
  stable `particle_index`. Target ~32 B (from 64 B). Layout chosen for coalesced
  vertex reads (candidate: SoA arrays for the hot render fields; AoS for the rest).
- **`GpuTrailRecord`** — trail head + sample records in their **own** buffer: explicit
  `ring_head`, `count`, `tick`, `owner_id`, `state`, plus the sampled transform. No
  reinterpretation of core fields.
- **`GpuRibbonLink`** — `prev`, `next`, `uv` in their own small buffer.
- **Bindings.** The sim compute pass and the render pipeline gain separate bindings for
  core vs trail vs ribbon buffers (bind-group layout change; watch the
  `max_storage_buffers_per_shader_stage ≥ 7` limit already checked in `gpu.rs`).
- **Readback ABI.** Define the new core record as the readback contract; update the
  CPU-readback backend and conformance expectations deliberately (versioned).
- **Ceiling.** Halving the live record (~64→32 B) also roughly **doubles** the
  storage-bound `max_particles` (it divides `binding_size / sizeof`), independent of the
  ~4.19M dispatch ceiling (which still needs multi-/2-D dispatch to exceed).

---

## 5. Phasing (each step lands green on its own)

1. **Extract trail/ribbon records first** (no perf goal): move trail ring state and
   ribbon links out of `GpuParticle`'s reused fields into dedicated buffers. Rewrite
   `trail_history` / `ribbon_link` / trail+ribbon vertex shaders against the new
   records. Verify: trail + ribbon conformance, the (bit-exact) prism/ember/plasma
   visual references, plus a **new ribbon/trail visual reference** to close the
   coverage gap noted in the vertex-strip work. This step is pure untangling —
   behavior-preserving, no ABI shrink yet.
2. **Compact / SoA the live-particle core** (the render+sim bandwidth win): now that
   the core is free of overloading, pack it and split hot render fields. Measure
   against `benchmarks/gpu-baselines/vertex-strip-441422a/` and the `scale_*` sweep at
   1M **and** 4M — 4M is where render turned bandwidth-bound, so it is the scenario
   that should move most.
3. **(Optional, gated) incremental playback backend** — only if a real weaker-GPU or
   persistent-state workload demands it. Prototype dead/alive lists + update-in-place
   as a *second* backend; keep analytical for authoring.

---

## 6. Risks and verification

- **Freshly-landed trail/ribbon code** is intricate (tri-state `alive`, ring buffers,
  epoch resets). Step 1 must be behavior-preserving and lean on conformance + a new
  ribbon/trail visual reference (currently ribbons/trails have **no** pixel-level
  regression — only the geometry conformance test).
- **Readback/ABI break** is deliberate and versioned; conformance tolerances re-derived
  once, as was done for the 12→8 curve-inversion change.
- **Bind-group/storage-buffer limits** — separating buffers adds bindings; verify
  against the portability floor (`aestra_gpu_architecture_portability.md`).
- **Determinism** — the analytical backend's bit-exact seeking and snapshots must be
  untouched; treat any analytical-path change as a regression.

---

## 7. Open decisions (resolve at implementation kickoff)

1. **SoA vs compact-AoS for the core.** SoA maximizes coalesced render reads but
   multiplies bindings and complicates the sim write; compact-AoS (~32 B) is simpler
   and still halves traffic. Prototype both against the 4M render number before
   committing.
2. **Trail storage location.** Keep trail samples in a parallel buffer indexed like
   today, or a separate arena? Affects `storage_records` accounting.
3. **Readback contract scope.** Does readback need trail/ribbon records, or only the
   live core? (Conformance currently reads the full particle.)
4. **Whether to build the incremental sim at all** — defer until a workload the
   analytical kernel cannot serve is identified; nothing measured this session
   requires it.

---

## 8. What this unlocks

- The **SoA/compact render win** (roadmap #1) that is currently blocked — the measured
  lever for the render pass at ≥1M and the sim bandwidth plateau.
- A clean home for **trails/ribbons** instead of overloading the particle struct.
- A roughly **2× storage-bound particle ceiling** from the smaller core record.
- An optional, well-isolated path to a **stateful backend** for weaker hardware —
  without ever compromising the analytical authoring model.

The measured through-line of the whole effort: the analytical kernel is faster at
scale than feared, the real costs are **memory-traffic and storage-model** problems,
and M7 — recast as a **storage redesign** — is where those are actually fixed.
