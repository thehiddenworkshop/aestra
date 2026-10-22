//! Actual graph pin/collapse, presentation restoration and socket visual consumers.
use super::{
    composition::{
        FeathersGraphNodeBody, FeathersGraphSocketDot, GraphCollapseAction, GraphCollapseIcon,
        GraphPinAction, GraphPinIcon,
    },
    memory::GraphViewportMemory,
    node::{FeathersGraphNode, GraphPinEdit, GraphPresentationEdit},
    node_control_input,
};
use crate::{
    feathers::{
        action_activation::{FeathersActionButton, PendingFeathersActivation, queue_activation},
        icon::{SvgColor, UiSvg},
    },
    theme,
};
use bevy::{prelude::*, ui_widgets::Activate};

#[derive(Clone, Copy, Debug)]
pub(super) enum SocketPhase {
    None,
    Hovered,
    Pressed,
}

pub(super) fn restore_graph_nodes(
    memory: Res<GraphViewportMemory>,
    mut nodes: Query<(Entity, &mut FeathersGraphNode, &mut Node), Added<FeathersGraphNode>>,
    mut bodies: Query<(&FeathersGraphNodeBody, &mut Node), Without<FeathersGraphNode>>,
    mut icons: Query<(&GraphCollapseIcon, &mut UiSvg)>,
    mut pin_icons: Query<(&GraphPinIcon, &mut SvgColor)>,
) {
    for (entity, mut graph_node, mut style) in &mut nodes {
        let key = (graph_node.graph_key.clone(), graph_node.node_key.clone());
        let Some(saved) = memory.nodes.get(&key) else {
            continue;
        };
        let position = memory
            .node_position(&graph_node.graph_key, &graph_node.node_key)
            .unwrap();
        graph_node.position = position;
        graph_node.collapsed = saved.collapsed;
        graph_node.pinned = memory.is_pinned(&graph_node.graph_key, &graph_node.node_key);
        style.left = Val::Px(position.x);
        style.top = Val::Px(position.y);
        apply_graph_node_collapse(entity, saved.collapsed, &mut bodies, &mut icons);
        apply_graph_node_pin(entity, graph_node.pinned, &mut pin_icons);
    }
}

/// Keeps every node's position (and collapse) in step with the shared memory, so a second view of
/// the same document reflects a node move or collapse live — not just when the pane rebuilds. Node
/// layout is keyed per document, so both views resolve to the same memory entry; the node being
/// dragged already matches and is skipped. Runs only when memory changed.
pub(super) fn sync_graph_nodes_from_memory(
    memory: Res<GraphViewportMemory>,
    mut nodes: Query<(Entity, &mut FeathersGraphNode, &mut Node)>,
    mut bodies: Query<(&FeathersGraphNodeBody, &mut Node), Without<FeathersGraphNode>>,
    mut icons: Query<(&GraphCollapseIcon, &mut UiSvg)>,
    mut pin_icons: Query<(&GraphPinIcon, &mut SvgColor)>,
) {
    if !memory.is_changed() {
        return;
    }
    for (entity, mut graph_node, mut style) in &mut nodes {
        let key = (graph_node.graph_key.clone(), graph_node.node_key.clone());
        let Some(saved) = memory.nodes.get(&key) else {
            continue;
        };
        let position = memory
            .node_position(&graph_node.graph_key, &graph_node.node_key)
            .unwrap();
        if graph_node.position != position {
            graph_node.position = position;
            style.left = Val::Px(position.x);
            style.top = Val::Px(position.y);
        }
        if graph_node.collapsed != saved.collapsed {
            graph_node.collapsed = saved.collapsed;
            apply_graph_node_collapse(entity, saved.collapsed, &mut bodies, &mut icons);
        }
        let pinned = memory.is_pinned(&graph_node.graph_key, &graph_node.node_key);
        if graph_node.pinned != pinned {
            graph_node.pinned = pinned;
            apply_graph_node_pin(entity, pinned, &mut pin_icons);
        }
    }
}

pub(super) fn queue_graph_collapse_activation(
    activate: On<Activate>,
    actions: Query<(), (With<FeathersActionButton>, With<GraphCollapseAction>)>,
    commands: Commands,
) {
    queue_activation(activate, actions, commands);
}

pub(super) fn queue_graph_pin_activation(
    activate: On<Activate>,
    actions: Query<(), (With<FeathersActionButton>, With<GraphPinAction>)>,
    commands: Commands,
) {
    queue_activation(activate, actions, commands);
}

