# Aestra implementation plan

Status: active delivery plan; M0–M5 complete, M6 in progress

This plan turns `aestra_ai_native_vfx_editor_plan.md` into a build sequence for the
current repository. The source document remains the long-term product and
architecture vision; this file is the shorter delivery plan.

## Current delivery status

- M0 reference behavior and architecture decisions: complete.
- M1 semantic core and format v3 foundation: complete for the native 3D module set.
- M2 deterministic authoring operations: complete for current editor operations.
- M3 module registry, compiler frontend, and CPU runtime: complete for the initial module set.
- M4 first GPU production slice: complete. The deterministic WESL compute substrate,
  bounded particle pools, compaction lists, counters, native indirect alpha/additive
  sprite drawing, conservative visibility bounds, and explicit CPU/readback fallbacks
  are implemented. Deterministic image regression with approved references, tolerant
  metrics, and diff reports is implemented. Adapter capability reporting, automatic
  backend selection, particle budgets, and per-effect fallback diagnostics close the milestone.
- M5 professional authoring workflow: complete. The editor has a searchable,
  registry-driven stage/module stack, metadata-defined typed property controls,
  undoable module and renderer structure edits, inline compiler diagnostics, and
  dedicated curve/gradient authoring with draggable keys and fine-grained semantic
  commands. Transactions can now be executed against a temporary document, previewed,
  reviewed as semantic changes with compiler diagnostics, and applied or discarded as
  one history entry. A shared fixed-step playback clock now gives the runtime, editor,
  and viewer exact 60 Hz frame addressing, deterministic seed controls, restart/frame
  stepping, tick-snapped scrubbing, and frame-addressed visual captures. A bounded,
  context-keyed checkpoint store now supports backward restore-and-replay for stateful
  effects, while stateless effects seek directly and snapshotless backends explicitly
  restart and replay. Backend-native GPU snapshot capture remains part of future
  stateful GPU module work. The editor workspace now uses an in-code BSN shell and a
  persisted recursive dock tree. Every authoring panel can be inserted and reordered
  through tab strips or split through directional panel-content drop regions; empty branches collapse automatically,
  splitters expose directional cursors, closed panels can be restored from View, and
  docked tabs can become persisted native secondary windows through an explicit
  context action, enabling multi-monitor layouts. Structured diagnostics now have a
  dedicated severity-filtered dock panel with navigation to owning semantic objects,
  a persistent compile-state footer shortcut, and panel visibility controls under View.
  The live compiler artifact is now exposed through a source-mapped Compiler Inspector
  panel covering execution stages, particle layout, parameter slots, renderer plans,
  optimizations, and WESL entry points. A dockable Profiler now consumes public,
  machine-readable runtime snapshots with measured CPU/live-particle data, rolling
  history, per-emitter peaks, and explicitly labeled compiler estimates; native GPU
  timestamps remain a capability-dependent extension. A dockable Settings workspace
  now owns versioned `settings.ron` persistence, live grid, particle-limit, and UI-scale
  controls, capture defaults, protected recovery from malformed or newer files, and a
  persisted locale. The Settings workspace and editor chrome now form the first
  complete Bevy Feathers migration slices, using the upstream dark theme and
  BSN-backed menus and tooling controls for consistent buttons, checkboxes,
  number entry, scrollbars, focus, cursors, and accessibility. Embedded
  `en-US` and `fr-FR` Fluent catalogs now localize the editor
  shell live with English fallback, semantic message IDs, interpolation, and catalog
  coverage tests. Metadata-defined Properties scalars, integers, vectors, ranges,
  and toggles now use Feathers controls, commit final values through semantic
  undoable commands, retain in-progress text across live preview frames, and expose
  localized built-in names and descriptions. Effect and emitter identity, playback,
  enabled state, capacity, and typed outgoing emitter-event links are now editable in
  the Properties through the same command history. An automated blank-document acceptance
  path authors a representative multi-emitter effect, proves full undo/redo, saves and
  reloads the asset, and compiles it without direct RON edits. Semantic emitter transforms now flow
  from persisted format-v3 assets through commands, compiler artifacts, CPU/GPU
  execution, compact Properties controls, and viewport gizmos. Gizmo drags preview
  temporary transactions and commit one undoable command on release. M5 workflow now
  includes debounced atomic recovery snapshots, startup restore/discard,
  stale-snapshot cleanup, save/discard cleanup, and persisted autosave enablement and
  interval controls. Format loading now detects the authored version before full
  deserialization and routes legacy v2 assets through an explicit typed v2→v3 migration.
  The editor asks before replacement, preserves a uniquely named synchronized backup,
  atomically writes the validated result, and leaves unsupported or failed migrations
  untouched. The
  Assets, Timeline, Curves, Diagnostics, Compiler Inspector, Profiler, and Changes now use
  stable Fluent messages for editor-owned chrome, summaries, and empty states while
  retaining exact compiler payloads, semantic paths, IDs, asset paths, and authored names.
  Remaining M5 localization work is limited to incidental action/status messages exposed
  as owning plugins evolve.
- M6 renderer, material, and asset breadth: in progress. Renderers now reference stable,
  reusable material definitions instead of owning presentation state. The first sprite
  material domain compiles blend state, typed softness/tint inputs, explicit particle-color
  consumption, stable texture assets, and UV regions into backend-independent material
  plans. Native GPU presentation consumes the full blend/softness/color/texture contract;
  CPU/readback presentation preserves color, texture/UV, flipbook frame, transform, and
  visibility but is not a pixel reference for blend or analytic softness. The Assets and Properties
  workspaces expose material creation, assignment, and undoable editing. Imported
  flipbooks now add explicit atlas frames, particle-age/effect-time animation,
  forward/reverse/ping-pong playback, looping, deterministic random starts, native WESL,
  CPU/readback parity, authoring controls, and a viewer example. Procedural recipes,
  mesh/ribbon production paths, renderer sorting, and richer material domains remain. The first
  reusable-composition foundation is also complete: `aestra-project` recursively indexes project
  effects, resolves typed `EffectAssetRef` values through persisted `EffectId`, preserves identity
  across source moves, and reports missing, duplicate, invalid, unsupported, and unavailable
  sources structurally. The Library now consumes that shared index instead of using its path hash
  as an effect reference. The first semantic composition slice now adds backward-compatible v3
  `EffectClip` serialization, local timing validation, transitive dependency resolution and cycle
  diagnostics, project compilation, child-time mapping, deterministic clip seeds, and nested CPU
  reference execution with instance provenance. `aestra-authoring` now provides atomic EffectClip
  create/delete/timing/seed commands, semantic selection and locks, granular diffs, selection
  repair, and stable undo/redo. Timeline drag/drop, clip presentation, trimming, reordering,
  instance transforms, Properties UI, recursive read-only expansion, source navigation, and
  Bevy/GPU child rendering are implemented. Project file watching, guarded rename/move,
  missing-reference repair, and project-aware preview recompilation are also complete. The first
  instance-override foundation is complete: typed clip overrides persist in v3 assets, compile
  against exposed source parameters, execute in nested CPU and Bevy/GPU preview instances, report
  orphaned/type-changed values, and participate in undo/redo. The Properties now authors typed
  property-level public toggles, in-place source defaults, instance overrides, and
  reset-to-source. Explode now replaces a referenced clip with editable local emitters, imports
  the resources they require, bakes valid instance overrides, and preserves the clip's visible
  timing and transform in one undoable transaction. Named timeline markers now provide stable,
  marker-relative anchors for emitters, clips, and choreography events. Typed choreography events
  have a dedicated timeline lane, transactional Properties editing, compiler artifacts, and
  deterministic interval dispatch across loop boundaries. Local emitter tracks can now expand
  their existing curve and gradient properties as automation lanes. Timeline keys share selection
  with Curves, can be added at the playhead, moved with timeline snapping, deleted with the standard
  Delete shortcut, and commit through the same undoable semantic curve commands. Module properties
  now retain separate authored values for each supported source instead of
  destructively converting one representation into another. The first end-to-end scalar source
  slice gives Spawn Rate Constant, deterministic Random Range, and Curve over Emitter Time modes;
  Drag and Turbulence now use the same reusable source pipeline for Constant, stable per-particle
  Random Range, and Curve over Particle Life. Gravity extends that contract to vectors: Constant
  XYZ, stable per-particle XYZ Random Range, and independent X/Y/Z curves over Particle Life,
  including compact per-axis Properties controls and channel editing in Curves. These properties
  run across serialization, source-preserving authoring, CPU compilation/execution, native GPU
  packing, and WESL. Scalar curves, gradients, and vector curves project into Timeline automation
  lanes; vector curves use independent X/Y/Z lanes that share key/channel selection with Curves.
  Public bindings and reusable-effect overrides use the active source's concrete value type.
  Top-level local emitter tracks can now
  be multi-selected and extracted into a collision-safe reusable effect asset, optionally replacing
  the source tracks with one referenced clip through an undoable transaction. The project index now
  exposes deterministic direct and transitive dependency and reverse-usage relations with owning
  clip identity. The Library presents those relations through a navigable Uses / Used By inspector,
  selects exact owner clips, and guards source deletion with refreshed dependency warnings.
  Recursive reusable extraction, preview-cache identity,
  procedural recipes, mesh/ribbon production paths, authored renderer sorting controls, and richer material domains
  remain. The semantic material-program foundation now includes backend-independent expression
  type and evaluation-domain inference, deterministic socket/output/resource/domain/render-state
  diagnostics, one transactional authoring boundary for project programs, effect-local instances,
  renderer assignments, semantic diffs, and bounded undo/redo, plus deterministic lowering into a
  typed backend-neutral IR with source mapping, constant folding, trivial arithmetic
  simplification, and dead-value elimination. `aestra-gpu` now lowers that IR into inspectable,
  Naga-validated WESL/WGSL; emits deterministic uniform, multi-texture, and shared-sampler layouts;
  reflects parameter and required-input bindings; validates portable backend limits; and separates
  shader fingerprints from render-state/target pipeline keys. `aestra-bevy-render` translates the
  portable resource ABI and device limits without owning material semantics.

