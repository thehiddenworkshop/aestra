//! M0 contracts for the shared widget, before introducing measured layout or movement.
use super::*;
use bevy::{
    ecs::system::RunSystemOnce,
    picking::pointer::{Location, PointerId},
};

fn node(graph_key: &str, node_key: &str) -> FeathersGraphNode {
    FeathersGraphNode {
        graph_key: graph_key.into(),
        node_key: node_key.into(),
        position: Vec2::ZERO,
        selected: false,
        collapsed: false,
        dragging: false,
        suppress_release_click: false,
    }
}

fn viewport(key: &str, zoom: f32) -> FeathersGraphViewport {
    FeathersGraphViewport {
        key: key.into(),
        pan: Vec2::ZERO,
        zoom,
        content_size: Vec2::new(720.0, 420.0),
        selection_bounds: None,
        frame_request: Some(GraphFrameTarget::All),
        measured_frame: None,
    }
}

#[test]
fn rebuilt_material_and_function_nodes_restore_manual_position_and_collapsed_body() {
    for (graph_key, node_key) in [
        ("material:p", "expression:n"),
        ("material:p", "output"),
        ("function:project:p", "n"),
        ("function:project:p", "outputs"),
    ] {
        let mut world = World::new();
        let mut memory = GraphViewportMemory::default();
        let position = Vec2::new(-120.0, 340.0);
        memory.set_node(graph_key, node_key, position, true);
        world.insert_resource(memory);
        for _ in 0..2 {
            let entity = world
                .spawn((node(graph_key, node_key), Node::default()))
                .id();
            let body = world
                .spawn((FeathersGraphNodeBody { node: entity }, Node::default()))
                .id();
            world.run_system_once(restore_graph_nodes).unwrap();

            let restored = world.get::<FeathersGraphNode>(entity).unwrap();
            assert_eq!(restored.position, position);
            assert!(restored.collapsed);
            let style = world.get::<Node>(entity).unwrap();
            assert_eq!(style.left, Val::Px(position.x));
            assert_eq!(style.top, Val::Px(position.y));
            assert_eq!(world.get::<Node>(body).unwrap().display, Display::None);
            world.despawn(body);
            world.despawn(entity);
        }
    }
}

#[test]
fn viewport_restore_keeps_per_view_cameras_separate_from_document_nodes() {
    let mut world = World::new();
    let mut memory = GraphViewportMemory::default();
    memory.set_view("material:p#view:1", Vec2::new(20.0, 40.0), 0.5);
    memory.set_view("material:p#view:2", Vec2::new(-80.0, 10.0), 1.5);
    memory.set_node("material:p", "expression:n", Vec2::new(90.0, 30.0), true);
    world.insert_resource(memory);
    let first = world.spawn(viewport("material:p#view:1", 1.0)).id();
    let second = world.spawn(viewport("material:p#view:2", 1.0)).id();
    world.run_system_once(restore_graph_viewports).unwrap();
    for (entity, pan, zoom) in [
        (first, Vec2::new(20.0, 40.0), 0.5),
        (second, Vec2::new(-80.0, 10.0), 1.5),
    ] {
        let restored = world.get::<FeathersGraphViewport>(entity).unwrap();
        assert_eq!(restored.pan, pan);
        assert_eq!(restored.zoom, zoom);
        assert!(restored.frame_request.is_none());
    }
    assert_eq!(
        world
            .resource::<GraphViewportMemory>()
            .node("material:p", "expression:n"),
        Some((Vec2::new(90.0, 30.0), true))
    );
}

#[test]
fn pointer_drag_uses_own_view_zoom_and_inverse_ui_scale_without_semantic_edits() {
    for inverse_scale in [1.0, 0.8, 0.5] {
        for zoom in [0.5, 1.0, 1.75] {
            let mut app = App::new();
            let session = crate::test_support::session_with_timing_slack();
            let effect = session.effect.clone();
            let revision = session.document_revision();
            let undo_len = session.effect_undo_len();
            app.insert_resource(session)
                .init_resource::<GraphViewportMemory>()
                .init_resource::<OverrideCursor>()
                .add_observer(drag_graph_node);
            // The other view deliberately has a different zoom. The node key is per document,
            // so matching the key instead of walking the hierarchy would give the wrong scale.
            app.world_mut().spawn(viewport("material:p#view:1", 0.25));
            let owner = app
                .world_mut()
                .spawn(viewport("material:p#view:2", zoom))
                .id();
            let mut graph_node = node("material:p", "expression:n");
            graph_node.position = Vec2::new(90.0, 30.0);
            graph_node.begin_drag();
            let entity = app
                .world_mut()
                .spawn((
                    graph_node,
                    Node::default(),
                    ComputedNode {
                        inverse_scale_factor: inverse_scale,
                        ..default()
                    },
                    ChildOf(owner),
                ))
                .id();
            let delta = Vec2::new(35.0, -21.0);
            app.world_mut().trigger(Pointer::new(
                PointerId::Mouse,
                Location {
                    target: bevy::camera::NormalizedRenderTarget::None {
                        width: 800,
                        height: 600,
                    },
                    position: Vec2::new(100.0, 120.0),
                },
                Drag {
                    button: PointerButton::Primary,
                    distance: delta,
                    delta,
                },
                entity,
            ));
            let expected = Vec2::new(90.0, 30.0) + delta * inverse_scale / zoom;
            assert!(
                app.world()
                    .get::<FeathersGraphNode>(entity)
                    .unwrap()
                    .position
                    .distance(expected)
                    < 0.001
            );
            let saved = app
                .world()
                .resource::<GraphViewportMemory>()
                .node("material:p", "expression:n")
                .unwrap();
            assert!(saved.0.distance(expected) < 0.001);
            assert!(!saved.1);
            assert_eq!(
                app.world().resource::<crate::EditorSession>().effect,
                effect
            );
            let session = app.world().resource::<crate::EditorSession>();
            assert_eq!(session.document_revision(), revision);
            assert_eq!(session.effect_undo_len(), undo_len);
        }
    }
}
