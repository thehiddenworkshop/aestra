# Requires PowerShell 7. Native GPU tests are opt-in and run in separate processes.
[CmdletBinding()]
param(
    [string]$ReportsDirectory,
    [string]$Toolchain = '1.98.1-x86_64-pc-windows-msvc',
    [switch]$Native
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$manifest = Join-Path $PSScriptRoot 'patches-020/Cargo.toml'
$target = Join-Path $root 'target/bevy-020-qualification'
if (!$ReportsDirectory) {
    $ReportsDirectory = Join-Path $target ('runs/' + [Guid]::NewGuid().ToString('N'))
}
$reports = [IO.Path]::GetFullPath($ReportsDirectory)
if (Test-Path -LiteralPath $reports) { throw 'Use a fresh report directory; evidence is never overwritten.' }
New-Item -ItemType Directory -Path $reports -Force | Out-Null

function Hash([string]$Path) {
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}
function Inputs {
    $paths = @('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml')
    # A direct Cargo/format invocation may create a nested target directory.
    # Generated compiler metadata is not source input; retain all actual fixture files.
    $fixtureTarget = (Join-Path $PSScriptRoot 'patches-020/target') + [IO.Path]::DirectorySeparatorChar
    $paths += @(Get-ChildItem -LiteralPath (Join-Path $PSScriptRoot 'patches-020') -File -Recurse |
        Where-Object { !$_.FullName.StartsWith($fixtureTarget, [StringComparison]::OrdinalIgnoreCase) } |
        ForEach-Object { [IO.Path]::GetRelativePath($root, $_.FullName).Replace('\', '/') })
    $paths += 'benchmarks/bevy-upgrade/keyboard-020.rs', 'benchmarks/bevy-upgrade/run-020-patches.ps1'
    $paths += @(Get-ChildItem -LiteralPath (Join-Path $root 'apps/aestra-editor/src/feathers/node_graph') -File -Recurse |
        ForEach-Object { [IO.Path]::GetRelativePath($root, $_.FullName).Replace('\', '/') })
    $paths += 'benchmarks/graph-layout/resize_profile.rs'
    $paths += @(
        'apps/aestra-editor/src/theme.rs',
        'apps/aestra-editor/src/theme/palette.rs',
        'apps/aestra-editor/src/theme/bevy_019.rs',
        'apps/aestra-editor/src/theme/bevy_020.rs',
        'apps/aestra-editor/src/theme/tests.rs',
        'apps/aestra-editor/src/feathers/mod.rs',
        'apps/aestra-editor/src/feathers/scenes.rs',
        'apps/aestra-editor/src/feathers/scenes/common.rs',
        'apps/aestra-editor/src/feathers/scenes/viewport.rs',
        'apps/aestra-editor/src/feathers/text_input.rs',
        'apps/aestra-editor/src/feathers/text_input/tests.rs',
        'apps/aestra-editor/src/feathers/pointer_019.rs',
        'apps/aestra-editor/src/feathers/pointer_020.rs',
        'apps/aestra-editor/src/feathers/button.rs',
        'apps/aestra-editor/src/feathers/action_activation.rs',
        'apps/aestra-editor/src/feathers/action_activation/tests.rs',
        'apps/aestra-editor/src/feathers/button/activation_019.rs',
        'apps/aestra-editor/src/feathers/button/activation_020.rs',
        'apps/aestra-editor/src/feathers/context_menu.rs',
        'apps/aestra-editor/src/feathers/code_editor.rs',
        'apps/aestra-editor/src/feathers/code_editor/buffer.rs',
        'apps/aestra-editor/src/feathers/code_editor/buffer/tests.rs',
        'apps/aestra-editor/src/feathers/code_editor/interaction_tests.rs',
        'apps/aestra-editor/src/feathers/search_field.rs',
        'apps/aestra-editor/src/feathers/search_field/interaction_tests.rs',
        'apps/aestra-editor/src/feathers/tooltip.rs',
        'apps/aestra-editor/src/feathers/tooltip/interaction_tests.rs',
        'apps/aestra-editor/src/feathers/number_input.rs',
        'apps/aestra-editor/src/feathers/number_input/widget.rs',
        'apps/aestra-editor/src/feathers/number_input/widget_tests.rs',
        'apps/aestra-editor/src/feathers/number_input/arrows.rs',
        'apps/aestra-editor/src/feathers/icon.rs',
        'apps/aestra-editor/src/feathers/icon/svg.rs',
        'apps/aestra-editor/src/localization.rs',
        'apps/aestra-editor/locales/en-US/editor.ftl',
        'apps/aestra-editor/locales/fr-FR/editor.ftl',
        'apps/aestra-editor/src/main.rs',
        'apps/aestra-editor/src/editor_identity.rs',
        'apps/aestra-editor/src/document.rs',
        'apps/aestra-editor/src/document_registry.rs',
        'apps/aestra-editor/src/editor_view.rs',
        'apps/aestra-editor/src/editor_view_registry.rs',
        'apps/aestra-editor/src/editor_target.rs',
        'apps/aestra-editor/src/editor_target/tests.rs',
        'apps/aestra-editor/src/material_document.rs',
        'apps/aestra-editor/src/material_target.rs',
        'apps/aestra-editor/src/material_graph.rs',
        'apps/aestra-editor/src/material_graph/selection.rs',
        'apps/aestra-editor/src/timeline.rs',
        'apps/aestra-editor/src/timeline/actions.rs',
        'apps/aestra-editor/src/timeline/state.rs',
        'apps/aestra-editor/src/timeline/interaction_state.rs',
        'apps/aestra-editor/src/timeline/navigation.rs',
        'apps/aestra-editor/src/timeline/gesture_math.rs',
        'apps/aestra-editor/src/timeline/clip_gestures.rs',
        'apps/aestra-editor/src/timeline/clip_gestures/tests.rs',
        'apps/aestra-editor/src/timeline/gestures.rs',
        'apps/aestra-editor/src/timeline/gestures/tests.rs',
        'apps/aestra-editor/src/timeline/region_gestures.rs',
        'apps/aestra-editor/src/timeline/pointer_019.rs',
        'apps/aestra-editor/src/timeline/pointer_020.rs',
        'apps/aestra-editor/src/timeline/navigation/tests.rs',
        'apps/aestra-editor/src/timeline/cursor_019.rs',
        'apps/aestra-editor/src/timeline/cursor_020.rs',
        'apps/aestra-editor/src/feathers/context_menu/dismissal.rs',
        'apps/aestra-editor/src/feathers/context_menu/intent.rs',
        'apps/aestra-editor/src/timeline/clicks.rs',
        'apps/aestra-editor/src/asset_drop/transport.rs',
        'apps/aestra-editor/src/asset_drop.rs',
        'apps/aestra-editor/src/timeline/asset_drop.rs',
        'apps/aestra-editor/src/timeline/asset_drop/tests.rs',
        'apps/aestra-editor/src/timeline/asset_drop/source.rs',
        'apps/aestra-editor/src/timeline/drop_execution.rs',
        'apps/aestra-editor/src/timeline/drop_execution/tests.rs',
        'apps/aestra-editor/src/timeline/reorder.rs',
        'apps/aestra-editor/src/timeline/reorder/tests.rs',
        'apps/aestra-editor/src/timeline/presentation.rs',
        'apps/aestra-editor/src/timeline/presentation/tests.rs',
        'apps/aestra-editor/src/timeline/projections.rs',
        'apps/aestra-editor/src/timeline/lane_composition.rs',
        'apps/aestra-editor/src/timeline/lane_composition/tests.rs',
        'apps/aestra-editor/src/timeline/track_style.rs',
        'apps/aestra-editor/src/timeline/effect_clip_composition.rs',
        'apps/aestra-editor/src/timeline/effect_clip_composition/tests.rs',
        'apps/aestra-editor/src/timeline/emitter_composition.rs',
        'apps/aestra-editor/src/timeline/emitter_composition/tests.rs',
        'apps/aestra-editor/src/timeline/cue_composition.rs',
        'apps/aestra-editor/src/timeline/cue_composition/tests.rs',
        'apps/aestra-editor/src/timeline/toolbar_composition.rs',
        'apps/aestra-editor/src/timeline/toolbar_composition/tests.rs',
        'apps/aestra-editor/src/timeline/track_composition.rs',
        'apps/aestra-editor/src/timeline/track_composition/tests.rs',
        'apps/aestra-editor/src/timeline/host_composition.rs',
        'apps/aestra-editor/src/timeline/host_composition/tests.rs',
        'apps/aestra-editor/src/timeline/inspector_composition.rs',
        'apps/aestra-editor/src/timeline/inspector_composition/tests.rs',
        'apps/aestra-editor/src/timeline/panel_composition.rs',
        'apps/aestra-editor/src/timeline/panel_composition/tests.rs',
        'apps/aestra-editor/src/curves/action.rs',
        'apps/aestra-editor/src/shell/scroll_key.rs',
        'apps/aestra-editor/src/feathers/button/action.rs',
        'apps/aestra-editor/src/feathers/field_row.rs',
        'apps/aestra-editor/src/timeline/scroll_composition.rs',
        'apps/aestra-editor/src/timeline/scroll_composition/tests.rs',
        'apps/aestra-editor/src/feathers/combo_box.rs',
        'apps/aestra-editor/src/feathers/combo_box/selection.rs',
        'apps/aestra-editor/src/feathers/combo_box/selection/tests.rs',
        'apps/aestra-editor/src/feathers/combo_box/selection_input_019.rs',
        'apps/aestra-editor/src/feathers/combo_box/selection_input_020.rs',
        'apps/aestra-editor/src/shell.rs',
        'apps/aestra-editor/src/shell/action.rs',
        'apps/aestra-editor/src/asset_actions/action.rs',
        'apps/aestra-editor/src/feathers/context_menu/composition.rs',
        'apps/aestra-editor/src/timeline/referenced_effect/composition_tests.rs',
        'apps/aestra-editor/src/feathers/button/tool.rs',
        'apps/aestra-editor/src/properties/module_data.rs',
        'apps/aestra-editor/src/timeline/action_controls.rs',
        'apps/aestra-editor/src/timeline/action_controls/tests.rs',
        'apps/aestra-editor/src/timeline/control_input_019.rs',
        'apps/aestra-editor/src/timeline/control_input_020.rs',
        'apps/aestra-editor/src/feathers/list_row.rs',
        'apps/aestra-editor/src/feathers/list_row/state.rs',
        'apps/aestra-editor/src/timeline/clicks/tests.rs',
        'apps/aestra-editor/src/timeline/motion_state.rs',
        'apps/aestra-editor/src/timeline/motion_execution.rs',
        'apps/aestra-editor/src/timeline/motion_gestures.rs',
        'apps/aestra-editor/src/timeline/motion_gestures/tests.rs',
        'apps/aestra-editor/src/timeline/execution.rs',
        'apps/aestra-editor/src/timeline/region_execution.rs',
        'apps/aestra-editor/src/timeline/referenced_execution.rs',
        'apps/aestra-editor/src/timeline/execution_tests.rs',
        'apps/aestra-editor/src/timeline/automation_execution.rs',
        'apps/aestra-editor/src/timeline/automation_gestures.rs',
        'apps/aestra-editor/src/timeline/automation_gestures/tests.rs',
        'apps/aestra-editor/src/timeline/action_execution_tests.rs',
        'apps/aestra-editor/src/curves.rs',
        'apps/aestra-editor/src/curves/state.rs',
        'apps/aestra-editor/src/curves/property_editing.rs',
        'apps/aestra-editor/src/curves/property_editing/tests.rs',
        'apps/aestra-editor/src/curves/controls.rs',
        'apps/aestra-editor/src/curves/controls/tests.rs',
        'apps/aestra-editor/src/curves/composition.rs',
        'apps/aestra-editor/src/curves/composition/tests.rs',
        'apps/aestra-editor/src/curves/host_composition.rs',
        'apps/aestra-editor/src/curves/host_install.rs',
        'apps/aestra-editor/src/curves/plugin.rs',
        'apps/aestra-editor/src/curves/plugin/tests.rs',
        'apps/aestra-editor/src/timeline/panel_input.rs',
        'apps/aestra-editor/src/properties/stack_button.rs',
        'apps/aestra-editor/src/curves/control_input_019.rs',
        'apps/aestra-editor/src/curves/control_input_020.rs',
        'apps/aestra-editor/src/curves/keyboard.rs',
        'apps/aestra-editor/src/curves/host_editing.rs',
        'apps/aestra-editor/src/docking/model.rs',
        'apps/aestra-editor/src/shell/reveal.rs',
        'apps/aestra-editor/src/curves/host_motion.rs',
        'apps/aestra-editor/src/timeline/automation_data.rs',
        'apps/aestra-editor/src/timeline/keyboard.rs',
        'apps/aestra-editor/src/timeline/keyboard/tests.rs',
        'apps/aestra-editor/src/timeline/automation.rs',
        'apps/aestra-editor/src/timeline/regions.rs',
        'apps/aestra-editor/src/timeline/referenced_effect.rs',
        'apps/aestra-editor/src/timeline/host_motion.rs',
        'apps/aestra-editor/src/timeline/host_motion_install.rs',
        'apps/aestra-editor/src/timeline/plugin.rs',
        'apps/aestra-editor/src/asset_drop/payload.rs',
        'apps/aestra-editor/src/asset_drop/payload/model.rs',
        'apps/aestra-editor/src/feathers/automation_curve.rs',
        'apps/aestra-editor/src/timeline/document_actions.rs',
        'apps/aestra-editor/src/timeline/document_actions/tests.rs',
        'apps/aestra-editor/src/timeline/document_actions/native_tests.rs',
        'apps/aestra-editor/src/material_graph/edit_context.rs',
        'apps/aestra-editor/src/material_graph/execution.rs',
        'apps/aestra-editor/src/material_graph/keyboard.rs',
        'apps/aestra-editor/src/material_graph/keyboard/tests.rs',
        'apps/aestra-editor/src/material_graph/clipboard/execution.rs',
        'apps/aestra-editor/src/properties/material_state.rs',
        'apps/aestra-editor/src/material_graph/clipboard.rs',
        'apps/aestra-editor/src/material_graph/clipboard/tests.rs',
        'apps/aestra-editor/src/material_graph/clipboard/model.rs',
        'apps/aestra-editor/src/material_graph/clipboard/model/tests.rs',
        'apps/aestra-editor/src/material_function_editor/graph/clipboard_context.rs',
        'apps/aestra-editor/src/material_function_editor/graph/clipboard.rs',
        'apps/aestra-editor/src/material_function_editor/graph/clipboard/tests.rs',
        'apps/aestra-editor/src/material_function_editor/graph/clipboard/execution.rs',
        'apps/aestra-editor/src/material_function_editor/graph/clipboard/execution/tests.rs',
        'apps/aestra-editor/src/material_graph/command_preparation.rs',
        'apps/aestra-editor/src/material_graph/command_preparation/tests.rs',
        'apps/aestra-editor/src/history.rs',
        'apps/aestra-editor/src/history/focus.rs',
        'apps/aestra-editor/src/history/focus_pointer_019.rs',
        'apps/aestra-editor/src/history/focus_pointer_020.rs',
        'apps/aestra-editor/src/history/focus/tests.rs',
        'apps/aestra-editor/src/history/ownership.rs',
        'apps/aestra-editor/src/history/ownership/tests.rs',
        'apps/aestra-editor/src/persistence.rs',
        'apps/aestra-editor/src/persistence/state.rs',
        'apps/aestra-editor/src/history/action.rs',
        'apps/aestra-editor/src/history/asset_order.rs',
        'apps/aestra-editor/src/history/asset_order/tests.rs',
        'apps/aestra-editor/src/history/synchronization.rs',
        'apps/aestra-editor/src/history/synchronization/tests.rs',
        'apps/aestra-editor/src/history/compound.rs',
        'apps/aestra-editor/src/history/compound/tests.rs',
        'apps/aestra-editor/src/history/routing.rs',
        'apps/aestra-editor/src/history/routing/tests.rs',
        'apps/aestra-editor/src/history/controls.rs',
        'apps/aestra-editor/src/history/control_input_019.rs',
        'apps/aestra-editor/src/history/control_input_020.rs',
        'apps/aestra-editor/src/history/controls/tests.rs',
        'apps/aestra-editor/src/history/keyboard.rs',
        'apps/aestra-editor/src/input.rs',
        'apps/aestra-editor/src/input/keypresses.rs',
        'apps/aestra-editor/src/viewport.rs',
        'apps/aestra-editor/src/viewport/scene.rs',
        'apps/aestra-editor/src/viewport/plugin.rs',
        'apps/aestra-editor/src/viewport/plugin/tests.rs',
        'apps/aestra-editor/src/viewport/scene/tests.rs',
        'apps/aestra-editor/src/viewport/workspace.rs',
        'apps/aestra-editor/src/viewport/workspace/tests.rs',
        'apps/aestra-editor/src/viewport/gizmo_state.rs',
        'apps/aestra-editor/src/viewport/shape_gizmo.rs',
        'apps/aestra-editor/src/viewport/shape_gizmo/tests.rs',
        'apps/aestra-editor/src/viewport/transform_gizmo.rs',
        'apps/aestra-editor/src/viewport/transform_gizmo/tests.rs',
        'apps/aestra-editor/src/viewport/host_motion.rs',
        'apps/aestra-editor/src/viewport/host_motion/tests.rs',
        'apps/aestra-editor/src/viewport/shortcuts.rs',
        'apps/aestra-editor/src/viewport/navigation.rs',
        'apps/aestra-editor/src/viewport/navigation/tests.rs',
        'apps/aestra-editor/src/viewport/lifecycle.rs',
        'apps/aestra-editor/src/viewport/lifecycle/tests.rs',
        'apps/aestra-editor/src/viewport/photographic.rs',
        'apps/aestra-editor/src/viewport/photographic/tests.rs',
        'apps/aestra-editor/src/viewport/presentation.rs',
        'apps/aestra-editor/src/viewport/presentation/tests.rs',
        'apps/aestra-editor/src/viewport/runtime_preview.rs',
        'apps/aestra-editor/src/viewport/runtime_preview/tests.rs',
        'apps/aestra-editor/src/viewport/feedback.rs',
        'apps/aestra-editor/src/viewport/feedback/tests.rs',
        'apps/aestra-editor/src/viewport/telemetry.rs',
        'apps/aestra-editor/src/viewport/telemetry/tests.rs',
        'apps/aestra-editor/src/viewport/telemetry/candidate_tests.rs',
        'apps/aestra-editor/src/profiler.rs',
        'apps/aestra-editor/src/profiler/plugin.rs',
        'apps/aestra-editor/src/profiler/controls.rs',
        'apps/aestra-editor/src/profiler/control_input_019.rs',
        'apps/aestra-editor/src/profiler/control_input_020.rs',
        'apps/aestra-editor/src/profiler/workspace.rs',
        'apps/aestra-editor/src/profiler/workspace/tests.rs',
        'apps/aestra-editor/src/feathers/panel.rs',
        'apps/aestra-editor/src/profiler/state.rs',
        'apps/aestra-editor/src/profiler/state/tests.rs',
        'bevy/aestra-bevy-render/src/gpu/event_statistics.rs',
        'bevy/aestra-bevy-render/src/gpu/geometry_sample.rs',
        'bevy/aestra-bevy-render/src/gpu/field_observations.rs',
        'bevy/aestra-bevy-render/src/gpu/simulation_observation.rs',
        'bevy/aestra-bevy-render/src/gpu/preparation_observation.rs',
        'bevy/aestra-bevy-render/src/gpu/timestamp_observation.rs',
        'assets/shaders/preview_grid.wesl',
        'apps/aestra-editor/src/viewport/actions.rs',
        'apps/aestra-editor/src/viewport/actions/tests.rs',
        'apps/aestra-editor/src/viewport/control_input_019.rs',
        'apps/aestra-editor/src/viewport/control_input_020.rs',
        'apps/aestra-editor/src/persistence/status.rs',
        'apps/aestra-editor/src/docking/maximized.rs',
        'apps/aestra-editor/src/settings.rs',
        'benchmarks/bevy-upgrade/patches-020/editor_viewport.rs',
        'apps/aestra-editor/src/viewport/keyboard.rs',
        'apps/aestra-editor/src/viewport/keyboard/tests.rs',
        'apps/aestra-editor/src/transport.rs',
        'apps/aestra-editor/src/transport/control_input_019.rs',
        'apps/aestra-editor/src/transport/control_input_020.rs',
        'apps/aestra-editor/src/transport/toolbar_tests.rs',
        'apps/aestra-editor/src/transport/playback.rs',
        'apps/aestra-editor/src/transport/playback/tests.rs',
        'apps/aestra-editor/src/transport/playback/qualification.rs',
        'apps/aestra-editor/src/input/tab_navigation.rs',
        'apps/aestra-editor/src/input/tests.rs',
        'apps/aestra-editor/src/input/ownership_tests.rs',
        'apps/aestra-editor/src/input/api_019.rs',
        'apps/aestra-editor/src/input/api_020.rs',
        'apps/aestra-editor/src/asset_actions.rs',
        'apps/aestra-editor/src/asset_actions/state.rs',
        'apps/aestra-editor/src/material_graph/palette_state.rs',
        'apps/aestra-editor/src/material_graph/connections.rs',
        'apps/aestra-editor/src/material_graph/palette.rs',
        'apps/aestra-editor/src/material_graph/palette/tests.rs',
        'apps/aestra-editor/src/material_graph/menu_actions.rs',
        'apps/aestra-editor/src/material_graph/menu_actions/tests.rs',
        'apps/aestra-editor/src/material_graph/menu_opening.rs',
        'apps/aestra-editor/src/material_graph/menu_opening/tests.rs',
        'apps/aestra-editor/src/material_graph/sockets.rs',
        'apps/aestra-editor/src/material_graph/connection_drag.rs',
        'apps/aestra-editor/src/material_graph/connection_drag/tests.rs',
        'apps/aestra-editor/src/material_graph/connection_state.rs',
        'apps/aestra-editor/src/material_graph/node_actions.rs',
        'apps/aestra-editor/src/material_graph/node_actions/tests.rs',
        'apps/aestra-editor/src/material_graph/node_controls.rs',
        'apps/aestra-editor/src/material_graph/controls.rs',
        'apps/aestra-editor/src/material_graph/controls/tests.rs',
        'apps/aestra-editor/src/material_graph/header.rs',
        'apps/aestra-editor/src/material_graph/header/tests.rs',
        'apps/aestra-editor/src/material_graph/workspace_layout.rs',
        'apps/aestra-editor/src/material_graph/workspace_layout/tests.rs',
        'apps/aestra-editor/src/material_graph/appearance.rs',
        'apps/aestra-editor/src/material_graph/appearance/tests.rs',
        'apps/aestra-editor/src/material_graph/appearance_019.rs',
        'apps/aestra-editor/src/material_graph/appearance_020.rs',
        'apps/aestra-editor/src/material_graph/presentation/preview.rs',
        'apps/aestra-editor/src/asset_browser/actions/locate.rs',
        'apps/aestra-editor/src/asset_browser/actions.rs',
        'apps/aestra-editor/src/asset_browser/actions/open_function.rs',
        'apps/aestra-editor/src/material_graph/drag_019.rs',
        'apps/aestra-editor/src/material_graph/drag_020.rs',
        'apps/aestra-editor/src/material_graph/click_019.rs',
        'apps/aestra-editor/src/material_graph/click_020.rs',
        'apps/aestra-editor/src/material_graph/control_input_019.rs',
        'apps/aestra-editor/src/material_graph/control_input_020.rs',
        'apps/aestra-editor/src/material_graph/node_metrics.rs',
        'apps/aestra-editor/src/material_graph/arrange/request.rs',
        'apps/aestra-editor/src/material_graph/arrange.rs',
        'apps/aestra-editor/src/material_function_editor/graph/menu_state.rs',
        'apps/aestra-editor/src/properties.rs',
        'apps/aestra-editor/src/properties/palette_state.rs',
        'apps/aestra-editor/src/asset_browser/panel.rs',
        'apps/aestra-editor/src/dock_ui.rs',
        'apps/aestra-editor/src/dock_ui/metrics.rs',
        'apps/aestra-editor/src/feathers/scenes/root.rs',
        'apps/aestra-editor/src/menus.rs',
        'apps/aestra-editor/src/menus/state.rs',
        'apps/aestra-editor/src/docking/panels.rs',
        'apps/aestra-editor/src/asset_browser/deletion.rs',
        'apps/aestra-editor/src/asset_browser/deletion/documents.rs',
        'apps/aestra-editor/src/asset_browser/deletion/reference_checks.rs',
        'apps/aestra-editor/src/asset_browser/deletion/history.rs',
        'apps/aestra-editor/src/asset_browser/deletion/history/tests.rs',
        'apps/aestra-editor/src/asset_browser/deletion/panel.rs',
        'apps/aestra-editor/src/asset_browser/deletion/panel/tests.rs',
        'apps/aestra-editor/src/asset_browser/state.rs',
        'apps/aestra-editor/src/asset_browser/bookmarks.rs',
        'apps/aestra-editor/src/asset_browser/bookmarks/model.rs',
        'apps/aestra-editor/src/feathers/scroll.rs',
        'apps/aestra-editor/src/feathers/scroll/bars.rs',
        'apps/aestra-editor/src/asset_browser/inspection.rs',
        'apps/aestra-editor/src/history/asset_order/model.rs',
        'apps/aestra-editor/src/history/asset_order/model/tests.rs',
        'apps/aestra-editor/src/material_graph/presentation.rs',
        'apps/aestra-editor/src/material_graph/presentation/tests.rs',
        'apps/aestra-editor/src/material_graph/presentation/transaction.rs',
        'apps/aestra-editor/src/material_graph/presentation/context.rs',
        'apps/aestra-editor/src/material_graph/presentation/state.rs',
        'apps/aestra-editor/src/material_graph/preview_state.rs',
        'apps/aestra-editor/src/material_graph/node_previews.rs',
        'apps/aestra-editor/src/material_graph/graph_inventory.rs',
        'apps/aestra-editor/src/material_graph/presentation/transaction/tests.rs',
        'apps/aestra-editor/src/material_graph/preview_target.rs',
        'apps/aestra-editor/src/material_graph/keys.rs',
        'apps/aestra-editor/src/material_graph/layout_lifecycle.rs',
        'apps/aestra-editor/src/material_graph/function_layout.rs',
        'apps/aestra-editor/src/material_graph/presentation/semantic.rs',
        'apps/aestra-editor/src/material_graph/presentation/semantic/tests.rs',
        'apps/aestra-editor/src/material_function_editor.rs',
        'apps/aestra-editor/src/material_function_editor/graph.rs',
        'apps/aestra-editor/src/material_function_editor/history.rs',
        'apps/aestra-editor/src/material_function_editor/history/tests.rs',
        'assets/test/materials/pulse_wave.aestra.material-function.ron',
        'apps/aestra-editor/src/history/material_program.rs',
        'apps/aestra-editor/src/history/material_program/tests.rs',
        'apps/aestra-editor/src/history/publication.rs',
        'apps/aestra-editor/src/history/publication/tests.rs',
        'apps/aestra-editor/src/history/material_program_execution.rs',
        'apps/aestra-editor/src/history/document_execution.rs',
        'apps/aestra-editor/src/history/document_execution/tests.rs',
        'apps/aestra-editor/src/history/asset_order/session_context.rs',
        'apps/aestra-editor/src/material_document/context.rs',
        'apps/aestra-editor/src/material_function_editor/execution.rs',
        'apps/aestra-editor/src/session.rs',
        'apps/aestra-editor/src/session/preview_tests.rs',
        'apps/aestra-editor/src/test_support.rs',
        'apps/aestra-editor/src/preview_mocks.rs',
        'assets/test/effects/material_graph_lab.aestra.ron',
        'assets/test/materials/material_graph_lab.aestra.material.ron',
        'apps/aestra-editor/src/material_drafts.rs',
        'apps/aestra-editor/src/project_content/mod.rs',
        'apps/aestra-editor/src/project_content/model.rs',
        'apps/aestra-editor/src/project_content/catalog_tests.rs',
        'apps/aestra-editor/src/project_content/refresh.rs',
        'apps/aestra-editor/src/project_content/refresh/tests.rs',
        'apps/aestra-editor/src/project_content/io.rs',
        'apps/aestra-editor/src/project_content/io/tests.rs',
        'apps/aestra-editor/src/project_content/plugin.rs',
        'apps/aestra-editor/src/project_content/services.rs',
        'apps/aestra-editor/src/project_content/services/tests.rs',
        'apps/aestra-editor/src/docking.rs',
        'apps/aestra-editor/src/wesl_document.rs',
        'apps/aestra-editor/src/feathers/color_picker.rs',
        'apps/aestra-editor/src/feathers/color_picker/interaction_tests.rs',
        'apps/aestra-editor/src/feathers/color_picker/pointer_019.rs',
        'apps/aestra-editor/src/feathers/color_picker/pointer_020.rs',
        'apps/aestra-editor/src/feathers/node_graph.rs',
        'apps/aestra-editor/src/feathers/node_graph/geometry/tests.rs',
        'apps/aestra-editor/src/timeline/host_motion.rs',
        'apps/aestra-editor/src/feathers/number_input/scrub.rs',
        'apps/aestra-editor/src/feathers/number_input/scrub_tests.rs',
        'apps/aestra-editor/src/feathers/number_input/pointer_019.rs',
        'apps/aestra-editor/src/feathers/number_input/pointer_020.rs',
        'bevy/aestra-bevy-render/src/gpu.rs',
        'bevy/aestra-bevy-render/src/lib.rs',
        'bevy/aestra-bevy-render/src/render_plugin.rs',
        'bevy/aestra-bevy-render/src/cpu.rs',
        'bevy/aestra-bevy-render/src/gpu/bootstrap.rs',
        'bevy/aestra-bevy/src/host_plugin.rs',
        'bevy/aestra-bevy/src/input_events.rs',
        'bevy/aestra-bevy/src/project_profile.rs',
        'bevy/aestra-bevy-render/src/project_assets.rs',
        'bevy/aestra-bevy-render/src/preview.rs',
        'bevy/aestra-bevy-render/src/sampling.rs',
        'bevy/aestra-bevy-render/src/gpu/preparation.rs',
        'bevy/aestra-bevy-render/src/gpu/preparation_tests.rs',
        'bevy/aestra-bevy-render/src/gpu/bounds.rs',
        'bevy/aestra-bevy-render/src/gpu/ribbon_bounds.rs',
        'bevy/aestra-bevy-render/src/gpu/bounds_test_types_019.rs',
        'bevy/aestra-bevy-render/src/gpu/bounds_test_types_020.rs',
        'bevy/aestra-bevy-render/src/gpu/material_lighting.rs',
        'bevy/aestra-bevy-render/src/gpu/material_shader_019.rs',
        'bevy/aestra-bevy-render/src/gpu/material_shader_020.rs',
        'bevy/aestra-bevy-render/src/presented_effect.rs',
        'bevy/aestra-bevy-render/src/gpu/output_context.rs',
        'bevy/aestra-bevy-render/src/gpu/particle_output_readback.rs',
        'bevy/aestra-bevy-render/src/gpu/particle_outputs.rs',
        'bevy/aestra-bevy-render/src/capabilities.rs',
        'bevy/aestra-bevy-render/src/render_settings.rs',
        'bevy/aestra-bevy-render/src/gpu/catchup_pacing.rs',
        'bevy/aestra-bevy-render/src/gpu/capability_publication.rs',
        'bevy/aestra-bevy-render/src/gpu/extraction_systems.rs',
        'bevy/aestra-bevy-render/src/gpu/extraction_control_tests.rs',
        'bevy/aestra-bevy-render/src/gpu/render.rs',
        'bevy/aestra-bevy-render/src/gpu/draw_commands.rs',
        'bevy/aestra-bevy-render/src/gpu/draw_preparation.rs',
        'bevy/aestra-bevy-render/src/gpu/draw_resources.rs',
        'bevy/aestra-bevy-render/src/gpu/particle_statistics.rs',
        'bevy/aestra-bevy-render/src/gpu/physics.rs',
        'bevy/aestra-bevy-render/src/gpu/transform_sync.rs',
        'bevy/aestra-bevy-render/src/gpu/visibility_updates.rs',
        'bevy/aestra-bevy-render/src/gpu/visibility_tests.rs',
        'bevy/aestra-bevy-render/src/gpu/scene_depth_019.rs',
        'bevy/aestra-bevy-render/src/gpu/scene_depth_020.rs',
        'bevy/aestra-bevy-render/src/gpu/alpha_sort.rs',
        'bevy/aestra-bevy-render/src/gpu/alpha_sort_tests.rs',
        'bevy/aestra-bevy-render/src/gpu/alpha_sort.wgsl',
        'bevy/aestra-bevy-render/src/gpu/trail_compaction.rs',
        'bevy/aestra-bevy-render/src/gpu/trail_culling.rs',
        'bevy/aestra-bevy-render/src/gpu/preparation_context.rs',
        'bevy/aestra-bevy-render/src/gpu/preparation_timing.rs',
        'bevy/aestra-bevy-render/src/gpu/simulation_timing.rs',
        'bevy/aestra-bevy-render/src/gpu/simulation_pipeline.rs',
        'bevy/aestra-bevy-render/src/gpu/simulation_graph.rs',
        'bevy/aestra-bevy-render/src/gpu/stage_coupling.rs',
        'bevy/aestra-bevy-render/src/gpu/presentation_readiness.rs',
        'bevy/aestra-bevy-render/src/gpu/stateful_simulation.rs',
        'bevy/aestra-bevy-render/src/gpu/coupled_simulation.rs',
        'bevy/aestra-bevy-render/src/execution.rs',
        'bevy/aestra-bevy-render/src/gpu/paged_trails.rs',
        'bevy/aestra-bevy-render/src/gpu/timestamp_transport.rs',
        'bevy/aestra-bevy-render/src/gpu/mapped_readback_019.rs',
        'bevy/aestra-bevy-render/src/gpu/mapped_readback_020.rs',
        'bevy/aestra-bevy-render/src/gpu/geometry_statistics.rs',
        'bevy/aestra-bevy-render/src/gpu/pipeline.rs',
        'bevy/aestra-bevy-render/src/material.rs',
        'bevy/aestra-bevy-render/src/material_layout.rs',
        'bevy/aestra-bevy-render/src/gpu/view_phases.rs',
        'bevy/aestra-bevy-render/src/gpu/shader_composition.rs',
        'bevy/aestra-bevy-render/src/gpu/storage_encoding.rs',
        'bevy/aestra-bevy-render/src/gpu/storage_buffers.rs',
        'bevy/aestra-bevy-render/src/gpu/storage_buffers_020.rs',
        'bevy/aestra-bevy-render/src/gpu/draw_instance.rs',
        'bevy/aestra-bevy-render/src/gpu/extraction.rs',
        'bevy/aestra-bevy-render/src/gpu/extraction_020.rs',
        'bevy/aestra-bevy-render/src/gpu/extraction_cleanup.rs',
        'bevy/aestra-bevy-render/src/gpu/extraction_tests.rs',
        'bevy/aestra-bevy-render/src/gpu/extraction_state_tests.rs',
        'bevy/aestra-bevy-render/src/gpu/clone_extraction.rs',
        'bevy/aestra-bevy-render/src/gpu/effect_inputs.rs',
        'bevy/aestra-bevy-render/src/gpu/stage_inputs.rs',
        'bevy/aestra-bevy/src/playback.rs',
        'bevy/aestra-bevy/src/project.rs',
        'bevy/aestra-bevy/src/bindings.rs',
        'bevy/aestra-bevy/src/lib.rs',
        'bevy/aestra-bevy/src/particle_lights.rs',
        'bevy/aestra-bevy/src/particle_light_gpu.rs',
        'bevy/aestra-bevy/src/particle_light_gpu.wgsl',
        'bevy/aestra-bevy/src/particle_light_boundary_019.rs',
        'bevy/aestra-bevy/src/particle_light_boundary_020.rs',
        'bevy/aestra-bevy/src/lighting_quality.rs',
        'bevy/aestra-bevy/src/lights.rs',
        'bevy/aestra-bevy-render/src/gpu/stage_runtimes.rs',
        'bevy/aestra-bevy-render/src/gpu/stage_output_delivery.rs',
        'bevy/aestra-bevy-render/src/gpu/particle_light_inputs.rs',
        'bevy/aestra-bevy-render/src/gpu/particle_light_transport.rs',
        'bevy/aestra-bevy-render/src/gpu/particle_lights.rs',
        'bevy/aestra-bevy-render/src/gpu/particle_light_readback.rs',
        'bevy/aestra-bevy-render/src/gpu/trail_checkpoints.rs',
        'bevy/aestra-bevy-render/src/gpu/trail_context.rs',
        'bevy/aestra-bevy-render/src/gpu/stateful_trails.rs',
        'bevy/aestra-bevy-render/src/gpu/stateful_trails_tests.rs',
        'bevy/aestra-bevy-render/src/gpu/volume.rs',
        'bevy/aestra-bevy-render/src/render_set.rs',
        'bevy/aestra-bevy-render/src/gpu/volume_shader_019.rs',
        'bevy/aestra-bevy-render/src/gpu/volume_shader_020.rs',
        'bevy/aestra-bevy-render/src/gpu/mesh_inputs.rs',
        'bevy/aestra-bevy-render/src/gpu/world_sdf.rs',
        'bevy/aestra-bevy-render/src/gpu/extension_stages.rs',
        'bevy/aestra-bevy-render/src/gpu/sprite_culling.rs',
        'bevy/aestra-bevy-render/src/gpu/wireframe.rs',
        'bevy/aestra-bevy-render/src/gpu/material_lighting.wgsl',
        'bevy/aestra-bevy-render/src/gpu/volume_lighting.wgsl',
        'extensions/aestra-fluid/src/volume.wgsl',
        'extensions/aestra-fluid/src/liquid_look.wgsl',
        'apps/aestra-editor/src/feathers/shaders/node_graph_grid.wgsl',
        'apps/aestra-editor/src/feathers/shaders/node_graph_wire.wgsl',
        'assets/test/materials/fireworks_lit_smoke.aestra.material.ron'
    )
    # The shader fixture compiles real engine-neutral Aestra path dependencies too.
    $paths += @(Get-ChildItem -LiteralPath (Join-Path $root 'assets/icons') -Filter '*.svg' -File |
        ForEach-Object { [IO.Path]::GetRelativePath($root, $_.FullName).Replace('\', '/') })
    foreach ($crate in @('aestra-core', 'aestra-authoring', 'aestra-compiler', 'aestra-gpu', 'aestra-runtime', 'aestra-extension', 'aestra-project')) {
        $directory = Join-Path $root "crates/$crate"
        $paths += "crates/$crate/Cargo.toml"
        $paths += @(Get-ChildItem -LiteralPath "$directory/src" -File -Recurse |
            ForEach-Object { [IO.Path]::GetRelativePath($root, $_.FullName).Replace('\', '/') })
    }
    $paths += 'bevy/aestra-bevy-render/src/host_transform.rs', 'bevy/aestra-bevy-render/src/gpu/trail_replay.rs', 'extensions/aestra-fluid/Cargo.toml'
    $paths += @(Get-ChildItem -LiteralPath (Join-Path $root 'extensions/aestra-fluid/src') -File -Recurse |
        ForEach-Object { [IO.Path]::GetRelativePath($root, $_.FullName).Replace('\', '/') })
    $paths += 'extensions/aestra-example-extension/Cargo.toml'
    $paths += @(Get-ChildItem -LiteralPath (Join-Path $root 'extensions/aestra-example-extension/src') -File -Recurse |
        ForEach-Object { [IO.Path]::GetRelativePath($root, $_.FullName).Replace('\', '/') })
    foreach ($directory in @('vendor/bevy_input_focus_020', 'vendor/bevy_pbr_020')) {
        $paths += @(Get-ChildItem -LiteralPath (Join-Path $root $directory) -File -Recurse -Force |
            ForEach-Object { [IO.Path]::GetRelativePath($root, $_.FullName).Replace('\', '/') })
    }
    $result = [ordered]@{}
    foreach ($path in ($paths | Sort-Object -Unique)) { $result[$path] = Hash (Join-Path $root $path) }
    $result
}
function RunCargo([string]$Name, [string[]]$Arguments) {
    $log = Join-Path $reports "$Name.log"
    & cargo "+$Toolchain" @Arguments 2>&1 | Tee-Object -FilePath $log | ForEach-Object {
        # Retain machine-readable evidence without flooding the console with it.
        if ($Name -in @('native-build', 'shader-native-build', 'storage-native-build', 'extraction-native-build', 'async-native-build') -and $_.ToString().StartsWith('{')) {
            $message = $_ | ConvertFrom-Json
            if ($message.reason -eq 'compiler-message') { Write-Host $message.message.rendered }
        } elseif ($Name -ne 'metadata') { $_ | Out-Host }
    }
    if ($LASTEXITCODE -ne 0) { throw "$Name failed ($LASTEXITCODE); retained $log" }
}

Push-Location $root
try {
    # Verify every untouched published file, and reject unlisted source additions.
    $allowed = @{
        bevy_input_focus_020 = @('src/lib.rs', 'src/keyboard_dispatch.rs')
        bevy_pbr_020 = @('src/cluster/gpu.rs', 'src/cluster/mod.rs')
    }
    foreach ($crate in $allowed.Keys) {
        $directory = Join-Path $root "vendor/$crate"
        $provenance = Get-Content -LiteralPath "$directory/UPSTREAM_SHA256.json" -Raw | ConvertFrom-Json -AsHashtable
        if ($provenance.version -ne '0.20.0' -or $provenance.registry_package_checksum -notmatch '^[a-f0-9]{64}$') {
            throw "Invalid published provenance: $crate"
        }
        foreach ($entry in $provenance.files.GetEnumerator()) {
            if ($entry.Key -notin $allowed[$crate] -and (Hash "$directory/$($entry.Key)") -ne $entry.Value) {
                throw "Unexpected upstream modification: $crate/$($entry.Key)"
            }
        }
        Get-ChildItem -LiteralPath $directory -File -Recurse -Force | ForEach-Object {
            $relative = [IO.Path]::GetRelativePath($directory, $_.FullName).Replace('\', '/')
            if (!$provenance.files.ContainsKey($relative) -and $relative -notin $allowed[$crate] -and
                $relative -notin @('AESTRA_PATCH.md', 'UPSTREAM_SHA256.json', '.gitattributes')) {
                throw "Unexpected added file: $crate/$relative"
            }
        }
    }
    $before = Inputs
    $before | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath "$reports/inputs-before.json" -Encoding utf8NoBOM
    RunCargo 'versions' @('--version')
    & rustc "+$Toolchain" -vV 2>&1 | Set-Content -LiteralPath "$reports/rustc.txt" -Encoding utf8NoBOM
    if ($LASTEXITCODE -ne 0) { throw 'rustc version failed' }
    RunCargo 'metadata' @('metadata', '--manifest-path', $manifest, '--locked', '--offline', '--format-version', '1', '--filter-platform', 'x86_64-pc-windows-msvc')
    $metadata = Get-Content -LiteralPath "$reports/metadata.log" -Raw | ConvertFrom-Json
    $bevyVersions = @($metadata.packages | Where-Object { $_.name -match '^bevy($|_)' -and $_.name -notin @('bevy_mikktspace') } | Select-Object -ExpandProperty version -Unique)
    if ($bevyVersions.Count -ne 1 -or $bevyVersions[0] -ne '0.20.0') { throw "Mixed Bevy graph: $bevyVersions" }
    foreach ($crate in @('bevy_input_focus', 'bevy_pbr')) {
        $package = $metadata.packages | Where-Object name -EQ $crate
        $expected = [IO.Path]::GetFullPath((Join-Path $root "vendor/$crate`_020/Cargo.toml"))
        if ($package.source -or $package.manifest_path -ne $expected) { throw "Candidate patch not selected: $crate" }
    }
    $common = @('--manifest-path', $manifest, '--locked', '--offline', '--target-dir', $target)
    RunCargo 'keyboard' (@('test') + $common + @('--test', 'keyboard', '--', '--test-threads=1'))
    RunCargo 'editor-theme' (@('test') + $common + @('--test', 'editor_theme', '--features', 'editor-qualification', '--', '--test-threads=1'))
    RunCargo 'editor-controls' (@('test') + $common + @('--test', 'editor_controls', '--features', 'editor-qualification', '--', '--test-threads=1'))
    RunCargo 'editor-numbers' (@('test') + $common + @('--test', 'editor_numbers', '--features', 'editor-qualification', '--', '--test-threads=1'))
    RunCargo 'editor-colors' (@('test') + $common + @('--test', 'editor_colors', '--features', 'editor-qualification', '--', '--test-threads=1'))
    RunCargo 'editor-text' (@('test') + $common + @('--test', 'editor_text', '--features', 'editor-qualification', '--', '--test-threads=1'))
    RunCargo 'editor-tooltips' (@('test') + $common + @('--test', 'editor_tooltips', '--features', 'editor-qualification', '--', '--test-threads=1'))
    RunCargo 'editor-graph' (@('test') + $common + @('--test', 'editor_graph', '--features', 'editor-qualification,async-qualification', '--', '--test-threads=1'))
    RunCargo 'upstream-input-focus' (@('test') + $common + @('-p', 'bevy_input_focus', '--lib', '--', '--test-threads=1'))
    RunCargo 'cluster-units' (@('test') + $common + @('-p', 'bevy_pbr', '--lib', 'aestra_reuse_tests', '--', '--test-threads=1'))
    RunCargo 'shader-composition' (@('test') + $common + @('--test', 'shader_composition', '--', '--test-threads=1'))
    RunCargo 'storage-buffers' (@('test') + $common + @('--test', 'storage_buffers', '--', '--test-threads=1'))
    RunCargo 'extraction' (@('test') + $common + @('--test', 'extraction', '--', '--test-threads=1'))
    $binary = $null
    $shaderBinary = $null
    $storageBinary = $null
    $extractionBinary = $null
    $asyncBinary = $null
    if ($Native) {
        RunCargo 'native-build' (@('test') + $common + @('--test', 'cluster_buffer_reuse', '--no-run', '--message-format=json'))
        $artifacts = Get-Content -LiteralPath "$reports/native-build.log" | ForEach-Object {
            try { $_ | ConvertFrom-Json } catch { $null }
        }
        $binary = @($artifacts | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'cluster_buffer_reuse' -and $_.executable } | Select-Object -ExpandProperty executable -Unique)
        if ($binary.Count -ne 1) { throw 'Expected exactly one native test executable.' }
        $binary = $binary[0]
        $binaryBefore = Hash $binary
        foreach ($test in @('native_cluster_buffers_reuse_grow_reset_and_retire_per_view', 'native_private_generations_overflow_recover_and_retire')) {
            $log = Join-Path $reports "$test.log"
            & $binary $test --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
            if ($LASTEXITCODE -ne 0) { throw "$test failed ($LASTEXITCODE); retained $log" }
            if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        }
        if ((Hash $binary) -ne $binaryBefore) { throw 'Native binary changed during qualification.' }
        # Run after both cluster tests have exited: never contend for the native GPU.
        RunCargo 'shader-native-build' (@('test') + $common + @('--test', 'shader_composition', '--no-run', '--message-format=json'))
        $artifacts = Get-Content -LiteralPath "$reports/shader-native-build.log" | ForEach-Object {
            try { $_ | ConvertFrom-Json } catch { $null }
        }
        $shaderBinary = @($artifacts | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'shader_composition' -and $_.executable } | Select-Object -ExpandProperty executable -Unique)
        if ($shaderBinary.Count -ne 1) { throw 'Expected exactly one shader test executable.' }
        $shaderBinary = $shaderBinary[0]
        $shaderBinaryBefore = Hash $shaderBinary
        $log = Join-Path $reports 'native-shader-pipelines.log'
        & $shaderBinary native_020_shader_modules_and_render_pipelines_validate --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native shader test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $shaderBinary) -ne $shaderBinaryBefore) { throw 'Shader binary changed during qualification.' }
        # Shader asset upload/resize qualification is another separate native process.
        RunCargo 'storage-native-build' (@('test') + $common + @('--test', 'storage_buffers', '--no-run', '--message-format=json'))
        $artifacts = Get-Content -LiteralPath "$reports/storage-native-build.log" | ForEach-Object {
            try { $_ | ConvertFrom-Json } catch { $null }
        }
        $storageBinary = @($artifacts | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'storage_buffers' -and $_.executable } | Select-Object -ExpandProperty executable -Unique)
        if ($storageBinary.Count -ne 1) { throw 'Expected exactly one storage test executable.' }
        $storageBinary = $storageBinary[0]
        $storageBinaryBefore = Hash $storageBinary
        $log = Join-Path $reports 'native-storage-buffers.log'
        & $storageBinary native_020_storage_uploads_reuse_resize_and_invalidate_bindings --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native storage test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $storageBinary) -ne $storageBinaryBefore) { throw 'Storage binary changed during qualification.' }
        # Device publication accesses the actual main world during extraction, on hardware.
        # It runs only after all preceding native processes have exited.
        RunCargo 'extraction-native-build' (@('test') + $common + @('--test', 'extraction', '--no-run', '--message-format=json'))
        $artifacts = Get-Content -LiteralPath "$reports/extraction-native-build.log" | ForEach-Object {
            try { $_ | ConvertFrom-Json } catch { $null }
        }
        $extractionBinary = @($artifacts | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'extraction' -and $_.executable } | Select-Object -ExpandProperty executable -Unique)
        if ($extractionBinary.Count -ne 1) { throw 'Expected exactly one extraction test executable.' }
        $extractionBinary = $extractionBinary[0]
        $extractionBinaryBefore = Hash $extractionBinary
        $log = Join-Path $reports 'native-device-publication.log'
        & $extractionBinary native_020_device_capabilities_publish_through_real_extract_schedule --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native extraction test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Extraction binary changed during qualification.' }
        $log = Join-Path $reports 'native-production-pipelines.log'
        & $extractionBinary pipeline_native::native_020_production_pipeline_descriptors_validate_with_explicit_layouts --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native pipeline test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Pipeline binary changed during qualification.' }
        $log = Join-Path $reports 'native-draw-bindings.log'
        & $extractionBinary draw_native::native_020_commands_submit_with_real_mesh_view_and_prepass_bindings --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native draw command test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Draw command binary changed during qualification.' }
        $log = Join-Path $reports 'native-queue-schedule.log'
        & $extractionBinary queue_native::native_020_installed_queues_render_and_retire_through_real_schedule --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native queue schedule test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Queue schedule binary changed during qualification.' }
        $log = Join-Path $reports 'native-alpha-compute-schedule.log'
        & $extractionBinary alpha_sort_native::native_020_alpha_compute_feeds_installed_queues_and_gates_stale_indices --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native alpha compute schedule test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Alpha compute schedule binary changed during qualification.' }
        $log = Join-Path $reports 'native-alpha-sparse-pools.log'
        & $extractionBinary alpha_sort::tests::native_alpha_sort_handles_large_sparse_pools_and_opposite_views --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native alpha sparse-pool test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Alpha sparse-pool binary changed during qualification.' }
        $log = Join-Path $reports 'native-trail-compute-schedule.log'
        & $extractionBinary trail_native::native_020_trail_producers_feed_installed_queues_and_fail_open --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native trail producer test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Trail producer binary changed during qualification.' }
        $log = Join-Path $reports 'native-analytic-simulation.log'
        & $extractionBinary simulation_native::native_020_analytic_simulation_feeds_alpha_and_installed_queues --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native analytic simulation test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Analytic simulation binary changed during qualification.' }
        $log = Join-Path $reports 'native-trail-simulation.log'
        & $extractionBinary trail_simulation_native::native_020_actual_trail_history_feeds_installed_producers_and_queues --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native trail simulation test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Trail simulation binary changed during qualification.' }
        $log = Join-Path $reports 'native-stateful-simulation.log'
        & $extractionBinary stateful_native::native_020_stateful_simulation_feeds_alpha_and_installed_queues --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native stateful simulation test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Stateful simulation binary changed during qualification.' }
        $log = Join-Path $reports 'native-coupled-simulation.log'
        & $extractionBinary coupled_native::native_020_coupled_routes_feed_alpha_and_installed_queues --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native coupled simulation test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Coupled simulation binary changed during qualification.' }
        $log = Join-Path $reports 'native-fluid-joint-trails.log'
        & $extractionBinary coupled_trails_native::native_020_fluid_coupling_and_joint_trails_feed_installed_queues --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native fluid/joint trail test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Fluid/joint trail binary changed during qualification.' }
        $log = Join-Path $reports 'native-domain-births.log'
        & $extractionBinary coupled_trails_native::native_020_domain_births_feed_particles_trails_and_output_rings --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native domain birth test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Domain birth binary changed during qualification.' }
        $log = Join-Path $reports 'native-async-host-outputs.log'
        & $extractionBinary coupled_trails_native::native_020_async_particle_outputs_reach_routed_host_messages_once --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native asynchronous host output test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Asynchronous host output binary changed during qualification.' }
        $log = Join-Path $reports 'native-stage-outputs.log'
        & $extractionBinary stage_outputs_native::native_020_stage_outputs_reach_host_values_and_impact_messages --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native stage output test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Stage output binary changed during qualification.' }
        $log = Join-Path $reports 'native-host-stage-schedule.log'
        & $extractionBinary host_stage_native::native_020_host_clock_drives_stage_reset_and_suppressed_seek_outputs --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native host stage schedule test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Host stage schedule binary changed during qualification.' }
        $log = Join-Path $reports 'native-project-host-schedule.log'
        & $extractionBinary host_stage_native::native_020_nested_project_bindings_drive_independent_stage_timelines --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native project host schedule test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Project host schedule binary changed during qualification.' }
        $log = Join-Path $reports 'native-timestamp-transport.log'
        & $extractionBinary trail_native::native_020_timestamp_transport_bounds_and_recycles_in_flight_batches --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native timestamp test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Timestamp binary changed during qualification.' }
        foreach ($test in @(
            'stateful_native::native_020_outer_graph_stateful_particles_reach_draws_and_profiling',
            'coupled_native::native_020_outer_graph_coupled_routes_reach_draws_and_profiling',
            'coupled_trails_native::native_020_outer_graph_fluid_and_joint_trails_restore_and_present',
            'coupled_trails_native::native_020_outer_graph_domain_births_reach_trails_and_output_rings',
            'particle_lights_native::native_020_production_light_installer_selects_mixed_playback_and_retires_stale_sources',
            'host_lights_native::native_020_host_lights_realize_async_and_same_frame_and_retire',
            'volume_native::native_020_production_volume_installer_copies_live_fields_draws_and_retires'
        )) {
            $log = Join-Path $reports (($test -replace '::', '-') + '.log')
            & $extractionBinary $test --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
            if ($LASTEXITCODE -ne 0) { throw "$test failed ($LASTEXITCODE); retained $log" }
            if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
            if (!(Select-String -LiteralPath $log -Pattern 'test result: ok\. 1 passed' -Quiet)) { throw "Native gate did not execute exactly one test: $log" }
            if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Outer-graph binary changed during qualification.' }
        }
        # Keep the local synchronous gates unchanged. Only these explicit
        # gates enable multithreaded shader compilation / real render pipelining.
        RunCargo 'async-native-build' (@('test') + $common + @('--test', 'extraction', '--features', 'async-qualification', '--no-run', '--message-format=json'))
        $artifacts = Get-Content -LiteralPath "$reports/async-native-build.log" | ForEach-Object {
            try { $_ | ConvertFrom-Json } catch { $null }
        }
        $asyncBinary = @($artifacts | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'extraction' -and $_.executable } | Select-Object -ExpandProperty executable -Unique)
        if ($asyncBinary.Count -ne 1) { throw 'Expected exactly one asynchronous test executable.' }
        $asyncBinary = $asyncBinary[0]
        $asyncBinaryBefore = Hash $asyncBinary
        foreach ($test in @(
            'stateful_native::native_020_delayed_shader_recovers_stateful_simulation_and_draws',
            'stateful_native::native_020_outer_graph_recovers_delayed_stateful_readiness',
            'host_stage_native::native_020_pipelined_project_outputs_survive_restart_seek_and_teardown',
            'pipelined_installers_native::native_020_pipelined_installers_share_nested_clock_outputs_and_lights',
            'pipelined_coupled_native::native_020_full_plugins_hold_and_resume_coupled_particles_fluid_and_trails'
        )) {
            $log = Join-Path $reports (($test -replace '::', '-') + '.log')
            & $asyncBinary $test --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
            if ($LASTEXITCODE -ne 0) { throw "$test failed ($LASTEXITCODE); retained $log" }
            if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
            if (!(Select-String -LiteralPath $log -Pattern 'test result: ok\. 1 passed' -Quiet)) { throw "Native gate did not execute exactly one test: $log" }
            if ((Hash $asyncBinary) -ne $asyncBinaryBefore) { throw 'Asynchronous binary changed during qualification.' }
        }
    }
    $after = Inputs
    $after | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath "$reports/inputs-after.json" -Encoding utf8NoBOM
    if (($before | ConvertTo-Json -Compress) -ne ($after | ConvertTo-Json -Compress)) { throw 'Qualification inputs changed during run.' }
    [ordered]@{
        accepted = $true; bevy = $bevyVersions; toolchain = $Toolchain
        native_executed = [bool]$Native; binary = $binary
        binary_sha256 = $(if ($Native) { $binaryBefore } else { $null })
        shader_binary = $shaderBinary
        shader_binary_sha256 = $(if ($Native) { $shaderBinaryBefore } else { $null })
        storage_binary = $storageBinary
        storage_binary_sha256 = $(if ($Native) { $storageBinaryBefore } else { $null })
        extraction_binary = $extractionBinary
        extraction_binary_sha256 = $(if ($Native) { $extractionBinaryBefore } else { $null })
        async_binary = $asyncBinary
        async_binary_sha256 = $(if ($Native) { $asyncBinaryBefore } else { $null })
        source_hashes_unchanged = $true; shipping_lock_sha256 = $after['Cargo.lock']
    } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath "$reports/summary.json" -Encoding utf8NoBOM
} finally { Pop-Location }
Write-Output $reports
