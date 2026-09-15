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

**Status: COMPLETE** (A1–A7 + B1–B3 landed; A8 deferred to pre-M9 by design — see below). The
before-picture for the S1 refactor is locked: correctness as hard `cargo test` gates over seven
showcase fixtures (structure, artifact round-trip, CPU determinism, native-GPU conformance,
source-map, renderer plans) and performance as a provenance-carrying reference artifact. Next up:
Milestone S1.

**Exit gate:** one reproducible baseline of the *current* implementation, covering correctness and
performance, sparse and dense, with no architecture change landed.

### S0-A · Correctness fixtures

- [x] **S0-A1 — Semantic effect fixtures.** Curated 7 representative real effects (sprite, textured,
  flipbook, ribbon, trail, mesh, material-graph) from the existing `assets/test/effects/`; the
  load-bearing set is the case list in the baseline test below. **Done:** each loads clean and
  compiles.
- [x] **S0-A2 — Compile snapshot for built-in lifecycle stages.** Characterization net pinning each
  fixture's compiled `ExecutionPlan` instruction counts (`eu`/`ps`/`pu`), renderer plan kinds,
  `seek_mode`, `max_particles`, source-map size, and portable requirements against a blessed baseline.
  *Where:* `crates/aestra-compiler/tests/foundation_baseline_contract.rs` +
  `foundation_baseline.txt`. **Done:** any structural lowering change fails the test.
- [x] **S0-A3 — Artifact round-trip fixtures.** For each showcase fixture: assert the encoded
  artifact carries the magic + `format_version:2`, `decode(encode(compiled)) == compiled`, and a
  byte-identical re-encode (deterministic serialization). *Where:*
  `crates/aestra-artifact/tests/foundation_baseline_contract.rs`. **Done:** the v2 compiled format is
  locked across all seven fixtures; an accidental format change fails before any deliberate bump.
- [x] **S0-A4 — CPU evaluation fixtures.** For each fixture at canonical frames (0.1/0.5/1.0/1.5s,
  seed 42): assert re-evaluation is bit-identical (determinism), and pin the alive-particle count to
  a blessed baseline (`foundation_cpu_baseline.txt`). Plus a cross-crate check that a decoded artifact
  evaluates identically to its source (ties S0-A3↔S0-A4). *Where:*
  `crates/aestra-compiler/tests/foundation_baseline_contract.rs` +
  `crates/aestra-artifact/tests/foundation_baseline_contract.rs`. **Done:** determinism + presented
  counts locked. *(Float positions intentionally not pinned to a committed value — cross-platform
  rounding; determinism is asserted within-run and round-trip instead.)*
- [x] **S0-A5 — Native-GPU conformance fixtures.** For each showcase fixture, build a per-fixture
  `GpuHarness` from its own generated shader and assert GPU simulation matches the CPU reference at
  canonical frames (reuses `assert_effect_matches_at_times`). *Where:*
  `bevy/aestra-bevy-render/tests/gpu_conformance.rs`
  (`showcase_effects_match_the_cpu_reference_on_gpu`). **Done:** all 7 verified on a real adapter
  (`AESTRA_REQUIRE_GPU_CONFORMANCE=1`). *Skips cleanly on GPU-less CI (GitHub Actions) via the same
  adapter gate as the existing conformance test; the GPU is exercised in the `gpu-visual` workflow.*
- [x] **S0-A6 — Source-map assertions.** For each fixture, assert every `source_map` entry points at
  a real instruction (emitter + stage plan in range) and pin the sorted set of `RuntimeStage`-based
  locations (`foundation_source_map_baseline.txt`). *Where:*
  `crates/aestra-compiler/tests/foundation_baseline_contract.rs`
  (`showcase_effects_have_valid_stable_source_maps`). **Done:** a re-targeting of diagnostics/profiler
  source mapping is caught.
- [x] **S0-A7 — Renderer-plan fixtures.** Pin each renderer's *full* compiled `RendererPlanKind`
  (kind + parameters — widths, strand counts, flipbook settings, mesh asset) to
  `foundation_renderer_baseline.txt`, not just the variant name. *Where:* same file
  (`showcase_effects_have_stable_renderer_plans`). **Done:** renderer lowering is pinned in detail.
