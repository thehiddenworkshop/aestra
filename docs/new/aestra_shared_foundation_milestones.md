# Aestra — Shared Foundation Milestones (S0–S1)

**Status:** Binding shared front end for two architecture plans
**Belongs to:** neither plan exclusively — both depend on it
**Supersedes:** the separate `M0`/`M1` milestones in both plans below

---

## Why this document exists

Two architecture plans evolve Aestra in parallel:

- `AESTRA_EXTENSIBLE_STAGES_PLUGINS_PROPERTIES.md` — generic stages, plugins, schema-driven
  authoring, portable lowering.
- `aestra_hybrid_simulation_architecture_roadmap.md` — analytic / stateful / staged simulation
  classes, compiler-derived islands, GPU checkpoints.

Both open with the *same two milestones*: a frozen baseline, then a semantics/identity pass that
extends `ModuleMetadata`. Run independently they redo the baseline, extend `ModuleMetadata` twice in
incompatible ways, and each risk bumping the same formats. **This document is the single, merged
front end.** Both plans start here and only diverge afterward.

The recommended lead after this foundation is the **hybrid track** (stateful + collision) for
near-term feature value at low structural risk; the **extensible track** (generic stages + plugin
boundary) is picked up when staged simulation / fluids force it. See the extensible plan §44 for the
full reconciliation. This document is agnostic to that choice — S0 and S1 are required either way.

---

## Shared decisions locked here (before either track diverges)

These five primitives are shared by both plans (extensible §44). Two are **designed** in this
foundation; three are **reserved** here and first implemented on whichever track reaches them first,
against the shape agreed here.

| Shared primitive | Status in this foundation |
|---|---|
| `ModuleMetadata` requirement extension (capabilities + simulation requirements) | **Designed & landed in S1** — one extension, not two |
| Backend-capability vocabulary (what a unit needs / a backend offers) | **Shape agreed in S1**, first implemented per track |
| Portable execution IR (`ExecutionBlock`/`ComputeOp` ≡ `SimulationPassPlan`) | **Reserved** — designed once when first needed (extensible §44.1) |
| Resource model (`ResourceDescriptor` ≡ `SimulationResource`) | **Reserved** — one model, bounded-allocation rule (extensible §11.1) |
| Compiled-artifact version (v2 → v3 → …) | **Reserved** — regenerated, so bumps are cheap; coordinate where practical but do not block one track to co-bump (extensible §44.5) |

Non-negotiables established here:

- `SimulationDomain` (topology, authored) ≠ `SimulationClass` (temporal, **derived**) ≠ `StageKind`
  ≠ `SimulationSeekMode` (resolved strategy). Nothing added in S0/S1 conflates them.
- No authored-format change in this foundation. The extensible plan's authored v3 → v4 happens later,
  on its own track; the hybrid plan adds no authored fields at all.
- Existing effects must compile to byte/behaviour-identical output through the entire foundation.

---

## Milestone S0 — Freeze the baseline (correctness + performance)

**Goal:** A reproducible baseline covering *both* correctness and performance before any execution
model or identity change, so every later refactor can prove it changed nothing it shouldn't.

This is the union of both plans' M0: correctness fixtures (extensible) and performance baselines
(hybrid).

### Work — correctness fixtures

- representative v3 semantic fixtures (real showcase effects, not toys);
- compile snapshots for the built-in emitter lifecycle stages;
- artifact round-trip fixtures (`aestra-artifact` v2);
- deterministic CPU evaluation fixtures;
- native-GPU conformance fixtures (CPU-vs-GPU at canonical frames);
- source-map assertions against the current `RuntimeStage`;
- renderer-plan fixtures;
- Properties selection/action tests around modules and renderers.

### Work — performance baselines

- keep the existing `aestra-bench` harness (do not build a new one);
- capture fresh CPU and native-GPU baselines from current `main`;
- at minimum cover `B002` (small/high occupancy), `B003` (dense), `B004` (sparse large capacity),
  `B005` (many instances);
- add scenarios stressing many emitters, short loops with long lifetimes, curve-driven emission;
- record per run: CPU evaluation, GPU simulation time, capacity, alive count, occupancy, workgroups,
  GPU buffer bytes;
- store results with commit / hardware / backend metadata.

### Do not change yet

Particle semantics, the 48-byte GPU presentation ABI, execution class, authored format, registry.

### Acceptance

- one reproducible baseline exists for the exact pre-change implementation;
- both **sparse and dense** analytic workloads are represented (`B004` is load-bearing);
- correctness fixtures cover CPU eval, GPU conformance, artifact round-trip, source maps, renderer
  plans, Properties actions;
- no architecture changes have landed.

---

## Milestone S1 — Identity, semantics, and the one `ModuleMetadata` extension

