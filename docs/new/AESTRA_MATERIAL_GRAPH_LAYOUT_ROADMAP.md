# Aestra Material Graph Layout & Interaction Roadmap

> **Status:** M0 contracts, M1 live geometry and M2 measured framing implemented, 2026-09-13; M3 is next. No automatic movement implemented.
> **Repository reviewed:** `thehiddenworkshop/aestra`, including the material/function graph and multi-view changes after the original 2026-09-10 audit.  
> **Scope:** Shared material/function graph layout, manual positioning, explicit arrangement, dynamic node sizing/previews, incremental placement, and AI-authored graph changes.

**Review outcome:** retain the hybrid interaction model. Before automatic movement is
exposed, establish coordinate units, document/view ownership, base-position persistence,
temporary-offset composition, and presentation Undo. M0 contract coverage, M1 live
geometry and M2 measured framing are implemented; the next task is M3 base placement,
persistence and presentation Undo.
The first automatic-movement feature
is the bounded preview-resize flow through M5. No layout dependency or automatic
movement is introduced by M0–M2.

The [M0 graph-layout contract audit](../material-system/graph-layout-contract.md) records
current material/function behavior, regression coverage and gaps GL01–GL08 with milestone
owners. Its target contracts are not claims that the corresponding features exist yet.

Priority: M0–M3 are P0 correctness prerequisites for this track; M4–M7 are P1 editing
improvements; explicit arrangement and subsequent polish are P2. These are not claims
that the existing editor is broken in every listed area.

Saved searches/filters (AB9f) and collections remain deferred by user decision; this is
a separate graph-layout track, not another Asset Browser milestone.

## 1. Executive summary

Aestra should **keep manual node placement**. Automatic layout should be an assistive operation, not a permanent constraint that continually snaps the graph back into an engine-generated arrangement.

Recommended interaction model:

```text
Free manual placement
        +
snap/alignment assistance
        +
smart local insertion
        +
local collision resolution on resize
        +
incremental placement for new/AI nodes
        +
explicit Tidy / Arrange Selection / Arrange Branch / Arrange Graph
```

For full or targeted automatic arrangement, Aestra should prototype **`elkrs` with ELK's layered/Sugiyama-style algorithm**.

However:

> **`elkrs` should be a layout service used by Aestra, not the owner of graph state or interaction policy.**

Aestra should own:

- measured node geometry;
- manual positions;
- pinning;
- local collision handling;
- incremental placement;
- layout history;
- persistence;
- semantic graph hints;
- the decision about when automatic layout is allowed.

The semantic `MaterialProgram` must remain completely independent from graph presentation.

---

# 2. Current Aestra state

Aestra already has the correct high-level separation between material semantics and editor presentation.

Important current files:

```text
apps/aestra-editor/src/material_graph.rs
apps/aestra-editor/src/feathers/node_graph.rs
crates/aestra-project/src/editor_layout.rs
apps/aestra-editor/Cargo.toml
docs/AESTRA_SEMANTIC_MATERIAL_AUTHORING_ROADMAP.md
```

The current material architecture already establishes:

```text
MaterialProgram = canonical semantic representation
Material Graph  = projection / authoring surface
WESL/WGSL       = generated backend representation
```

This should not change.

## 2.1 Graph presentation is already separate from material semantics

`ProjectEditorLayout` currently persists material graph editor state separately from material assets.

The project-local layout file is:

```text
.aestra/editor-layout.ron
```

Material graph layout metadata already includes concepts such as:

- viewport pan;
- viewport zoom;
- expression positions;
- collapsed state;
- output-node position;
- visible expression previews;
- output-preview visibility.

This is exactly the correct architectural direction.

Moving a graph node must never modify `MaterialProgram`.

## 2.2 Current function and multi-view support

Function graphs already use the shared Feathers graph widget through
`apps/aestra-editor/src/material_function_editor/graph.rs`. They are in scope from M0;
do not postpone their compatibility until compound/group support.

The current persistence schema has `material_graphs` keyed by `MaterialProgramId`,
whereas function graph positions use function-scoped in-memory keys. Function layout
persistence therefore requires an explicit, backward-compatible schema extension; it
must not be assumed to exist already.

Material node placement is shared between views of a document, while material
viewports can have independent camera keys. The geometry registry must distinguish
document/node identity from the concrete view that supplied a measurement. Existing
string keys are adapter inputs to audit, not proof that all project/view cases are safe.

## 2.3 Manual node placement already exists

The reusable node graph code maintains `GraphViewportMemory`.

Conceptually:

```text
GraphViewportMemory
    ├── viewport pan / zoom
    └── per-node
         ├── position
         └── collapsed
```

Dragging a node currently updates its graph position directly and records it in viewport memory.

That freeform behavior should remain the default.

## 2.4 Actual UI node dimensions already exist

The graph UI is built with Bevy UI and has access to `ComputedNode`.

Aestra already uses computed UI sizes for live node bounds when framing graph content.

This is an important foundation: the new layout system does not need a second UI measurement framework.

Instead, promote this into an explicit:

```text
GraphGeometryRegistry
```

which stores the actual measured rectangle of every graph node.

## 2.5 Material previews already resize nodes

Current graph constants include approximately:

```text
NODE_WIDTH        = 224
NODE_PREVIEW_SIZE = 208
```

The current fallback material layout estimates extra node height when a preview is visible.

The preview content is added after the principal port/control area. Keep this behavior because expandable content should ideally not move existing sockets.

## 2.6 A visible grid already exists

The reusable graph viewport already renders a grid at roughly:

```text
32 px spacing
```

Node dragging currently does not appear to use this as a snapping constraint.

This makes grid/alignment assistance a natural improvement without changing the semantic model.

---

# 3. Current fallback layout

The fallback layout in `material_graph.rs` is currently a simple depth-column algorithm.

Approximately:

```text
1. Collect expression dependencies.
2. Compute dependency depth.
3. Group expressions by depth.
4. Place each depth in a fixed-width column.
5. Stack nodes vertically within each column.
6. Place the output after the deepest expression.
```

Current constants include values approximately like:

```text
COLUMN_WIDTH   = 282
CANVAS_PADDING = 34
NODE_GAP       = 22
```

Conceptually:

```text
Depth 0        Depth 1        Depth 2

Texture ─────► Sample ───────► Multiply
UV ──────────►
Color ───────────────────────►
```

This is a good bootstrap layout, but it is not sufficient as a professional material-graph layout system.

## 3.1 Main limitations

The current fallback:

- does not minimize edge crossings;
- uses predicted node height rather than live measured geometry;
- cannot robustly react to arbitrary dynamic content;
- does not preserve spacing automatically when previews expand;
- does not provide targeted selection/branch arrangement;
- has no explicit incremental placement policy for AI-created nodes;
- has no pinning concept;
- has no distinction between small "tidy" corrections and full topology-based layout.

---