- [~] **S0-A8 — Properties selection/action tests. DEFERRED to pre-M9 (Phase 9a).** Editor tests
  around module/renderer select/add/reorder on a fixture emitter. *Where:*
  `apps/aestra-editor/src/properties*` test modules (`cargo test --bin aestra-editor`).
  **Why deferred:** (1) `properties.rs` / `properties/renderer_controls.rs` are under active,
  uncommitted WIP — freezing their behavior now, while those changes aren't committed, would pin an
  incoherent state; (2) the Properties panel is rewritten wholesale at M9 (stack + inspector
  redesign), so tests written now are thrown away almost immediately. This task's own purpose is to
  cover the panel *"before the eventual redesign"* — so the correct moment is immediately before
  Phase 9a, when Properties is stable and about to change, not now. **Do when:** starting Phase 9a.

### S0-B · Performance baselines

- [x] **S0-B1 — Capture CPU baseline from `main`.** Ran `aestra-bench --all` (release) covering
  `b001`–`b008`, including the load-bearing `b004_sparse_large` (500k @ ~1%). *Where:*
  `benchmarks/cpu-baselines/S0_foundation.json`. **Done.** *(Native-GPU perf is captured on demand via
  `aestra-bench --features gpu --gpu-trails …` → `benchmarks/gpu-baselines/`; not part of this CPU
  snapshot.)*
- [x] **S0-B2 — Record the full metric set.** The JSON carries per-scenario median/p95/p99/max/mean/
  stddev for each CPU stage plus capacity, alive, occupancy, and normalized ns/1k. **Done.**
- [x] **S0-B3 — Store results with provenance.** Each report embeds `commit`, `seed`, and a
  `hardware` block (cores/OS/arch/backend). Recorded as a **reference artifact, not a `cargo test`
  gate** (machine-specific timing must not fail CI) — see `benchmarks/cpu-baselines/README.md`.
  **Done.**

### S0 — Do NOT change

Particle semantics · the 48-byte `GpuParticle` presentation ABI · execution class · authored format ·
registry surface. S0 only *observes*.

---

## Milestone S1 — Identity, semantics, and the one `ModuleMetadata` extension

**Status: COMPLETE** (A–F landed). Namespaced IDs + one `CapabilityId`; derived `SimulationClass` /
`TemporalSemantics`; the single `ModuleMetadata` simulation-requirement extension with class
derivation; the unified `ExtensionRegistry` with three conflict diagnostics; the agreed
`BackendSupport` shape. Every step is proven behaviour-neutral by the S0 baselines, and
`ModuleMetadata` was extended exactly once. The shared foundation (S0 + S1) is done — both the
extensible and hybrid tracks can now diverge from here.

**Exit gate:** all vocabulary and the unified registry both plans need are in place; existing effects
compile byte/behaviour-identically; `ModuleMetadata` was extended **once**.

### S1-A · Namespaced identities (extensible)

- [x] **S1-A1 — Introduce the ID newtypes.** `PluginId`, `StageTypeId`, `DomainTypeId`,
  `ResourceTypeId`, `CapabilityId` as `#[serde(transparent)]` string newtypes (via a `namespaced_id!`
  macro; `new()` + `as_str()`), re-exported from `aestra-core`; `ModuleTypeId`/`RendererTypeId`
  unchanged. *Where:* `crates/aestra-core/src/model.rs`. **Done.**
- [x] **S1-A2 — Reconcile with the existing `Capability` type.** Decided: **replace**, not wrap — the
  old closed `Capability` enum was declarative-only (nothing consumed it), so it is removed and
  `ModuleMetadata.capabilities` is now `Vec<CapabilityId>`. Built-in set documented as a governed
  `aestra.*` contract: `CAPABILITY_CPU_REFERENCE`, `CAPABILITY_PARTICLE_SIMULATION`. **Done** — one
  namespaced capability type, unit-tested; S0 baselines unchanged.

### S1-B · Unified registry (extensible)

