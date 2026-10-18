# B20-2 slice 52: host-motion lane and timeline scrollbar composition

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now builds
the production host-motion header/pose-key lane and both timeline scrollbar
gutters. The native panel calls the same extracted constructors.

- `timeline/host_composition.rs` shares the header, lane and text-action helper,
  plus their actual activation queue/consumer. The existing version-specific
  `control_input` boundary preserves native Interaction pressed/reset behavior
  on 0.19 and upstream Pressed insertion/removal on 0.20. Action classification,
  pending consumption and dispatch policy are otherwise unchanged; no substitute
  action handler or deprecated 0.20 compatibility component is introduced.
- The existing HostMotionControl identity moves unchanged into shared interaction
  state. The native host module reexports it and the two constructors, preserving
  the API used by the timeline and region-selection dismissal code.
- `timeline/scroll_composition.rs` shares the original horizontal/vertical gutter
  expressions, target wiring and actual Feathers bars. It retains scroll-content
  geometry, clipping, initial visibility and press/drag stoppers. The candidate
  exposes the already-shared real horizontal scrollbar constructor as well as
  the existing vertical constructor.

Audits compare five constructor/helper expression contracts, both action-bridge
contracts with the explicit input-boundary expansion, and native host installation,
label/inspector/number/initialization helpers. Native TimelinePlugin wiring and
both complete existing root timeline/host-motion test blocks remain unchanged.
Root dependencies, features, toolchain, authored formats and interaction policy
are unchanged. Existing hard-coded host-motion text is preserved, not newly
localized in this compatibility slice.

## Regression coverage

Fourteen new shared host-motion tests use actual scenes, attached observers,
production motion/action consumers and global history routing. They cover header
classification and geometry, key identity/labels/cursors/lane offsets, empty-track
creation, Inspect, existing-key selection, once-only Add and undo/redo, raw Pressed
versus Activate, unclassified controls, attached key selection, anchor protection,
exact preview/commit timing at two window scales, crossing keys/trailing clicks,
press isolation and in-place selection/zoom visuals. Authored files stay unchanged.

Ten new shared scrolling tests construct the actual Feathers bars and exercise
production horizontal/vertical synchronization and overflow visibility. Coverage
includes orientation/targets, gutter geometry, content ratio, view-to-position and
position-to-view mapping, bounds, paired-pane synchronization and same-frame
precedence, content shrink, half-pixel overflow thresholds, attached press/drag
isolation and stable non-pickable viewport/content identities.

These are headless scene/observer checks with explicit computed geometry and
typed events. They do not qualify native pointer picking, scrollbar thumb input,
layout/rendering, menu/floating-window lifecycle, persisted scroll memory,
host-motion inspector composition or full timeline/Curves plugins. Prior host
pointer ownership, stale-action and cancellation hardening remains separate.

## Validation

Accepted frozen headless run (2026-10-18):
`target/bevy-020-qualification/runs/8d5214227e804179ad93e9ae2a432995/`.
It executes **1,471 checks**, including **1,051 graph/session/history/consumer
checks**, with all **863 input hashes** unchanged. Counts are executions across
gates, not unique tests. Four new production/test paths join the inventory; the
native host module and scrollbar implementation were already included. The
existing graph timing probe stays ignored; the graph process takes 34.47 seconds
after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, `native_executed: false`,
and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
Shipping passes **2,082 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting checks and
whitespace validation. Root dependency/toolchain diffs remain empty. Existing
Windows incremental-cache hard-link fallback warnings are non-fatal.

Next: remaining full timeline/Curves panel/plugin integration and host-motion
inspector construction, then viewport/application consumers and native workflow
acceptance. B20-2 remains open; shipping stays on 0.19.1 until the complete
compatibility gate.
