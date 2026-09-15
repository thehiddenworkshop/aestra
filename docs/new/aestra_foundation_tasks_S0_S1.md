# Aestra — Foundation Tasks (S0 & S1)

**Scope:** the actionable task breakdown for the two shared-foundation milestones in
`aestra_shared_foundation_milestones.md`. These are the *only* things to build before either the
hybrid or extensible track diverges. Nothing here changes runtime behaviour or the authored format.

**Conventions grounded in the current repo:**
- Tests are hand-written `crates/<crate>/tests/<area>_contract.rs` with explicit asserts + RON
  round-trips (no snapshot library). Match that style.
- Benchmarks live in `apps/aestra-bench` (`src/scenario.rs`, `src/metrics.rs`); scenarios `b001`–`b008`
  already exist, including `b006_many_emitters`, `b007_loop_pressure`, `b008_curve_stress`.
- `ModuleMetadata` and `ModuleRegistry` already exist in `crates/aestra-compiler/src/lib.rs`;
  `ModuleMetadata` already has a `capabilities: Vec<Capability>` field. `SimulationSeekMode` and
  `CheckpointStore<T>` already exist in `crates/aestra-runtime/src/checkpoint.rs`.
- `CURRENT_FORMAT_VERSION = 3` (authored), `CURRENT_ARTIFACT_VERSION = 2` (compiled). Neither changes
  in S0/S1.

Legend: `[ ]` todo · each task names its file(s) and a **Done when** gate.

---

## Milestone S0 — Freeze the baseline (correctness + performance)

**Exit gate:** one reproducible baseline of the *current* implementation, covering correctness and
performance, sparse and dense, with no architecture change landed.

### S0-A · Correctness fixtures

- [ ] **S0-A1 — Semantic effect fixtures.** Curate 4–6 representative real effects (not toys) as v3
  RON fixtures under a shared test-assets path. *Where:* `assets/test/effects/` (reuse existing) +
  a manifest listing which are load-bearing. **Done when** each loads, compiles, and is referenced by
  at least one contract test below.
- [ ] **S0-A2 — Compile snapshot for built-in lifecycle stages.** Assert the compiled
  `ExecutionPlan { emitter_update, particle_spawn, particle_update }` shape/contents for the fixtures.
  *Where:* `crates/aestra-compiler/tests/compiler_contract.rs`. **Done when** a change to lowering
  order/content fails the test.
- [ ] **S0-A3 — Artifact round-trip fixtures.** Encode→decode each fixture at
  `CURRENT_ARTIFACT_VERSION = 2`; assert `format_version:2` and byte-stable RON. *Where:*
  `crates/aestra-artifact/tests/artifact_contract.rs`. **Done when** round-trip is asserted for every
  S0-A1 fixture.
- [ ] **S0-A4 — CPU evaluation fixtures.** Assert deterministic CPU particle state at fixed
  canonical frames for each fixture. *Where:* `crates/aestra-runtime` tests. **Done when** repeated
  runs are bit-identical.
- [ ] **S0-A5 — Native-GPU conformance fixtures.** CPU-vs-GPU comparison at canonical frames within
  the existing tolerance. *Where:* `crates/aestra-gpu/tests/` (follow existing GPU contract tests).
  **Done when** CPU and native GPU agree for every fixture. *(Needs a GPU; gate on `native-gpu` test
  cfg as existing GPU tests do.)*
- [ ] **S0-A6 — Source-map assertions.** Pin current `RuntimeStage`-based source mapping for the
  fixtures. *Where:* wherever source-map tests live today. **Done when** a mapping change is caught.
- [ ] **S0-A7 — Renderer-plan fixtures.** Assert compiled `RendererPlanKind` for the fixtures'
  renderers. *Where:* `crates/aestra-compiler/tests/`. **Done when** renderer lowering is pinned.
- [ ] **S0-A8 — Properties selection/action tests.** Editor tests around module/renderer
  select/add/reorder on a fixture emitter. *Where:* `apps/aestra-editor/src/properties*` test modules
  (`cargo test --bin aestra-editor`, editor exe is locked while running). **Done when** the current
  panel's actions are covered before the eventual redesign.

### S0-B · Performance baselines

- [ ] **S0-B1 — Capture CPU + native-GPU baselines from `main`.** Run `apps/aestra-bench` for
  `b002`–`b008`; confirm `b004_sparse_large` (500k @ ~1%) is included (it is load-bearing for the
  analytic-vs-stateful decision). **Done when** a baseline run exists for all of them.
- [ ] **S0-B2 — Record the full metric set.** Per scenario: CPU evaluation, GPU simulation time,
  capacity, alive count, occupancy, workgroups, GPU buffer bytes. *Where:* `apps/aestra-bench/src/metrics.rs`
  (extend only if a field is missing). **Done when** every metric above is emitted.
- [ ] **S0-B3 — Store results with provenance.** Persist baseline output tagged with commit /
  hardware / backend. **Done when** the baseline file names the commit it was taken at.

### S0 — Do NOT change

Particle semantics · the 48-byte `GpuParticle` presentation ABI · execution class · authored format ·
registry surface. S0 only *observes*.

---

## Milestone S1 — Identity, semantics, and the one `ModuleMetadata` extension

**Exit gate:** all vocabulary and the unified registry both plans need are in place; existing effects
compile byte/behaviour-identically; `ModuleMetadata` was extended **once**.

### S1-A · Namespaced identities (extensible)