**Goal:** Introduce every vocabulary and registry both plans need, and extend `ModuleMetadata`
**exactly once**, while every existing effect still compiles unchanged as analytic.

### Work — extensible identity & registry

Introduce or normalize the namespaced string identities:

```text
PluginId
StageTypeId
DomainTypeId
ResourceTypeId
CapabilityId
```

Keep the existing `ModuleTypeId` and `RendererTypeId`.

Add a unified in-process `ExtensionRegistry`, and route **built-in** module discovery through it
(built-ins register like any extension — no privileged path). Add registry diagnostics for duplicate
IDs, provider conflicts, and invalid descriptor definitions.

### Work — hybrid simulation semantics (derived, not authored)

Introduce the derived compiled vocabulary:

```rust
pub enum SimulationClass    { Analytic, Stateful, Staged }
pub enum TemporalSemantics  { Direct, HistoryDependent }
```

Keep the existing `SimulationSeekMode` as the *resolved runtime strategy* only — stop treating it as
the sole compiled semantic description (the compiler still emits `StatelessDirect` for everything
until the hybrid track's island milestone). Do **not** yet remove the hardcoded
`seek_mode: SimulationSeekMode::StatelessDirect` in the compiler; that removal belongs to the hybrid
island milestone.

### Work — the single `ModuleMetadata` requirement extension (the critical merge)

The extensible plan's module **capabilities / reads / writes / multiplicity** and the hybrid plan's
**simulation requirements** are the same extension of `ModuleMetadata`, designed together here so the
compiler has one requirement system, not two. A built-in module declares, in one place:

```text
capabilities required / provided        (compatibility — extensible §9)
reads / writes                          (resource & hazard analysis)
multiplicity                            (singleton-per-stage vs multiple)
temporal        : Direct | PreviousState
synchronization : None | OrderedPass | Iterative
neighborhood    : None | Particles | Grid
```

The compiler derives `SimulationClass` from the temporal/synchronization/neighborhood fields
(`PreviousState → Stateful`; `Iterative`/`Grid`/neighborhood → Staged) — the derived class is never
authored and never a capability. All current built-in modules declare `temporal = Direct`,
`synchronization = None`, `neighborhood = None`, i.e. Analytic.

### Work — backend-capability shape (agreed, not yet implemented)

Agree one backend-capability model that a descriptor declares and a backend answers, covering both
`cpu_reference` / `gpu_compute` (extensible `BackendSupport`) and `checkpoints` / `staged_dispatch`
(hybrid `BackendSimulationCapabilities`). Only the *shape* is fixed in S1; implementations land per
track.

### Clarify (documented, tested)

```text
SimulationDomain != SimulationClass
StageKind        != SimulationClass
SimulationSeekMode != SimulationClass
capability        != execution order   (extensible §34)
```

### Keep operational

Existing compiler/runtime/GPU behaviour is unchanged. No authored-format change. No GPU runtime
change.

### Acceptance

- all existing effects compile to byte/behaviour-identical output;
- all current CPU/GPU conformance tests pass unchanged;
- built-in modules are registered through the unified `ExtensionRegistry`;
- a test can register a fake third-party module **and** a fake stateful module, and:
  - the fake third-party module appears via the registry with no editor regression;
  - the fake stateful module's `ModuleMetadata` causes the compiler to *report* it as Stateful
    (even though no backend executes it yet);
- the compiler can explain, per emitter, why it is Analytic and which requirement would promote it;
- `ModuleMetadata` was extended once — there is no second, parallel requirement system.

---

## After S1 — where the tracks diverge

```text
S0  baseline (correctness + performance)
        │
S1  identity + derived semantics + one ModuleMetadata extension
        │
        ├─────────────────────────────┐
        ▼                             ▼
  HYBRID track                  EXTENSIBLE track
  (recommended lead)            (when staged/fluids force it,
  compiler islands →             or when Properties pain justifies
  stateful CPU/GPU →             pulling its Phase 9a shell forward)
  checkpoints → collision        generic StageInstance + authored v4 →
        │                        property schema → execution IR →
        │                        GPU generic scheduler → renderers →
        └──────────────┬─────────  Properties redesign → plugin proof
                       ▼
      Converge at STAGED simulation:
      one execution IR (§44.1), one resource model,
      compiled-artifact bumps coordinated where practical (§44.5),
      fluids as a GPU-only plugin (extensible §44.6).
```

Both tracks consume S1's `ModuleMetadata` extension and the agreed backend-capability shape. Neither
may re-extend `ModuleMetadata` or bump a format without honoring the shared-primitive contract in the
extensible plan's §44.

---

## Unified milestone sequence (one ordered list)

The full interleaving of both plans after S0/S1, ordered for the **recommended hybrid-first lead**:
feature value early (stateful particles, collision) at low structural risk, then the extensible
structural work that unlocks the plugin boundary, converging at staged simulation / fluids. Each
unified step cites its source milestone(s): `H-Mn` = hybrid roadmap, `E-Mn` = extensible plan.

### Phase A — Shared foundation

| # | Step | Source | Delivers |
|---|---|---|---|
| U0 | Freeze baseline (correctness + performance) | S0 | reproducible pre-change baseline |
| U1 | Identity + derived semantics + one `ModuleMetadata` extension | S1 | registry, IDs, requirement metadata |

### Phase B — Stateful & collision (hybrid lead; no plugin refactor needed)

| # | Step | Source | Delivers |
|---|---|---|---|
| U2 | Compiler simulation islands (remove hardcoded `StatelessDirect`) | H-M2 | per-island execution class |
| U3 | Compiled island representation **+** sim/presentation storage split | H-M3, H-M4 | **compiled-artifact bump → v3**; separate state buffers |
| U4 | Minimal stateful CPU reference backend | H-M5 | deterministic stateful yardstick |
| U5 | Minimal stateful GPU backend | H-M6 | persistent GPU particle sim |
| U6 | Generic GPU checkpoint backend | H-M7 | GPU-resident checkpoint/replay |
| U7 | Mixed-island coherent seeking | H-M8 | analytic+stateful at one logical time |
| U8 | Hybrid benchmarks + analytic-kernel optimization | H-M9 | evidence-based perf policy |
| **U9** | **Stateful collision primitives** | H-M10 | **first shipped feature** |
| U10 | Collision input-provider abstraction | H-M11 | portable scene-collision boundary |
| U11 | Preview vs exact editor seeking | H-M12 | responsive scrubbing of heavy sims |

> **Floating — pull forward anytime:** the Properties **shell** (E-M9 Phase 9a) depends on nothing in
> Phase B and can land as an independent UI win whenever the current all-cards panel blocks daily
> work. Only its *schema-driven* phase (9b) waits on Phase C.

### Phase C — Generic stages & plugin boundary (extensible; unlocks staged/fluids)

| # | Step | Source | Delivers |
|---|---|---|---|
| U12 | Property schema + generic extension payload | E-M2 | plugin data without plugin structs |
| U13 | Explicit `StageInstance` + **authored format v4** | E-M3 | the single authored-format cut |
| U14 | Descriptors + capability-based compatibility | E-M4 | (metadata shape already from U1) |
| U15 | Generic compiled stages | E-M5 | folds islands (U3) + stages into one compiled model |
| **U16** | **Portable execution IR + resource model** | E-M6 | **the shared IR** (§44.1) — also the staged-pass foundation |
| U17 | GPU generic stage scheduler | E-M7 | multi-pass GPU execution |
| U18 | Renderer extension model | E-M8 | community renderers |
| U19 | Properties redesign (schema-driven, Phase 9b) | E-M9 | scalable inspector |
| U20 | Linked plugin SDK vertical slice | E-M10 | ecosystem API proof |
| U21 | Missing-plugin / versioning UX | E-M11 | safe third-party asset exchange |
| U22 | Packaged plugin host | E-M12 | install plugins without rebuilding |

### Phase D — Staged simulation & fluids (convergence)

| # | Step | Source | Delivers |
|---|---|---|---|
| U23 | Generic staged simulation plan | H-M13 | built on U16's IR (not a new one); GPU-only determinism fixture (2D diffusion) |
| U24 | Real fluid plugin proof (GPU-only, `cpu_reference = Unavailable`) | E-M13 | **Aestra's fluid** (§44.6) |
| U25 | Advanced cross-system dependency graph | H-M15 | analytic→fluid→stateful chains |
| U26 | Ecosystem hardening + production optimization | E-M14, H-M16 | distribution + AAA-scale perf |

### Sequencing rules baked into this order

- **The execution IR is designed once, at U16**, and U23 (staged) consumes it — `SimulationPassPlan`
  is not reinvented. If Phase D is likely to start soon, bring U16 forward rather than duplicating.
- **Authored format bumps exactly once, at U13.** The *compiled* artifact bumps at U3 (islands) and
  again at U15/U16 (generic stages + IR) — see the relaxed rule in the extensible plan §44.5:
  compiled artifacts are regenerated, so a second bump is a recompile, not a migration, and must not
  block Phase B on Phase C.
- **Collision (U9) ships with zero plugin work** — the payoff that justifies the hybrid-first lead.
- **If the plugin ecosystem (or Properties pain) is the real priority**, move Phase C ahead of
  Phase B; you then wait longer for collision. Phase A is required either way.
