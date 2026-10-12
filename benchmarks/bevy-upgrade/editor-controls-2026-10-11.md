# B20-2 slice 2 — Shared scenes and single-line editing

## Implementation

`apps/aestra-editor/src/feathers/scenes/common.rs` holds 16 actual editor
Feathers scene wrappers, used by both the shipping editor and the isolated
0.20 fixture. Buttons, checkbox, text fields, sliders, scrollbars and menu
composition retain their existing settings. Numeric inputs, color-plane and
editor/dock geometry remain in `scenes.rs` pending their compatibility slices.

0.20 removes the slider's `value` scene property. The shared wrapper now applies
`SliderValue(value)` explicitly after constructing the scene, which works on
both versions. Regression tests verify a non-default initial value and range.
No replacement widget, palette change or geometry redesign is included.

The existing text composition and its three value observers are registered by
`EditorTextInputPlugin`, used by the shipping `AestraFeathersPlugin` and candidate
tests. Their behavior is unchanged: live edits originate at the field container;
Enter and focus loss submit a final value. The fixture imports the production
source rather than maintaining a second implementation of the bridge.

## Required regression gate

Eight tests in `feathers/text_input/tests.rs` run against both versions:

- Initial Unicode text, accessibility labels and field identity.
- Real keyboard Unicode typing through Feathers and the text editor.
- Exactly one Enter submission without inserting a newline.
- Exactly one focus-loss submission for the field being left.
- Independent text/value routing for separate fields.
- Exclusion of unrelated editable text from the editor value bridge.
- Programmatic text replacement followed by submission, preserving identity.
- Instantiation of all 16 shared wrappers and slider value/range preservation.

The isolated `patches-020/editor_controls.rs` adds two 0.20-specific checks:

- Escape clears input focus, emits one final value and still reaches an ancestor
  dialog observer with the original text-input target.
- The actual Feathers scene installs the new `TextInput` behavior marker on the
  editable child, not its container; removing it disables keyboard editing.

These headless tests run real scene composition, keyboard dispatch, text-edit
application and focus systems. They do not manually trigger `TextEditChange` or
fake bridge events. A primary-window entity supplies input identity, but no
native window or GPU is created. Pointer hit testing, complete modal cancellation,
IME/clipboard acceptance and visual parity are not claimed by this slice.

`run-020-patches.ps1` requires this gate with `editor-qualification`, freezes all
five production scene/plugin/text files as well as the fixture, and retains the
existing theme and native renderer gates unchanged. The root dependency graph
and shipping lockfile are not switched.

## Validation

Shipping editor validation: **1,179 unit tests and 3 architecture tests pass**;
the ten existing ignores are unchanged. The first shared harness used
`DefaultPlugins`, which initialized Winit under the shipping feature graph and
failed on test worker threads. It now installs explicit real headless dependencies
with GPU creation disabled, without suppressing any runtime validation. All eight
shared regressions pass in the complete shipping suite. Temporary project/settings
tests run with approved filesystem access.

Strict workspace all-target Clippy and checking with `aestra-bench/gpu`, strict
candidate all-target Clippy with `editor-qualification`, both formatting checks
and `git diff --check` pass. Existing incremental-cache hard-link warnings fall back to copying.
No lint suppression, reduced assertions or production editing behavior changes
are introduced.

Accepted source-frozen native run, completed **2026-10-12**:
`target/bevy-020-qualification/runs/f46cb1a2366548378de5017efbc03969/`.
Pinned MSVC Rust **1.98.1**, locked/offline dependencies: **278 checks pass**
(9 keyboard, 8 editor theme, 10 editor controls, 40 upstream focus, 4 cluster units,
4 shader, 3 storage, 166 shared/candidate checks and **34 explicit native gates**).
All **475 input hashes and five native executable hashes** remain unchanged.
The editor-controls gate completes in **2.79 seconds**. The renderer gates run
serially on NVIDIA RTX 4070 SUPER / Vulkan 616.92 and AMD integrated Vulkan 26.3.1.
This is correctness evidence, not visual/performance parity approval.

The accepted summary retains these SHA-256 identities:

- Synchronous extraction executable:
  `da5a788aa304034dad0ba0be1e9b3b840106c27dc98acc9811c1a68f86a295c5`.
- Asynchronous extraction executable:
  `c7c499cc9f2ce78cb004577efce0fbb6130c944be14f9f29c5a6b79968b743d3`.
- Unchanged shipping lockfile:
  `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.

Earlier native attempts interrupted before producing an acceptance summary are
not used as acceptance evidence. The final run uses the corrected headless
harness throughout.

## Remaining work

B20-2 is still in progress. Next qualify the remaining numeric/color-plane scene
composition and pointer/cursor boundary, then custom code/search consumers and
the full editor workflow. Preserve numeric scrub/arrow behavior, menu geometry,
settings header styling, project creation, recovery and save/reopen acceptance.
Shipping remains Bevy 0.19.1; the isolated candidate is Bevy 0.20.0 / wgpu 30.0.1.
The coherent root graph switch and final native visual/performance parity remain
separate release gates.
