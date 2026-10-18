# B20-2 slice 59: full Curves plugin integration

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now
installs the actual `EditorCurvesPlugin`, rather than reconstructing its schedule
inside the action and panel fixtures.

- `curves/plugin.rs` shares the original plugin, `CurvesSet::Actions`, state
  initialization, activation observer and chained keyboard/action registration.
- `curves/host_install.rs` shares the original host cancellation registration.
  Both native and candidate `host_motion` namespaces reexport that installer;
  the candidate does not substitute a cancellation handler or schedule.
- The native Curves API continues to reexport the same plugin and system set.
  Source audits confirm the original plugin implementation, installer body and
  both native Curves/host root test blocks are unchanged.
- The 23 action-routing and 18 panel-composition tests now install the real
  plugin. Their semantic assertions remain intact; the fixture no longer
  duplicates observer or system registration.

The three schedule consumers remain the original keyboard handler, host drag
cancellation and action dispatcher. Keyboard and cancellation each run before
the dispatcher, and all belong to the existing Actions set. Resource ownership,
event consumption, semantic handlers, authored formats and root dependencies
are unchanged.

## Regression coverage and limits

Three new tests run on both engine graphs:

- Missing CurvesState is initialized, existing key/channel selection survives
  installation, and an ordinary idle update leaves the document clean.
- The executable Update schedule contains exactly the three original consumers,
  installed once, with the original set membership and ordering dependencies.
  Assertions use registered system identities rather than debug names.
- A run condition on the installed Actions set defers a real activation, then
  consumes its pending/pressed state exactly once when enabled. Repeated idle
  updates neither change the effect nor dirty it or advance its UI revision.

All **71 focused Curves tests** pass: 27 property-editing, 23 action-routing,
18 panel-composition and three plugin tests. The latter 41 action/panel tests
exercise actual plugin installation, including the existing semantic history and
attached observer checks. Fixtures keep Curves active to avoid saving user
workspace settings through the production reveal consumer.

This qualifies the Curves plugin's own registration and headless execution, not
the entire editor application schedule. Explicit-geometry pointer fixtures do not
establish native layout, picking, rendering, application rebuilding, persisted
scrolling or floating-window acceptance. Existing interaction hardening remains
separate from the compatibility port.

## Validation

Accepted frozen headless run (2026-10-18):
`target/bevy-020-qualification/runs/f965c0dbf74a4b829494abe6f947fca5/`.
The editor graph/session/history/consumer suite passes **1,171 tests**, with its
existing timing probe ignored, in 35.91 seconds after compilation.

The complete run executes **1,592 checks**, with all **892 input hashes**
unchanged. Counts are executions across gates, not unique tests. The three new
inventory paths are the plugin, plugin tests and host installer. The runner
confirms Bevy **0.20.0**, Rust **1.98.1**, `native_executed: false`, and unchanged
shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.

Shipping passes **2,199 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines; workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and Git
whitespace validation pass. Root dependency/toolchain inputs remain unchanged.
Post-documentation input drift is zero. Existing Windows incremental-cache
hard-link fallback warnings are non-fatal.

Next: remaining viewport execution/navigation consumers, then graph palette/menu
and application handlers before native editor workflow acceptance. B20-2 remains
open; shipping stays on 0.19.1 until the complete compatibility release gate.
