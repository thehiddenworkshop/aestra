# B20-2 slice 26: actual shortcut ownership and tab navigation

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now imports
the complete production `input.rs` keyboard plugin and `ShortcutContext`, including
actual modal/menu/window/focus queries, browser-local ownership and tab eligibility.
The real application history keyboard wrapper also runs in the candidate; its
blocking decision is no longer replaced by an explicit test signal.

- Actual extraction-dialog, material palette/node/connection menu, function graph
  menu and module palette models move to bounded source files. Shipping reexports
  retain existing consumers. All payloads, defaults and `is_open` predicates are
  preserved. Candidate tests construct real state variants, not substitute booleans.
- BrowserSurface moves to the existing shared browser state source, with its old
  panel reexport preserved. Assets descendants remain isolated from panel-local
  shortcuts without disabling global history; editable descendants still block.
- The existing tab eligibility system and synchronous retained-popup restoration
  observer are shared unchanged. The small `input/api_019.rs` and `api_020.rs`
  adapters select the actual lifecycle observer type, upstream text-input plugin
  spelling and required test input marker. They contain no ownership policy.
- The actual editor root scene and window-size constants move to shared sources
  so the original tab-order regression uses production root composition on both
  engines. Layout, tab-group identity and size floors remain unchanged.
- Shared keyboard regressions use the real plugins and KeyboardInput messages.
  Real Tab/Shift+Tab coverage includes the public production visibility plugin and
  its dependencies, rather than manually inventing computed visibility.
- The candidate preserves existing focused text input, logical-letter mapping,
  event-time modifiers, repeat handling and focus-loss regressions. No control
  replacement, authored-format change or dependency update is included.

Source comparisons confirm the complete ShortcutContext policy, four extracted
state models, root scene and window floor constants are identical apart from
imports, internal visibility and formatting.

## Regression coverage

Twenty-two new shared tests cover:

- Missing optional resources, empty/stale focus and closed state models do not
  capture shortcuts. Grid display is not treated as menu ownership.
- Every document-protection branch, actual extraction with a retryable error, all
  three material palette/menu variants, both function-menu variants, all four main
  menus, panel submenu, tab context and About block without changing owner state.
- Real window focus supports headless fixtures and a focused floating window;
  all-unfocused windows suppress shortcuts. Focused editable/code/menu ancestors
  block, while unrelated siblings, reparenting and despawn release ownership.
- Browser surface descendants isolate local shortcuts while allowing global
  history; browser search descendants still own keyboard input.
- Actual AZERTY history chords traverse the full keyboard plugin, real ownership
  context and real undo router. Each modal/menu/palette/window/focus blocker prevents
  application history and does not close its owner. Clearing ownership does not
  replay old snapshots; a fresh chord executes exactly once.
- Nested hidden/disabled controls preserve exact tab indices, explicit opt-out and
  tab-order changes made while suppressed. Reparenting restores eligibility.
- Popup Closed/Opening/Open transitions restore only eligible children immediately,
  respecting hidden ancestors before the next PreUpdate.
- Real Tab and Shift+Tab messages skip hidden/disabled controls and observe
  same-frame restored eligibility before keyboard dispatch.

Eight existing input/tab tests additionally enter the candidate gate. Together,
all **30 input tests** pass on both engines. The original assertions remain intact.

## Remaining scope

This qualifies actual application shortcut ownership and history dispatch, not
every keyboard consumer. Transport/timeline/viewport/material clipboard and other
application action handlers still require their own composition gates. State
extraction does not qualify complete asset-extraction or graph-menu UI handlers.
Full panel/plugin composition and interactive native workflow acceptance remain
open; no new native window/GPU acceptance is claimed.

Next: remaining keyboard/action consumers, then complete editor panel/workflow
acceptance. The root engine switch still requires B20-3.

## Validation

Shipping passes **1,502 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines, as do workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation.

Accepted final frozen headless run (2026-10-13):
`target/bevy-020-qualification/runs/d13a14697d7344679bb582a22063f00b/`.
It executes **796 checks**, including **377 graph/session/history checks**, with
all **740 input hashes** unchanged. One existing graph timing probe remains ignored.
Counts are executions across gates, not unique tests. The graph/session/history
test process takes 8.73 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, and unchanged shipping lock
SHA-256 `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
`native_executed` is false. Root manifest/toolchain/lockfile diffs remain empty.
Existing Windows incremental-cache hard-link fallback warnings do not fail the gates.