# 4. Core UX rule

> **Automatic layout must preserve user intent whenever possible.**

A graph is not only computation. It is also a spatial document.

Artists and technical users communicate meaning through:

- proximity;
- alignment;
- rows;
- whitespace;
- branch separation;
- comments/groups;
- hand-chosen placement.

A layout engine must not continuously erase this information.

---

# 5. Recommended behavior matrix

| Event | Aestra behavior |
|---|---|
| Drag node | Free manual positioning |
| Drag near grid/alignment | Soft snapping/guides |
| Drag node onto compatible wire | Smart insertion |
| Create connected node | Place near semantic neighborhood |
| Create disconnected node | Place at cursor |
| AI creates node | Incremental semantic placement |
| Preview opens | Resize in place, resolve local overlap |
| Preview closes | Restore temporary displacement when safe |
| Node collapses/expands | Treat as local resize |
| Content changes node size | Local geometry reconciliation |
| `Tidy` | Fix small spacing/alignment problems |
| `Arrange Selection` | Auto-layout selection only |
| `Arrange Upstream` | Auto-layout upstream branch |
| `Arrange Downstream` | Auto-layout consumer branch |
| `Arrange Graph` | Full layered auto-layout |
| Explicitly pinned node | Preserve as an anchor |
| Undo layout | Restore exact prior positions |

---

# 6. Target architecture

```text
                   MaterialProgram
                         │
                         ▼
                MaterialGraphProjection
                         │
                         ▼
                      Node UI
                         │
                  Bevy UI layout
                         │
                         ▼
                 ComputedNode sizes
                         │
                         ▼
               GraphGeometryRegistry
                  │               │
                  │               └── overlap/spatial queries
                  │
                  ▼
               LayoutController
                  │
       ┌──────────┼───────────────┐
       │          │               │
       ▼          ▼               ▼
 Manual Move   Local Solver   Auto Arrange
                  │               │
                  │             elkrs
                  │               │
       └──────────┴───────┬───────┘
                          ▼
                 GraphViewportMemory
                          │
                 ProjectEditorLayout
                          │
                          ▼
                      Node UI
                          │
                          ▼
                     Wire update
```

Ownership boundary:

```text
MaterialProgram
    = semantic ownership

GraphViewportMemory / ProjectEditorLayout
    = persistent base placement and presentation ownership

Temporary layout overlay
    = session-only effective offsets; never serialized as base positions

LayoutController
    = interaction and placement policy

elkrs
    = optional batch layout implementation
```

---

# 7. Live graph geometry, identity and units

Measure actual UI geometry, not predicted material-expression heights.

## Coordinate contract

All solver positions, sizes, spacing, port offsets and results use **unzoomed logical
graph units**. Screen coordinates, physical pixels and graph units are distinct types
or explicitly named conversion boundaries.

- `FeathersGraphNode.position` is a graph-space placement.
- Bevy `ComputedNode.size()` is measured in physical pixels. For the current UI
  layout path, normalize it with the computed inverse UI scale factor before
  combining it with graph positions.
- Canvas `UiTransform` zoom is a separate display transform. Do not divide a
  layout size by zoom unless the input actually came from a transformed/screen-space
  rectangle.
- Convert pointer motion and screen-space port centers through their own inverse
  viewport transform. Pan and zoom must not change solver geometry.
- Keep floating-point graph coordinates; apply finite-value checks and a small
  documented tolerance for measurement noise, not pixel quantization.

The existing frame-bounds helper combines node positions and computed dimensions
directly. Audit and replace that conversion through the shared geometry path; do not
treat it as the coordinate specification.

## Identity and ownership contract

Use three separate identities:

| Identity | Purpose |
| --- | --- |
| Project/document key | Project namespace + document kind (material/function) + semantic identity. Never just an ECS entity or an unqualified expression ID. |
| Node key | Stable adapter key within the document, including explicit output/signature-node variants. Must survive UI rebuilds and distinguish synthetic nodes from expressions. |
| View key | Concrete open view/window instance; owns its camera and measurement lifecycle, not separate semantic or base-position state. |

Conceptual registry records:

```rust
GraphNodeGeometry {
    effective_position: Vec2, // logical graph units
    size: Vec2,               // logical graph units
    measured_in: GraphViewKey,
    geometry_revision: u64,
}
// Indexed by (document key, view key, node key).
```

Position ownership remains in the placement model. Measurements are observations,
not a second mutable source of position truth.

For duplicate views, choose one authoritative visible measurement owner per document:
prefer the focused view; otherwise retain the current valid owner, then choose a stable
view-key fallback. Only that owner can request reconciliation. Other views consume the
shared effective placement and update their own wires/cameras; they do not run competing
solvers. Reevaluate ownership on focus, visibility or view lifetime changes. If view
content differs, use the owner's stable logical geometry rather than last-writer wins.

An owner switch, DPI change or UI rebuild is a remeasurement, not a new user preview
toggle. It must not itself push nodes. Reconcile when a pending explicit resize cause
becomes measurable, or when the owner observes a genuine stable content-size change
against an established baseline. Test this policy with two views and different DPIs.

## Persistence contract

Persist only base positions, collapse/preview visibility, cameras under their declared
scope, and later explicit pin state. Never persist measured sizes, port positions,
ECS entities, temporary offsets, measurement ownership or displacement journals.

Extend `ProjectEditorLayout` for function layouts with a documented version/migration
policy. Preserve old material layouts and existing atomic-write/error handling; reject
unsupported newer schemas without overwriting them. Project switching must not reuse
another project's placements or pending work.

---

# 8. Dynamic node geometry events

Add geometry-change detection.

Conceptually:

```rust
enum GraphGeometryEvent {
    NodeMoved {
        node: GraphNodeKey,
        position: Vec2,
    },

    NodeResized {
        node: GraphNodeKey,
        old_size: Vec2,
        new_size: Vec2,
        reason: GraphResizeReason,
    },

    NodeInserted {
        node: GraphNodeKey,
    },

    NodeRemoved {
        node: GraphNodeKey,
    },
}
```

Possible resize reasons:

```rust
enum GraphResizeReason {
    PreviewOpened,
    PreviewClosed,
    Expanded,
    Collapsed,
    ContentChanged,
    DiagnosticsChanged,
    UserResize,
}
```

The reason allows temporary UI expansion to behave differently from a permanent user edit.
Carry document/view identity, a resize-cause token, and geometry/topology revisions.
A size delta alone cannot distinguish preview closure from localization or view recreation.
Semantic insertion/removal and UI entity rebuilds must remain distinct events.

---

# 9. Measurement timing

Observe geometry in `PostUpdate` after Bevy UI layout has produced valid dimensions.
Coalesce each document's changes into a snapshot, then queue at most one reconciliation
for the next placement-application phase. Apply positions before the following UI layout
and update wires from the resulting geometry. Specify these system-set dependencies;
do not rely on registration order or rerun Bevy layout recursively from an observer.