## M6 composition release gate

The local release gate separates immutable semantic/runtime contract fixtures from the editable
example effects used by the editor and native-GPU visual workflow. Formatting, all-target workspace
checks, strict Clippy, and workspace tests must pass together before the next M6 feature slice.
Intentional changes to Prism Bloom, Ember Sigil, or Plasma Burst are approved through the dedicated
native-GPU visual workflow rather than by rewriting semantic golden contracts. GPU reference
approval remains a manual or scheduled self-hosted-runner gate.

## Pre-material authoring hardening milestone

Complete this gate before implementing the semantic material-program vertical slice described in
`aestra_material_authoring_architecture.md`. The purpose is to keep the next compiler and editor
expansion built on stable, understandable foundations rather than adding another large feature
surface to mutable examples and monolithic panel modules.

1. **Isolate deterministic test fixtures — complete.** Compact, stable effect and editor
   session builders whose semantic IDs, timing, playback mode, modules, and renderers do not depend
   on Prism Bloom, Ember Sigil, or Plasma Burst now cover editor behavior. Persistence, Library, and
   Session contracts use deterministic path-aware, textured, and purpose-built assets for
   serialization and filesystem workflows. Showcase effects remain only at the application bundle
   boundary for startup, bundled-content compilation, and native-GPU visual approval.
2. **Decompose Timeline and Properties internals — complete.** Preserve the existing `TimelinePlugin` and
   `PropertiesPlugin` public boundaries while splitting state/actions, region and automation
   interaction, referenced-effect presentation, module controls, renderer controls, and tests into
   focused internal modules. This is a behavior-preserving refactor guarded by the existing tests.
   Timeline semantic actions, state/view/navigation, emitter-region interaction, automation
   interaction, and referenced-effect presentation are now extracted with focused tests. Properties
   module actions, source authoring, card composition, referenced-effect navigation, repair,
   instance overrides, and nested read-only presentation are extracted. Renderer actions, card
   composition, input synchronization, and numeric scrub semantics now have focused ownership too.
3. **Reconcile roadmap status — complete.** Architecture and implementation documentation now
   distinguish the shipped GPU simulation/presentation, flipbook, reusable composition,
   exposed-parameter/instance-override, automation, choreography-event, artifact, and portability
   slices from the remaining mesh/ribbon/trail, authored sorting, quality-budget, release packaging,
   and semantic material-program work.
4. **Approve showcase content independently — complete.** Keep editable examples outside ordinary
   semantic unit-test assumptions. The Material 5 bundle-boundary contract migrates all three
   showcases in memory, validates the command result, compiles the effects and every generated
   program for the portable GPU ABI, and passes the native-GPU semantic reference comparisons.
   Intentional future visual changes still require the self-hosted native-GPU workflow.
5. **Audit the current material contract — complete.**
   [`material-system/current-state.md`](material-system/current-state.md) inventories semantic and
   compiled types, renderer relationships, WESL entry points, render state, commands, editor
   surfaces, compatibility fixtures, migration classification, and the first-slice test contract.
6. **Deliver the first material vertical slice — complete.** The repository-aligned semantic core,
   complete semantic validation, baseline transactional command layer, and typed backend-neutral
   material IR are implemented. The portable WESL/resource ABI and backend layout adapter are also
   complete. The live runtime bridge, non-destructive legacy migration transaction, animated
   additive-flame path, and native-GPU preview approval are complete. Reflection and dynamic
   parameter binding are the next slice; the node-graph projection remains deferred.

Exit gate: deterministic behavioral tests remain stable when showcase timing or playback changes;
Timeline and Properties have focused internal ownership; roadmap status matches the shipped code;
the three showcase effects pass format/compiler checks plus native-GPU approval; and the first
material slice has an agreed semantic contract and test plan. This gate is complete.

## Semantic material-program delivery milestone

This milestone tracks the implementation sequence defined in
[`aestra_material_authoring_architecture.md`](aestra_material_authoring_architecture.md). Preserve
the current sprite-material path until the native-GPU compatibility gate approves its replacement.

### Phase A — semantic foundation

- [x] **Material 0 — current-system audit.** The compatibility contract, fixtures, and migration
  classification are recorded in [`material-system/current-state.md`](material-system/current-state.md).
- [x] **Material 1 — semantic core types.** Stable program, parameter, and expression IDs;
  project/built-in references; effect-local instances; typed values; render-state policy; the
  semantic expression DAG; resource metadata; structural/program-aware validation; normalized RON;
  and project indexing are implemented. Material-program sources retain identity across create,
  rename, and move operations; duplicate, replaced, missing, and invalid program dependencies
  produce typed diagnostics while the legacy sprite-material path remains compatible.
- [x] **Material 2 — validation and baseline commands.** Deterministic type inference validates
  expression sockets, outputs, evaluation domains, material-domain capabilities, declared texture
  resources, and render-state policy. `aestra-authoring` now provides atomic add/remove/replace,
  output, expression rewiring, instance-parameter/render-state, and renderer-assignment commands
  with semantic diffs, stable identity checks, and bounded undo/redo.
- [x] **Material 3 — typed material IR.** Valid semantic programs lower deterministically into a
  typed backend-neutral SSA-like value graph. Bidirectional expression/value source mapping
  survives aliases and records eliminated expressions; authored sRGB constants become linear; and
  constant folding, trivial add/multiply simplification, and dead-value elimination run before the
  artifact is exposed. Invalid programs never reach lowering.
