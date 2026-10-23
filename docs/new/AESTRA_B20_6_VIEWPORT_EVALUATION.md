# B20-6 viewport controller evaluation

## Decision

Keep Aestra's current controller. Do not enable `bevy/pan_orbit_camera` in the
shipping editor or make it a prerequisite for the Bevy 0.20 upgrade.

The optional candidate could not be compiled in this environment: both offline
and network-enabled isolated qualification checks failed to resolve
`bevy_camera_controller = "^0.20.0"` from the configured crates.io index. The
reported candidates were 0.18 releases. This is an observed resolution result,
not a statement about other registries or future publication.

The temporary qualification feature/test scaffold was removed rather than leaving
an unresolvable optional graph behind. Neither workspace nor qualification
manifest/lockfile has a net dependency change. No fallback to another Bevy version,
new controller vendor copy or Git dependency was introduced.

This closes the bounded source/dependency evaluation with **adoption deferred**.
It does not claim native candidate parity, performance measurements or visual
acceptance. Existing navigation remains the tested implementation.

## Evidence and integration cost

Inspected the installed, pinned Bevy `=0.20.0` sources:

- `_release-content/release-notes/pan_orbit_camera.md`;
- `examples/camera/pan_orbit_camera_minimal.rs`;
- `examples/camera/pan_orbit_camera_custom_input_plugin.rs`.

Compared those examples with the shipping
`apps/aestra-editor/src/viewport/navigation.rs`, `lifecycle.rs`, `scene.rs`,
`plugin.rs` and gizmo state/interaction modules. Example behavior below is not
claimed to be a restriction of the unavailable controller implementation.

| Contract | Aestra today | Inspected native example / work still needed |
| --- | --- | --- |
| Orbit/pan | RMB orbit about retained effect focus; MMB pan | Example uses RMB orbit and LMB pan, and chooses anchors through picking. Remap input and preserve focus/feel deliberately. |
| Wheel | Exponential zoom; 50 pixel-wheel units equal one line; distance 1–4,000 | Example sends 150 units per line and one per pixel. Reusing it unchanged changes wheel response. Qualify limits and smoothing separately. |
| Flight | WASD without a held mouse button; normalized diagonals, Shift boost, real-time stall clamp | The inspected input example only supplies orbit/pan/zoom. Keep an application flight adapter; do not infer that adding the plugin replaces flight. |
| Framing | Reset about the single effect proxy, otherwise origin; defer while input is captured | Synchronize native motion/anchor state with frame requests and avoid competing transform writers. |
| Ownership | Canvas/browser boundaries, press-inside capture, modal/text/focus cancellation, gizmos | Still application policy. Remapping a mouse button does not implement these gates. |
| Scheduling | Navigation in Update; committed physical viewport published after UI layout; native TRS gizmos in PostUpdate | Example input/update runs in PreUpdate after picking. Qualify viewport readiness, DPI, minimize/restore and gizmo sequencing before changing this boundary. |

No reduction in application-owned input policy, no performance improvement and no
native visual benefit have been demonstrated. Do not add a second transform
writer merely to adopt an upstream component.

## Hardening retained navigation

The audit found that the camera did not explicitly yield to gizmo capture.
`PreviewInputCapture` now reads the existing public native TRS state and Aestra's
shape/TRS transaction state. While either owns an interaction, navigation:

- discards queued motion/wheel input and retires orbit/pan capture;
- prevents held flight keys from resuming until released;
- leaves frame-to-effect requests pending;
- reserves a left press on a previously hovered handle before later gizmo
  activation systems run. This uses committed hover state, not a new hit test.

Hover alone still permits explicit RMB orbit/MMB pan. No picking plugin,
camera hints, authored data or camera transform convention changed.

Three new tests in `viewport/tests/navigation.rs` exercise the actual navigation
system: native/shape/transaction capture and initial handle press, stale-input
discard/fresh-gesture recovery, and hover without capture. Existing navigation
tests remain the parity baseline for pan/orbit/flight, wheel units/limits,
modals/focus/browser ownership, framing, capture outside the viewport, idle
transform writes and ambiguous canvas ownership.

## Validation

Rust `1.98.1`, shipping Bevy `=0.20.0`:

- `cargo test -p aestra-editor --locked --offline -- --test-threads=1`:
  2,879 unit tests passed, 10 existing opt-in tests ignored; all five integration
  guards passed. All three new capture regressions passed.
- Strict all-target editor Clippy and the `patches-020` editor-qualification
  Clippy passed with `--locked --offline -- -D warnings`.
- `cargo fmt --all -- --check` and `git diff --check` passed.

These checks validate the retained controller and ownership guard, not the
unresolved native candidate. Native drag/gizmo/DPI/minimize acceptance remains
part of the editor UI acceptance pass.

## Revisit gate

Revisit only when the exact optional controller dependency resolves and there is
a concrete benefit to measure. Use an isolated, opt-in candidate with custom
input; run the same scripted input sequences against the current controller.
Require matching transforms/limits and ownership behavior, one authoritative
camera writer, and native docking/DPI/minimize/gizmo checks before adoption.
Removing custom code requires demonstrated parity, not just successful compile.
Do not reopen the completed engine compatibility migration for this optional work.