Ignore hidden, zero-size, non-finite and not-yet-measured nodes. Preserve a last valid
measurement only while its document/node/view generation remains valid. UI rebuilds,
tab closure, node removal and project switching clean up observations and pending causes.

Use tolerance/stability checks to prevent rounding jitter from generating continuous
resize events. Preview pixel updates, pan/zoom and solver-generated position changes
must not manufacture another resize cause.

The estimated `node_height()` remains bootstrap-only. First measurement establishes
a baseline, rather than automatically tidying a manually overlapped graph. Restored
preview state reconstructs its temporary overlay once stable measurements are available,
without saving those offsets as new base positions.

---

# 10. Stable resize anchor

A node opening a preview should not shift its own position.

Use the existing graph position as a stable top-left/header anchor.

```text
x,y
 │
 ▼
┌──────────────┐
│ Noise        │
├──────────────┤
│ controls     │
└──────────────┘
```

When expanded:

```text
x,y
 │
 ▼
┌────────────────────┐
│ Noise              │
├────────────────────┤
│ controls           │
│                    │
│      PREVIEW       │
│                    │
└────────────────────┘
```

The node grows predominantly:

```text
→ right
↓ downward
```

rather than around its center.

---

# 11. Keep ports stable during expansion

Preserve the current general structure where preview content appears after principal ports/controls.

Prefer:

```text
┌──────────────────┐
│ Noise            │
├──────────────────┤
● UV           Out ●
● Scale            │
├──────────────────┤
│                  │
│     PREVIEW      │
│                  │
└──────────────────┘
```

rather than:

```text
┌──────────────────┐
│ Noise            │
├──────────────────┤
│     PREVIEW      │
├──────────────────┤
● UV           Out ●
● Scale            │
└──────────────────┘
```

This reduces wire movement when preview visibility changes.

---

# 12. Local resize collision solver

Dynamic size changes should first use a **local solver**, not `elkrs`.

Example:

```text
Before

A ─────► B ─────► C
```

B grows:

```text
A ─────► [        B        ] C
```

Aestra should produce:

```text
A ─────► [        B        ] ─────► C
```

by moving the minimum required neighborhood.

It should not globally rearrange the graph.

## Initial collision algorithm

For each resized node:

```text
1. Keep resized node anchored.
2. Find rectangles overlapping new bounds + minimum spacing.
3. Classify conflicts relative to graph direction.
4. Calculate minimum separation.
5. Move affected neighbor.
6. Re-check collisions caused by that movement.
7. Propagate only as far as required.
```

For a left-to-right graph, width conflicts should preferentially push downstream nodes right.

```text
required_x = resized.right + horizontal_spacing
delta_x    = required_x - neighbor.left

neighbor.x += delta_x
```

Height conflicts should preferentially move local sibling rows vertically instead of changing graph layers.

## Determinism, bounds and unsatisfiable constraints

This is a bounded heuristic, not a guarantee of globally minimum movement. Sort roots,
conflicts and tie-breaks by stable node keys; use an explicit direction policy, tolerance,
minimum spacing, maximum affected-node count, maximum total displacement and iteration
budget. Record the selected limits and justify them with small-graph measurements.

Solve against a snapshot and produce a candidate transaction. Validate finite positions,
anchors, protected manual edits and overlap constraints before applying it atomically.
Do not publish a partial cascade when a later step fails. A node already moved by this
solve is not a fresh resize source. Ordinary manual dragging does not invoke this solver.

Pinned/frozen nodes and the resize source remain fixed. Pre-existing intentional overlaps
outside the affected region are not a reason to tidy the entire graph. If no bounded
solution satisfies the hard constraints, preserve existing neighbor positions and show
a compact conflict indication with an explicit Arrange/manual-edit option. The requested
preview may remain open with a visible overlap; do not silently unpin or globally rearrange.

The pin/frozen-node contract is required now, even though the pinning UI ships later.
Measure no-op cost, cascades and worst-case conflict termination in this milestone;
M18 is the expanded performance pass, not the first responsiveness gate.

---

# 13. Start simple: O(N) overlap queries

Material graphs are usually modest in size.

The first implementation can use direct rectangle checks.

Do not introduce a spatial tree before profiling.

If required later, the `GraphGeometryRegistry` can internally use:

- R-tree;
- quadtree;
- uniform spatial hash.

The public layout API should remain unchanged.

---

# 14. Temporary displacement for previews

The placement model must distinguish:

```text
persistent base position
    + session-only effective offset
    = rendered position
```

The current material persistence code reads positions directly from graph memory.
Not serializing a displacement journal alone would still save shifted coordinates.
Make the base/effective distinction explicit at the memory API and persistence adapter
before enabling preview-induced movement.

## Composing several expansion causes

Track all active temporary causes by stable document/node/cause identity. A journal may
record affected nodes and their manual revisions, but **never restore by blindly
subtracting one cause's old delta**.

Opening or closing a preview recomputes the affected overlay from base positions and
the remaining active causes in stable order. Removing cause A must not undo space still
needed by cause B. Closing in either order must converge to the same result when the
user has made no intervening edit.

Before restoring a node, validate its placement revision and the candidate bounds
against current geometry, new nodes and remaining expansions. If restoration is unsafe,
retain its current effective position and report or defer that conflict; never create a
new overlap solely to reproduce an obsolete arrangement.

## Manual edits and other placement operations

A manual drag starts from the displayed position. At commit, record the chosen displayed
position as the new base, increment its placement revision, and invalidate restoration
claims from older causes for that node. Those older causes may not snap it back when
they close. This is temporary restoration protection, not persistent pinning.

Explicit Arrange/Tidy, Undo/Redo, node deletion and semantic replacement also invalidate
or rebase affected cause records. Treat a drag as one transaction, not one history item
per pointer event. While dragging, freeze that node against background reconciliation.

## Save, rebuild and restart

Save base positions plus declared preview visibility; never save effective offsets.
Rebuilds retain the session overlay by document/node identity, not ECS entity identity.
After restart, reconstruct offsets from saved bases and active previews once measurement
settles. Do not accumulate another displacement on top of an already shifted saved base.

Test A-open/B-open/A-close/B-close and the reverse order, a manual move between toggles,
deleted/inserted neighbors, explicit Arrange while previews are open, rebuild, project
switch and restart with previews visible.

---

# 15. Do not react to every preview frame

Changing pixels inside the preview is not a layout event.

Only:

```text
measured node bounds changed
```

should invalidate geometry.

If opening/closing is animated, avoid performing an expensive reconciliation at every intermediate height.

Prefer:

```text
animate UI
   ↓
settle
   ↓
measure final bounds
   ↓
one committed layout reconciliation
```

---

# 16. Manual placement stays authoritative

Do not run ELK after normal dragging.

Current behavior is conceptually correct:

```text
drag
  ↓
update node position
  ↓
GraphViewportMemory
  ↓
persist editor layout
```

Keep that model.

---

# 17. Soft snapping and guides

Aestra's visible graph grid gives a natural basis for assisted placement.

Recommended behavior:

```text
normal drag
    → snapping/guides

modifier key
    → temporarily disable snapping
```

Useful snap targets:

- grid;
- neighboring node left/right edges;
- horizontal centers;
- vertical centers;
- port rows;
- equal-spacing intervals.

Do not make every graph position permanently quantized.

## Alignment guides

During drag, render temporary guides when near alignment.

Possible conditions:

```text
abs(a.left - b.left) < threshold
abs(a.center_y - b.center_y) < threshold
abs(a.port_y - b.port_y) < threshold
```

This remains presentation-only.

---

# 18. Smart insertion

Dropping a compatible node onto an existing connection should support semantic insertion.

Before:

```text
A ─────────────────► B
```

Drop X:

```text
A ─────► X ─────► B
```

If there is insufficient space:

```text
insert X
    +
push only downstream/local neighbors
```

The same placement mechanism can later be reused when AI wraps an expression semantically.

---

# 19. Incremental new-node placement

New nodes should use semantic neighborhood when possible.

## Created from an output socket

Place to the right:

```text
Source ──► New
```

## Created from an input socket

Place to the left:

```text
New ──► Consumer
```

## Inserted between source and consumer

Use available midpoint space or create space locally.

## Created by AI

AI should **not choose canvas coordinates**.

It should issue a semantic command such as:

```text
Wrap expression with Fresnel
```

Then Aestra determines:

```text
sources
  ↓
new expression
  ↓
consumers
```

and computes a local presentation position.

---

# 20. `Tidy` is different from `Arrange`

This distinction should be explicit in the editor.

## Tidy

Preserves overall user organization.

It may:

- remove overlaps;
- normalize small spacing differences;
- snap nearly aligned rows;
- straighten nearly horizontal chains;
- correct accidental small offsets.

Conceptually:

```text
minimize total movement

subject to:
    no overlap
    minimum spacing
```

It should not significantly reorder topology.

## Arrange

Allows topology-driven repositioning.

This is where `elkrs` belongs.

---

# 21. Auto-layout algorithm

Material graphs are directed dataflow graphs, so use a **layered/Sugiyama-style layout**.

General pipeline:

```text
graph
  ↓
cycle handling
  ↓
layer/rank assignment
  ↓
crossing minimization
  ↓
coordinate assignment
  ↓
edge routing
```

Default Aestra direction:

```text
LEFT → RIGHT
```

because material flow is naturally:

```text
inputs → operations → outputs
```

---

# 22. `elkrs` integration

Treat `elkrs` as a **prototype candidate**, not an approved dependency. At the
2026-09-13 review, its docs.rs documentation identified version 0.1.1 and exposed a
layered implementation. Upstream ELK capabilities below are not a substitute for
testing the exact Rust release Aestra would adopt.

Before integration, record the pinned version, license/notice obligations, supported
Rust toolchain and Windows build result, required port/size options, determinism,
panic/error handling, dependency/build cost and representative performance. A failed
qualification keeps the current fallback; it does not justify a new unbounded scope.

ELK Layered provides the kinds of capabilities Aestra is likely to need:

- layered placement;
- crossing minimization;
- supplied node dimensions;
- ports and port ordering;
- compound graphs;
- interactive/stability-oriented options;
- multiple routing strategies.

Do not immediately reimplement a full Sugiyama engine.

## Keep it behind an Aestra interface

```rust
pub trait GraphLayoutEngine {
    fn layout(
        &self,
        input: &GraphLayoutInput,
    ) -> Result<GraphLayoutResult, GraphLayoutError>;
}
```

Possible input:

```rust
pub struct GraphLayoutInput {
    pub direction: GraphDirection,
    pub nodes: Vec<GraphLayoutNode>,
    pub edges: Vec<GraphLayoutEdge>,
    pub region: GraphLayoutRegion,
}
```

```rust
pub struct GraphLayoutNode {
    pub key: GraphNodeKey,
    pub position: Vec2,
    pub size: Vec2,
    pub pinned: bool,
    pub selected: bool,
}
```

```rust
pub struct GraphLayoutEdge {
    pub source: GraphNodeKey,
    pub target: GraphNodeKey,
    pub source_port: Option<GraphPortKey>,
    pub target_port: Option<GraphPortKey>,
}
```

```rust
pub struct GraphLayoutResult {
    pub positions: HashMap<GraphNodeKey, Vec2>,
    pub bounds: Rect,
}
```

This lets Aestra later substitute:

```text
ElkLayeredLayout
CustomAestraLayout
DeterministicTestLayout
```

without touching material or function editor logic.

## Execution and stale-result contract

Pass owned snapshots to the engine; it must not read or mutate ECS/world/document state.
Tag each request with project/document lifetime, topology revision, geometry revision,
placement revision and requested region. A newer manual move, edit, resize, close or
project switch supersedes pending results.

Use a bounded worker queue for expensive layout. Cancelling a request means its result
cannot be applied; a library call without cooperative cancellation still occupies its
worker until it exits. Do not spawn unlimited replacement jobs or block input waiting
for layout. Reject stale, non-finite, incomplete or constraint-violating output before
creating a single undoable placement transaction. Keep existing positions on failure.

---

# 23. Initial ELK scope

Keep the first integration small.

Recommended configuration intent:

```text
algorithm               = layered
direction               = right
node dimensions         = supplied by Aestra
input ports             = west
output ports            = east
node spacing            = moderate
between-layer spacing   = larger
```

Initially use ELK only for **node positions**.

Keep Aestra's existing cubic wire rendering.

Do not make new edge routing a prerequisite for better placement.

---

# 24. Manual placement vs ELK

Relationship:

```text
normal manual editing
        ↓
Aestra controls exact coordinates
```

versus:

```text
explicit arrange command
        ↓
Aestra builds GraphLayoutInput
        ↓
elkrs computes candidate layout
        ↓
Aestra applies positions
        ↓
Aestra persists them
```

`elkrs` is therefore a targeted/batch service.

---

# 25. Pinning

Later add explicit pinning:

```text
Free
Pinned
```

Pinned means:

> Automatic layout should treat this node as a layout anchor.

Do **not** automatically pin every manually moved node.

Otherwise almost every mature graph becomes impossible to arrange.

## Hard-pinning caveat

A layered algorithm naturally wants to control ranks and coordinates. Arbitrary hard `(x,y)` anchors may conflict with this.

Aestra should own pin policy, for example:

```text
1. Extract only movable region.
2. Treat pinned nodes as boundary anchors.
3. Run ELK on that region.
4. Locally reconcile the result around fixed nodes.
```

Do not architect pinning around an assumption that ELK can solve every arbitrary geometric constraint.