- [x] **Material 4 — WESL and resource ABI.** `aestra-gpu` generates inspectable,
  Naga-validated WESL/WGSL from typed IR; assigns deterministic 16-byte uniform slots plus stable
  multi-texture and descriptor-shared sampler bindings; emits parameter/input reflection and a
  visible missing-texture fallback contract; validates backend limits; and separates normalized
  program fingerprints from render-state/target/sample/feature pipeline keys. Ordinary instance
  values and texture asset IDs affect neither key. `aestra-bevy-render` maps physical device limits
  and the portable layout into Bevy/WGPU descriptors without introducing backend types upstream.
- [x] **Material 5 — legacy migration and visual approval.** The live 2D
  and 3D sprite pipelines accept compiled semantic fragment shaders, deterministic group-2 resource
  layouts, constant/default instance values, explicit samplers, missing-texture fallbacks, and
  portable pipeline keys while preserving the legacy compatibility draw. A deterministic,
  non-destructive command transaction now converts legacy sprite/flipbook presentation into
  semantic programs and instances, including tint/particle color, sampled alpha, UV rectangles,
  texture/flipbook resources, blend state, and the temporary softness-coverage adapter. The viewer's
  `--semantic-materials` path powers the scheduled showcase comparison. The native-GPU references
  approve this path with pixel-identical legacy/semantic showcase output; this is not a claim of
  CPU pixel parity.

### Phase B — useful artist workflow

- [x] **Material 6 — reflection and parameter binding.** Expose typed parameters, resources,
  evaluation domains, particle inputs, and scene requirements. Scoped effect/emitter parameters and
  deterministic instance/effect/emitter random ranges refresh without replacing their compiled
  program. Project compilation and artifact format 2 retain the semantic programs, instances, and
  binding descriptors; presentation now compiles and refreshes emitter-specific bindings
  automatically. The compiler now publishes a serializable, engine-neutral control catalog with
  typed controls, defaults/current sources, resource constraints, live vertex/particle/scene input
  requirements, and render-state policy. GPU reflection consumes the shared input classification.
- [x] **Material 7 — Properties material editor.** The completed vertical slice resolves a selected
  renderer's semantic instance/program, generates typed constant and random-range controls from
  compiler reflection, identifies effect/emitter bindings, offers texture assets, and submits edits
  through `SetMaterialInstanceParameter` into the editor's shared undo/redo history. Reflected
  source pickers now switch compatible controls between constants, random ranges, and exposed typed
  effect/emitter bindings while preserving useful values. Blend, depth-test, depth-write, and cull
  controls project only transitions allowed by the reflected render-state policy and commit through
  the same semantic history.
- [x] **Material 8 — VFX semantic primitives.** `PanUV`, `RotateUV`, `ScaleUV`, `Remap`,
  `Smoothstep`, `RadialMask`, `Dissolve`, `DissolveEdge`, `DepthFade`, and `SoftParticle` are complete vertical slices: authored programs retain their explicit typed sockets; validation
  reports socket-specific type errors; semantic commands rewire every socket with undo/redo;
  backend-neutral IR preserves each operation and source map; and generated portable shaders
  retain the semantic operation. Rotation uses radians around an explicit center; scale uses
  `center + (uv - center) * scale`. Remap supports scalar-to-vector promotion and extrapolation;
  degenerate input-range components resolve to the corresponding output minimum instead of
  producing NaN/Infinity. Smoothstep promotes scalar edges to the value shape, supports reversed
  edges, and resolves equal-edge components as a deterministic step rather than relying on
  backend-undefined behavior. Radial masks retain explicit UV, center, radius, softness, and invert
  sockets; clamp negative radius and softness to zero; and produce a deterministic hard boundary
  when softness is zero. Dissolve retains explicit source, threshold, edge-width, and invert
  sockets; clamps negative edge width to zero; and produces a deterministic hard cut when its edge
  width is zero. DissolveEdge reuses those typed sockets to produce a one-sided band that peaks at
  the threshold and fades across the edge width; inversion selects the opposite side, and zero
  width deterministically produces no edge. DepthFade compares linear view-space scene and fragment
  depth against an explicit fade distance, supports inversion, and uses a deterministic hard
  intersection test for non-positive distances. The Bevy adapter supplies a separate 3D depth
  prepass through a portable group-3 ABI and specializes single-sample/MSAA shaders; depth inputs
  are intentionally unavailable on the 2D presentation path. SoftParticle is the artist-facing
  alpha operation: it multiplies a typed source alpha by the same deterministic depth fade and
  reuses the existing prepass contract. Flipbook integration deliberately remains renderer-owned:
  timing, playback mode, and frame tables resolve the current atlas rectangle before the final
  coordinates cross the material ABI as `Uv0`. Semantic materials therefore sample sprite and
  flipbook renderers through the same typed texture operation without duplicating animation state
  in the expression graph.
- [x] **Material 9 — material stack.** The compiler
  deterministically projects reachable semantic operations into a source-to-output stack with
  stable expression IDs. Linear chains appear in Properties; branched or independent modifier
  chains explicitly fall back to an Advanced representation instead of implying an unsafe order.
  Safe reorder planning is now complete for direct, homogeneous chains: it reports only moves
  that preserve type/domain validity, keeps stable expression identities and storage order, and
  returns a full replacement that commits as one exactly reversible material transaction.
  Properties exposes those compatible before/after positions as actions while Advanced graphs
  remain non-reorderable. Compiler-planned add/remove actions now expose only type- and
  domain-compatible insertion edges, reconnect direct chains safely, and remove only the detached
  owned subgraph. A persisted disabled-expression list bypasses a modifier as a typed alias while
  retaining its stable ID and settings for lossless re-enabling. Project-program replacements are
  persisted atomically with stale-source conflict detection, refresh every consumer through the
  catalog, and participate chronologically in the editor's shared undo/redo stream. Selecting a
  modifier now opens a compiler-reflected inspector for its owned literal settings; numeric,
  vector, and boolean edits preserve expression identity and use that same validated transaction
  path. Compiler-owned UV Drift, Soft Dissolve, and Contrast Shape presets expose only compatible
  insertion edges, configure useful defaults, and commit their complete modifier chains as one
  validated undoable replacement. The stack therefore covers common flame, mask/shield, and
  dissolve authoring without requiring a node graph.

### Phase C — AI-first authoring

- [x] **Material 10 — advanced semantic commands/tool API.** Compose validated wrap, connect,
  preset, insertion, and extraction transformations from baseline commands. The first vertical
  slice now exposes `ApplyMaterialPreset` as a UI-independent request that compiler-plans a full
  preset, returns one validated baseline transaction plus its semantic diff, and is consumed by
  the Properties workflow without directly constructing or mutating stored material programs.
  `InsertMaterialOperation` now follows the same path and addresses insertion points with stable
  start/end or before/after-expression anchors instead of fragile stack indices; Properties
  captures those semantic anchors when presenting compatible modifier and preset choices.
  `ConnectMaterialExpression` unifies expression-input and program-output destinations, rejects
  missing sources or destinations before mutation, and validates socket type/domain compatibility
  through the same serializable transaction contract with socket-specific semantic diffs.
  `WrapMaterialExpression` now compiler-plans a default semantic modifier around one exact stable
  connection edge, verifies that the wrapper consumes the prior source and replaces only the
  requested destination, and rejects fan-out, non-primary, stale, or incompatible edges atomically.
  `ReplaceMaterialExpression` now replaces an expression kind while preserving its stable identity
  and every downstream connection. Its upstream references and resulting type/domain compatibility
  are validated as one undoable baseline transaction, so stale or invalid substitutions are atomic.
  `BindMaterialParameter` now addresses a stable material instance and program parameter with an
  explicit constant, effect parameter, emitter parameter, random range, or program-default source.
  It rejects stale and unexposed binding parameters plus incompatible types/domains before returning
  one baseline transaction with an exact instance-parameter diff. Function extraction is owned by
  Material 15 so it can target the canonical typed function model rather than a temporary format.
