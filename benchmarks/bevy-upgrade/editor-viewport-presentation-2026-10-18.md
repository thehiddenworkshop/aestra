# B20-2 slice 64: grid/display presentation consumers

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now
executes the actual grid, display-mode routing and viewport control-feedback
consumers from `viewport/presentation.rs`.

- Six original functions are shared: grid synchronization/uniform construction,
  adaptive spacing, presentation display routing, display-icon feedback and
  transform-gizmo mode/space feedback.
- Original grid material/uniform/plane definitions, Material implementation,
  display/gizmo icon markers, presentation ownership and label marker are shared.
  Parent-required visibility preserves native setup and consumer access.
- The candidate imports the complete production PresentedEffect payload and its
  real material binding/layout dependencies, previously qualified in the renderer
  gate. It does not substitute a smaller presentation component or render-mode API.
- The unchanged production grid shader is included in frozen inputs. A new
  required shader test runs it through Bevy 0.20's ShaderCache/WESL path and Naga
  validation, checking the actual material bind-group index, binding zero and
  six vec4 offsets/96-byte uniform span against the Rust record-size assertion.

Source audits confirm all six function bodies, material definitions/implementation,
native ViewportPlugin registration, all 55 remaining root functions and the complete
existing viewport root test block are unchanged after visibility normalization
where needed. Root engine/dependency/toolchain inputs, shader asset and authored
formats remain unchanged. No new control or renderer feature is adopted.

## Regression coverage and limits

Twenty new shared headless tests execute actual assets, presentation payloads,
menu/action consumers and SVG/text feedback on both engines:

- World-origin grid/fade anchoring under pan/zoom, plane-radius clamps, view-angle
  fade, visibility, fixed colors, adaptive 1/2/5 spacing and invalid-input fallbacks.
- Missing/replaced material handles, unrelated assets/planes, missing/ambiguous
  single grid ownership and recovery preserve original policies.
- The Material shader path, alpha/depth-bias/prepass/shadow settings and Rust
  uniform size retain their contracts; the material implementation compiles on 0.20.
- Wireframe/rendered modes reach real owned PresentedEffect components, restore
  owned visibility and leave unrelated presentations alone. Playback time, seed,
  history policy, seek quality and compiled artifact identity survive mode changes.
  Newly added presentations receive the active mode.
- Selected display/gizmo icons use actual theme colors. All mode/space label
  combinations, the existing hover/just-pressed X contract, missing-canvas behavior
  and retained entity/material identities are checked.
- Real viewport actions reach grid visibility, presentation payloads, labels and
  icons without authored effect edits. The grid preference round-trips through the
  real writer at an explicit temporary path; transient display/gizmo actions do not
  create a settings file.

The candidate also executes eight existing tests from the newly imported renderer
dependency modules (six material binding, one layout and one presentation payload).
The shader test is an additional required execution, not a native GPU pipeline test.

Retained identity does not imply suppressed idle component/asset writes; the
original mutation policy remains unchanged. No unrelated input, ownership or
optimization hardening is bundled into the port. The shader test does not link
the full native mesh pipeline, execute material specialization/bind-group creation
on a device or establish pixel quality/performance. Native layout/picking/rendering,
full viewport/application scheduling and runtime/gizmo workflows remain separate
acceptance gates. No visual reference is changed or approved.

## Validation

Accepted frozen headless run (2026-10-18):
`target/bevy-020-qualification/runs/a3a3e180c4e84814bcf62e85184228f1/`.
The editor graph/session/history/consumer suite passes **1,285 tests**, with its
existing timing probe ignored, in 46.68 seconds after compilation. All twenty
new presentation tests and the new production grid shader test also pass in
focused runs.

The complete qualification executes **1,707 checks**, with all **910 input hashes**
unchanged. Counts are executions across gates, not unique tests. The presentation
module, tests and unchanged grid shader are the three new inventory paths. The
runner confirms Bevy **0.20.0**, Rust **1.98.1**, `native_executed: false`, and
unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
Shipping passes **2,288 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting checks and
Git whitespace validation. Root dependency/toolchain inputs remain unchanged;
post-documentation input drift is zero. Existing Windows incremental-cache
hard-link fallback warnings are non-fatal.

Next: runtime preview/player setup and project-instance reconciliation, then
gizmo/host-motion integration and remaining graph/application consumers. B20-2
remains open; shipping stays on 0.19.1 until the complete compatibility release gate.
