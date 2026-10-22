use super::super::{
    composition::{FeathersGraphSocket, GraphCollapseAction},
    graph_pointer,
    marquee::GraphMarqueeOverlay,
    node::FeathersGraphNode,
};
use super::tests::{app, mount};
use crate::feathers::{action_activation::FeathersActionButton, number_input::EditorNumberInput};
use bevy::{
    feathers::controls::{FeathersCheckbox, FeathersTextInputContainer},
    picking::pointer::{PointerButton, PointerId},
    prelude::*,
};

#[test]
fn original_pointer_target_excludes_every_control_and_descendant_through_bubbling() {
    let mut app = app();
    let m = mount(&mut app, "view");
    app.update();
    let world = app.world_mut();
    let window = world
        .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
        .single(world)
        .unwrap();
    for parent in [m.node, m.canvas] {
        let controls = [
            app.world_mut()
                .spawn((
                    FeathersGraphSocket {
                        color: Color::WHITE,
                    },
                    ChildOf(parent),
                ))
                .id(),
            app.world_mut()
                .spawn((
                    GraphCollapseAction {
                        node: m.node,
                        collapse_label: "Collapse".into(),
                        expand_label: "Expand".into(),
                    },
                    ChildOf(parent),
                ))
                .id(),
            app.world_mut()
                .spawn((EditorNumberInput, ChildOf(parent)))
                .id(),
            app.world_mut()
                .spawn((FeathersCheckbox, ChildOf(parent)))
                .id(),
            app.world_mut()
                .spawn((FeathersTextInputContainer, ChildOf(parent)))
                .id(),
            app.world_mut()
                .spawn((FeathersActionButton, ChildOf(parent)))
                .id(),
        ];
        for control in controls {
            let child = app.world_mut().spawn(ChildOf(control)).id();
            for target in [control, child] {
                app.world_mut()
                    .get_mut::<FeathersGraphNode>(m.node)
                    .unwrap()
                    .suppress_release_click = true;
                app.world_mut().trigger(graph_pointer::tests::press(
                    target,
                    window,
                    Vec2::ZERO,
                    PointerId::Mouse,
                    PointerButton::Primary,
                ));
                assert!(
                    app.world()
                        .get::<FeathersGraphNode>(m.node)
                        .unwrap()
                        .suppress_release_click
                );
                app.world_mut().trigger(graph_pointer::tests::start(
                    target,
                    window,
                    Vec2::ZERO,
                    PointerId::Mouse,
                    PointerButton::Primary,
                ));
                app.world_mut().flush();
                assert!(
                    !app.world()
                        .get::<FeathersGraphNode>(m.node)
                        .unwrap()
                        .dragging
                );
                let world = app.world_mut();
                assert_eq!(
                    world
                        .query_filtered::<Entity, With<GraphMarqueeOverlay>>()
                        .iter(world)
                        .count(),
                    0
                );
            }
        }
    }
}