- [x] **Material 11 — AI material API.** Expose machine-readable inspection, editing, compilation,
  and diagnostics. The first read-only slice now resolves stable program or instance targets into a
  deterministic serializable report containing authored snapshots, reflected controls, stack
  projection, compiler-approved operation/preset insertion edges, and structured diagnostics.
  `MaterialCompilationReporter` uses those same targets to return deterministic backend-neutral IR
  with its expression source map and optimization statistics; invalid program or instance state
  returns scoped diagnostics with no misleading partial IR. `MaterialApi` now unifies inspection,
  non-mutating edit planning, and compilation behind serializable tagged requests and responses;
  stable error codes preserve structured validation diagnostics for tool clients. The ID-free
  `AddFresnelEdge` command proves the full contract by composing a typed Fresnel mask with constant
  or particle-age intensity, returning one previewable transaction, compiling to portable IR and
  WESL. Fresnel is reflected as a normal stack operation for single-root graphs; compositions with
  independent color and alpha modifier roots retain the explicit Advanced projection.

### Phase D — advanced human authoring

- [x] **Material 12 — read-only graph projection.** The compiler now projects every authored
  expression into a deterministic backend-neutral graph with stable expression identity, labeled
  typed input ports, evaluation domains, explicit color/alpha output nodes, generated links,
  disabled/unreachable state, validation diagnostics, and optional optimized-IR source-map aliases.
  Invalid programs remain inspectable with unresolved types instead of losing their authored
  topology. Material inspection and the serializable tool API expose the same projection. A
  dockable read-only Material Graph workspace resolves the selected emitter or renderer's semantic
  material, displays its complete topology and output roots, and synchronizes node selection with
  the existing Properties modifier inspector.
- [x] **Material 13 — editable graph.** Translate graph interaction into semantic commands. The
  first projectional editing slice is implemented: the workspace uses reusable Feathers node and
  typed-socket widgets, deterministic dependency-column layout, an anti-aliased GPU wire layer,
  compatible-target previews, and drag-to-reconnect through `ConnectMaterialExpression` plus the
  shared material undo/redo history. Its reusable graph viewport now provides an infinite
  viewport-space grid, zoom-stable wires, middle-mouse and Space-drag panning, cursor-anchored
  wheel zoom, persistent per-program views and node positions, collapsible nodes, and frame-all or
  frame-selection commands. Right-click or Tab now opens a searchable, pointer-anchored Add Node
  palette populated from compiler-approved semantic operations; it inserts or wraps at the nearest
  valid graph edge, participates in material undo/redo, and places/selects the resulting expression.
  Nodes now support clear Ctrl/Shift multi-selection, contextual and keyboard duplication/deletion,
  stable-ID duplication with internal links preserved, and compiler-validated bypass deletion.
  Connections can be selected directly and Delete resets them to a typed default through the same
  semantic history. Socket dragging is bidirectional: dragging an input temporarily detaches its
  existing wire for direct reconnection, while dropping either endpoint on empty space opens a
  type-filtered palette. Input-originated creation wraps and reconnects the requested edge;
  output-originated creation uses the semantic `CreateMaterialExpression` command to build a typed
  downstream node without modifying unrelated consumers. A compiler-owned node catalog now exposes
  constants, supported material inputs, program parameters, arithmetic and interpolation, UV,
  mask, depth, texture-sampling, and component operations to every authoring client. Palette
  choices are compiler-filtered for the active socket, general node creation is one validated
  semantic transaction, and scalar/boolean connection defaults plus all numeric constant
  components use reusable Feather controls with the shared material undo/redo history.
- [x] **Material 14 — graph layout metadata.** A versioned project-local editor-layout sidecar now
  persists material-graph viewport pan/zoom, stable expression and output-node positions,
  collapsed state, and preview visibility. The sidecar is keyed by semantic program/expression IDs,
  prunes deleted-expression entries, validates finite geometry on load, writes atomically, and is
  excluded from project asset discovery. Layout changes never dirty or recompile semantic material
  programs, and missing or invalid metadata safely falls back to generated layout.

### Phase E — reuse and extensibility

- [x] **Material 15 — typed material functions.** The reusable authoring slice is implemented:
  canonical stable function/input/output identities, typed function signatures, function-input
  and function-call expressions, normalized RON assets, project-local discovery and resolution,
  exact call-signature validation, missing-reference and recursion diagnostics, deterministic
  compiler inlining, and source-map aliases for authored calls. Project effect compilation erases
  function boundaries before handing semantic programs to the runtime, so render backends remain
  isolated from authoring assets. The compiler now registers a canonical built-in catalog; graph
  projection exposes signature-named, typed call sockets; and the categorized node browser offers
  built-in and project-local functions with validated creation, rewiring, undo, and diagnostics.
  The bundled Material Graph Lab exercises a built-in call and ships a project-local Dissolve Edge
  function. Connected graph selections can now be extracted from the context menu or with
  `Ctrl+Shift+E`: the authoring planner infers typed boundary inputs and outputs, absorbs inline
  constants, rejects disconnected or unusable selections, persists a project-local function,
  replaces the selection with stable call nodes, preserves the replacement layout, and treats the
  function asset plus graph rewrite as one rollback-safe undo/redo operation.
- [x] **Material 16 — semantic preset library.** The registry and project-asset slices are
  implemented. Presets use stable semantic IDs and portable descriptors containing a schema
  version (currently v2), category, description, and normalized search tags. Recipes may be ordered modifier
  stacks with editable defaults or typed, topologically ordered semantic graphs whose named local
  nodes can splice the current source and override color/alpha outputs without storing expression
  IDs. Project `.aestra.material-preset.ron` sources participate in the typed asset index,
  including validation, stable loading, duplicate-ID diagnostics, and deterministic merging with
  built-ins. UV Drift, Soft Dissolve, Contrast Shape, and Dissolve are built in; a project-local
  Hologram preset now exercises a genuinely branched Fresnel/particle-age graph through catalog
  loading → compatibility filtering → atomic graph materialization → transactional authoring →
  categorized editor presentation. Explicit catalogs are also available to machine-readable
  inspection and tool planning. A deterministic CPU preview pipeline now renders every compatible
  preset in an isolated canonical material, caches images by preset ID and normalized recipe
  fingerprint, and presents square previews with explicit incompatible/error fallbacks in both the
  categorized Library browser and stack insertion menu. The initial portable pack now includes
  Additive Flame, Soft Smoke, Energy Beam, Magic Shield, Dissolve, Hologram, Ghost, Portal, and
  Impact Flash. Every shipped recipe has preview, transactional undo/redo, compiler, and portable
  GPU shader coverage. Heat Distortion remains gated on a future scene-color/refraction contract;
  Trail belongs to the mesh/ribbon-domain work in Material 20 rather than pretending to be a
  sprite-only material preset.
- [x] **Material 17 — validated custom WESL escape hatch.** Material-function assets may now opt
  into a typed custom WESL implementation while retaining stable function/input/output identities,
  evaluation-domain reflection, project indexing, graph discovery, and semantic call nodes. The
  v1 sandbox admits only ordinary side-effect-free function declarations: bindings, resources,
  shader entry-point attributes, and imports are rejected structurally. Output-to-entry-point maps
  are validated, calls are type checked before lowering, function symbols are deterministically
  namespaced by stable ID, custom source lines map back to semantic IR values, and the final module
  is validated and translated through portable WGSL, SPIR-V, and HLSL targets. The bundled Pulse
  Wave example covers project loading, graph insertion, undo/redo, compilation, and portability.

### Phase F — next-generation tooling

