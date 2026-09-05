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
trails/ribbons stop overloading the particle struct. **That storage work (Steps 1–2) is
now done** (see §5 status). A stateful/incremental *sim* backend is a later layer on
that storage — **not** justified by the sweep for dense throughput, but **required for
collisions and fluids**, which the analytical kernel provably cannot express at any
speed. That backend is specified in **§6a**.

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

### Non-goals for *this* (storage) work — grounded in measurement
- **Not** a stateful/incremental sim backend *for dense throughput* — the sweep showed
  the analytical kernel already does 4M in ~2.2 ms, so update-in-place is not worth
  building **to go faster**. (It *is* worth building for capability — see §6a; that is a
  separate, expressiveness-driven decision, not a perf one.)
- **Not** FP16 on trail-timing fields — `rotation`-as-timestamp and bitcast ring state
  are precision/bit critical.
- **Not** overdraw/fill work — the fill ablation showed it is nearly free on capable
  GPUs at ≤1M.

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

> **Status (2026-09-05): Steps 1 and 2 are DONE and measured.** The untangling shipped
> as a shared `aux` buffer rather than separate per-record buffers (the WebGPU
> 8-storage-buffer baseline forced one shared slot): ribbon links `8ad3ff5`, trail ring
> state `d3b657c`. Compaction shipped as `6ee4b3a` — `emitter_index`+`alive` packed into
> one word, padding dropped, **64 → 48 B (−25%)**, measured **simulate −12–16%** at 1M/4M
> and ~2× the storage ceiling. What remains open is Step 3, now reframed in **§6a**.

1. **Extract trail/ribbon records first** (no perf goal): move trail ring state and
   ribbon links out of `GpuParticle`'s reused fields into dedicated buffers. Rewrite
   `trail_history` / `ribbon_link` / trail+ribbon vertex shaders against the new
   records. Verify: trail + ribbon conformance, the (bit-exact) prism/ember/plasma
   visual references, plus the **ribbon/trail visual references** — now established
   (`apps/aestra-viewer/tests/references/{ribbon_lab,trail_lab}`, wired into
   `gpu-visual.yml`) to close the coverage gap noted in the vertex-strip work; refactor
   against these. This step is pure untangling — behavior-preserving, no ABI shrink yet.
   (Caveat: both effects render a small element, so the pixel net catches gross breakage
   — vanished/inverted geometry — more sharply than subtle sub-pixel drift; the geometry
   conformance tests remain the fine-grained check.)
2. **Compact / SoA the live-particle core** (the render+sim bandwidth win): now that
   the core is free of overloading, pack it and split hot render fields. Measure
   against `benchmarks/gpu-baselines/vertex-strip-441422a/` and the `scale_*` sweep at
   1M **and** 4M — 4M is where render turned bandwidth-bound, so it is the scenario
   that should move most.
3. **Incremental / stateful playback backend** — no longer "optional, gated on a
   hypothetical": it is **required** for the collisions and fluids on the roadmap, which
   the analytical model provably cannot express. Fully specified in **§6a** below.

---

## 5a. Step 1 implementation design (turnkey, from the current code)

Mapped against the shaders as they stand, so the extraction is executable without
re-derivation. **Safety net is already in place** (`ribbon_lab`/`trail_lab` visual
references + conformance). Do the two halves in order; neither shrinks `GpuParticle`
(trails still need `_padding` until their half lands) — the size/perf win is Step 2,
after both halves free the struct.

