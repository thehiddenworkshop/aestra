//! Actual toolbar/preview dispatch and inline value/focus/synchronization consumers.
use crate::{
    EditorSession, ProjectEffectCatalog,
    feathers::{
        action_activation::PendingFeathersActivation,
        node_graph::{
            FeathersGraphNodePreviewToggle, FeathersGraphViewport,
            geometry::{GraphDocumentKey, GraphViewKey},
        },
        number_input::{NumberInputValue, UpdateNumberInput},
    },
    history::{EditorHistoryLedger, MaterialProgramEditHistory},
    material_graph::{
        MaterialGraphAction, MaterialGraphPaletteState, MaterialGraphPreviewState,
        MaterialGraphPreviewTarget, MaterialGraphSelectionState, MaterialGraphViewport, arrange,
        click_input::{PointerClick, PointerPress},
        control_input,
        execution::{activate_material_graph_target, apply_material_tool_command},
        keys::material_graph_view_key,
        menu_actions::{MaterialGraphPreviewToggle, MaterialGraphToolbarAction, arrange_seeds},
        node_controls::{MaterialGraphDefaultNumberControl, MaterialGraphDefaultToggleControl},
        palette_state::MaterialGraphPaletteOpen,
        presentation,
    },
};
use aestra_authoring::MaterialToolCommand;
use aestra_core::{
    MaterialExpressionId, MaterialProgramId,
    material::{MaterialExpressionKind, MaterialValue},
};
use bevy::{
    input_focus::{FocusCause, InputFocus},
    picking::pointer::PointerButton,
    prelude::*,
    text::EditableText,
    ui_widgets::ValueChange,
};

pub(super) fn handle_material_graph_toolbar_actions(
    mut commands: Commands,
    actions: control_input::Controls<MaterialGraphToolbarAction>,
    viewports: Query<(
        &MaterialGraphViewport,
        &FeathersGraphViewport,
        &ComputedNode,
    )>,
    graph_nodes: Query<&MaterialGraphAction>,
    mut palette: ResMut<MaterialGraphPaletteState>,
    mut previews: ResMut<MaterialGraphPreviewState>,
    selection: Res<MaterialGraphSelectionState>,
    catalog: Res<ProjectEffectCatalog>,
    mut session: ResMut<EditorSession>,
) {
    for (entity, pressed, action, pending) in actions.iter() {
        if !pressed || !pending {
            continue;
        }
        commands
            .entity(entity)
            .remove::<PendingFeathersActivation>();
        control_input::clear_pressed(&mut commands, entity);
        match *action {
            MaterialGraphToolbarAction::EffectContext => {
                session.return_to_effect_material();
                palette.open = None;
                palette.node_menu = None;
                palette.connection_menu = None;
                continue;
            }
            MaterialGraphToolbarAction::LocateSource(program) => {
                commands.trigger(crate::asset_browser::LocateInAssets(
                    aestra_project::ProjectAssetId::MaterialProgram(program),
                ));
                continue;
            }
            MaterialGraphToolbarAction::AddNode(program, scope) => {
                let Some((marker, viewport, computed)) = viewports
                    .iter()
                    .find(|(marker, _, _)| marker.program == program && marker.scope == scope)
                else {
                    continue;
                };
                let menu_position = computed.size() * computed.inverse_scale_factor * 0.5;
                palette.open = Some(MaterialGraphPaletteOpen {
                    program,
                    editing_target: marker.editing_target.clone(),
                    scope: marker.scope,
                    menu_position,
                    graph_position: viewport.unproject_viewport_point(menu_position),
                    graph_key: material_graph_view_key(program),
                    connection: None,
                });
                palette.node_menu = None;
                palette.connection_menu = None;
                palette.query.clear();
            }
            MaterialGraphToolbarAction::Arrange(program, selection_scope, arrange_scope) => {
                let Some((viewport, _, _)) = viewports.iter().find(|(viewport, _, _)| {
                    viewport.program == program && viewport.scope == selection_scope
                }) else {
                    session.status =
                        "Arrange Graph unavailable: the graph view is no longer mounted".into();
                    continue;
                };
                let seeds = arrange_seeds(&selection, selection_scope, program);
                commands.trigger(arrange::ArrangeGraph {
                    view: GraphViewKey {
                        document: GraphDocumentKey {
                            project: catalog.root().to_owned(),
                            asset: crate::document::DocumentKey::MaterialProgram(program),
                        },
                        view: selection_scope,
                    },
                    editing_target: viewport.editing_target.clone(),
                    scope: arrange_scope,
                    seeds,
                });
                // The mounted view and its geometry revision are part of the async request token.
                // Rebuilding here would invalidate every result before it can be applied.
                continue;
            }
            MaterialGraphToolbarAction::ToggleAllPreviews(program, scope) => {
                let Some((viewport, _, _)) = viewports.iter().find(|(viewport, _, _)| {
                    viewport.program == program && viewport.scope == scope
                }) else {
                    session.status =
                        "Preview controls unavailable: the graph view is no longer mounted".into();
                    continue;
                };
                let before = presentation::visible(&previews, program);
                let targets = graph_nodes
                    .iter()
                    .filter(|node| node.program == program)
                    .map(|node| MaterialGraphPreviewTarget::Expression(node.expression))
                    .chain(std::iter::once(MaterialGraphPreviewTarget::Output))
                    .collect::<Vec<_>>();
                let show = targets
                    .iter()
                    .any(|target| !previews.is_visible(program, *target));
                previews.set_visible(program, targets, show);
                presentation::preview_edit(
                    &mut commands,
                    program,
                    viewport.editing_target.clone(),
                    before,
                    presentation::visible(&previews, program),
                );
            }
        }
        session.ui_revision += 1;
    }
}

