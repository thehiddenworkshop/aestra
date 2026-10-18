# B20-2 slice 61: viewport action activation and execution

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now
executes the actual viewport action queue, dispatcher, executor and control audit.

- `viewport/actions.rs` shares the complete executor, display state and original
  menu closing, revision updates, pending-activation consumption and audit policy.
  The native viewport plugin still registers the same observers and action chain.
- `viewport/control_input_019.rs` retains the original Interaction change query,
  pressed check and reset. The 0.20 adapter uses changed native Pressed markers;
  releases have no dispatch side effects, as in the original consumer. Both require
  a pending Activate plus a pressed state. They retain the existing behavior when
  release precedes consumption; this slice does not harden that policy.
- `persistence/status.rs` shares the actual settings writer entry point and
  complete localized persistence status enum/functions. Existing native APIs and
  descendants keep access through the original persistence namespace.
- `docking/maximized.rs` shares the original transient maximized-panel resource.
  Maximization still does not persist layout or edit an effect document.
- The candidate imports the complete production settings module and the actual
  renderer `preview.rs` source that supplies its photographic settings type.
  A candidate-only crate alias resolves that existing renderer namespace to the
  shared source; no settings or camera-profile stand-in is used. This does not
  import the shipping renderer crate or qualify its complete plugin.
- A test-only settings loader forwards to the real loader with an explicit path,
  avoiding process-wide config environment changes. Runtime settings behavior and
  the rest of the settings source remain unchanged.

Source audits confirm the complete executor, display state, settings persistence
function and localized status implementation are unchanged after visibility/
format normalization. Native viewport plugin wiring, existing viewport,
persistence and docking root test blocks, and all remaining root functions in
those modules compare unchanged. Settings differ only by the test-only loader.
Root engine/dependency/toolchain inputs and authored formats stay intact.

## Regression coverage and limits

Seventeen new shared headless tests execute real consumers on both engines:

- Grid preferences toggle twice and round-trip the complete settings file, with
  localized success status in English and French; unrelated preferences survive.
- Frame, all three transform modes, both display modes and maximize remain
  transient. Maximizing replaces another maximized panel and toggles back.
- Classified activation closes menus, clears tab context, consumes pending and
  pressed state once and leaves repeated idle updates unchanged. Other action
  domains, unclassified actions and pressed-without-activation do not dispatch.
- Pending-without-pressed behavior is preserved; repeated activations coalesce
  before consumption and independent buttons retain their exact payloads and
  Feathers background styling.
- Settings write failures report localized feedback without touching prior bytes;
  malformed settings remain write-protected and retain their diagnostic.
- Toolbar framing reaches the real navigation consumer and frames its actual
  emitter proxy. Real keyboard event dispatch reaches grid, maximize, frame and
  gizmo execution; text focus blocks those shortcuts before any settings write.
- Set gating defers activation until reenabled, and the original debug control
  audit accepts classified/explicit-native controls and rejects unclassified ones.

Assertions compare the effect, document revision, dirty state and undo/redo lengths
to their original values. Every test write uses an explicit temporary directory;
the user's settings and workspace files are not written. The candidate also executes
the fourteen existing settings tests and three photographic-profile tests from
their unchanged production sources.

These checks qualify action consumers and temporary-file persistence, not native
pointer/OS input delivery, toolbar visual composition, whole-plugin/application
scheduling, rendered camera viewport lifecycle, surface minimize/restore or
floating-window workflows. The fixture assembles the original action registration
contract, not the complete ViewportPlugin. Photographic component tests do not
establish native HDR rendering or visual acceptance. Those remain separate gates.

## Validation

Accepted frozen headless run (2026-10-18):
`target/bevy-020-qualification/runs/0c7f5f5b4fd545eda00259a6718a6806/`.
The editor graph/session/history/consumer suite passes **1,223 tests**, with its
existing timing probe ignored, in 53.31 seconds after compilation. All seventeen
viewport action tests also pass in isolation.

The complete run executes **1,644 checks**, with all **903 input hashes**
unchanged. Counts are executions across gates, not unique tests. Eight additional
paths join the inventory: actions/tests, two control adapters, settings, the
persistence status helper, maximized state and photographic profile source.
The runner confirms Bevy **0.20.0**, Rust **1.98.1**, `native_executed: false`,
and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
Shipping passes **2,234 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting checks and
Git whitespace validation. Root dependency/toolchain inputs are unchanged and
post-documentation input drift is zero. Existing Windows incremental-cache
hard-link fallback warnings are non-fatal.

Next: camera viewport lifecycle, then remaining preview/render execution, graph
palette/menu and application consumers before native workflow acceptance.
B20-2 remains open; shipping stays on 0.19.1 until the complete compatibility
release gate.