- [x] **Material 18 — AI visual feedback loop.** `aestra-viewer` accepts explicit frame indices or
  times in addition to evenly spaced sampling, preserves deterministic 60 Hz seeks, and writes a
  versioned `preview-report.json` beside its individual frames and contact sheet. The report
  exposes compiler diagnostics and optimization counts, semantic material fingerprints, exact
  frame/time pairs, backend selection and compatibility, adapter budgets, and measured/estimated
  effect metrics with provenance. Visual tests retain per-frame RMSE, differing fraction,
  coverage, centroid drift, thresholds, worst-frame summaries, and diff paths in the same report
  even when thresholds fail. Preparation and capture failures remain machine-readable and return
  non-zero status. This keeps visual reasoning caller-owned while providing the deterministic
  edit/compile/render/analyze contract required by AI tools.
- [x] **Material 19 — advanced compiler optimization.** Deterministic common-subexpression
  elimination is complete for pure constants, inputs, parameters, and semantic operations. Add
  and Multiply canonicalize operand order. Implicit-derivative texture sampling now carries an
  explicit merge-safety contract: samples with identical texture and UV operands share one IR and
  shader operation, while different operands remain distinct. Custom WESL calls remain excluded
  until they carry explicit purity contracts. Aliased semantic expressions retain complete source
  mapping, and optimization plus authored/eliminated/live texture-sample counts flow through
  compiled effects, backward-compatible artifacts, the Compiler Inspector, and preview JSON.
  Explicit-LOD sampling is complete end to end: the shared graph catalog exposes a typed
  `Sample Texture Level` node, semantic validation requires a declared texture, `Vec2` UV, and
  Float level, IR and CSE identity include the level operand, and portable shaders emit
  `textureSampleLevel` with artifact round-trip coverage.
  Explicit-gradient sampling is also complete end to end: typed `Derivative X`, `Derivative Y`,
  and `Sample Texture Gradient` nodes validate fragment-local `Vec2` gradients, preserve both
  gradient operands in IR and CSE identity, round-trip through artifacts, and emit portable
  `dpdx`, `dpdy`, and `textureSampleGrad` shader operations.
  Shader-static parameter specialization is
  also complete at the IR boundary: typed program defaults replace static reads before dependent
  folding and CSE, while parameter metadata and fingerprint invalidation remain intact. Static
  `Select` branch pruning is now complete end to end: only the chosen branch is lowered, dead
  inputs, parameter bindings, texture samples, and custom calls are omitted from live reflection,
  and branch/feature counts flow through artifacts, the Compiler Inspector, and preview JSON.
  Function deduplication is complete for semantic functions: stable function references and
  resolved input bindings share one expansion namespace across calls and multiple outputs.
  Nested functions participate, custom WESL and its transitive wrappers remain excluded, and
  source mappings retain original calls or mark them eliminated. Resolved call-output-site,
  eliminated, and surviving invocation/output counts flow through effect compilation,
  backward-compatible artifacts, the Compiler Inspector, and preview reports. Nested sites are
  counted within shared expansions; eliminated also includes outputs removed by IR optimization.
  Varying minimization is complete: live IR inputs select a compact, deterministic interface
  shared by generated vertex and fragment stages, while coverage/visibility fields remain.
  Particle opacity reuses color alpha when both inputs are live. ABI/generator versions and
  layout fingerprints protect shader/pipeline caches. Shared sprite geometry preserves flipbook
  UVs, and legacy/wireframe entry points keep their own compact layout. Regression coverage
  includes static pruning, matched stage interfaces, portable translation, and native pipeline
  linking with single/multisampled depth. Required particle-attribute pruning is complete for
  native sprite presentation: optimized material inputs, legacy tint, flipbook time source, and
  wireframe needs determine per-renderer reads and per-emitter unions. Simulation skips unused
  gradient/opacity/geometry work and vertex shaders read only required particle members. Omission
  masks reuse emitter/renderer padding; the particle storage ABI remains 64 bytes with explicit
  defaults for omitted fields. Full artifact/readback paths retain CPU-reference data. Runtime
  bindings and mode changes refresh requirements after successful preparation, including retained
  bindings on failure. The Compiler Inspector displays a static rendered-mode estimate; native
  conformance covers pruned fields across Once, Restart, and Continuous playback. Physical buffer
  compaction and CPU-reference execution pruning are intentionally outside this optimization.
- [ ] **Material 20 — mesh and ribbon domains.**

The first release gate is the two-texture animated additive-flame slice: stable IDs and normalized
RON, command-only edits, deterministic resource layout and artifact round trip, native-GPU visual
approval, and ordinary instance updates without shader recompilation. A node graph is explicitly
outside this gate.

## Stability-hardening milestone

Complete this milestone before expanding the editor or renderer feature surface further.

1. **Correct native-GPU 3D view integration — complete.** GPU draws now consume Bevy's
   per-view visible-entity classes, preserving inherited visibility, render-layer, and
   frustum decisions. World-space bounds centers feed camera-relative transparent
   sorting with deterministic renderer-order bias. Focused tests cover per-view
   selection, transformed sort centers, depth ordering, and renderer tie-breaking; the
   `--editor-viewport-smoke` capture remains the acceptance gate for constrained preview
   and overlay-camera changes.
2. **Harden recovery cleanup — complete.** Active recovery snapshots remain tracked until
   deletion succeeds, with throttled retries after transient filesystem failures.
   Cleanup is driven by the persisted snapshot rather than a revision marker, and
   document switches wait for the previous snapshot to be removed. Focused tests cover
   failed deletion and retry, save, document switch, startup discard tracking, and
   autosave-disable behavior.
3. **Make settings replacement crash-safe — complete.** Settings now stage into unique,
   same-directory temporary files and atomically replace the canonical file without first
   moving it away. File contents are synchronized before replacement, directory metadata
   is synchronized where supported, and startup recovers interrupted legacy replacements
   or initial writes. Tests cover unique staging, canonical-file continuity, legacy and
   first-write recovery, protected malformed/newer files, and repeated replacements.
4. **Remove multiplicative renderer work — complete.** GPU compaction now writes each
   emitter into its own bounded alive-index segment and maintains one indirect command
   per emitter. Renderers share only their emitter's command and segment, so particles
   from unrelated emitters are no longer submitted and discarded for every draw.
   Profiles expose submitted instances alongside live particles, the buffer estimate
   includes the indirect command table, focused tests cover command/range isolation,
   and the native editor-viewport GPU smoke remains the visual acceptance gate.
