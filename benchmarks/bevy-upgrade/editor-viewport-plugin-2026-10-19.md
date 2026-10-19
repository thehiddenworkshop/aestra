# B20-2 slice 72: installed viewport-plugin integration

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now
installs the actual complete main-world ViewportPlugin, rather than assembling
its individual consumers only in separate fixtures.

`viewport/plugin.rs` shares the original plugin implementation: upstream transform
gizmo and grid-material plugins, resources, activation/action observers, startup
and post-startup setup, the ordered Update consumer chain, PostUpdate layout/
gizmo/feedback ordering and the renderer preparation boundary. The native
`viewport::ViewportPlugin` path is retained through a re-export. The candidate
uses the actual renderer AestraRenderSet definition, not a replacement schedule
tag. Its now-obsolete runtime-preview and shortcut dead-code allowances are
removed; no new lint exemptions are added.

Source audits confirm the extracted plugin implementation, both remaining root
test-helper bodies and the original root test body are unchanged. Root imports
are reduced to re-exports/native keyboard access and test-only imports. No system
ordering, run condition, authored format, dependency, toolchain, gesture,
cancellation, telemetry or playback policy changes are introduced.

Two new plugin/test paths enter the frozen source inventory. The renderer-set
source was already included by the renderer qualification gates.

## Regression coverage and limits

**15 new shared integration tests** execute on both engines. Each fixture
installs the complete actual ViewportPlugin and Bevy's real transform-gizmo mesh
plugin, with the real Feathers panel and actual session, project catalog,
timeline, profiler, input/dispatch, menu, localization, settings and SVG modules.
The fixture retains the relative viewport set ordering from the larger shipping
application chain; it does not replace production consumer registration.

Coverage includes:

- Startup/Update/PostUpdate execution with the upstream gizmo logic, grid
  MaterialPlugin, all interaction resources and Aestra's original gizmo settings.
  Repeated updates retain camera, grid, player, gizmo and toolbar identities and
  do not duplicate mesh assets.
- Actual upstream overlay camera and mesh materials receive Aestra's post-startup
  layer, order, transparent material and depth-bias configuration. A real toolbar
  Activate changes the upstream mesh mode and presented player's wireframe mode
  in one Update without document edits.
- Real KeyboardInput passes through the input-focus snapshot collector and
  installed viewport Input/Actions/Update registration. Mode and maximize actions
  arrive once; modal gating drops blocked shortcuts, and a fresh press recovers.
- Application action-set run conditions defer real pending activation, then
  consume it once. Update-set gating holds player synchronization, then resumes
  the current session time without respawning or dirtying the document.
- Supplied computed-layout output runs in UiSystems::Layout. A probe inside
  TransformGizmoSystems observes both cameras' updated physical viewports, with
  the original one-valid-frame activation delay. Resize updates both views;
  invalid layout disables them and restoration rearms them. The actual upstream
  gizmo render step also executes; neither camera nor mesh plugin is substituted.
- Seeking/seeding reaches the retained player without semantic changes or clock
  advancement. A real undoable live transform edit and undo update the installed
  proxy without replacing the presented player. Empty documents run all installed
  schedules, keep a valid empty player and hide upstream transform visuals.
- A test observation producer in the actual AestraRenderSet::Prepare sees the
  current root mock trace and publishes a real EffectRuntimeStatus. The installed
  telemetry and status consumers see it in that same Update; GPU status without
  observations does not manufacture CPU evaluation samples.

This is **headless installed main-world plugin/scheduling evidence**, not a native
GPU/render-surface or complete editor application acceptance run. Computed layout
is supplied by the fixture, not measured through a native UI layout/render pass.
The preparation probe is a test producer, not real renderer GPU preparation or
dispatch. The actual output message stream is registered, but this fixture does
not install the full Aestra renderer or qualify joint extraction/pipelining.
Rasterization/loading, renderer-native pipelines, visual/DPI acceptance and
performance retain their separate gates. No visual reference is approved.

## Validation

The focused candidate gate passes all **15 installed-plugin tests**. Accepted
frozen run (2026-10-19):
`target/bevy-020-qualification/runs/5add4dda23084ad083462aa2a3dfe944/`.
The complete qualification executes **1,921 checks**, including **1,499 editor
graph/session/history/consumer checks** with the existing timing probe ignored,
in 55.26 seconds for that suite after compilation. Counts are executions across
gates, not unique tests.

All **949 input hashes** are unchanged. The runner confirms Bevy **0.20.0**,
Rust **1.98.1**, `native_executed: false`, and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.

Shipping passes **2,469 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting gates and
Git whitespace validation. Root dependency/toolchain inputs are unchanged and
post-documentation input drift is zero. Existing Windows incremental-cache
hard-link fallback warnings remain non-fatal.

Next: graph palette/menu handlers, remaining application action dispatch and
complete editor workflow acceptance. B20-2 remains open; shipping stays on
0.19.1 until the full compatibility gate passes.