pub(super) fn handle_graph_pin_buttons(
    mut commands: Commands,
    actions: node_control_input::Controls<GraphPinAction>,
    mut nodes: Query<&mut FeathersGraphNode>,
    mut icons: Query<(&GraphPinIcon, &mut SvgColor)>,
    mut memory: ResMut<GraphViewportMemory>,
) {
    for (entity, pressed, action, pending) in actions.iter() {
        if !pressed || !pending {
            continue;
        }
        commands
            .entity(entity)
            .remove::<PendingFeathersActivation>();
        node_control_input::clear_pressed(&mut commands, entity);
        let Ok(mut node) = nodes.get_mut(action.node) else {
            continue;
        };
        // A bootstrap-only node has no authored base yet. Pinning makes its current displayed
        // position an explicit anchor, so materialize that position before storing the pin.
        if memory.node(&node.graph_key, &node.node_key).is_none() {
            memory.set_node(
                node.graph_key.clone(),
                node.node_key.clone(),
                node.position,
                node.collapsed,
            );
        }
        let before = memory.is_pinned(&node.graph_key, &node.node_key);
        let after = !before;
        memory.set_pinned(&node.graph_key, &node.node_key, after);
        node.pinned = after;
        apply_graph_node_pin(action.node, after, &mut icons);
        commands.entity(entity).insert(AccessibleLabel(if after {
            "Unpin node".into()
        } else {
            "Pin node".into()
        }));
        commands.trigger(GraphPinEdit {
            graph: node.graph_key.clone(),
            node: node.node_key.clone(),
            before,
            after,
            origin: Some(action.node),
        });
    }
}

pub(super) fn apply_graph_node_pin(
    entity: Entity,
    pinned: bool,
    icons: &mut Query<(&GraphPinIcon, &mut SvgColor)>,
) {
    for (icon, mut color) in icons.iter_mut() {
        if icon.node == entity {
            color.0 = if pinned {
                theme::ACCENT
            } else {
                theme::TEXT_FAINT
            };
        }
    }
}

pub(super) fn handle_graph_collapse_buttons(
    mut commands: Commands,
    actions: node_control_input::Controls<GraphCollapseAction>,
    mut nodes: Query<&mut FeathersGraphNode>,
    mut bodies: Query<(&FeathersGraphNodeBody, &mut Node), Without<FeathersGraphNode>>,
    mut icons: Query<(&GraphCollapseIcon, &mut UiSvg)>,
    mut memory: ResMut<GraphViewportMemory>,
) {
    for (entity, pressed, action, pending) in actions.iter() {
        if !pressed || !pending {
            continue;
        }
        commands
            .entity(entity)
            .remove::<PendingFeathersActivation>();
        node_control_input::clear_pressed(&mut commands, entity);
        let Ok(mut node) = nodes.get_mut(action.node) else {
            continue;
        };
        let before = memory
            .node(&node.graph_key, &node.node_key)
            .unwrap_or((node.position, node.collapsed));
        node.collapsed = !node.collapsed;
        let collapsed = node.collapsed;
        memory.set_collapsed(&node.graph_key, &node.node_key, node.position, collapsed);
        commands.trigger(GraphPresentationEdit {
            graph: node.graph_key.clone(),
            node: node.node_key.clone(),
            before,
            after: (before.0, collapsed),
            origin: Some(action.node),
        });
        apply_graph_node_collapse(action.node, collapsed, &mut bodies, &mut icons);
        commands
            .entity(entity)
            .insert(AccessibleLabel(if collapsed {
                action.expand_label.clone()
            } else {
                action.collapse_label.clone()
            }));
    }
}

pub(super) fn apply_graph_node_collapse(
    entity: Entity,
    collapsed: bool,
    bodies: &mut Query<(&FeathersGraphNodeBody, &mut Node), Without<FeathersGraphNode>>,
    icons: &mut Query<(&GraphCollapseIcon, &mut UiSvg)>,
) {
    for (body, mut style) in bodies.iter_mut() {
        if body.node == entity {
            style.display = if collapsed {
                Display::None
            } else {
                Display::Flex
            };
        }
    }
    for (icon, mut svg) in icons.iter_mut() {
        if icon.node == entity {
            svg.0 = if collapsed {
                icon.expand_icon.clone()
            } else {
                icon.collapse_icon.clone()
            };
        }
    }
}

pub(super) fn update_socket_visuals(
    mut sockets: node_control_input::SocketControls,
    mut dots: Query<(&mut BackgroundColor, &mut BorderColor), With<FeathersGraphSocketDot>>,
) {
    for (interaction, socket, children) in sockets.iter() {
        for child in children.iter() {
            let Ok((mut background, mut border)) = dots.get_mut(child) else {
                continue;
            };
            match interaction {
                SocketPhase::None => {
                    background.0 = theme::PANEL_DARK;
                    *border = BorderColor::all(socket.color);
                }
                SocketPhase::Hovered => {
                    background.0 = socket.color;
                    *border = BorderColor::all(theme::TEXT);
                }
                SocketPhase::Pressed => {
                    background.0 = theme::TEXT;
                    *border = BorderColor::all(socket.color);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "node_controls/tests.rs"]
mod tests;