5. **Decompose the editor application — complete.** The timeline, viewport,
   properties, and docking lifecycle are extracted domain plugins with explicit
   system-set boundaries. `DockingPlugin` owns persisted layout loading, transient
   drag/resize state, drop-affordance reconciliation, and native floating-window
   synchronization. The
   properties owns module-stack construction, semantic property controls, numeric scrub
   transactions, renderer fields, focus navigation, contextual help, and focused tests.
   Recursive main-window dock construction and revision-aware native floating-panel
   reconstruction now run behind `DockingPlugin`; the editor shell declares only a
   required transparent dock host. The reusable Bevy Feathers layer is now extracted
   into `src/feathers/`, with one plugin owning the upstream Feathers setup, theme,
   activation auditing, and scrollbar synchronization. Action buttons, combo/action
   menus, compact field rows, numeric scrub policy, panel headings, BSN scenes,
   separators, status surfaces, and scroll areas no longer live in `main.rs`. Menu chrome,
   popup behavior, delayed submenu opening, panel visibility synchronization, and tab
   context menus now run through `EditorMenusPlugin` with their own action contract and
   focused tests. Settings workspace construction, category navigation, locale/reset
   actions, Feathers value observers, and live preference application now run through
   `EditorSettingsUiPlugin`, with focused activation and constraint tests. Document creation/opening/saving,
   application exit, window-close confirmation, recovery discovery, autosave, and
   cleanup now run through `EditorPersistencePlugin` and a shared `DocumentAction`
   contract used by menus, keyboard shortcuts, and project-effect rows.
   Project-effect discovery, Assets workspace construction, material and flipbook creation,
   semantic layer selection, emitter creation/duplication/reviewed deletion, related keyboard
   shortcuts, and selection styling now run through `EditorAssetsPlugin`. Assets-panel and
   Edit-menu controls share one `AssetsAction` contract; catalog rows still emit
   `DocumentAction` so the Assets plugin cannot bypass persistence safeguards.
   The Curves workspace, curve/gradient selection, Properties navigation contract, graph
   interaction, and key controls now run through `EditorCurvesPlugin`. Its actions retain
   the existing semantic command path, including one-history-entry key edits and undo/redo.
   The Diagnostics workspace, filter state, semantic-source navigation, compile-status
   synchronization, and footer entry point now run through `EditorDiagnosticsPlugin`;
   validation and compilation remain owned by `EditorSession`.
   The advanced Compiler Inspector, artifact formatting, and semantic navigation now run
   through `EditorCompilerInspectorPlugin`. New layouts keep it hidden, while a Serde
   alias preserves the placement of legacy `GeneratedCode` tabs.
   Runtime profile ingestion, aggregation, bounded history, reset actions, presentation,
   and UI synchronization now run through `EditorProfilerPlugin`. Viewport evaluation
   submits a borrowed `ProfilerFrameSample`, keeping the boundary explicit without cloning
   compiled effects or particle buffers.
   Pending transaction review now runs through `EditorChangesPlugin`. The plugin owns the
   Changes workspace, apply/discard actions, and navigation from semantic diff rows back to
   live Properties targets, while `EditorSession` remains the transaction and history owner.
   Preview playback now runs through `EditorTransportPlugin`. Toolbar, Timeline, View-menu,
   and keyboard controls share one `TransportAction` contract; the plugin owns playback
   mutation, clock advancement, Feathers activation, shortcuts, and play/pause icon sync.
   Timeline framing, snapping, and effect-duration controls now run through `TimelinePlugin`.
   Timeline buttons and combo options share one `TimelineAction` contract, while transport
   stepping and seed changes continue to use `TransportAction`.
   Viewport grid visibility, effect framing, gizmo modes, and rendered/wireframe display now
   run through `ViewportPlugin`. View-menu items, viewport tools, and keyboard shortcuts share
   one `ViewportAction` contract, including persisted grid preference updates.
   Undo and redo now run through `EditorHistoryPlugin`. Edit-menu controls and keyboard
   shortcuts share one `HistoryAction` contract, and the plugin owns history execution plus
   menu availability synchronization without touching unrelated UI entities.
   Panel selection, visibility, floating, and workspace reset commands now use the
   `DockingAction` contract owned by `DockingPlugin`; tab, context-menu, View-menu, drag/drop,
   reorder, and floating-window outcomes use localized panel names and status messages.
   `EditorLocalizationPlugin` now owns Fluent resource setup and live generic-text
   synchronization. All primary deep workspaces use complete `en-US` and `fr-FR`
   messages for editor-owned presentation; technical compiler detail, generated
   instructions, semantic paths, IDs, file paths, and asset-authored names remain
   unchanged. Document creation, open/save, recovery, autosave, settings persistence,
   unsaved-change prompts, and lifecycle cancellation now produce structured outcomes
   localized by `EditorPersistencePlugin`; domain session methods no longer author their
   UI prose. Properties controls now emit a dedicated `PropertiesAction` contract;
   `PropertiesPlugin` owns module-palette navigation, module and renderer mutations,
   renderer configuration, persisted disclosure state, and localized validation/status
   outcomes. Root chrome construction, About actions, global document shortcuts, editor
   labels, font setup, and revision-aware content rebuilds now run through
   `EditorShellPlugin`. Scroll restoration remains part of that rebuild lifecycle, while
   `main.rs` is reduced to application composition, cross-plugin ordering, and startup
   configuration.
6. **Establish automated quality gates — complete.** Hosted Windows CI runs formatting,
   workspace checks, strict Clippy, and tests. A separate scheduled/manual workflow targets
   a self-hosted Windows runner labeled `gpu`, validates editor viewport composition and all
   approved effect references on the native backend, and uploads captures and reports even
   when validation fails.
7. **Refresh contributor documentation — complete.** The README documents local quality
   gates and GPU-runner requirements. The assessment below is retained explicitly as the
   historical prototype baseline that motivated the implemented semantic/compiler
   architecture.

## 1. Historical prototype assessment

This assessment records the original vertical slice. It is retained as historical context;
the milestone status and architecture sections above describe the current implementation.
The prototype already proved several useful contracts:

- three top-level products: `aestra-editor`, `aestra-bevy`, and `aestra-viewer`;
- a versioned RON effect asset;
- deterministic CPU evaluation;
- Bevy playback through `AestraPlugin` and `EffectPlayer`;
- an editor session with selection, file workflows, undo, redo, and preview;
- a standalone viewer and visual capture workflow.

It is not yet the semantic/compiler architecture described by the evolution
document:

- `aestra-bevy` currently owns the authored model, evaluator, persistence, and
  Bevy adapter in one crate;
- `EffectAsset`, compiled data, and runtime instance state are not distinct;
- emitter behavior is a fixed Rust struct rather than an ordered stage/module
  program;
- IDs are user-facing strings and some references use vector indices;
- validation returns one enum error rather than a structured diagnostic report;
- editor history stores whole-document snapshots around unrestricted closures,
  not serializable semantic commands and atomic transactions;
- the editor and viewer call the evaluator directly; there is no compiler IR;
- renderer declarations exist, but simulation, assets, materials, and renderer
  instances do not yet have independent models.

The prototype's visual behavior should be preserved as a conformance reference,
but its format-v1 schema is not retained.

## 2. Build-order decisions

1. AI is deferred until the semantic model, commands, transactions, diagnostics,
   compilation, and preview APIs work without a UI.
2. The visual node graph is a projection of semantic data. It is never the
   canonical saved effect.
3. Group executable products under `apps/`, keep engine integration adapters under
   their engine boundary (`bevy/` today), and put reusable internal libraries under `crates/`.
4. Do not create every future crate immediately. Extract a crate when its public
   boundary is needed and tested.
5. Keep the CPU evaluator as the conformance oracle while introducing a compiler
   and GPU backend.
6. Replace the planar prototype formats with native-3D format v3. Only v3 is supported; no legacy
   parser or migration layer is maintained.
7. Implement commands immediately after the v3 semantic foundation. Although the
   source document lists commands later in one revised priority list, commands do
   not depend on the GPU runtime and are necessary to make subsequent editor
   changes safe and automatable.

## 3. Target repository boundaries

```text
apps/
  aestra-editor/               Bevy UI authoring product
  aestra-viewer/               Playback, capture, and analysis product
bevy/
  aestra-bevy/                 Public Bevy game-runtime integration
crates/
  aestra-core/                 Semantic source model, IDs, values, diagnostics
  aestra-project/              Project asset index, typed references, source resolution
  aestra-compiler/             Validation, lowering, IR, optimization, artifacts
  aestra-runtime/              Runtime contracts and deterministic CPU backend
  aestra-authoring/            Commands, transactions, history, diff, locks
  aestra-graph/                Custom module/subgraph language, added when needed
  aestra-ai/                   Optional AI client layer, added only at its milestone
```

Dependency direction:

```text
aestra-core
    ^
    +-- aestra-authoring
    +-- aestra-project
    +-- aestra-compiler ----> aestra-runtime ----> aestra-bevy
    +-- aestra-graph -----------^

aestra-editor ----> project + authoring + compiler + runtime/Bevy preview
aestra-viewer ----> compiler + runtime/Bevy playback
```

No internal semantic crate depends on Bevy UI. Bevy-specific asset loaders, data
interfaces, render extraction, GPU resources, and ECS components remain in
`aestra-bevy`.

