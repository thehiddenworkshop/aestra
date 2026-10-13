# B20-2 slice 28: complete transport toolbar composition

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now imports
the complete production `transport.rs` plugin, including toolbar activation,
button handling/audit, play/pause icons, loop-mode synchronization and scene spawning.
It uses the real SVG loader, tooltip component and Feathers tool buttons.

- The shipping input adapter retains its original Interaction change filter.
  The 0.20 adapter reads actual Pressed/Hovered components, including removal
  transitions, without deprecated Button/Interaction aliases. Both feed the same
  toolbar handler and preserve native hover/press/disabled colors.
- The existing shared Activate queue and dispatcher consume accepted actions once.
  Disabled controls remain blocked and preserve their queued marker; unrelated
  activations are ignored. Only transient menus close, with a UI revision change
  only when tab context is removed.
- The actual production plugin registers both playback and toolbar systems.
  Candidate tests establish the same relative Input/Actions/Playback/Sync ordering;
  this is not a substitute plugin or a claim of whole-main scheduling parity.
- The candidate imports real icon/tooltip sources, not host-marker substitutes.
  Native scene consumers keep their existing entry points. Query filters are
  factored into aliases to satisfy strict linting without weakening it.
- Source comparisons confirm toolbar/control/icon scene composition, tooltip
  construction and icon/mode synchronization bodies are unchanged apart from
  imports, query aliases and formatting. No dependency or authored-format changes
  are included.

## Two behavior fixes exposed by real integration

1. Focused headless buttons already translate Space into Activate. The global
   transport shortcut now leaves Space to that button instead of also toggling
   playback. Enter still activates normally; global R and frame-step shortcuts
   retain their existing policy. Disabled focused buttons cannot toggle global
   playback as a side effect.
2. An accepted Activate remains authoritative if its visual pressed state clears
   before the Update handler runs. The shared handler consumes the pending action,
   still checks disabled state first, and does not treat a bare press as activation.
   The regression flushes real observer commands before simulating release so it
   actually exercises this ordering boundary.

Both fixes apply to shipping and candidate code; they are not candidate-only patches.

## Regression coverage

Nineteen new shared toolbar regressions, the two existing toolbar tests, and the
26 earlier transport tests form **47 passing transport checks** on the candidate.
New coverage includes:

- Single consumption, menu closure boundaries, no unnecessary panel rebuild,
  unclassified-button audit, bare-press rejection and disabled/unrelated activation.
- Native hover/press/release colors, held-press non-repeat and actual marker removal.
- The real four-action toolbar with localized accessibility labels/tooltips,
  28px controls and 34px toolbar height at 1x English and 2x French.
- Real play/stop activation and exclusive loop modes, toggle-back-to-Once and
  selection following session undo without respawning controls.
- Actual focused Space/Enter handling, no global playback toggle when activating
  Stop/loop controls, disabled/repeated keyboard input, recovery after re-enabling,
  and preserved R/arrow shortcuts while a toolbar button has focus.
- Actual headless pointer-click dispatch using native picking events and real
  pressed state, with disabled clicks rejected.
- Real asynchronous SVG loading, stable focused control/image identity across
  idle frames, shared raster handles across two toolbars, and release of SVG
  assets and their raster images after teardown.

Tests use real Feathers/UI/text/layout/visibility plugins with explicit headless
window/camera and no native renderer backend. Synthetic picking/keyboard messages
exercise production observers, not OS event-loop acceptance.

## Remaining scope

Transport toolbar composition is now in the candidate gate. Timeline, viewport,
material/function clipboard and other application action consumers remain open,
as do full panel/main-plugin composition and interactive native workflows.
The engine switch still requires B20-3; no new native-window/GPU acceptance is claimed.

Next: remaining timeline/viewport/clipboard keyboard and action consumers, then
complete editor workflows.

## Validation

The final shipping source snapshot passes **1,540 editor unit + 3 architecture
tests**, with ten existing ignores unchanged. Strict all-target Clippy passes on
both engines, as do workspace all-target checking with `aestra-bench/gpu`, both
formatting checks and whitespace validation. Existing Windows incremental-cache
hard-link fallback warnings do not fail the gates.

Accepted final frozen headless run (2026-10-14):
`target/bevy-020-qualification/runs/b528b654fef94b6e9b95af3445656b30/`.
It executes **864 checks**, including **445 graph/session/history checks**, with
all **747 input hashes** unchanged. One existing graph timing probe remains ignored.
Counts are executions across gates, not unique tests; SVG and tooltip dependencies
are exercised in this gate as well as their earlier dedicated fixtures.
The graph/session/history test process takes 14.19 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, and unchanged shipping lock
SHA-256 `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
`native_executed` is false. Root manifest/toolchain/lockfile diffs remain empty.