**Data-flow facts that shape the design:**
- `aestra_ribbon_link.wesl` runs inside the **simulation** shader module (it uses the
  sim's `group(0)` `particles`/`emitters`/`alive_indices`/`indirect`) and writes the
  link triple into `particles[slot]._padding_0/1/2` (next / prev / uv-param).
- `aestra_ribbon_vertex.wesl` (in the **sprite_render** `group(1)`) reads those three.
- `aestra_trail_history.wesl` (sim `group(0)`) owns the trail ring via `_padding_0/1/2`
  (head / count / tick), `alive` tri-state, `rotation`-as-timestamp, `particle_index`
  as owner id — and writes **trail samples into `particles[owner+1+head]` slots**.
- `aestra_trail_vertex.wesl` (`group(1)`) reads that ring.

> **Hard constraint found while starting the implementation (2026-09-05).** The sim
> `group(0)` already binds **7 storage buffers** (emitters, particles, alive, dead,
> counters, indirect, globals). The **WebGPU baseline `maxStorageBuffersPerShaderStage`
> is 8** — one free slot. A *separate* `ribbon_links` buffer plus a *separate* trail
> buffer would need **two** → **9 → over the WebGPU baseline**, breaking portable
> backends and Half B. **So ribbons and trails must share ONE `aux` buffer**, and the
> two halves migrate **together**, not ribbon-first. Also: removing `_padding` alone
> does **not** shrink `GpuParticle` — encase rounds 52 B back to 64 B (16-byte align) —
> so there is still **no size/perf win until Step 2 compaction** repacks the real
> fields. This revises the ribbon-first/separate-buffers sketch below.

**Half A+B together — one shared `aux` buffer (revised design):**
1. Add a single `aux: array<u32>` (3 words/slot, sized to `storage_records`) — the last
   storage-buffer slot under the WebGPU-8 baseline. Any slot is *either* a ribbon
   particle *or* a trail head, so both reuse the same 3-word layout: ribbons store
   `[next, prev, uv]`, trail heads store `[ring_head, count, tick]`.
2. Bind `aux` into **both** groups: sim `group(0) @binding(7)` (read_write, used by
   `link_ribbons` and `update_trails`); render `group(1) @binding(7)` (read, used by
   `ribbon_vertex`/`trail_vertex`). Update both `BindGroupLayoutDescriptor`s and both
   bind-group *creations* (`gpu.rs` sim group; `render.rs` `effect_layout` 7→8 entries)
   — with a 1-element **dummy** `aux` for effects that have neither ribbons nor trails,
   so 4M sprite effects don't pay 12 B/slot.
3. `ribbon_link.wesl` + `trail_history.wesl`: write `aux[slot*3 + k]` instead of
   `particles[...]._padding_k`. **Trails also reuse `alive` (tri-state), `rotation`
   (timestamp), `particle_index` (owner) — those stay on the particle record for now;
   only the `_padding` triple moves to `aux`.** (Fully separating trail state is a
   larger Half-B++ that can follow.)
4. `ribbon_vertex.wesl` + `trail_vertex.wesl`: read `aux` instead of `_padding`.
5. **Capability floor:** both groups reach 8 bindings and the compute stage reaches
   **8 storage buffers = the WebGPU baseline**. Bump the `< 7` binding/storage guards to
   `< 8` in `detect_gpu_capabilities`; document in
   `aestra_gpu_architecture_portability.md` that the compute stage now sits exactly at
   the WebGPU storage-buffer floor (no room for a 9th — future additions must reuse
   `aux` or pack into existing buffers).
6. Regenerate `simulation.wgsl` + `sprite_render.wgsl` snapshots; drop `_padding_0/1/2`
   from `GpuParticle` (CPU + all shader `Particle` structs) — stride stays 64 B.
7. **Test harnesses:** `ribbon_conformance.rs` (and any trail probe) seed link/ring data
   through `_padding`; rebind them to an `aux` buffer. Verify: ribbon + trail
   conformance **and** the `ribbon_lab`/`trail_lab` visual references (RMSE 0).

**Then Step 2 (compaction):** repack the real live fields (position/size/rotation/
color/age — and, if trail state is fully separated, `alive`/`rotation`/`particle_index`)
for the render-gather + sim-bandwidth win and the ~2× ceiling. **This is the first step
that actually banks a size/perf benefit** — schedule it with the aux migration, never
ship the migration alone.

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

## 6a. Incremental / stateful backend — required for collisions and fluids

**This is no longer a hypothetical, perf-gated option.** The roadmap needs collisions
and fluids for complex/realistic effects, and those provably **cannot** be expressed by
the stateless/analytical kernel. This section reframes Step 3 from "maybe someday" into
a staged capability roadmap. The trigger is **expressiveness, not speed** — the sweep
already showed analytical is fast enough at dense scale; this backend exists to do
things analytical *cannot do at any speed*.

### Why analytical cannot do it (the hard boundary)
Analytical motion is a **closed-form function of time**: `position = f(age)`, recomputed
each frame. It holds only while forces depend on time/age alone (constant gravity,
linear drag, the age-parameterized turbulence Aestra ships). The moment a force depends
on **position, the scene, or other particles**, the motion becomes a feedback ODE
(`dx/dt = f(x, neighbours, scene)`) with no closed form — it *must* be integrated
step-by-step with persistent state. Both roadmap items cross that boundary:

- **Collisions.** *Static analytic colliders + a few bounces* (ground plane, sphere,
  simple SDF) can be chained as closed-form segments and stay analytical. **General
  collision** — scene meshes, the depth buffer, moving geometry, many bounces — cannot.
- **Fluids.** *True* fluids (SPH / grid Navier–Stokes / FLIP) are irreducibly stateful:
  a particle's velocity depends on its neighbours' positions **this frame**. And the
  common *fake-fluid* shortcut — **flow-field / curl-noise advection** — is **also not
  analytical**, because velocity is a function of *position* (`v = curl(x)`), so
  advecting it is a position-feedback ODE that needs numerical stepping. (Aestra's
  turbulence escapes this only by being keyed on age, not position — it wiggles but
  cannot follow a vortex or flow around geometry.)

### Architecture — a second backend, not a replacement
- **Per-emitter backend choice.** Most VFX (sparks, magic, explosions, embers, trails)
  stay analytical — cheaper, seekable, and the editor depends on it. Only emitters that
  need collision/flow/fluid opt into stateful. Both backends share the semantic effect
  IR; analytical stays the default and the **authoring/reference/seeking** backend.
- **Seeking is what stateful gives up.** A stateful effect cannot scrub backward. The
  editor answer is bounded **history record/replay** — exactly what the trail system
  already does — so plan a replay buffer for stateful emitters from the start.
- **Determinism.** Fixed-timestep integration + seeded spawn keeps runs reproducible
  enough for regression; expect looser tolerances than the analytical bit-exact path.

### Staged roadmap (each stage ships a capability)
1. **Stateful particle backend** *(the smaller lift, biggest capability jump)*.
   Persistent per-particle state (position, **velocity**, age) + GPU-driven spawn from a
   dead/alive free list + fixed-step Euler/Verlet integration + compaction. Unlocks
   **flow-field / curl-noise advection** (real swirly "fluid-look") and **collision
   against SDF / simple colliders and the depth buffer**. This alone covers a large
   slice of "complex and realistic". Note the stateful record must **store velocity**
   (analytical recomputes it), so it is larger than the 48 B analytical core — its
   memory bill partly offsets the "skip reconstruction" compute saving, which is the
   measured reason this was never worth building for *speed*.
2. **Fluid simulation domain** *(the heavy milestone)*. Grids (2D/3D) and/or neighbour
   structures with a pressure/velocity solve feeding particle motion — the Niagara
   "Simulation Stages + Grid2D/3D" shape. Only when true CFD-grade smoke/water/splash is
   required; the flow-field look from stage 1 defers this a long way.
3. **AAA polish** — sorting for correct blending, LOD/culling, and the >4M dispatch
   ceiling (multi-/2-D dispatch) become relevant once these dense stateful sims exist.

### What stays true regardless
Analytical remains the editor/authoring/reference/seeking backend and the default for
everything it can express (which, post-compaction, it does efficiently to millions of
particles). The stateful backend is *additive*, opt-in per emitter, and justified by
capability — collisions and fluids — not by the per-frame throughput numbers, which
already favour keeping analytical wherever it suffices.

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