## 4. Delivery milestones

### M0 — Freeze the reference behavior

Goal: make the current prototype a safe baseline for refactoring.

Deliverables:

- architecture decisions for terminology, IDs, the format reset, and crate
  boundaries;
- a golden v3 asset and deterministic evaluator snapshots at selected times;
- save/load, evaluator determinism, and viewer capture smoke tests;
- performance baseline for the bundled example;
- explicit statement that `EffectAsset`, `CompiledEffect`, and `EffectInstance`
  are different types with different lifecycles.

Exit gate: the current sample can be compared mechanically with every subsequent
runtime implementation.

### M1 — Semantic core and format v3

Goal: define a complete authored effect independently of Bevy and editor layout.

Start `crates/aestra-core` with:

- typed stable IDs for effects, emitters, modules, renderers, curves, parameters,
  events, materials, and referenced assets;
- `EffectAsset` containing parameters, emitters, events, dependencies, metadata,
  and quality profiles;
- emitters with an explicit simulation domain, ordered module instances, and
  zero or more renderer instances;
- explicit stages such as emitter spawn/update and particle spawn/update;
- open `ModuleTypeId` and `RendererTypeId` values rather than a permanently
  closed enum of all possible modules;
- typed values, parameter bindings, curves, gradients, and custom particle
  attributes;
- editor layout metadata in a separate document keyed by semantic IDs;
- deterministic serialization and explicit format-version rejection;
- structured diagnostics with severity, code, message, semantic path, and optional
  remediation.

Initial built-in semantic set:

```text
Particle Spawn: Shape, InitialLifetime, InitialVelocity, InitialRotation
Particle Update: Gravity, Drag, Turbulence, ColorOverLife, SizeOverLife,
                 OpacityOverLife, Integrate
Renderer:        SpriteRenderer
```

Exit gate: a v3 effect can be created, validated, serialized deterministically,
loaded without Bevy, and can represent the current bundled example.

### M2 — Deterministic authoring operations

Goal: make every meaningful change callable without simulating UI input.

Start `crates/aestra-authoring` with:

- `EffectCommand` for add/remove/move/enable operations, value changes,
  connections, renderer changes, and curve edits;
- `EffectTransaction` with all-or-nothing validation and rollback;
- `CommandExecutor` and bounded undo/redo history;
- semantic selection and lock state based on typed IDs;
- structured semantic diffs suitable for UI review and tests;
- command preconditions so stale IDs, type mismatches, and locked targets fail
  without mutating the document.

Refactor `EditorSession` to own document, editor metadata, selection, locks,
history, diagnostics, and preview state. UI systems may read session state and
submit commands, but may not mutate the effect directly.

Exit gate: editor edits, tests, and a small CLI test harness all use the same
commands; multi-command failure leaves the document unchanged; undo and redo are
tested across structural and parameter edits.

### M3 — Module registry, compiler frontend, and CPU runtime

Goal: compile semantic effects into an immutable executable representation.

Start `crates/aestra-compiler` and `crates/aestra-runtime` with:

- a module registry exposing category, stages, typed inputs/outputs, read/write
  attributes, tags, capability requirements, and approximate cost;
- graph validation for stage compatibility, bindings, dependencies, cycles, and
  required attributes;
- a typed IR representing values, attributes, parameters, stages, math, and
  renderer dependencies;
- lowering from built-in module instances into IR;
- initial constant folding and dead-attribute elimination;
- `CompiledEffect` with particle layout, compiled curves, execution plans,
  renderer plans, and source-to-IR diagnostic mapping;
- `EffectInstance` with time, seed, transform, parameter overrides, active
  emitters, and runtime allocation handles;
- a deterministic CPU interpreter for the initial IR.

Exit gate: format-v3 examples compile and match the frozen CPU reference within
defined tolerances; editor and viewer preview only through compile/instantiate/
update APIs.

### M4 — First GPU production slice

Goal: prove the architecture with one complete, professional-looking GPU path.

Implement in `aestra-bevy`:

- Bevy asset loading and compiled-effect caching;
- particle storage derived from compiler attribute liveness;
- bounded allocation, spawn, update, kill, and compaction;
- seeded Particle Spawn and Particle Update compute passes;
- one additive/alpha sprite renderer with typed material bindings;
- explicit bounds and basic culling;
- indirect dispatch/draw where supported, with a documented fallback;
- conformance tests against the CPU interpreter and a headless render smoke test;
- the same runtime path in games, editor preview, viewer, and capture mode.

Shader policy: Aestra authors and tests WESL modules. Bevy's WESL loader performs
the final lowering required by wgpu; checked-in runtime shader sources are not WGSL.

The demo effect for this milestone should exercise a timed burst, continuous
emission, forces, curves, gradients, at least two emitters, and multiple sprite
renderers without relying on product-specific examples.

Exit gate: the GPU path renders the demo deterministically for a seed, stays
within authored particle bounds, reports unsupported capabilities, and visually
matches the CPU reference closely enough for approved image snapshots.

### M5 — Professional authoring workflow

Goal: expose the semantic model efficiently to technical artists.

Deliver:

- effect hierarchy and searchable module stack;
- properties generated from module metadata;
- timeline, curve, diagnostics, compiler-properties, and profiler tabs;
- node graph as an optional projection for dataflow-heavy authoring;
- preview transaction, semantic diff, accept/reject, and undo;
- semantic viewport selection and gizmos that submit normal commands;
- autosave/recovery and explicit asset migrations;
- a dockable Settings workspace backed by a versioned, persistent editor-settings
  document stored separately from effect assets and workspace layout;
- editor localization through Fluent message catalogs, including runtime locale
  switching, fallback, and localized diagnostics without translating asset data;
- fixed-step preview, restart, frame step, seeded playback, and checkpoint-based
  backward scrubbing.

Exit gate: the initial demo can be authored from an empty effect without editing
RON, all mutations are undoable, and invalid operations produce targeted
diagnostics without damaging the document.

The blank-to-multi-emitter save/reload/compile portion of this gate is automated in
`aestra-editor`. Typed, confirmed, backup-preserving v2→v3 asset migration is complete;
the optional node-graph projection remains.

Before M6 semantic composition begins, the editor-only professional UI foundation is
specified in `aestra_ui_pre_m6_implementation_plan.md`. It separates the project Library
from current-document resources, moves emitter hierarchy/actions into choreography track
headers, adds searchable catalog state and synchronized timeline overflow, and changes no
effect format, compiler, runtime, or migration contract. This foundation is complete:
shared compact-list and search widgets constrain narrow layouts, Library and Timeline use
keyboard-accessible semantic rows, unsupported project-effect drops provide localized
non-mutating feedback, and automated blank/current-document composition coverage protects
the pre-M6 boundary. M6 can therefore begin at the project asset index/resolver rather
than extending editor-local catalog or emitter semantics.

A follow-up choreography polish slice makes emitter names directly editable in Timeline
track headers and persists an optional emitter display color. The color is edited directly
from each track's anchored Bevy Feathers color picker, including RGB/HSL channels, alpha,
editable RGBA hex, and live track preview. Both edits use semantic commands with undo/redo;
Properties and Timeline therefore project the same name state. The color is
backward-compatible authoring metadata and is deliberately ignored by the compiler and runtime.

Persistent settings slice:

1. Define a serde-defaulted `EditorSettings` model with an explicit format version.
2. Store it as `settings.ron` in Aestra's platform configuration directory, separate
   from `editor-layout.ron`, using atomic temporary-file replacement.
3. Add a Settings command and dockable Settings panel with General, Preview,
   Performance, Capture, Appearance, Language, and Keybindings categories.
4. Apply reversible settings live where possible; clearly mark restart-only values.
5. Preserve unknown/newer files without destructive rewrites, recover malformed files
   to defaults with diagnostics, and test defaults, round trips, and persistence.