pub(super) fn focus_material_graph_number_input(
    mut press: On<PointerPress>,
    controls: Query<(), With<MaterialGraphDefaultNumberControl>>,
    editable_text: Query<(), With<EditableText>>,
    children: Query<&Children>,
    parents: Query<&ChildOf>,
    mut focus: ResMut<InputFocus>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let target = press.event_target();
    let mut candidate = target;
    for _ in 0..6 {
        if controls.contains(candidate) {
            let input = editable_text
                .contains(target)
                .then_some(target)
                .or_else(|| {
                    children
                        .iter_descendants(candidate)
                        .find(|descendant| editable_text.contains(*descendant))
                });
            if let Some(input) = input {
                focus.set(input, FocusCause::Pressed);
                press.propagate(false);
            }
            return;
        }
        let Ok(parent) = parents.get(candidate) else {
            return;
        };
        candidate = parent.parent();
    }
}

pub(super) fn material_graph_value_component(value: &MaterialValue, component: u8) -> Option<f32> {
    let index = usize::from(component);
    match value {
        MaterialValue::Float(value) => (index == 0).then_some(*value),
        MaterialValue::Vec2(value) => value.get(index).copied(),
        MaterialValue::Vec3(value) => value.get(index).copied(),
        MaterialValue::Vec4(value) | MaterialValue::ColorSrgb(value) => value.get(index).copied(),
        MaterialValue::Texture2D(_) | MaterialValue::Bool(_) => None,
    }
}

pub(super) fn sync_material_graph_default_number_inputs(
    mut commands: Commands,
    controls: Query<
        (Entity, &MaterialGraphDefaultNumberControl),
        Added<MaterialGraphDefaultNumberControl>,
    >,
    children: Query<&Children>,
    editable_text: Query<(), With<EditableText>>,
    mut nodes: Query<&mut Node>,
    mut text_layouts: Query<&mut TextLayout>,
) {
    for (entity, control) in &controls {
        constrain_graph_number_input(
            &mut commands,
            entity,
            &children,
            &editable_text,
            &mut nodes,
            &mut text_layouts,
        );
        if let Some(value) = material_graph_value_component(&control.value, control.component) {
            commands.trigger(UpdateNumberInput {
                entity,
                value: NumberInputValue::F32(value),
            });
        }
    }
}

pub(super) fn stop_material_graph_preview_toggle_click(
    mut click: On<PointerClick>,
    toggles: Query<(), With<MaterialGraphPreviewToggle>>,
) {
    if click.button == PointerButton::Primary && toggles.contains(click.event_target()) {
        click.propagate(false);
    }
}