---

# 26. Arrange Selection

This is likely more useful than full auto-layout during normal editing.

Process:

```text
selection
   ↓
extract selected subgraph
   ↓
find external connection anchors
   ↓
freeze unselected graph
   ↓
elkrs layout selected region
   ↓
place in existing area
   ↓
local boundary collision resolution
```

Nothing unrelated should move. Boundary reconciliation may move only the declared
movable region. If frozen neighbors make that impossible, return a conflict; expanding
the region requires an explicit user choice.

---

# 27. Arrange Branch

Provide:

```text
Arrange Upstream
Arrange Downstream
```

Example:

```text
A → B → C → D → Output
        ↑
     selected
```

`Arrange Upstream` should operate on the dependencies feeding C without reorganizing D or Output.

This is particularly useful for generated or messy procedural material branches.

---

# 28. Arrange Entire Graph

Use full layout for:

- graphs with no editor layout yet;
- imported/generated semantic material programs;
- AI-generated material from scratch;
- explicit user cleanup.

It may significantly change spatial organization, so it must be reversible.

---

# 29. Presentation history and editor Undo routing

Layout state remains separate from `MaterialProgram`, but that does not imply a second
unrelated user-facing Undo button. Define how presentation transactions enter the
focused document's existing Undo/Redo chronology before exposing automatic placement.

A transaction identifies its project/document lifetime and affected stable node keys,
with exact before/after base placement and relevant presentation state. Drag commits,
Arrange and Tidy each create one entry; camera panning/zooming do not consume graph edit
Undo. Derived preview offsets do not create an entry per moved neighbor or per frame.

Preview visibility changes and their derived overlay are one presentation action.
Undo recomputes the overlay for the restored visibility; it does not replay stale
absolute displacements. Undo/Redo invalidate outstanding solver results.

For smart insertion, coordinate the semantic command and associated presentation delta
as one user action, with rollback if either side fails. Keep shader data free of layout
fields while allowing Undo to restore both the original connection and arrangement.
Layout-only changes must not dirty material assets, trigger compilation, or change
material content revisions.

Route Ctrl+Z/Redo and menu availability consistently through the focused document/view.
Test interleaving a shader edit, drag, preview toggle, insertion and Arrange across
material/function tabs, including delete/Undo and document replacement. Missing nodes
or stale document generations must not be silently recreated by a layout history entry.

---

# 30. Wire routing

Keep current cubic wires initially.

Separate:

```text
node placement
      ↓
port geometry
      ↓
wire routing
```

Future routing options could include:

- current cubic Bézier;
- ELK spline routes;
- orthogonal routing;
- obstacle-aware routing.

Expanded preview rectangles may make obstacle avoidance useful later, but it is not required for the first layout work.

---

# 31. Semantic layout hints

Aestra can eventually improve on a generic layout engine because it understands material semantics.

Possible hints:

```text
MaterialInput   → prefer left/early layer
Parameter       → near first consumer
Texture         → near texture-sampling branch
MaterialOutput  → strongly prefer final/right layer
FunctionCall    → treat as semantic unit
```

Example desired organization:

```text
Inputs              Processing                     Output

Texture ──► Sample ───► Distort ───► Multiply ───► Color
UV ───────►                 ▲
Time ─────► Pan ────────────┘
ParticleColor ─────────────────────────────────────┘
```

These hints should remain Aestra policy on top of the generic engine.

---

# 32. Suggested code organization

Initially, keep this in the editor rather than creating a new crate prematurely.

```text
apps/aestra-editor/src/
    material_graph.rs

    feathers/
        node_graph.rs
        graph_geometry.rs   # new
        graph_layout.rs     # new
```

Persistence remains:

```text
crates/aestra-project/src/editor_layout.rs
```

Dependency:

```text
apps/aestra-editor/Cargo.toml
```

Material and function graphs already share this widget. Both must use the generic
geometry/controller from the first slice, with thin semantic adapters. Keep material
types, project I/O and editor history routing out of reusable Feathers geometry code;
orchestrate document policy in an editor-level graph-layout module as it becomes needed.
No new crate is required merely to introduce these boundaries.

## Suggested responsibilities

### `material_graph.rs`

Own:

- material semantic projection;
- dependency relationships;
- material-specific layout hints;
- preview semantics;
- semantic insertion commands.

### `feathers/node_graph.rs`

Own:

- generic node UI;
- drag interactions;
- viewport pan/zoom;
- node/port rendering;
- generic graph memory.

### `feathers/graph_geometry.rs`

Own:

- measured rectangles;
- size-change detection;
- geometry events;
- overlap queries.

### `feathers/graph_layout.rs`

Own:

- layout controller;
- local collision resolver;
- Tidy;
- `GraphLayoutEngine`;
- `elkrs` adapter;
- layout regions and bounded candidate generation (no direct persistence/semantic writes).

### `editor_layout.rs`

Persist:

- positions;
- collapsed state;
- viewport;
- previews;
- future explicit pin state.

---

# 33. Testing strategy

Layout logic should be testable without rendering whenever possible.

## Pure geometry tests

Test:

```text
resize with no overlap moves nobody
resize overlaps one downstream node
resize cascades through two nodes
vertical preview expansion moves lower sibling
pinned node remains fixed
preview close restores temporary displacement
manual move invalidates restoration
```

## Auto-layout tests

Use small deterministic graphs:

```text
linear chain
diamond
fan-in
fan-out
shared subexpression
multiple branches
disconnected components
```

Assert invariants such as:

```text
no overlap
sources precede consumers
outputs are on final side
minimum spacing is respected
same input produces deterministic placement
```

Avoid over-relying on exact pixel golden coordinates except for adapter-specific tests.

## Editor integration tests

Use material graph test/lab infrastructure to test:

```text
toggle preview
  ↓
ComputedNode changes size
  ↓
geometry registry detects resize
  ↓
local overlap is resolved
```

Also test persisted freeform position restoration across graph rebuilds for both material
and function documents. Add explicit gates for:

- graph zoom 0.5/1/2 and UI scale 1/1.5/2, including detached windows;
- two views of one document, owner handoff and hidden/unmeasured views;
- identical semantic IDs under different project roots and material/function key separation;
- overlapping expansion causes, close order, manual revision guards and safe restoration;
- save/restart with a preview open: base positions unchanged and no accumulated offset;
- deterministic solver output under different container/entity iteration orders;
- bounded failure with frozen/pinned neighbors and no partial placement application;
- mixed semantic/presentation Undo/Redo, including smart insertion and node deletion;
- cancelled/stale background results, document reload and project switching;
- unchanged authored bytes, content revisions and compile counts for layout-only changes.

The proposed test matrix is not a record of tests already run.

---

# 34. Performance approach

Do not optimize prematurely.

Normal material graphs should not require exotic spatial algorithms.

Avoid:

```text
full ELK layout every frame
```

and:

