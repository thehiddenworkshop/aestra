//! Actual function node/canvas/marquee selection and nested-call navigation observers.
use crate::feathers::node_graph::{FeathersGraphNode, GraphMarqueeSelection};
use crate::material_function_editor::graph::{
    clipboard_context::{FunctionGraphNodeAction, View, ViewScope},
    model::{BodyAction, Constant, Socket},
    socket_palette,
};
use crate::material_graph::click_input::PointerClick;
use crate::{EditorSession, ProjectEffectCatalog};
use aestra_core::material::MaterialExpressionKind;
use bevy::{picking::pointer::PointerButton, prelude::*};
use std::collections::BTreeSet;

pub(super) fn register(app: &mut App) {
    app.add_observer(select_function_graph_node)
        .add_observer(open_nested_function_call)
        .add_observer(select_function_graph_canvas)
        .add_observer(select_function_graph_marquee);
}

#[allow(
    clippy::too_many_arguments,
    reason = "Preserve production observer dependencies and shipping lint policy"
)]
fn select_function_graph_node(
    mut click: On<PointerClick>,
    actions: Query<&FunctionGraphNodeAction>,
    mut graph_nodes: Query<&mut FeathersGraphNode>,
    parents: Query<&ChildOf>,
    sockets: Query<(), With<Socket>>,
    controls: Query<(), Or<(With<Constant>, With<BodyAction>)>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut selection: ResMut<crate::material_graph::MaterialGraphSelectionState>,
    mut session: ResMut<EditorSession>,
) {
    if click.button != PointerButton::Primary || keys.pressed(KeyCode::Space) {
        return;
    }
    // Keep control/socket guards anchored to the picked descendant while this event bubbles.
    let mut entity = click.original_event_target();
    let (action, node_entity) = loop {
        if sockets.contains(entity) || controls.contains(entity) {
            return;
        }
        if let Ok(action) = actions.get(entity) {
            break (*action, entity);
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    if graph_nodes
        .get_mut(node_entity)
        .is_ok_and(|mut node| node.consume_suppressed_release_click())
    {
        click.propagate(false);
        return;
    }
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    selection.select_function_expression(
        action.scope,
        action.owner,
        action.expression,
        control,
        shift,
    );
    session.ui_revision += 1;
    click.propagate(false);
}

fn open_nested_function_call(
    mut click: On<PointerClick>,
    actions: Query<&FunctionGraphNodeAction>,
    parents: Query<&ChildOf>,
    session: Res<EditorSession>,
    catalog: Res<ProjectEffectCatalog>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary || click.count < 2 {
        return;
    }
    // Keep control/socket guards anchored to the picked descendant while this event bubbles.
    let mut entity = click.original_event_target();
    let action = loop {
        if let Ok(action) = actions.get(entity) {
            break *action;
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    let Ok(function) = session.graph_function_for(
        &crate::material_document::MaterialEditingTarget::Function {
            root: catalog.root().to_owned(),
            id: action.owner,
        },
        &catalog,
    ) else {
        return;
    };
    if function.id != action.owner {
        return;
    }
    let Some(expression) = function
        .expressions
        .iter()
        .find(|expression| expression.id == action.expression)
    else {
        return;
    };
    let MaterialExpressionKind::FunctionCall {
        function: aestra_core::material::MaterialFunctionRef::Project(function),
        ..
    } = &expression.kind
    else {
        return;
    };
    commands.trigger(crate::asset_browser::OpenFunction {
        function: *function,
        new_view: false,
    });
    click.propagate(false);
}

fn select_function_graph_canvas(
    mut click: On<PointerClick>,
    views: Query<(&View, &ViewScope)>,
    controls: Query<
        (),
        Or<(
            With<FunctionGraphNodeAction>,
            With<Socket>,
            With<Constant>,
            With<BodyAction>,
            With<socket_palette::Surface>,
        )>,
    >,
    parents: Query<&ChildOf>,
    mut selection: ResMut<crate::material_graph::MaterialGraphSelectionState>,
    mut session: ResMut<EditorSession>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    // Keep control/socket guards anchored to the picked descendant while this event bubbles.
    let mut entity = click.original_event_target();
    let view = loop {
        if controls.contains(entity) {
            return;
        }
        if let Ok(view) = views.get(entity) {
            break (*view.0, *view.1);
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    if selection.clear_function_selection(view.1.0, view.0.0) {
        session.ui_revision += 1;
    }
    click.propagate(false);
}

fn select_function_graph_marquee(
    event: On<GraphMarqueeSelection>,
    views: Query<(&View, &ViewScope)>,
    graph_nodes: Query<(&FunctionGraphNodeAction, &FeathersGraphNode)>,
    mut selection: ResMut<crate::material_graph::MaterialGraphSelectionState>,
    mut session: ResMut<EditorSession>,
) {
    let Ok((function, scope)) = views.get(event.viewport) else {
        return;
    };
    let expressions = graph_nodes
        .iter()
        .filter(|(action, node)| {
            action.owner == function.0 && event.nodes.contains(node.node_key())
        })
        .map(|(action, _)| action.expression)
        .collect::<BTreeSet<_>>();
    if selection.select_function_expressions(scope.0, function.0, &expressions, event.mode) {
        session.ui_revision += 1;
    }
}
