# Milestone 14 — Effect Document Architecture: Design

Status: design (no code yet). Companion to
[`aestra_multi_document_editor_plan.md`](./aestra_multi_document_editor_plan.md) milestones 14–15
("PR 14+ — Effect documents").

## Goal

Separate an Effect's **authored state** from its **live simulation/preview state**, so that:

- multiple Effect documents can be open at once (`Explosion.aestra`, `SmokeTrail.aestra`, …), and
- only the active/visible Effect retains the expensive live preview (GPU/particle buffers,
  checkpoints), while hidden Effects keep only their small authored + view settings.

Exit criterion (M14): Effect authoring can migrate to the generic document system without requiring
every open Effect to run a full simulation. Exit criterion (M15): Effects, Materials, Functions, and
WESL coexist as documents; only active/visible Effects hold live preview state.

## Where we are today

`EditorSession` (`apps/aestra-editor/src/session.rs`) is a single, central `Resource` that holds
**one** effect and mixes three concerns. Everything in the editor reads it (~290 references to the
live-preview fields alone: `.preview`, `.clock`, `.playing`, `.samples`, `.solo_emitter`,
`.preview_seed`, `.speed`). The live preview is **derived** from the authored effect:
`refresh_preview()` recompiles `preview` from `effect` + `solo_emitter` + `preview_seed` + the
effect's material programs. This derivation is the seam M14 formalizes.

### Field inventory and target ownership

| Field | Concern | Target home |
|---|---|---|
| `effect: EffectAsset` | authored | `EffectDocumentState` |
| `source_path` | authored | `EffectDocumentState` |
| `saved_effect`, `saved_source_bytes` | authored (save baseline) | `EffectDocumentState` |
| `dirty` | authored (derived) | `EffectDocumentState` |
| `pending_change`, `interaction_source` | authored (in-flight edit) | `EffectDocumentState` |
| `history`, `operation_order`, `history_generation` | authored (undo) | `EffectDocumentState` |
| `diagnostics`, `last_diff` | authored (validation) | `EffectDocumentState` |
| `locks` | authored | `EffectDocumentState` |
| `selection`, `selected_emitter_region` | authored (per-document selection) | `EffectDocumentState` |
| `effect_revision` | bridge (bumps invalidate the runtime) | `EffectDocumentState` (read by runtime) |
| `preview: Option<EffectInstance>` | **live runtime** | `EffectPreviewRuntime` |
| `samples` | **live runtime** | `EffectPreviewRuntime` |
| `checkpoints` | **live runtime** | `EffectPreviewRuntime` |
| `last_seek` | **live runtime** | `EffectPreviewRuntime` |
| `clock: PlaybackClock` | live **settings** (playhead) | `EffectPreviewSettings` |
| `preview_seed` | live settings | `EffectPreviewSettings` |
| `solo_emitter` | live settings | `EffectPreviewSettings` |
| `playing` | live settings | `EffectPreviewSettings` |
| `speed` | live settings | `EffectPreviewSettings` |
| `material_target` | active-context (which material is being edited) | `ActiveEditorContext` (already exists) |
| `material_drafts` | **project**-level (shared material edits) | project catalog / drafts store |
| `material_history_active` | active-context flag | `ActiveEditorContext` |
| `status`, `ui_revision` | global UI | stays global (shell/session shell) |

The key refinement over the plan's two-way split: the live side divides into **settings** (small,
always kept per document so a re-activated Effect restores its playhead/seed/solo/play state) and
**runtime** (heavy: the compiled `EffectInstance`, particle `samples`, `checkpoints`, `last_seek` —
dropped when the Effect is hidden, rebuilt on activation).

## Target types

```rust
/// Authored, persistent state of one Effect document. One per open Effect. Cheap to keep resident
/// for hidden Effects (no particle buffers). This is what the timeline, properties, graph, undo,
/// and save operate on.
struct EffectDocumentState {
    effect: EffectAsset,
    source_path: Option<PathBuf>,
    saved_effect: Option<EffectAsset>,
    saved_source_bytes: Option<Vec<u8>>,
    dirty: bool,
    pending_change: Option<PendingChange>,
    interaction_source: Option<EffectAsset>,
    history: CommandHistory,
    operation_order: EditOrder,
    history_generation: u64,
    diagnostics: ValidationReport,
    last_diff: EffectDiff,
    locks: LockState,
    selection: Selection,
    selected_emitter_region: Option<EmitterRegionId>,
    effect_revision: u64,
}

/// Small, persistent per-document preview settings. Survive suspension so a re-activated Effect
/// restores the user's playhead and playback options without recompiling from scratch state loss.
struct EffectPreviewSettings {
    clock: PlaybackClock,   // playhead
    preview_seed: u64,
    solo_emitter: Option<EmitterId>,
    playing: bool,
    speed: f32,
}

/// Heavy live runtime. Present ONLY for the active/visible Effect. Rebuilt from the document +
/// settings on activation; dropped on suspension to free particle buffers and checkpoints.
struct EffectPreviewRuntime {
    instance: EffectInstance,
    samples: Vec<ParticleSample>,
    checkpoints: CheckpointStore<EffectInstance>,
    last_seek: SeekPlan,
}

/// One open Effect. Settings persist; runtime is Some only while active.
struct EffectSession {
    document: EffectDocumentState,
    settings: EffectPreviewSettings,
    runtime: Option<EffectPreviewRuntime>,
}
```

## Suspension / activation policy