pub(super) fn handle_material_graph_default_number_change(
    change: On<ValueChange<f32>>,
    controls: Query<&MaterialGraphDefaultNumberControl>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut material_history: ResMut<MaterialProgramEditHistory>,
    mut history_ledger: ResMut<EditorHistoryLedger>,
) {
    if !change.is_final || !change.value.is_finite() {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    let mut value = control.value.clone();
    let Some(current) = material_graph_value_component(&value, control.component) else {
        return;
    };
    if (current - change.value).abs() <= f32::EPSILON
        || !set_material_graph_value_component(&mut value, control.component, change.value)
    {
        return;
    }
    commit_material_graph_default(
        &mut session,
        &mut catalog,
        &mut material_history,
        &mut history_ledger,
        control.program,
        &control.editing_target,
        control.expression,
        value,
    );
}

pub(super) fn commit_material_graph_default(
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    material_history: &mut MaterialProgramEditHistory,
    history_ledger: &mut EditorHistoryLedger,
    program: MaterialProgramId,
    editing_target: &crate::material_document::MaterialEditingTarget,
    expression: MaterialExpressionId,
    value: MaterialValue,
) {
    if let Err(error) = activate_material_graph_target(session, catalog, editing_target, program) {
        session.status = format!("Could not edit material input default: {error}");
        session.ui_revision += 1;
        return;
    }
    match apply_material_tool_command(
        session,
        catalog,
        material_history,
        program,
        "Edit material input default",
        MaterialToolCommand::ReplaceMaterialExpression {
            program,
            expression,
            replacement: MaterialExpressionKind::Constant(value),
        },
    ) {
        Ok(_) => {
            history_ledger.record_material_edit(session);
            session.status = "Edited material input default".into();
        }
        Err(error) => session.status = format!("Could not edit material input default: {error}"),
    }
    session.ui_revision += 1;
}

pub(super) fn handle_material_graph_preview_actions(
    mut commands: Commands,
    actions: control_input::Controls<
        MaterialGraphPreviewToggle,
        With<FeathersGraphNodePreviewToggle>,
    >,
    mut previews: ResMut<MaterialGraphPreviewState>,
    mut session: ResMut<EditorSession>,
) {
    for (entity, pressed, toggle, pending) in actions.iter() {
        if !pressed || !pending {
            continue;
        }
        commands
            .entity(entity)
            .remove::<PendingFeathersActivation>();
        control_input::clear_pressed(&mut commands, entity);
        let before = presentation::visible(&previews, toggle.program);
        previews.toggle(toggle.program, toggle.target);
        presentation::preview_edit(
            &mut commands,
            toggle.program,
            toggle.editing_target.clone(),
            before,
            presentation::visible(&previews, toggle.program),
        );
        session.ui_revision += 1;
    }
}

pub(super) fn constrain_graph_number_input(
    commands: &mut Commands,
    entity: Entity,
    children: &Query<&Children>,
    editable_text: &Query<(), With<EditableText>>,
    nodes: &mut Query<&mut Node>,
    text_layouts: &mut Query<&mut TextLayout>,
) {
    if let Ok(mut node) = nodes.get_mut(entity) {
        node.width = Val::Percent(100.0);
        node.min_width = Val::Px(0.0);
        node.max_width = Val::Percent(100.0);
        node.flex_shrink = 1.0;
        node.overflow = Overflow::clip();
    }
    for descendant in children.iter_descendants(entity) {
        if !editable_text.contains(descendant) {
            continue;
        }
        if let Ok(mut node) = nodes.get_mut(descendant) {
            node.width = Val::Percent(100.0);
            node.min_width = Val::Px(0.0);
            node.max_width = Val::Percent(100.0);
            node.flex_shrink = 1.0;
            node.overflow = Overflow::clip();
        }
        if let Ok(mut layout) = text_layouts.get_mut(descendant) {
            *layout = TextLayout::justify(Justify::Center);
        }
        // Bevy 0.19 derives an editable text node's private clip from its untransformed
        // content size. That clip shears glyphs when a graph canvas is zoomed with UiTransform.
        // The containing Feathers input and graph viewport already provide the desired clipping.
        control_input::remove_legacy_text_clip(commands, descendant);
    }
}

pub(super) fn set_material_graph_value_component(
    value: &mut MaterialValue,
    component: u8,
    replacement: f32,
) -> bool {
    let index = usize::from(component);
    match value {
        MaterialValue::Float(value) if index == 0 => *value = replacement,
        MaterialValue::Vec2(value) => {
            let Some(value) = value.get_mut(index) else {
                return false;
            };
            *value = replacement;
        }
        MaterialValue::Vec3(value) => {
            let Some(value) = value.get_mut(index) else {
                return false;
            };
            *value = replacement;
        }
        MaterialValue::Vec4(value) | MaterialValue::ColorSrgb(value) => {
            let Some(value) = value.get_mut(index) else {
                return false;
            };
            *value = replacement;
        }
        MaterialValue::Float(_) | MaterialValue::Texture2D(_) | MaterialValue::Bool(_) => {
            return false;
        }
    }
    true
}

pub(super) fn handle_material_graph_default_toggle_change(
    change: On<ValueChange<bool>>,
    controls: Query<&MaterialGraphDefaultToggleControl>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut material_history: ResMut<MaterialProgramEditHistory>,
    mut history_ledger: ResMut<EditorHistoryLedger>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if control.value == change.value {
        return;
    }
    commit_material_graph_default(
        &mut session,
        &mut catalog,
        &mut material_history,
        &mut history_ledger,
        control.program,
        &control.editing_target,
        control.expression,
        MaterialValue::Bool(change.value),
    );
}

#[cfg(test)]
#[path = "controls/tests.rs"]
mod tests;