- [ ] **S1-A1 — Introduce the ID newtypes.** `PluginId`, `StageTypeId`, `DomainTypeId`,
  `ResourceTypeId`, `CapabilityId` as `#[serde(transparent)]` string newtypes, mirroring the existing
  `ModuleTypeId`/`RendererTypeId`. *Where:* `crates/aestra-core/src/model.rs`. **Done when** they
  round-trip and keep `ModuleTypeId`/`RendererTypeId` unchanged.
- [ ] **S1-A2 — Reconcile with the existing `Capability` type.** `ModuleMetadata.capabilities:
  Vec<Capability>` already exists — decide whether `CapabilityId` replaces or wraps `Capability`, and
  document the built-in capability vocabulary as a governed public contract (extensible §9.1).
  **Done when** there is one capability type, namespaced, with a documented built-in set.

### S1-B · Unified registry (extensible)

- [ ] **S1-B1 — Generalize `ModuleRegistry` → `ExtensionRegistry`.** Today `ModuleRegistry` in
  `crates/aestra-compiler/src/lib.rs` holds only modules. Introduce a registry that will host stage /
  module / renderer / domain / resource / capability descriptors; seed it with today's built-in
  modules via the same `builtin()` path. *Where:* `crates/aestra-compiler/src/lib.rs`. **Done when**
  `EffectCompiler` resolves built-in modules through the unified registry with no behaviour change.
- [ ] **S1-B2 — Registry diagnostics.** Duplicate IDs, provider conflicts, invalid descriptors.
  **Done when** each produces a distinct diagnostic and is unit-tested.

### S1-C · Derived simulation semantics (hybrid)

- [ ] **S1-C1 — Add the derived enums.** `SimulationClass { Analytic, Stateful, Staged }` and
  `TemporalSemantics { Direct, HistoryDependent }`. *Where:* `crates/aestra-runtime` (next to
  `SimulationSeekMode`) or a shared contract module. **Done when** they exist and are documented as
  *derived, never authored*.
- [ ] **S1-C2 — Keep `SimulationSeekMode` as resolved-strategy only.** Do **not** yet remove the
  hardcoded `seek_mode: SimulationSeekMode::StatelessDirect` in the compiler
  (`crates/aestra-compiler/src/lib.rs` ~L680) — that removal belongs to hybrid M2. **Done when** a
  comment/doc marks it as the resolved strategy, not the sole compiled semantic.

### S1-D · The single `ModuleMetadata` requirement extension (the critical merge)

- [ ] **S1-D1 — Extend `ModuleMetadata` ONCE.** Add the hybrid requirement fields co-designed with
  the existing extensible fields, so there is one requirement system:
  ```text
  temporal        : Direct | PreviousState
  synchronization : None | OrderedPass | Iterative
  neighborhood    : None | Particles | Grid
  ```
  alongside the existing `capabilities` / `reads` / `writes` (+ `multiplicity` if landing it here).
  *Where:* `crates/aestra-compiler/src/lib.rs` (`ModuleMetadata`, and `builtin_modules()`). **Done
  when** every built-in module declares `temporal = Direct, synchronization = None, neighborhood =
  None` and no second parallel requirement struct exists.
- [ ] **S1-D2 — Derive `SimulationClass` from requirements.** `PreviousState → Stateful`;
  `Iterative`/`Grid`/neighborhood → `Staged`; else `Analytic`. *Where:* compiler. **Done when** the
  compiler can report, per emitter, its derived class and the field that would promote it.

### S1-E · Backend-capability shape (agreed, not implemented)

- [ ] **S1-E1 — Fix the shape only.** One model covering `cpu_reference`/`gpu_compute` (extensible
  `BackendSupport`) and `checkpoints`/`staged_dispatch` (hybrid `BackendSimulationCapabilities`).
  **Done when** the type shape is written down; no backend implements it yet.

### S1-F · Guardrail docs & tests

- [ ] **S1-F1 — Distinction tests.** Assert/document `SimulationDomain != SimulationClass`,
  `StageKind != SimulationClass`, `SimulationSeekMode != SimulationClass`, and capability ≠ execution
  order. **Done when** each has a test or doc-comment.
- [ ] **S1-F2 — Fake-module tests.** Register (a) a fake third-party module and (b) a fake stateful
  module in tests. **Done when** (a) appears via the registry with no editor regression and (b) makes
  the compiler *report* Stateful (no backend executes it).

### S1 — Do NOT change

Authored format · GPU runtime · particle semantics · presentation ABI. Existing effects must compile
byte/behaviour-identically throughout S1.

---

## Ordering within the foundation

```text
S0-A (correctness) ┐
                   ├─ both independent, do in parallel
S0-B (perf)        ┘
        │  (baseline must be green before S1 lands)
        ▼
S1-A IDs → S1-B registry ┐
S1-C derived enums       ├─ S1-D is the keystone; it depends on A+C
                         ┘
        ▼
S1-D one ModuleMetadata extension → S1-D2 derive class
        ▼
S1-E backend shape · S1-F guardrail tests
```

**S1-D1 is the keystone** — it is the "extend `ModuleMetadata` exactly once" step both plans depend
on. Do S1-A and S1-C before it so its fields are named against final vocabulary; get it right and the
hybrid/extensible tracks share one requirement system forever after.

## Explicitly out of scope for S0/S1

Simulation islands (hybrid M2) · authored format v4 / `StageInstance` (extensible M3) · property
schema (extensible M2) · any GPU/runtime execution change · removing the hardcoded `StatelessDirect`.
Those begin *after* this foundation, on their tracks — see the unified sequence U2+ in
`aestra_shared_foundation_milestones.md`.