Localization slice:

1. Introduce a small editor localization service backed by Fluent bundles and stable,
   semantic message identifiers; UI code must not use visible English text as keys.
2. Ship an embedded `en-US` catalog as the complete fallback and load additional locale
   catalogs from editor resources without coupling them to effect assets.
3. Persist the selected locale in `EditorSettings`, allow live switching from the
   Settings panel, and fall back from region to language and then `en-US`.
4. Localize menus, commands, panels, tooltips, status text, validation messages, and
   variable interpolation while leaving user-authored names, paths, and code untouched.
5. Add catalog validation and tests for missing or malformed messages, fallback,
   interpolation, and coverage of every registered editor command and panel title.

The Fluent runtime, English fallback, French catalog, live persisted switching, shell,
Properties, Assets, Timeline, Curves, Diagnostics, Compiler Inspector, Profiler, and Changes
coverage are complete. Incidental action/status messages migrate alongside their owning
plugins; user-authored names, file paths, semantic IDs, and generated code stay
untranslated.

### M6 — Renderer, material, and asset breadth

Goal: move from a sprite particle tool to effect choreography.

Add incrementally:

- independent sprite, texture, mesh, flipbook, and material asset IDs/registry;
- typed material inputs and attribute/parameter bindings;
- multiple renderers per emitter;
- mesh instancing, ribbons/strips, flipbooks, and renderer-controlled sorting;
- procedural mesh and sprite recipes before external generation;
- effect dependencies, child effects, parameter forwarding, and event routing;
- modular collision and event outputs.

Each renderer ships with compiler validation, CPU/reference semantics where
meaningful, GPU tests, profiler counters, and a viewer example.

### M7 — Scale, profiling, and platform quality

Goal: make runtime cost visible and controllable.

Deliver:

- per-effect/emitter/module counters and GPU timing where available;
- debug attribute/event inspection;
- renderer batching, culling, bounds, sorting, and indirect draw refinements;
- quality tiers and compiler-visible platform capability profiles;
- a global VFX budget manager with deterministic degradation decisions;
- performance regression scenes and CI thresholds on a defined reference target.

Exit gate: an effect can declare quality behavior, the runtime enforces aggregate
budgets, and the editor explains both estimated and measured costs.

### M8 — Stable tool API and AI transactions

Goal: let scripts and AI use the same safe public operations as the editor.

Expose versioned tools for inspection, discovery, commands, validation,
compilation, profiling, preview, and semantic diff. First test the API through
Rust integration tests and scripts. Then add `crates/aestra-ai` as an optional
client that:

1. inspects context and selection;
2. proposes a typed transaction;
3. validates and compiles it in a temporary document;
4. produces a semantic diff and preview;
5. commits only after acceptance.

Provider SDKs, local models, retrieval, MCP, multimodal input, and external asset
generation are later adapters. None may define or bypass Aestra semantics.

Exit gate: a provider can be removed entirely and all editor/runtime workflows
still function; rejected or invalid AI proposals never modify the working asset.

## 5. First implementation slice

The first build slice should end after M2 and be delivered in small, reviewable
changes:

1. Add architecture decisions and golden reference-behavior tests.
2. Create `aestra-core` with typed IDs, diagnostic types, and deterministic value
   primitives.
3. Add the minimal v3 `EffectAsset`, stage, module instance, and sprite renderer
   model.
4. Replace the example with format v3 and add golden serialization tests.
5. Create `aestra-authoring` with commands, atomic transactions, selection, locks,
   history, and semantic diff.
6. Refactor `EditorSession` to submit commands while retaining the existing UI.
7. Move persistence and semantic validation out of `aestra-bevy`.
8. Update editor/viewer loading so both accept and save only v3 assets.

This slice intentionally does not redesign the UI, generate WESL, add an LLM, or
implement every future module.

## 6. Verification required at every milestone

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test --workspace`
- stable serialization fixtures and unsupported-version tests
- structured validation snapshots
- deterministic seed/time tests
- CPU/compiler conformance tests once the IR exists
- headless GPU and image regression tests once the GPU backend exists
- viewer capture smoke tests for every public example
- no dependency from semantic crates to Bevy UI or an AI provider

## 7. Planning rules

- A milestone is complete only when its exit gate is automated where practical.
- New semantic features require commands, diagnostics, serialization, migration
  consideration, compiler support, runtime support, and an example.
- New renderer/runtime features require explicit capability and fallback behavior.
- Experimental advanced features stay behind narrow interfaces until the P0 and
  P1 acceptance criteria in the source plan are satisfied.

## 8. Docking design reference

The `jackdaw_panels` crate was reviewed as a Bevy-native reference. Aestra adopts its
most useful architectural ideas without copying its broader editor feature set:

- compose docking as a plugin with named scheduling phases instead of registering
  unrelated systems from the application root;
- keep a serializable layout model distinct from transient drag, hover, resize, and
  spawned-entity state;
- reconcile visible dock entities and native windows from that model after structural
  changes;
- keep tabs, splits, drag/drop, and window synchronization as separable concerns even
  while they share one public plugin boundary.

Jackdaw's workspace tabs, sidebars, and add-window popup are intentionally deferred:
Aestra currently needs one VFX workspace and already restores panels from the View
menu. Its descriptor/registry approach becomes worthwhile when third-party panels are
supported; until then the closed `DockPanel` enum gives exhaustive behavior and simpler
persistence migrations.
- The source plan's large checklists are product acceptance criteria; this file's
  milestones are the implementation order.

## 9. Editor Feathers design reference

Jackdaw's current `main` branch was reviewed after its Bevy 0.19 migration. Aestra
adopts the architectural ideas that improve consistency without importing editor
domain assumptions:

- keep every reusable editor widget below one Feathers plugin and source folder;
- expose data-driven option/props types instead of rebuilding combo and field-row
  structure at each call site;
- use a stable label/control column that wraps on narrow panels;
- preserve collapse and scroll state by semantic keys rather than transient entities;
- build on Bevy Feathers focus, accessibility, menus, text editing, and theme tokens;
- keep numeric scrub mechanics independent from the semantic command committed by
  the owning Properties field.

The first slice is implemented in `apps/aestra-editor/src/feathers/`. Its widgets cover
Aestra's current shared controls. The following Jackdaw primitives become useful as
the corresponding Aestra feature lands, rather than being copied unused:

1. add swatch rows when continuous color authoring expands;
2. add list/tree primitives for the future searchable content browser;
3. evaluate Jackdaw's Bevy 0.19 scrub input as a replacement for the remaining
   Properties-owned pointer state after its semantic preview/commit adapter is isolated;
4. add dialogs, toasts, progress, and file-browser widgets only when those workflows
   need in-editor non-native surfaces.

The generic tooltip slice is complete: Properties and viewport help now share delayed,
popover-based content with optional titles, shortcuts, and footers. Text remains localized at
the call site, and parenting the popup to the hovered control keeps placement scoped to the
correct native window.

The remembered panel-card slice is also complete. Module and renderer cards share one Feathers
composition for disclosure, compact spacing, help, accessibility, and domain-owned header/body
slots. Expansion preferences remain in editor settings under stable module or renderer type keys,
so UI rebuilds and equivalent instances restore the same state without persisting ECS entities.

The bounded scalar slice is complete. Metadata-defined scalar ranges, normalized UV bounds, and
flipbook frame rate use a shared Feathers slider plus a precise numeric input. Slider motion updates
the compiled preview continuously, release commits one semantic undo command, and the final commit
keeps the Properties tree intact so scroll and disclosure state do not jump. Unbounded transforms,
vectors, and ranges remain numeric-only because an arbitrary slider range would misrepresent them.

Dock tabs, splitters, timeline clips, curve keys, and viewport gizmos remain specialized
Aestra controls. They consume the shared widget and theme layer but are not generic
Feathers primitives.