```text
global collision solving on every preview pixel update
```

Start simple, but establish no-op and worst-case bounds with the first local solver.
Measure 25/50/100/250/500-node fixtures before choosing automatic-movement limits.
Check idle frames and animated preview pixels do not trigger repeated solves or writes.
M18 expands this baseline to routing/compound/worker workloads; it does not defer input
responsiveness or termination checks until the end.

---

# 35. Ordered implementation milestones

## Milestone 0 — Lock down the current graph contract

**Complete — 2026-09-13.** Tests and documentation only. See the
[contract audit and validation results](../material-system/graph-layout-contract.md).
Validation: 839 editor tests passed (6 existing GPU tests ignored), 7 project-layout
tests passed, architecture test passed, strict Clippy and formatting checks passed.

### Goal

Document and test current graph behavior before changing layout.

### Tasks

- document current `MaterialGraphLayout`;
- document depth-column fallback;
- document `GraphViewportMemory`;
- document `.aestra/editor-layout.ron`;
- retain/add regression tests for:
  - depth placement;
  - preview-expanded node height;
  - persisted positions;
  - collapsed state;
  - viewport restoration;
- assert that node placement remains presentation-only;
- audit material and function node-key adapters, outputs/signature nodes and per-view cameras;
- specify graph units and document the physical-to-logical measurement conversion;
- record authoritative-view selection, project isolation and measurement cleanup rules;
- lock down base/effective persistence, overlapping causes and history-routing contracts;
- define frozen/pinned constraints and bounded-solver failure behavior before UI pinning exists.

This milestone is contract documentation and regression coverage only. Do not add an
auto-layout dependency or automatic movement.

### Files

```text
apps/aestra-editor/src/material_graph.rs
apps/aestra-editor/src/material_function_editor/graph.rs
apps/aestra-editor/src/feathers/node_graph.rs
apps/aestra-editor/src/history.rs
apps/aestra-editor/src/editor_view.rs
crates/aestra-project/src/editor_layout.rs
```

### Done when

Current material/function behavior is characterized, including known gaps rather than
silently assuming parity. Coordinate, identity, persistence and Undo contracts are
reviewable and tested where current code already implements them. Missing capabilities
are assigned to M1–M3, not reported as existing guarantees.

---

## Milestone 1 — Live Graph Geometry Registry

