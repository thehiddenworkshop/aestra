# B20-2 slice 6 — Custom text, search and context menus

The isolated Bevy 0.20 fixture now compiles the **production** custom code
editor, search fields, context menus and action-button bridge, with real
Feathers scenes, input dispatch, text editing, rendering composition and focus
plugins. Shipping remains Bevy 0.19.1; root manifests and lockfile are unchanged.

## Compatibility and fixes

- A small pointer boundary selects 0.19 generic events/cursor imports or 0.20
  flat events/picking cursor imports. Menu hover, sibling flyouts and click
  containment use the same production observers on both graphs.
- Action controls retain the shipping legacy button audit and pressed-state
  contract. The candidate uses the native widget button and `Pressed` marker,
  avoiding deprecated 0.20 aliases. This qualifies the button bridge, **not**
  the surrounding application's still-unported action dispatcher.
- Code-editor pointer focus now calls `InputFocus::set` with the appropriate
  cause instead of replacing the resource and dropping buffered focus changes.
  Leaving a single-line field by clicking code must still submit that field.
- Search Clear relies on the native text-edit pass and existing container
  bridge for its single live change, rather than also emitting a duplicate.
  Empty fields and disabled controls/ancestors cannot clear or emit changes.
- Context-menu tests run the actual headless UI/visibility schedule. Bevy 0.20
  navigation checks inherited visibility; the previous minimal test schedule
  left UI nodes hidden and falsely reported the menu closed. No visibility
  eligibility check is bypassed in production or in the candidate patch.

## Required regression gate

`editor-text` is a required locked/offline gate in the source-frozen runner.
It executes 32 checks: five new code-plugin regressions, three new search
regressions, two new menu regressions and one candidate action-bridge check,
alongside seven existing code tests, three search tests, three menu tests and
eight real text/scene dependency checks. The dependency checks intentionally
repeat earlier gates; counts represent executions, not unique tests.

New coverage includes Unicode/multiline editing, isolation between editors,
real same-frame layout-aware Ctrl+Z/Ctrl+Y and Shift selection, change counts,
active-editor retention through menu focus, history-scope changes, buffered
focus-loss submission, context-menu select/undo actions, ArrowRight/ArrowLeft
flyout navigation, hover/sibling closing, click containment, search retyping,
one clear notification and empty/disabled clear rejection.

All widget implementations are imported from shipping source. Only host action
identity, the history-scope identity and the graph navigation blocker marker
are fixture-local stand-ins; no code/search/menu behavior is substituted.
The candidate enables `bevy_clipboard` for the custom editor's resource; its
headless configuration uses Bevy's in-process fallback, not the OS clipboard.
These tests do not read or overwrite user clipboard contents.

## Remaining scope

This is headless composition/event/focus qualification, not a full editor port
or native visual acceptance. OS hit testing, cursor/DPI behavior, scrolling,
syntax/error visual appearance, OS clipboard/IME flows and full editor workflows
remain to be exercised. Clipboard permission/error handling and asynchronous
web paste are not newly qualified by this slice.

Next: remaining pointer consumers, starting with tooltips and graph interaction,
then application action dispatch and complete workflow acceptance before the
coherent root dependency switch. Keep optional widget replacement separate.

## Validation

Accepted source-frozen **headless** run:
`target/bevy-020-qualification/runs/72f0518d7b694e96b179d485b6963f97/`.
Pinned MSVC Rust 1.98.1, locked/offline: **393 executed checks pass**
(9 keyboard, 8 theme, 18 editor controls, 46 numeric, 63 color/dependency,
32 custom text/menu/dependency, 40 upstream focus, 4 cluster units, 4 shader,
3 storage, 166 shared/candidate). All **556 input hashes** remain unchanged.
The new text gate takes **3.22 seconds** after compilation. Shipping lock SHA256
remains `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.

Shipping: **1,217 editor unit and 3 architecture tests pass**, with the ten
existing ignores unchanged. The approved filesystem run permits the existing
temporary project/settings fixtures. Strict all-target workspace Clippy with
`aestra-bench/gpu`, strict all-target candidate Clippy with
`editor-qualification`, both formatting checks and whitespace validation pass.
All-target workspace checking with `aestra-bench/gpu` also passes.
Existing incremental-cache hard-link warnings fall back to copying. The first
candidate run required updating only its isolated lockfile offline to select
the already-locked Bevy clipboard dependency; final evidence is locked/offline.

Renderer-native gates were not rerun for this UI-only slice; earlier evidence
remains separately dated. This run does not claim full-editor native acceptance.