- **Active Effect** (the visible/focused Effect tab, or the singleton today): `runtime.is_some()`.
  It advances each frame, seeks, records checkpoints, evaluates samples for the viewport.
- **Hidden Effect** (open but not the active tab): `runtime.is_none()`. It keeps `document` and
  `settings` only. No per-frame simulation, no particle buffers, no checkpoints. Memory footprint is
  the authored effect plus a few scalars.
- **Activate(effect)**: build `runtime` via `compile_preview_with_solo_and_material_programs(
  &document.effect, settings.preview_seed, settings.solo_emitter, &programs)`, then seek to
  `settings.clock` playhead. Equivalent to today's `from_effect` preview construction.
- **Suspend(effect)**: drop `runtime` (frees `instance`, `samples`, `checkpoints`). `settings` is
  already current (playhead/seed/solo/play updated live while active), so nothing else to save.
- **Edit while active**: `invalidate_effect_checkpoints()` bumps `document.effect_revision` and
  clears `runtime.checkpoints`; `refresh_preview()` rebuilds `runtime.instance` from the document —
  unchanged behavior, just relocated.
- **Edit is only possible on the active Effect** (it is the focused tab), so a hidden Effect never
  needs to recompile a runtime it does not have. If a background process must invalidate a hidden
  Effect (e.g. a shared material it references changed), it marks the document's `effect_revision`;
  activation rebuilds from the current document, so the stale runtime is never shown.

At most one Effect holds a runtime under the current single-viewport model. M15 keeps that
invariant: switching the active Effect tab suspends the previous and activates the next.

## Integration with the document system

- Add `DocumentKey::Effect(EffectId)` and `EditorViewKind::Effect` (mirrors Material/WESL).
- `DocumentManager` gains Effect documents; the active Effect's `EffectSession` is what the existing
  timeline/properties/graph/viewport systems read. The viewport preview follows
  `active.effect_document`'s `runtime`.
- `ActiveEditorContext` already tracks the active view/document; extend it to resolve the active
  Effect so the shared systems know which `EffectSession` is "hot".
- Workspace persistence (M11) already persists editor views; effect views persist their asset key
  like material/WESL views, and reopen on restart (runtime built lazily on activation).

## Staged refactor plan (each stage compiles, tests green, no behavior change until noted)

Because ~290 sites read the live fields, the migration is mechanical but must be sub-staged so the
editor core is never broken:

1. **Stage A — group the runtime.** Introduce `EffectPreviewRuntime` and `EffectPreviewSettings`
   as fields inside today's single `EditorSession` (`session.runtime: Option<…>`,
   `session.settings: …`), moving the live fields into them behind accessor methods
   (`session.preview()`, `session.clock()`, `session.playing()`, …) so call sites change to methods
   incrementally. Behavior-preserving. Largest mechanical stage; split by field group if needed.
2. **Stage B — suspend/activate on the session.** Add `EditorSession::suspend_preview()` /
   `activate_preview()` that drop/rebuild `runtime` from the document + settings. Prove it round-trips
   the playhead with a test. Still single Effect; nothing calls suspend yet in production.
3. **Stage C — group the document.** Introduce `EffectDocumentState` holding the authored fields,
   again behind accessors. Behavior-preserving. Now `EditorSession` is `{ document, settings,
   runtime, + global UI }` — the M14 shape, still single Effect.
4. **Stage D — Effect as a document.** Add `DocumentKey::Effect` / `EditorViewKind::Effect`; let the
   `DocumentManager` own Effect documents; route the active Effect through `ActiveEditorContext`.
   The singleton session becomes "the active Effect's session". Highest-risk stage — gated behind
   Stages A–C so the boundaries already exist.
5. **Stage E (M15) — multiple Effect tabs.** Opening a second Effect suspends the first; only the
   active Effect holds a runtime. Wire tab switching to suspend/activate. Deliver the plan's Document
   tests (open two Effects; dirty/save/close independently; only active simulates).

Move `material_drafts` to the project level and `material_target`/`material_history_active` into
`ActiveEditorContext` as a clean-up within Stage C/D (they are not per-Effect authored state).

## Testing strategy

- **Stage A/C (refactor):** the existing session/preview/undo/save suite must stay green unchanged —
  it is the regression guard for the mechanical moves.
- **Stage B (suspension):** `activate` after `suspend` restores the same playhead frame and samples;
  `suspend` drops the runtime (`runtime.is_none()`, checkpoints empty); memory estimate drops.
- **Stage D/E (multi-effect):** open two Effects → two documents, two views; editing/undo/save/close
  are independent; only the active Effect advances its clock (a hidden Effect's frame does not move
  across app updates); switching tabs suspends the previous runtime and builds the next.
- Reuse the plan's Document tests (§44): open same asset twice, open two assets, dirty/save/close
  independently.

## Risks and mitigations

- **Core breakage:** `EditorSession` is read everywhere. Mitigation: accessor-method indirection in
  Stages A/C so field moves are compiler-guided and reversible; never change behavior and refactor
  in the same commit.
- **Preview correctness under suspend/activate:** the playhead/seed/solo must round-trip. Mitigation:
  settings are updated live while active (already true today), so activation only rebuilds the
  derived runtime and re-seeks. Covered by Stage B tests.
- **Shared-material invalidation of hidden Effects:** a hidden Effect referencing an edited shared
  material must not show a stale runtime. Mitigation: invalidation marks the document revision;
  activation always rebuilds from the current document.
- **Scope:** this is multiple PRs. Each stage is independently reviewable and green; M15 (multiple
  tabs) only begins after Stage D lands.
