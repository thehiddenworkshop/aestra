# B20-2 slice 1 — Editor theme compatibility

## Implementation

The editor's former `UiTheme.0.color` map is a version-selected boundary rather
than an engine API used throughout palette construction. The shipping
`apps/aestra-editor/src/theme.rs` selects `theme/bevy_019.rs`; the isolated
candidate selects `theme/bevy_020.rs`. Both consume `theme/palette.rs` and the
same theme regression tests. No root dependency or shipping adapter is switched.

All **29 palette constants and 96 shipping token assignments** are unchanged.
A mechanical comparison with the original source checks the constants and the
normalized assignment key/value pairs, including the four legacy group colors
now held by the 0.19 adapter.

The 0.20 adapter starts with the complete upstream dark theme and gives each
Aestra design-token override its own semantic identity. This matters because
upstream semantic fills can be shared between controls: assigning menu hover to
one shared semantic fill would also recolor plain buttons or list selection.
Existing Aestra values resolve exactly in Base, Higher, Highest and Floating
contexts; unassigned upstream tokens retain their contextual defaults.

0.20 merges the old group-header/body background and border tokens into
`GROUP_BG` and `GROUP_BORDER`. Those map to the existing body fill and border.
The shipping adapter retains all four old assignments. The later scene/settings
composition port must explicitly preserve the darker header styling; this theme
slice does not claim that the complete settings UI already runs on 0.20.

## Required regression gate

`patches-020/editor_theme.rs` imports the actual palette and 0.20 adapter. Eight
checks cover:

- Shared menu hover/focus/pressed contrast and all shared color assignments.
- Exact values in all four surface contexts and distinct control semantic fills.
- Complete contextual defaults for tokens Aestra does not override.
- Merged group body fill and border in every context.
- Real BSN Feathers buttons: normal, hover, press, disable, re-enable and variant
  changes, in every context.
- A real floating Feathers menu item: hover, visible keyboard focus and disabled
  text-token selection.
- Actual inherited context changes, resolved text/background colors, resource
  replacement and restoration through Feathers' propagation/theme systems.

This is a headless **style-state** test, not synthetic pointer hit testing or
pixel approval. Real Feathers/scene/UI dependencies run without a window or GPU;
the fixture drives `Hovered`/`Pressed` directly and disables empty headless hover
map updates. Actual keyboard/pointer editing remains a separate migration gate.

The required `editor-qualification` feature enables Feathers and scene APIs only
in the isolated fixture. Its lockfile gains Bevy 0.20 Feathers/scene dependencies;
the shipping lockfile remains unchanged. `run-020-patches.ps1` runs the editor
gate and freezes all five production theme source files plus the fixture.

## Validation

Shipping editor: **1,171 unit tests and 3 architecture tests pass**; the ten
existing ignores are unchanged. The initial sandboxed run could not create
temporary settings/projects (Windows access denied); rerunning the same suite
with approved access passes. All three shipping theme regressions pass.

Strict workspace all-target Clippy and checking with `aestra-bench/gpu`, strict
candidate all-target Clippy with `editor-qualification`, both formatting checks
pass. Sandbox-denied candidate lint/format configuration access succeeds with
approved access. Existing incremental-cache hard-link warnings fall back to
copying; no lint suppression or weakened assertion is added.

Accepted source-frozen native run:
`target/bevy-020-qualification/runs/1473dde3b18f4c70b3dd94ef01013d04/`.
Pinned MSVC Rust **1.98.1**, locked/offline dependencies: **268 checks** pass
(9 keyboard, 8 editor theme, 40 upstream focus, 4 cluster units, 4 shader,
3 storage, 166 shared/candidate checks and **34 explicit native gates**).
All **469 input hashes and five native executable hashes** remain unchanged
during qualification. The new theme gate passes in **1.05 seconds**.
GPU gates run serially on NVIDIA RTX 4070 SUPER / Vulkan 616.92 and AMD
integrated Vulkan 26.3.1. This is correctness evidence, not visual/performance
parity approval.

The accepted summary retains these SHA-256 identities:

- Synchronous extraction executable:
  `da5a788aa304034dad0ba0be1e9b3b840106c27dc98acc9811c1a68f86a295c5`.
- Asynchronous extraction executable:
  `c7c499cc9f2ce78cb004577efce0fbb6130c944be14f9f29c5a6b79968b743d3`.
- Unchanged shipping lockfile:
  `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.

## Scope and next step

Shipping remains **Bevy 0.19.1**; the isolated candidate remains **Bevy 0.20.0 /
wgpu 30.0.1**. B20-2 is **in progress**, not complete. Next port the editor's BSN
scene/control and text-input composition boundary, then pointer/focus handling,
while retaining numeric behavior, menu shortcut layout, settings/recovery,
project creation and save/reopen acceptance. New upstream-control adoption is
still a later milestone. The coherent dependency switch and final native
visual/performance parity also remain open.

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-editor --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --features editor-qualification --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
```