- [x] **S1-B1 — Generalize `ModuleRegistry` → `ExtensionRegistry`.** Added `ExtensionRegistry
  { modules: ModuleRegistry, capabilities: CapabilityRegistry }` as a container (kept `ModuleRegistry`
  intact — it is used across the editor/bevy, so renaming would break widely; §23's sub-registry
  shape). `EffectCompiler` now holds it and resolves modules through `self.registry.modules`, while
  its public `registry() -> &ModuleRegistry` and `new(ModuleRegistry)` are preserved (added
  `with_extensions`/`extensions`). *Where:* `crates/aestra-compiler/src/lib.rs`. **Done** — no
  behaviour change (S0 baselines + downstream typecheck clean). *(Stage/renderer/domain/resource
  sub-registries land with their descriptor types.)*
- [x] **S1-B2 — Registry diagnostics.** `RegistryConflict` with three distinct variants —
  `DuplicateModule`, `DuplicateCapability`, `UnknownModuleCapability` (invalid descriptor referencing
  an unregistered capability, via `validate()`). **Done** — each unit-tested.

### S1-C · Derived simulation semantics (hybrid)

- [x] **S1-C1 — Add the derived enums.** `SimulationClass { Analytic, Stateful, Staged }` (ordered
  weakest→strongest) and `TemporalSemantics { Direct, HistoryDependent }`, in
  `crates/aestra-runtime/src/checkpoint.rs` beside `SimulationSeekMode`, re-exported from the crate
  root, documented as *derived, never authored*. **Done.**
- [x] **S1-C2 — Keep `SimulationSeekMode` as resolved-strategy only.** The hardcoded
  `seek_mode: SimulationSeekMode::StatelessDirect` in the compiler is untouched (removal belongs to
  hybrid M2); `SimulationClass` doc marks the class/seek separation. **Done.**

### S1-D · The single `ModuleMetadata` requirement extension (the critical merge)

- [x] **S1-D1 — Extend `ModuleMetadata` ONCE.** Added one `simulation: SimulationRequirements` field
  (`temporal` / `synchronization` / `neighborhood`) alongside `capabilities` / `reads` / `writes` —
  no second requirement system. Every built-in defaults to `SimulationRequirements::ANALYTIC` via the
  single base `metadata()` constructor. *Where:* `crates/aestra-compiler/src/lib.rs`. **Done** — the
  S0 baselines all still pass, proving no behaviour change. *(`multiplicity` deferred to S1-D/S4
  descriptors; not needed yet.)*
- [x] **S1-D2 — Derive `SimulationClass` from requirements.** `SimulationRequirements::derived_class()`
  (sync/neighbourhood → `Staged`; else `PreviousState` → `Stateful`; else `Analytic`) plus
  `temporal_semantics()` and a `max()` aggregator. *Where:* compiler. **Done** — unit-tested that all
  built-ins derive `Analytic` and the promotion rules are correct. *(Per-emitter reporting arrives
  with the hybrid island milestone that consumes this.)*

### S1-E · Backend-capability shape (agreed, not implemented)

- [ ] **S1-E1 — Fix the shape only.** One model covering `cpu_reference`/`gpu_compute` (extensible
  `BackendSupport`) and `checkpoints`/`staged_dispatch` (hybrid `BackendSimulationCapabilities`).
  **Done when** the type shape is written down; no backend implements it yet.

- [x] **S1-E1 — Fix the shape only.** Added the descriptor side: `SupportLevel
  { Required, Supported, Unavailable }` + `BackendSupport { cpu_reference, gpu_compute }`, documented
  as the complement of `aestra_runtime::BackendCapabilities` (the backend-offers side); the hybrid
  checkpoint/staged-dispatch axes extend that side, not this one. **Done** — no backend consumes it
  yet.

### S1-F · Guardrail docs & tests

- [x] **S1-F1 — Distinction tests.** The `SimulationDomain ≠ SimulationClass ≠ StageKind ≠
  SimulationSeekMode` distinctions and *capability ≠ execution order* are documented in the
  `SimulationClass` / `SimulationRequirements` / `CapabilityId` doc-comments; `SimulationClass` is
  never authored (derived only). **Done** via doc-comments.
- [x] **S1-F2 — Fake-module tests.** `extension_registry_hosts_builtins_registers_plugins_and_diagnoses_conflicts`
  registers a fake third-party module (resolves through the unified registry, no privileged built-in
  path) and a fake stateful module (its metadata derives `SimulationClass::Stateful` with no backend).
  **Done.**

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