**Complete — 2026-09-13.** The shared collector observes post-layout logical node sizes
and socket offsets for material/function views. Typed project/document/view/node keys,
two-frame stability, deterministic measurement ownership, generation/revision tags and
cleanup are covered by regression tests. Preview open/close across UI rebuilds generates
one resize observation, with no position or persistence writes. See the
[M1 implementation record](../material-system/graph-layout-contract.md#m1-implementation--live-graph-geometry-registry).
Interactive bounds, placement memory and camera persistence remain M2/M3 work.

### Goal

Make actual UI-measured node geometry available to layout code.

### Tasks

Implement:

```text
GraphGeometryRegistry
GraphNodeGeometry
```

Capture normalized size and port geometry from `ComputedNode` after UI layout, and
combine them with effective graph-space placement under the section 7 conversion.
Scope observations by document, view and stable node key; establish baselines without
triggering movement for an entity rebuild or owner handoff.

Detect:

```text
NodeMoved
NodeResized
NodeInserted
NodeRemoved
```

Do not move nodes automatically yet.

### Done when

Toggling a preview produces one stable logical-size change without relying on estimated
`node_height()`. Zoom/DPI and duplicate-view tests agree; hidden/stale observations
cannot schedule reconciliation. No node moves automatically.

---

## Milestone 2 — Use measured geometry for interactive bounds

**Implemented 2026-09-13.** Frame All/Selection now consumes the exact mounted view's
stable normalized geometry, including expanded content and collapsed nodes. New,
hidden or rebuilding graph views defer framing until measurements are ready. Camera
results apply before the next UI layout, keeping canvas, grid and wires in sync.
Selection bounds are no longer guessed by the material adapter; no selection falls
back to the measured whole graph. Function graphs share this path (their current
adapter has no node-selection UI, so Selection frames all). Bootstrap estimates remain
for initial placement and empty/unadapted widget content. See the
[M2 implementation record](../material-system/graph-layout-contract.md#m2-implementation--measured-interactive-bounds).

### Goal

Remove duplicated guessed geometry from runtime interaction decisions.

### Tasks

- retain estimated size only for initial bootstrap placement if necessary;
- use measured node rectangles afterward;
- compute live graph content bounds from measured geometry;
- never persist calculated node dimensions;
- verify diagnostics/expanded content cannot silently invalidate geometry assumptions.

### Done when

Frame All/Selection and relevant interaction bounds use the shared normalized geometry
for the correct view/document. Material and function graphs pass DPI/zoom/rebuild tests;
estimates are used only while live measurements are unavailable.

---

## Milestone 3 — Base placement, persistence and presentation Undo

### Goal

Make layout changes reversible and prevent temporary positions from entering saved metadata.

### Tasks

- implement base/effective placement APIs and persistence adapters;
- add function layout persistence with compatibility tests for existing material metadata;
- integrate document-scoped presentation transactions with focused Undo/Redo routing;
- coalesce manual dragging and preview visibility into single presentation actions;
- define the compound semantic/presentation transaction for smart insertion;
- implement revision invalidation for manual edits, Undo, node removal and document reload;
- keep layout-only actions out of shader compilation and material dirty state.

### Done when

Manual placement and visibility changes round-trip through save/restart and Undo/Redo
for material and function graphs. An effective-offset test cannot leak shifted positions
to disk. Two views agree on base placement without sharing their camera. Mixed history
tests preserve chronological user intent and reject stale document/node identities.

---

## Milestone 4 — Local resize collision resolver

### Goal

Handle preview/collapse resizing without global re-layout.

### Tasks

Implement:

```text
resized node stays anchored
        ↓
detect overlaps
        ↓
calculate minimal displacement
        ↓
move local neighbor
        ↓
cascade only if required
```

Prefer:

```text
width conflict  → push downstream/right
height conflict → local vertical displacement
```

Add minimum horizontal/vertical graph spacing and explicit iteration, affected-node
and displacement budgets. Solve deterministically on a snapshot; validate and apply
atomically. Keep frozen nodes and the resized node anchored. Add conflict reporting,
no-op/cascade performance tests and stale-revision rejection.

Keep this solver behind tests/internal opt-in until M5 provides reversible overlays.
Do not ship irreversible preview pushing as an intermediate release.

### Done when

Solvable resize cases respect spacing without rearranging unrelated branches. Impossible
or over-budget cases terminate, preserve existing placement and report a conflict.
Identical inputs produce deterministic candidates independent of ECS/container ordering.

---

## Milestone 5 — Reversible preview displacement

### Goal

Make temporary node expansion reversible.

### Tasks

- represent movement in a session overlay, separate from persisted base positions;
- compose all active preview/expand causes in stable order;
- revalidate restoration against current geometry and remaining causes;
- protect manual placement revisions and explicit layout/history changes;
- reconstruct overlays after rebuild/restart without saving or doubling offsets;
- enable the visible preview-resize feature only after M3 history/persistence and M4
  deterministic/failure gates pass.

### Done when

The section 37 vertical slice passes in material and function graph surfaces. Multiple
expansions close safely in either order; manual edits are not snapped back, unchanged
layouts round-trip, and restart while previews are visible preserves base positions.
Native acceptance includes different DPI/zoom, two views and wire alignment.

---

## Milestone 6 — Manual drag assistance

### Goal

Make freeform placement easier to keep clean.

### Tasks

Add:

- optional grid snapping;
- alignment guides;
- edge/center snapping;
- port-row alignment where useful;
- modifier to temporarily disable snapping.

### Done when

Users retain arbitrary positioning while easily building clean rows and columns.

---

## Milestone 7 — Smart local node placement

### Goal

Improve creation/insertion before introducing global layout.

### Tasks

Handle:

```text
node at cursor
node created from socket
node inserted on edge
node created by semantic command
```

Use dependency direction and local free space.

Push only the smallest required neighborhood.

### Done when

Creation uses a deterministic local placement and preserves unrelated manual placement.
Smart insertion validates compatibility/cycles and commits semantics plus placement as
one undoable action; failures leave both unchanged. Layout conflicts use the bounded
fallback rather than an unrequested global arrange.

---

## Milestone 8 — Aestra graph-layout abstraction

### Goal

Create an engine boundary before adding `elkrs`.

### Tasks

Implement:

```text
GraphLayoutEngine
GraphLayoutInput
GraphLayoutNode
GraphLayoutEdge
GraphLayoutRegion
GraphLayoutResult
```

Suggested location:

```text
apps/aestra-editor/src/feathers/graph_layout.rs
```

Keep it independent from material semantic types.

### Done when

The material graph can request topology-based arrangement without depending directly on ELK APIs.

---

## Milestone 9 — `elkrs` layered layout prototype

### Goal

Add high-quality explicit full auto-layout.

### Tasks

- complete the section 22 dependency qualification before adding a pinned `elkrs` release;
- benchmark representative snapshots and keep the fallback if required capabilities fail;
- translate Aestra graph nodes/edges to ELK input;
- pass measured node dimensions;
- configure left-to-right layered layout;
- return positions to Aestra coordinates;
- keep existing cubic wires;
- run expensive work through a bounded job boundary with cancellation/stale-result rejection;
- expose `Arrange Graph` only with M3 presentation Undo and result validation active.

### Acceptance

A complex material graph has:

- no node overlaps;
- sensible left-to-right dependency order;
- fewer avoidable edge crossings than depth-column stacking;
- stable deterministic result for identical input;
- one Undo restores the exact previous base placement;
- changed/deleted documents and intervening manual moves reject pending results;
- input remains responsive and engine errors leave placement unchanged.

---

## Milestone 10 — Arrange Selection and Branch

### Goal

Make automatic layout practical during normal editing.

### Tasks

Add:

```text
Arrange Selection
Arrange Upstream
Arrange Downstream
```

For partial layout:

- freeze unaffected nodes;
- extract relevant subgraph;
- identify external anchors;
- run ELK for movable region;
- reconcile boundary overlap locally.

### Done when

A user can clean one messy branch without moving the rest of the material graph.

---

## Milestone 11 — Incremental layout for AI edits

### Goal

Ensure AI semantic modifications preserve hand-authored graph layout.

### Tasks

When an expression appears through semantic commands:

```text
inspect source nodes
inspect consumer nodes
derive expected layer/branch
find local empty space
place new node
resolve local collision
```

Only request targeted ELK arrangement if local placement cannot produce a reasonable result.

### Example

Semantic request:

```text
Add Fresnel before Color output.
```

Visual change:

```text
Before:
Multiply ───────────────► Output

After:
Multiply ─► Fresnel ───► Output
```

not a completely rearranged graph.

### Done when

AI can insert/remove/wrap expressions without globally disturbing unrelated manually placed nodes.

---

## Milestone 12 — Explicit pinning

### Goal

Allow users to define layout anchors.

### Tasks

Add:

```text
Free
Pinned
```

Persist explicit pin state in editor layout metadata.

Update editor-layout format/version if necessary.

For hard constraints:

- extract movable regions around pins;
- treat pins as boundary anchors;
- locally reconcile ELK result.

Do not auto-pin manually moved nodes.

### Done when

Pinned nodes are hard anchors. Targeted/global layout either respects their exact
coordinates or reports an unsatisfied constraint without applying the candidate.
Never silently downgrade pinning because an engine cannot satisfy it.

---

## Milestone 13 — `Tidy` command

### Goal

Provide low-disruption cleanup separate from auto-layout.

### Tasks

Implement:

- remove overlap;
- normalize nearly equal gaps;
- snap nearly aligned nodes;
- straighten near-horizontal chains;
- preserve topology order and overall composition.

Optimization intent:

```text
minimize total movement
subject to
non-overlap + spacing constraints
```

### Done when

A hand-composed graph can be cleaned without looking newly auto-generated.

---

## Milestone 14 — Port-aware layout

### Goal

Improve crossing minimization using socket semantics.

### Tasks

Provide:

- input/output side;
- socket order;
- optional measured port position;
- semantic port constraints.

Prefer:

```text
inputs  = west
outputs = east
```

where appropriate.

### Done when

Dense fan-in/fan-out graphs contain fewer unnecessary edge crossings.

---

## Milestone 15 — Improved wire routing

### Goal

Improve readability independently from placement.

### Evaluate

```text
current cubic Bézier
ELK splines
orthogonal routing
obstacle-aware routing
```

Expanded previews should be treated as obstacles if routing mode supports it.

### Done when

Dense/expanded material nodes no longer cause avoidable wires through unrelated node bodies.

---

## Milestone 16 — Semantic layout hints

### Goal

Use Aestra knowledge to improve generic auto-layout.

### Add preferences for:

```text
MaterialInput
Parameter
Texture chain
MaterialFunction
MaterialOutput
```

Examples:

- material outputs strongly prefer final/right rank;
- parameters stay near first consumer;
- UV chains stay visually coherent;
- shared inputs avoid awkward crossings.

### Done when

Auto-arranged graphs resemble deliberate material graphs rather than generic directed-network diagrams.

---

## Milestone 17 — Compound graph preparation

### Goal

Prepare for comments/groups and nested or compound regions. Ordinary material/function
graph parity has already been required since M0; this milestone is not its prerequisite.

### Tasks

Allow layout input to represent:

- group ownership;
- nested regions;
- collapsed function nodes;
- group/comment bounding rectangles.

No need to implement every UI feature yet.

### Done when

Future compound/group features can extend the already-shared material/function layout
architecture without changing flat-graph identity or persistence contracts.

---

## Milestone 18 — Performance and stability pass

### Goal

Extend the earlier performance gates to all delivered layout features, including worker
queues, targeted arrangement, owner handoffs and larger graphs. Basic solver termination
and responsiveness were already required by M4.

### Benchmark graph sizes

```text
25 nodes
50 nodes
100 nodes
250 nodes
500 nodes
```

Measure:

- drag latency;
- geometry tracking;
- preview expansion;
- local collision resolution;
- targeted ELK layout;
- full layout.

Only introduce spatial indexing if profiling demonstrates a need.

### Done when

Common graph interactions remain responsive and no size/layout event causes visible oscillation or jitter.

---

# 36. Recommended phase grouping

| Phase | Milestones | Priority | Release gate |
| --- | --- | --- | --- |
| A — Correct shared foundations | M0 contracts, M1 live geometry, M2 interactive bounds, M3 base persistence/Undo | P0 prerequisite | Material/function and multi-view tests; no automatic movement yet. |
| B — Reversible local layout | M4 bounded solver, M5 temporary overlays | P1 | Ship preview-resize behavior together only after safe restoration and restart tests. |
| C — Better manual editing | M6 snapping/guides, M7 local placement/insertion | P1 | Manual intent preserved; insertion is one coherent Undo action. |
| D — Explicit arrangement | M8 engine abstraction, M9 qualified `elkrs`, M10 selection/branch | P2 | Validated candidates, stale-result rejection and presentation Undo. |
| E — AI-safe organization | M11 incremental AI edits, M12 pinning UI, M13 Tidy | P2 | Semantic adapters reuse placement policy; anchors stay fixed. |
| F — Advanced polish | M14 ports, M15 routing, M16 semantic hints, M17 compounds, M18 expanded performance | P2 | Measured benefit and no regression of prior interaction guarantees. |

Milestone numbers are local to this graph-layout track, not replacements for the
repository's broader M0–M8 milestones. Deliver incremental, reviewable changes; do not
implement all phases as one task.

---

# 37. First vertical slice to implement

**Immediate task: M3 base placement, persistence and presentation Undo.** M0–M2 are
implemented; use the [contract audit and implementation records](../material-system/graph-layout-contract.md)
as the baseline. Establish document-scoped base positions, function persistence,
project isolation and focused presentation history before enabling automatic movement.

After those prerequisites, implement this complete M4–M5 flow before integrating
`elkrs`:

```text
1. Open a graph with two manually positioned neighboring nodes.
2. Enable a preview on A; record one user resize/visibility cause.
3. The authoritative view produces stable logical geometry after Bevy layout.
4. The bounded solver stages a local candidate with A anchored.
5. Validate revisions/constraints and apply B's movement as a temporary offset.
6. Wires and any second view reflect the effective placement.
7. Save/rebuild: persisted base positions have not changed.
8. Disable A's preview; revalidate against remaining causes and current geometry.
9. Restore B if safe and unmodified; preserve a user's intervening drag.
10. Undo/Redo the visibility action without compiling or dirtying the material.
```

Acceptance also covers A/B previews closing in both orders, impossible frozen-node
conflicts, zero-size/hidden views, DPI/zoom changes, function graphs, project switching,
and restart with previews visible. No intermediate build should ship non-undoable
Arrange or permanently saved preview offsets.

Only after this flow is stable should full arrangement be integrated. These are proposed
acceptance gates; implementation and native verification have not yet occurred.

---

# 38. Explicit architectural rules

1. **`MaterialProgram` remains the source of semantic truth.**
2. **Graph positions remain editor metadata.**
3. **Actual node size comes from Bevy UI measurement normalized to logical graph units.**
4. **Calculated node size is not persisted.**
5. **Normal dragging never invokes full auto-layout.**
6. **Dynamic resize uses bounded, deterministic local movement; unsatisfied constraints are reported.**
7. **Preview expansion uses local collision resolution.**
8. **AI edits semantic material data, not graph coordinates.**
9. **New AI-created graph nodes are placed incrementally by Aestra.**
10. **`elkrs` is used only through an Aestra-owned abstraction.**
11. **`Tidy` and `Arrange` are different operations.**
12. **Presentation Undo and focused history routing precede all exposed automatic placement.**
13. **Pinning is explicit; manual dragging does not automatically pin.**
14. **Node placement and wire routing remain separate subsystems.**
15. **Full graph layout is explicit unless no meaningful layout exists.**
16. **Only base positions are persisted; temporary display offsets are reconstructed.**
17. **Geometry is view-scoped, placement is document-scoped, and projects are isolated.**
18. **Material and function graphs share the contracts from the first milestone.**
19. **Pinned/frozen coordinates are hard constraints, never best-effort suggestions.**
20. **Stale background results and partial solver candidates never mutate placement.**

---

# 39. Final decision

Aestra should use a **hybrid material graph layout model**:

```text
                         Aestra
                           │
            ┌──────────────┼──────────────┐
            │              │              │
            ▼              ▼              ▼
       manual UX      local layout    graph policy
                           │
                           ▼
                         elkrs
                  only when appropriate
```

The guiding principle is:

> **Manual placement communicates user intent. Local geometry changes should cause the minimum possible movement. Full automatic layout should happen only when explicitly requested or when no meaningful layout exists yet.**

This fits Aestra's current implementation particularly well because the repository already separates semantic materials from project-local graph layout metadata and already exposes real UI dimensions through Bevy's node layout.

The main architectural change is therefore not a replacement of the graph editor. It is the introduction of a proper:

```text
GraphGeometryRegistry
        +
LayoutController
        +
GraphLayoutEngine abstraction
```

around the existing graph presentation system, with base/effective placement,
document-aware history and multi-view ownership established before the controller
is allowed to move nodes.

---

# 40. Relevant references

Aestra:

- <https://github.com/thehiddenworkshop/aestra>
- <https://github.com/thehiddenworkshop/aestra/blob/main/apps/aestra-editor/src/material_graph.rs>
- <https://github.com/thehiddenworkshop/aestra/blob/main/apps/aestra-editor/src/feathers/node_graph.rs>
- <https://github.com/thehiddenworkshop/aestra/blob/main/crates/aestra-project/src/editor_layout.rs>
- <https://github.com/thehiddenworkshop/aestra/blob/main/docs/AESTRA_SEMANTIC_MATERIAL_AUTHORING_ROADMAP.md>
- <https://github.com/thehiddenworkshop/aestra/blob/main/docs/material-system/current-state.md>

Layout engine references:

- <https://crates.io/crates/elkrs>
- <https://docs.rs/elkrs/0.1.1/elkrs/> (reviewed API version; adopting it still requires qualification)
- <https://eclipse.dev/elk/reference/algorithms/org-eclipse-elk-layered.html>
